// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::time::Duration;

use rusb::{Context, DeviceHandle};

use crate::protocol::messages::*;

pub fn execute_control_transfer(
    handle: &DeviceHandle<Context>,
    req: &ControlTransferRequest,
) -> TransferResponse {
    let timeout = Duration::from_millis(req.timeout_ms);
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
    let timeout = Duration::from_millis(req.timeout_ms);
    let direction_in = req.endpoint & 0x80 != 0;

    if direction_in {
        let mut buf = vec![0u8; req.length as usize];
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
    let timeout = Duration::from_millis(req.timeout_ms);
    let direction_in = req.endpoint & 0x80 != 0;

    if direction_in {
        let mut buf = vec![0u8; req.length as usize];
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
