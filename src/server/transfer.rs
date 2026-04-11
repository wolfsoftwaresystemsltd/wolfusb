// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::time::Duration;

use rusb::{Context, DeviceHandle};

use crate::protocol::messages::*;

/// Maximum buffer size for a single transfer (16 MiB).
const MAX_TRANSFER_BUFFER: u32 = 16 * 1024 * 1024;

/// Minimum timeout to prevent infinite blocking (rusb treats 0 as infinite).
const MIN_TIMEOUT_MS: u64 = 1;

fn safe_timeout(timeout_ms: u64) -> Duration {
    Duration::from_millis(timeout_ms.max(MIN_TIMEOUT_MS))
}

fn capped_length(
    length: u32,
    req_session: u64,
    req_device: crate::protocol::types::DeviceId,
) -> std::result::Result<usize, TransferResponse> {
    if length > MAX_TRANSFER_BUFFER {
        Err(TransferResponse {
            session_id: req_session,
            device_id: req_device,
            success: false,
            data: Vec::new(),
            bytes_transferred: 0,
            error_message: Some(format!(
                "Requested length {length} exceeds maximum {MAX_TRANSFER_BUFFER}"
            )),
        })
    } else {
        Ok(length as usize)
    }
}

pub fn execute_control_transfer(
    handle: &DeviceHandle<Context>,
    req: &ControlTransferRequest,
) -> TransferResponse {
    let timeout = safe_timeout(req.timeout_ms);
    let direction_in = req.request_type & 0x80 != 0;

    if direction_in {
        let mut buf = vec![0u8; req.length as usize];
        match handle.read_control(
            req.request_type,
            req.request,
            req.value,
            req.index,
            &mut buf,
            timeout,
        ) {
            Ok(n) => {
                buf.truncate(n);
                TransferResponse {
                    session_id: req.session_id,
                    device_id: req.device_id,
                    success: true,
                    data: buf,
                    bytes_transferred: n as u32,
                    error_message: None,
                }
            }
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    } else {
        match handle.write_control(
            req.request_type,
            req.request,
            req.value,
            req.index,
            &req.data,
            timeout,
        ) {
            Ok(n) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: true,
                data: Vec::new(),
                bytes_transferred: n as u32,
                error_message: None,
            },
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    }
}

pub fn execute_bulk_transfer(
    handle: &DeviceHandle<Context>,
    req: &BulkTransferRequest,
) -> TransferResponse {
    let timeout = safe_timeout(req.timeout_ms);
    let direction_in = req.endpoint & 0x80 != 0;

    if direction_in {
        let len = match capped_length(req.length, req.session_id, req.device_id) {
            Ok(l) => l,
            Err(resp) => return resp,
        };
        let mut buf = vec![0u8; len];
        match handle.read_bulk(req.endpoint, &mut buf, timeout) {
            Ok(n) => {
                buf.truncate(n);
                TransferResponse {
                    session_id: req.session_id,
                    device_id: req.device_id,
                    success: true,
                    data: buf,
                    bytes_transferred: n as u32,
                    error_message: None,
                }
            }
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    } else {
        match handle.write_bulk(req.endpoint, &req.data, timeout) {
            Ok(n) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: true,
                data: Vec::new(),
                bytes_transferred: n as u32,
                error_message: None,
            },
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    }
}

pub fn execute_interrupt_transfer(
    handle: &DeviceHandle<Context>,
    req: &InterruptTransferRequest,
) -> TransferResponse {
    let timeout = safe_timeout(req.timeout_ms);
    let direction_in = req.endpoint & 0x80 != 0;

    if direction_in {
        let len = match capped_length(req.length, req.session_id, req.device_id) {
            Ok(l) => l,
            Err(resp) => return resp,
        };
        let mut buf = vec![0u8; len];
        match handle.read_interrupt(req.endpoint, &mut buf, timeout) {
            Ok(n) => {
                buf.truncate(n);
                TransferResponse {
                    session_id: req.session_id,
                    device_id: req.device_id,
                    success: true,
                    data: buf,
                    bytes_transferred: n as u32,
                    error_message: None,
                }
            }
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    } else {
        match handle.write_interrupt(req.endpoint, &req.data, timeout) {
            Ok(n) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: true,
                data: Vec::new(),
                bytes_transferred: n as u32,
                error_message: None,
            },
            Err(e) => TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            },
        }
    }
}
