// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Server-side bridge mode. After the wolfusb auth handshake and Bridge
//! request, we do the standard USB/IP OP_REQ_IMPORT → OP_REP_IMPORT exchange
//! ourselves, then hand the raw socket fd to the kernel's `usbip_host`
//! driver via sysfs. From that moment the Linux kernel drives the entire
//! USB/IP wire protocol — every URB type including isochronous, alt
//! settings, concurrent transfers, composite descriptors — with no
//! userspace code in the data path.
//!
//! This replaces our previous attempt to serve USB/IP from the `usbip`
//! Rust crate (which had to re-implement all of the above and did so
//! incompletely — iso transfers were stubbed, class-specific descriptors
//! dropped, URBs serialised behind a single mutex).

use std::os::fd::{AsRawFd, IntoRawFd};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::kernel_usbip;
use crate::protocol::types::DeviceId;

const USBIP_VERSION: u16 = 0x0111;
const OP_REQ_IMPORT: u16 = 0x8003;
const OP_REP_IMPORT: u16 = 0x0003;

/// Run the USB/IP bridge on an authenticated stream. The stream must be a
/// Tokio TcpStream so we can recover its underlying OS fd; anything else
/// can't be handed to the kernel.
pub async fn run_bridge(stream: tokio::net::TcpStream, target: DeviceId) {
    if let Err(e) = run_bridge_inner(stream, target).await {
        warn!("Bridge: {e:#}");
    }
}

async fn run_bridge_inner(mut stream: tokio::net::TcpStream, target: DeviceId) -> Result<()> {
    // Look up the real sysfs path — we have bus+address, need bus-port.
    let port_path = kernel_usbip::port_path_for_address(target.bus_number, target.address)
        .with_context(|| {
            format!(
                "resolving sysfs path for bus={} address={}",
                target.bus_number, target.address
            )
        })?;
    let busid = kernel_usbip::sysfs_busid(target.bus_number, &port_path);

    info!("Bridge: target device sysfs busid={}", busid);

    // If a previous session left the device bound to usbip-host, unbind it
    // first. The stub_dev driver hides bConfigurationValue, bDeviceClass, etc.
    // that we need to fill the OP_REP_IMPORT wire struct; only the normal
    // `usb` device driver exposes them.
    if kernel_usbip::is_bound(&busid) {
        info!(
            "Bridge: {} already bound to usbip-host, releasing first",
            busid
        );
        let _ = kernel_usbip::unbind(&busid);
        // Give the kernel a moment to re-probe normal drivers.
        for _ in 0..20 {
            if !kernel_usbip::is_bound(&busid)
                && std::path::Path::new(&format!(
                    "/sys/bus/usb/devices/{}/bConfigurationValue",
                    busid
                ))
                .exists()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // Read device info BEFORE binding to usbip-host. Once bound, the stub
    // driver hides the attributes we need.
    let info = kernel_usbip::DeviceInfo::from_sysfs(&busid)
        .with_context(|| format!("reading sysfs for {busid}"))?;

    // Bind the device to usbip-host so the kernel takes over from normal
    // drivers (uvcvideo, hid, btusb, etc.).
    kernel_usbip::bind(&busid).with_context(|| format!("binding {busid} to usbip-host"))?;

    // OP_REQ_IMPORT from client: version(2) + code(2) + status(4) + busid(32) = 40 bytes.
    let mut req = [0u8; 40];
    stream
        .read_exact(&mut req)
        .await
        .context("reading OP_REQ_IMPORT")?;
    let req_code = u16::from_be_bytes([req[2], req[3]]);
    if req_code != OP_REQ_IMPORT {
        return Err(anyhow!(
            "client sent unexpected USB/IP code 0x{:04x} (expected OP_REQ_IMPORT)",
            req_code
        ));
    }
    // We don't enforce the busid from the client matches — the wolfusb Bridge
    // handshake already identified the device. The client used the value we
    // returned in BridgeAccepted.busid so this always matches anyway.

    // OP_REP_IMPORT response: version(2) + code(2) + status(4) + device(312) = 320 bytes.
    let mut reply = Vec::with_capacity(320);
    reply.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    reply.extend_from_slice(&OP_REP_IMPORT.to_be_bytes());
    reply.extend_from_slice(&0u32.to_be_bytes()); // status = success
    reply.extend_from_slice(&info.to_wire_bytes());
    stream
        .write_all(&reply)
        .await
        .context("sending OP_REP_IMPORT")?;
    stream.flush().await.context("flushing OP_REP_IMPORT")?;

    info!(
        "Bridge: sent OP_REP_IMPORT (vid={:04x} pid={:04x} speed={}), handing socket to kernel",
        info.vendor_id, info.product_id, info.speed
    );

    // Convert the async Tokio stream into a blocking std socket and take its
    // raw fd. The kernel needs a real, kernel-managed fd (not a Tokio
    // non-blocking one) because it drives I/O directly via sock_recvmsg.
    let std_stream = stream
        .into_std()
        .context("converting stream to std for kernel handoff")?;
    std_stream
        .set_nonblocking(false)
        .context("setting socket blocking mode")?;

    let fd = std_stream.as_raw_fd();
    match kernel_usbip::attach_socket(&busid, fd) {
        Ok(()) => {
            info!("Bridge: kernel took ownership of fd {} for {}", fd, busid);
        }
        Err(e) => {
            let _ = kernel_usbip::unbind(&busid);
            return Err(e.context("kernel rejected usbip_sockfd attach"));
        }
    }
    // Kernel now owns the fd; leak the std socket so Drop doesn't close it.
    let _leaked = std_stream.into_raw_fd();

    // Wait for the kernel to release the device (client disconnect or detach).
    // Run on a blocking thread so we don't tie up the Tokio runtime.
    let busid_copy = busid.clone();
    tokio::task::spawn_blocking(move || {
        kernel_usbip::wait_until_detached(&busid_copy);
    })
    .await
    .ok();

    info!(
        "Bridge: kernel released {}, unbinding from usbip-host",
        busid
    );
    if let Err(e) = kernel_usbip::unbind(&busid) {
        warn!("Bridge: failed to unbind {}: {e}", busid);
    }
    Ok(())
}
