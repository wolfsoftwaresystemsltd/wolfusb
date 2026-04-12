// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Client `mount` command — creates a virtual USB device on the local
//! machine that proxies transfers to a remote wolfusb server via vhci_hcd.
//!
//! Flow:
//!   1. Open raw TCP connection to wolfusb server (no TLS — vhci_hcd needs
//!      plain socket FD; auth via HMAC is still performed)
//!   2. Send Hello / verify HelloResponse
//!   3. Send Bridge request; wait for BridgeAccepted
//!   4. Hand the raw TCP socket FD to /sys/devices/platform/vhci_hcd.0/attach
//!   5. Block until SIGINT or kernel detaches the port
//!   6. On exit: write port number to vhci_hcd detach; close socket

use std::os::fd::IntoRawFd;
use std::time::Duration;

use anyhow::{anyhow, Context};
use futures::{SinkExt, StreamExt};
use log::{info, warn};
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
    // Ensure vhci_hcd is loaded
    vhci::ensure_module_loaded()
        .context("vhci_hcd kernel module unavailable — USB virtualization won't work")?;

    info!("Connecting to {} (plain TCP — TLS not supported for mount mode)", server);
    let tcp_stream = TcpStream::connect(server).await
        .context("Failed to connect to wolfusb server")?;

    // Configure TCP for low-latency USB — disable Nagle
    tcp_stream.set_nodelay(true).ok();

    // Drop into framed for the Hello/Bridge handshake
    let mut framed = Framed::new(tcp_stream, WolfUsbCodec);

    // Hello with HMAC auth
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

    framed.send(Message::Hello(HelloRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "wolfusb-mount".to_string(),
        auth_nonce,
        auth_proof,
    })).await.context("Failed to send Hello")?;

    let hello_resp = match framed.next().await {
        Some(Ok(Message::HelloResponse(r))) => r,
        Some(Ok(other)) => return Err(anyhow!("Unexpected response: {:?}", other)),
        Some(Err(e)) => return Err(anyhow!("Protocol error: {}", e)),
        None => return Err(anyhow!("Connection closed during handshake")),
    };

    if !hello_resp.auth_accepted {
        return Err(anyhow!("Authentication failed: {}",
            hello_resp.error_message.unwrap_or_default()));
    }

    // Verify server HMAC if we sent a key
    if let Some(kb) = key_bytes {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(kb).expect("HMAC accepts any key length");
        mac.update(&auth_nonce);
        mac.update(b"wolfusb-server");
        if mac.verify_slice(&hello_resp.auth_challenge_response).is_err() {
            return Err(anyhow!("Server HMAC verification failed"));
        }
    }

    // Send Bridge request
    let device_id = DeviceId { bus_number: bus, address: addr };
    framed.send(Message::Bridge(BridgeRequest { device_id })).await
        .context("Failed to send Bridge request")?;

    let bridge_resp = match framed.next().await {
        Some(Ok(Message::BridgeAccepted(r))) => r,
        Some(Ok(Message::BridgeRejected(r))) => {
            return Err(anyhow!("Server rejected bridge: {}", r.error_message));
        }
        Some(Ok(other)) => return Err(anyhow!("Unexpected response: {:?}", other)),
        Some(Err(e)) => return Err(anyhow!("Protocol error: {}", e)),
        None => return Err(anyhow!("Connection closed during bridge handshake")),
    };

    info!(
        "Bridge accepted: device {:04x}:{:04x} (bcd={:04x}, class={:02x}), speed={}",
        bridge_resp.vendor_id, bridge_resp.product_id, bridge_resp.bcd_device,
        bridge_resp.device_class, bridge_resp.speed
    );

    // Extract raw TCP stream from framed and hand FD to vhci_hcd.
    // From this point on, the kernel reads USB/IP protocol from the socket.
    let tcp_stream = framed.into_inner();
    let std_stream = tcp_stream.into_std()
        .context("Failed to convert TCP stream to std")?;
    // Set blocking mode — the kernel expects a blocking socket
    std_stream.set_nonblocking(false).ok();

    let fd = std_stream.into_raw_fd();
    let speed = match bridge_resp.speed {
        1 => Speed::Low,
        2 => Speed::Full,
        3 => Speed::High,
        4 => Speed::Wireless,
        5 => Speed::Super,
        6 => Speed::SuperPlus,
        _ => Speed::High,
    };

    // Wrap fd in a small helper so Drop semantics work cleanly
    struct RawFdOwner(std::os::fd::RawFd);
    impl std::os::fd::AsRawFd for RawFdOwner {
        fn as_raw_fd(&self) -> std::os::fd::RawFd { self.0 }
    }
    let owner = RawFdOwner(fd);

    let port = vhci::attach(&owner, bridge_resp.devid, speed)
        .context("Failed to attach to vhci_hcd (check permissions; need CAP_SYS_ADMIN)")?;

    println!("Attached device as virtual USB on vhci_hcd port {}", port);
    println!("Device {:04x}:{:04x} is now visible in lsusb. Press Ctrl-C to detach.",
        bridge_resp.vendor_id, bridge_resp.product_id);

    // Wait for SIGINT. While we wait, the kernel owns the socket and talks
    // USB/IP protocol to the remote wolfusb server directly.
    tokio::signal::ctrl_c().await.ok();

    println!("\nDetaching port {}...", port);
    if let Err(e) = vhci::detach(port) {
        warn!("Failed to detach port {}: {}", port, e);
    }

    // Give the kernel a moment to close the socket
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}
