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
//! We keep our wolfusb auth layer in front; the usbip crate only ever sees
//! authenticated connections.

use std::sync::Arc;

use log::{info, warn};
use nusb::MaybeFuture;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::types::DeviceId;

/// Run the USB/IP bridge loop on an authenticated stream.
///
/// `target` identifies the specific USB device the client requested. We open
/// that device via nusb and only expose it to the USB/IP handler so the client
/// can only access the requested device, not all local USB devices.
pub async fn run_bridge<S>(stream: S, target: DeviceId)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    info!("Bridge: starting USB/IP handler for bus={} addr={}",
        target.bus_number, target.address);

    // Find the matching nusb device
    let all_devices: Vec<nusb::DeviceInfo> = match nusb::list_devices().wait() {
        Ok(it) => it.collect(),
        Err(e) => {
            warn!("Bridge: failed to list USB devices: {}", e);
            return;
        }
    };

    let matching: Vec<nusb::DeviceInfo> = all_devices.into_iter()
        .filter(|d| d.busnum() == target.bus_number
                 && d.device_address() == target.address)
        .collect();

    if matching.is_empty() {
        warn!("Bridge: no USB device found at bus={} addr={}",
            target.bus_number, target.address);
        return;
    }

    let usb_devices = usbip::UsbIpServer::with_nusb_devices(matching);
    if usb_devices.is_empty() {
        warn!("Bridge: failed to open USB device for bridging");
        return;
    }
    let server = Arc::new(usbip::UsbIpServer::new_simulated(usb_devices));

    let mut stream = stream;
    match usbip::handler(&mut stream, server).await {
        Ok(()) => info!("Bridge: USB/IP session ended cleanly"),
        Err(e) => warn!("Bridge: USB/IP session error: {}", e),
    }
}
