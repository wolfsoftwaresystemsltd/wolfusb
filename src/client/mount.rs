// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Client `mount` command — creates a virtual USB device on the local machine
//! that proxies transfers to a remote wolfusb server via vhci_hcd.
//!
//! Flow:
//!   1. Open raw TCP connection (plain socket — kernel needs direct FD access)
//!   2. wolfusb Hello / HMAC auth
//!   3. Send wolfusb Bridge request to select the device
//!   4. Receive BridgeAccepted — server is now speaking standard USB/IP
//!   5. Send USB/IP OP_REQ_IMPORT
//!   6. Receive OP_REP_IMPORT with device info (devid, speed)
//!   7. Hand the raw TCP socket FD to /sys/.../vhci_hcd.0/attach
//!   8. Block on SIGINT; on exit, detach the port

use std::os::fd::IntoRawFd;
use std::time::Duration;

use anyhow::{Context, anyhow};
use futures::{SinkExt, StreamExt};
use log::info;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use crate::bridge::vhci::{self, Speed};
use crate::protocol::codec::WolfUsbCodec;
use crate::protocol::messages::*;
use crate::protocol::types::DeviceId;

pub async fn cmd_mount(
    server: &str,
    bus: u8,
    addr: u8,
    key: Option<&String>,
) -> anyhow::Result<()> {
    vhci::ensure_module_loaded()
        .context("vhci_hcd kernel module unavailable — USB virtualization won't work")?;

    info!(
        "Connecting to {} (plain TCP — TLS not supported for mount mode)",
        server
    );
    let tcp_stream = TcpStream::connect(server)
        .await
        .context("Failed to connect to wolfusb server")?;
    tcp_stream.set_nodelay(true).ok();

    let mut framed = Framed::new(tcp_stream, WolfUsbCodec);

    do_hello(&mut framed, key).await?;
    let busid_str = send_bridge(&mut framed, bus, addr).await?;

    // Switch to raw USB/IP mode — recover the underlying TCP stream
    let mut tcp_stream = framed.into_inner();

    // USB/IP OP_REQ_IMPORT handshake using the busid the server told us.
    // The `usbip` crate's rusb backend formats bus_id as "{bus}-{addr}-{port_number}",
    // which the client can't compute on its own — the server provides it.
    info!("Sending OP_REQ_IMPORT for busid={}", busid_str);
    let import_reply = op_req_import(&mut tcp_stream, &busid_str).await?;

    info!(
        "OP_REP_IMPORT ok: vid=0x{:04x} pid=0x{:04x} speed={} devid=0x{:08x}",
        import_reply.vendor_id, import_reply.product_id, import_reply.speed, import_reply.devid
    );

    // Extract raw socket FD. From here, vhci_hcd drives SUBMIT/UNLINK traffic
    // directly on the socket; our process just parks until detach.
    let std_stream = tcp_stream
        .into_std()
        .context("Failed to convert TCP stream to std")?;
    std_stream.set_nonblocking(false).ok();
    let fd = std_stream.into_raw_fd();

    // Server speaks the kernel's USB/IP wire protocol directly (usbip_host
    // reads udev->speed which is the Linux USB_SPEED_* enum). Values map 1:1
    // to vhci_hcd's Speed: LOW=1, FULL=2, HIGH=3, WIRELESS=4 (unused),
    // SUPER=5, SUPER_PLUS=6.
    let speed = match import_reply.speed {
        1 => Speed::Low,
        2 => Speed::Full,
        3 => Speed::High,
        5 => Speed::Super,
        6 => Speed::SuperPlus,
        _ => Speed::High,
    };

    struct RawFdOwner(std::os::fd::RawFd);
    impl std::os::fd::AsRawFd for RawFdOwner {
        fn as_raw_fd(&self) -> std::os::fd::RawFd {
            self.0
        }
    }
    let owner = RawFdOwner(fd);

    let port = vhci::attach(&owner, import_reply.devid, speed)
        .context("Failed to attach to vhci_hcd (need CAP_SYS_ADMIN?)")?;

    println!("Attached device as virtual USB on vhci_hcd port {}", port);
    println!(
        "Device {:04x}:{:04x} is now visible in lsusb. Press Ctrl-C to detach.",
        import_reply.vendor_id, import_reply.product_id
    );

    // Park until EITHER the operator detaches (Ctrl-C) OR the kernel releases
    // our vhci port. The kernel frees the port when the USB/IP socket dies —
    // i.e. when the source server closes the connection or reboots. We used to
    // wait only on Ctrl-C, so on a source reboot this process sat here forever
    // while the device was already gone; systemd never saw an exit, so the
    // mount unit's Restart= never fired and the device stopped forwarding until
    // a manual re-attach (PapaSchlumpf 2026-06-13). Now we detect the released
    // port and exit NON-ZERO so the unit (Restart=always) reconnects on its own.
    // Poll cadence for the vhci port-liveness check, and how many consecutive
    // "free" reads confirm a real disconnect (vs a transient sysfs blip) before
    // we exit to reconnect. 2 × 2s ≈ 4s of confirmed-free.
    const POLL_INTERVAL: Duration = Duration::from_secs(2);
    const CONSECUTIVE_FREE_TO_EXIT: u8 = 2;
    let port_lost = async {
        let mut consecutive_free = 0u8;
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            if vhci::port_in_use(port) {
                consecutive_free = 0;
            } else {
                consecutive_free += 1;
                if consecutive_free >= CONSECUTIVE_FREE_TO_EXIT {
                    break;
                }
            }
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nDetaching port {}...", port);
            if let Err(e) = vhci::detach(port) {
                log::warn!("Failed to detach port {}: {}", port, e);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        }
        _ = port_lost => {
            log::warn!(
                "vhci port {} was released — the connection to {} dropped (source reboot or detach). Exiting so systemd reconnects.",
                port, server
            );
            Err(anyhow!("USB device connection lost (vhci port {} released)", port))
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn do_hello(
    framed: &mut Framed<TcpStream, WolfUsbCodec>,
    key: Option<&String>,
) -> anyhow::Result<()> {
    let key_bytes = key.map(|k| k.as_bytes());
    let mut auth_nonce = [0u8; 32];
    use rand::RngCore;
    rand::rng().fill_bytes(&mut auth_nonce);

    let auth_proof = if let Some(kb) = key_bytes {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(kb).expect("HMAC accepts any key length");
        mac.update(&auth_nonce);
        mac.update(b"wolfusb-client");
        mac.finalize().into_bytes().to_vec()
    } else {
        Vec::new()
    };

    framed
        .send(Message::Hello(HelloRequest {
            protocol_version: PROTOCOL_VERSION,
            client_name: "wolfusb-mount".to_string(),
            auth_nonce,
            auth_proof,
        }))
        .await
        .context("Failed to send Hello")?;

    let hello_resp = match framed.next().await {
        Some(Ok(Message::HelloResponse(r))) => r,
        Some(Ok(other)) => return Err(anyhow!("Unexpected response: {:?}", other)),
        Some(Err(e)) => return Err(anyhow!("Protocol error: {}", e)),
        None => return Err(anyhow!("Connection closed during handshake")),
    };

    if !hello_resp.auth_accepted {
        return Err(anyhow!(
            "Authentication failed: {}",
            hello_resp.error_message.unwrap_or_default()
        ));
    }

    if let Some(kb) = key_bytes {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(kb).expect("HMAC accepts any key length");
        mac.update(&auth_nonce);
        mac.update(b"wolfusb-server");
        if mac
            .verify_slice(&hello_resp.auth_challenge_response)
            .is_err()
        {
            return Err(anyhow!("Server HMAC verification failed"));
        }
    }
    Ok(())
}

/// Send Bridge request and return the busid the server wants us to use in OP_REQ_IMPORT.
async fn send_bridge(
    framed: &mut Framed<TcpStream, WolfUsbCodec>,
    bus: u8,
    addr: u8,
) -> anyhow::Result<String> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };
    framed
        .send(Message::Bridge(BridgeRequest { device_id }))
        .await
        .context("Failed to send Bridge request")?;

    match framed.next().await {
        Some(Ok(Message::BridgeAccepted(r))) => Ok(r.busid),
        Some(Ok(Message::BridgeRejected(r))) => {
            Err(anyhow!("Server rejected bridge: {}", r.error_message))
        }
        Some(Ok(other)) => Err(anyhow!("Unexpected response: {:?}", other)),
        Some(Err(e)) => Err(anyhow!("Protocol error: {}", e)),
        None => Err(anyhow!("Connection closed during bridge handshake")),
    }
}

/// Parsed OP_REP_IMPORT response
#[derive(Debug)]
struct ImportReply {
    devid: u32,
    speed: u8,
    vendor_id: u16,
    product_id: u16,
}

const USBIP_VERSION: u16 = 0x0111;
const OP_REQ_IMPORT: u16 = 0x8003;
const OP_REP_IMPORT: u16 = 0x0003;

/// Perform USB/IP OP_REQ_IMPORT / OP_REP_IMPORT on the raw TCP stream.
async fn op_req_import(stream: &mut TcpStream, busid: &str) -> anyhow::Result<ImportReply> {
    let mut req = Vec::with_capacity(40);
    req.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    req.extend_from_slice(&OP_REQ_IMPORT.to_be_bytes());
    req.extend_from_slice(&0u32.to_be_bytes());
    let mut busid_buf = [0u8; 32];
    let bb = busid.as_bytes();
    if bb.len() >= 32 {
        return Err(anyhow!("busid too long: {}", busid));
    }
    busid_buf[..bb.len()].copy_from_slice(bb);
    req.extend_from_slice(&busid_buf);

    stream
        .write_all(&req)
        .await
        .context("Failed to send OP_REQ_IMPORT")?;
    stream.flush().await.ok();

    // Reply header: version(2) + code(2) + status(4) = 8 bytes
    let mut hdr = [0u8; 8];
    stream
        .read_exact(&mut hdr)
        .await
        .context("Failed to read OP_REP_IMPORT header")?;
    let code = u16::from_be_bytes([hdr[2], hdr[3]]);
    let status = u32::from_be_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);

    if code != OP_REP_IMPORT {
        return Err(anyhow!("Unexpected USB/IP response code: 0x{:04x}", code));
    }
    if status != 0 {
        return Err(anyhow!(
            "Server rejected OP_REQ_IMPORT with status {} — device '{}' not available",
            status,
            busid
        ));
    }

    // usb_device struct: path(256) + busid(32) + bus_num(4) + dev_num(4) + speed(4)
    // + idVendor(2) + idProduct(2) + ... = 312 bytes
    let mut dev = [0u8; 312];
    stream
        .read_exact(&mut dev)
        .await
        .context("Failed to read OP_REP_IMPORT device data")?;

    let bus_num = u32::from_be_bytes([dev[288], dev[289], dev[290], dev[291]]);
    let dev_num = u32::from_be_bytes([dev[292], dev[293], dev[294], dev[295]]);
    let speed_raw = u32::from_be_bytes([dev[296], dev[297], dev[298], dev[299]]);
    let vendor_id = u16::from_be_bytes([dev[300], dev[301]]);
    let product_id = u16::from_be_bytes([dev[302], dev[303]]);

    let devid = (bus_num << 16) | (dev_num & 0xFFFF);
    Ok(ImportReply {
        devid,
        speed: speed_raw as u8,
        vendor_id,
        product_id,
    })
}
