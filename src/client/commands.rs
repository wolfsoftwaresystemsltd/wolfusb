// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

#![allow(clippy::too_many_arguments)]

use crate::client::display;
use crate::client::session::Session;
use crate::protocol::messages::*;
use crate::protocol::types::DeviceId;

pub async fn cmd_list(session: &mut Session, json: bool) -> anyhow::Result<()> {
    let devices = session.list_devices().await?;
    if json {
        println!("{}", serde_json::to_string(&devices)?);
    } else {
        display::print_device_list(&devices);
    }
    Ok(())
}

pub async fn cmd_info(session: &mut Session, bus: u8, addr: u8) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };
    let tree = session.get_descriptors(device_id).await?;
    display::print_descriptor_tree(&tree);
    Ok(())
}

pub async fn cmd_attach(session: &mut Session, bus: u8, addr: u8) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };
    let session_id = session.attach(device_id).await?;
    println!("Attached to {device_id}, session_id = {session_id}");
    Ok(())
}

pub async fn cmd_detach(
    session: &mut Session,
    bus: u8,
    addr: u8,
    session_id: u64,
) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };
    session.detach(device_id, session_id).await?;
    println!("Detached from {device_id}");
    Ok(())
}

pub async fn cmd_control(
    session: &mut Session,
    session_id: u64,
    bus: u8,
    addr: u8,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    data: Option<&str>,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };

    let data_bytes = match data {
        Some(hex) => display::parse_hex_data(hex)?,
        None => Vec::new(),
    };

    let req = ControlTransferRequest {
        session_id,
        device_id,
        request_type,
        request,
        value,
        index,
        data: data_bytes,
        length,
        timeout_ms,
    };

    let resp = session.control_transfer(req).await?;
    display::print_transfer_result(&resp);
    Ok(())
}

pub async fn cmd_bulk(
    session: &mut Session,
    session_id: u64,
    bus: u8,
    addr: u8,
    endpoint: u8,
    length: u32,
    data: Option<&str>,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };

    let data_bytes = match data {
        Some(hex) => display::parse_hex_data(hex)?,
        None => Vec::new(),
    };

    let req = BulkTransferRequest {
        session_id,
        device_id,
        endpoint,
        data: data_bytes,
        length,
        timeout_ms,
    };

    let resp = session.bulk_transfer(req).await?;
    display::print_transfer_result(&resp);
    Ok(())
}

pub async fn cmd_interrupt(
    session: &mut Session,
    session_id: u64,
    bus: u8,
    addr: u8,
    endpoint: u8,
    length: u32,
    data: Option<&str>,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let device_id = DeviceId {
        bus_number: bus,
        address: addr,
    };

    let data_bytes = match data {
        Some(hex) => display::parse_hex_data(hex)?,
        None => Vec::new(),
    };

    let req = InterruptTransferRequest {
        session_id,
        device_id,
        endpoint,
        data: data_bytes,
        length,
        timeout_ms,
    };

    let resp = session.interrupt_transfer(req).await?;
    display::print_transfer_result(&resp);
    Ok(())
}
