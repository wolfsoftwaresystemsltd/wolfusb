// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Server-side bridge mode — after a client has authenticated via wolfusb's
//! Hello+HMAC handshake and sent a Bridge request, we delegate the raw stream
//! to the `usbip` crate's server handler. That crate implements the full
//! Linux USB/IP kernel protocol including:
//!   - OP_REQ_IMPORT / OP_REP_IMPORT (device selection)
//!   - USBIP_CMD_SUBMIT / USBIP_RET_SUBMIT (async URB handling)
//!   - USBIP_CMD_UNLINK / USBIP_RET_UNLINK (proper cancellation)
//!   - Isochronous transfers (for webcams, audio devices, etc.)
//!
//! We use the rusb backend (`new_from_host_with_filter`) because the nusb
//! path in the usbip crate doesn't populate class-specific descriptors,
//! breaking UVC webcams, USB audio, and other composite devices.

use std::sync::Arc;

use log::{info, warn};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::types::DeviceId;

/// Run the USB/IP bridge loop on an authenticated stream.
///
/// `target` identifies the specific USB device the client requested. We expose
/// only that one device via the usbip crate so the client can't access other
/// local USB devices.
pub async fn run_bridge<S>(stream: S, target: DeviceId)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    info!("Bridge: starting USB/IP handler for bus={} addr={}",
        target.bus_number, target.address);

    // Build a UsbIpServer exposing ONLY the requested device.
    // Use the rusb-based constructor so class-specific descriptors (UVC, UAC, etc.)
    // are properly populated in the configuration descriptor.
    let target_bus = target.bus_number;
    let target_addr = target.address;
    let server = usbip::UsbIpServer::new_from_host_with_filter(move |d| {
        d.bus_number() == target_bus && d.address() == target_addr
    });

    let server = Arc::new(server);

    let mut stream = stream;
    match usbip::handler(&mut stream, server).await {
        Ok(()) => info!("Bridge: USB/IP session ended cleanly"),
        Err(e) => warn!("Bridge: USB/IP session error: {}", e),
    }
}
