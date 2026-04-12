// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Server-side bridge mode: after a client sends `Bridge` and is accepted,
//! this handler takes over the raw stream and translates USB/IP kernel protocol
//! into rusb calls on a real USB device.
//!
//! Behaviour:
//! - Reads USB/IP PDUs from the stream
//! - For each CMD_SUBMIT, spawns a tokio task that executes the transfer via rusb
//! - Sends RET_SUBMIT back with the result
//! - Handles CMD_UNLINK by cancelling in-flight URBs (best-effort)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use rusb::{Context, DeviceHandle};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};

use crate::bridge::usbip::{
    self, CmdSubmit, RetSubmit, RetUnlink, UsbipHeader, UsbipPdu,
    USBIP_DIR_IN, USBIP_RET_SUBMIT, USBIP_RET_UNLINK,
};

/// Run the USB/IP bridge loop. Consumes the stream and drives USB transfers
/// on the given device handle until the connection closes or errors.
pub async fn run_bridge<S>(
    stream: S,
    handle: Arc<DeviceHandle<Context>>,
    devid: u32,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    info!("Bridge: started for devid=0x{:08x}", devid);

    let (tx, mut rx) = mpsc::unbounded_channel::<UsbipPdu>();
    // Track in-flight URBs so CMD_UNLINK can cancel them (simplified:
    // we can't actually cancel a blocking rusb transfer, but we track them).
    let in_flight: Arc<Mutex<HashMap<u32, ()>>> = Arc::new(Mutex::new(HashMap::new()));

    // Split stream into reader/writer via a simple loop pattern
    // (we'll read and write in the same task since USB/IP is request-response
    // with seqnum matching, but submissions happen concurrently via spawned tasks)
    let (mut reader, mut writer) = tokio::io::split(stream);

    // Writer task: serialize all RET_SUBMIT/RET_UNLINK writes
    let write_task = tokio::spawn(async move {
        while let Some(pdu) = rx.recv().await {
            if let Err(e) = usbip::write_pdu(&mut writer, &pdu).await {
                warn!("Bridge: write failed: {}", e);
                break;
            }
        }
        let _ = writer.shutdown().await;
    });

    // Reader loop: read PDUs, dispatch work
    loop {
        let pdu = match usbip::read_pdu(&mut reader).await {
            Ok(p) => p,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    info!("Bridge: client disconnected");
                } else {
                    warn!("Bridge: read error: {}", e);
                }
                break;
            }
        };

        match pdu {
            UsbipPdu::CmdSubmit(cmd) => {
                let seqnum = cmd.header.seqnum;
                in_flight.lock().await.insert(seqnum, ());
                let handle_c = Arc::clone(&handle);
                let tx_c = tx.clone();
                let in_flight_c = Arc::clone(&in_flight);

                // Spawn transfer task — don't block the reader loop
                tokio::task::spawn_blocking(move || {
                    let response = execute_urb(&handle_c, &cmd);
                    let _ = tx_c.send(UsbipPdu::RetSubmit(response));
                    // Blocking runtime doesn't await; drop the async lock handle
                    tokio::runtime::Handle::current().block_on(async move {
                        in_flight_c.lock().await.remove(&seqnum);
                    });
                });
            }
            UsbipPdu::CmdUnlink(unlink) => {
                // Best-effort: send RET_UNLINK. We can't actually cancel a
                // blocking rusb transfer, but we acknowledge the unlink.
                let present = in_flight.lock().await.contains_key(&unlink.unlink_seqnum);
                let status = if present { -libc_econnreset() } else { 0 };
                let ret = RetUnlink {
                    header: UsbipHeader {
                        command: USBIP_RET_UNLINK,
                        seqnum: unlink.header.seqnum,
                        devid,
                        direction: 0,
                        ep: 0,
                    },
                    status,
                };
                let _ = tx.send(UsbipPdu::RetUnlink(ret));
            }
            _ => {
                warn!("Bridge: unexpected PDU type (clients shouldn't send RET_*)");
            }
        }
    }

    drop(tx); // signal writer task to finish
    let _ = write_task.await;
    info!("Bridge: exited");
}

/// Execute a single URB via rusb synchronously.
fn execute_urb(handle: &DeviceHandle<Context>, cmd: &CmdSubmit) -> RetSubmit {
    let seqnum = cmd.header.seqnum;
    let devid = cmd.header.devid;
    let direction_in = cmd.header.direction == USBIP_DIR_IN;
    let ep = cmd.header.ep as u8;
    // Kernel endpoint address = endpoint number + direction bit
    let ep_addr = if direction_in { ep | 0x80 } else { ep };
    let timeout_ms = if cmd.header.ep == 0 { 5000 } else { 5000 };
    let timeout = Duration::from_millis(timeout_ms);

    // Control transfer (endpoint 0) — parse setup packet
    let result = if cmd.header.ep == 0 {
        let setup = &cmd.setup;
        let request_type = setup[0];
        let request = setup[1];
        let value = u16::from_le_bytes([setup[2], setup[3]]);
        let index = u16::from_le_bytes([setup[4], setup[5]]);
        let length = u16::from_le_bytes([setup[6], setup[7]]);
        if direction_in {
            let mut buf = vec![0u8; length as usize];
            match handle.read_control(request_type, request, value, index, &mut buf, timeout) {
                Ok(n) => { buf.truncate(n); Ok(buf) }
                Err(e) => Err(e),
            }
        } else {
            match handle.write_control(request_type, request, value, index, &cmd.data, timeout) {
                Ok(_) => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        }
    } else if direction_in {
        // Bulk/interrupt IN
        let len = cmd.transfer_buffer_length.max(0) as usize;
        let mut buf = vec![0u8; len];
        // Use bulk transfer (works for both bulk and interrupt endpoints with rusb)
        let res = if cmd.header.ep != 0 && cmd.interval != 0 {
            handle.read_interrupt(ep_addr, &mut buf, timeout)
        } else {
            handle.read_bulk(ep_addr, &mut buf, timeout)
        };
        match res {
            Ok(n) => { buf.truncate(n); Ok(buf) }
            Err(e) => Err(e),
        }
    } else {
        // Bulk/interrupt OUT
        let res = if cmd.interval != 0 {
            handle.write_interrupt(ep_addr, &cmd.data, timeout)
        } else {
            handle.write_bulk(ep_addr, &cmd.data, timeout)
        };
        match res {
            Ok(_) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    };

    let (status, data) = match result {
        Ok(data) => (0, data),
        Err(rusb::Error::Timeout) => (-libc_etimedout(), Vec::new()),
        Err(rusb::Error::NoDevice) => (-libc_enodev(), Vec::new()),
        Err(rusb::Error::Pipe) => (-libc_epipe(), Vec::new()),
        Err(e) => {
            debug!("Bridge URB failed: {}", e);
            (-libc_eio(), Vec::new())
        }
    };

    RetSubmit {
        header: UsbipHeader {
            command: USBIP_RET_SUBMIT,
            seqnum,
            devid,
            direction: cmd.header.direction,
            ep: cmd.header.ep,
        },
        status,
        actual_length: data.len() as i32,
        start_frame: 0,
        number_of_packets: 0,
        error_count: 0,
        data,
    }
}

// Linux errno values (from <errno.h>)
fn libc_eio() -> i32 { 5 }
fn libc_enodev() -> i32 { 19 }
fn libc_epipe() -> i32 { 32 }
fn libc_etimedout() -> i32 { 110 }
fn libc_econnreset() -> i32 { 104 }
