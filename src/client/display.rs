// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use crate::protocol::messages::TransferResponse;
use crate::protocol::types::*;

pub fn print_device_list(devices: &[DeviceInfo]) {
    if devices.is_empty() {
        println!("No USB devices found.");
        return;
    }

    println!(
        "{:<7} {:<9} {:<6} {:<10} {:<24} {:<24} Serial",
        "Bus:Addr", "VID:PID", "Speed", "Class", "Manufacturer", "Product"
    );
    println!("{}", "-".repeat(100));

    for dev in devices {
        let speed = match dev.speed {
            UsbSpeed::Unknown => "?",
            UsbSpeed::Low => "1.5M",
            UsbSpeed::Full => "12M",
            UsbSpeed::High => "480M",
            UsbSpeed::Super => "5G",
            UsbSpeed::SuperPlus => "10G",
        };

        let class = format_class(dev.class_code, dev.sub_class_code);

        println!(
            "{:<7} {:04x}:{:04x} {:<6} {:<10} {:<24} {:<24} {}",
            format!("{}:{}", dev.device_id.bus_number, dev.device_id.address),
            dev.vendor_id,
            dev.product_id,
            speed,
            class,
            dev.manufacturer.as_deref().unwrap_or("-"),
            dev.product.as_deref().unwrap_or("-"),
            dev.serial_number.as_deref().unwrap_or("-"),
        );
    }
}

pub fn print_descriptor_tree(tree: &DeviceDescriptorTree) {
    let dev = &tree.device;
    println!("Device: {}:{}", dev.device_id.bus_number, dev.device_id.address);
    println!("  VID:PID:        {:04x}:{:04x}", dev.vendor_id, dev.product_id);
    println!(
        "  USB Version:    {}.{}.{}",
        dev.usb_version.0, dev.usb_version.1, dev.usb_version.2
    );
    println!(
        "  Device Version: {}.{}.{}",
        dev.device_version.0, dev.device_version.1, dev.device_version.2
    );
    println!(
        "  Class:          {} ({:02x}:{:02x}:{:02x})",
        format_class(dev.class_code, dev.sub_class_code),
        dev.class_code,
        dev.sub_class_code,
        dev.protocol_code
    );
    println!("  Max Packet EP0: {}", dev.max_packet_size_ep0);
    println!("  Speed:          {:?}", dev.speed);
    if let Some(ref m) = dev.manufacturer {
        println!("  Manufacturer:   {m}");
    }
    if let Some(ref p) = dev.product {
        println!("  Product:        {p}");
    }
    if let Some(ref s) = dev.serial_number {
        println!("  Serial:         {s}");
    }
    println!("  Configurations: {}", dev.num_configurations);

    for config in &tree.configurations {
        println!();
        println!("  Configuration {}:", config.number);
        println!("    Interfaces:    {}", config.num_interfaces);
        println!("    Max Power:     {} mA", config.max_power_ma);
        println!("    Self Powered:  {}", config.self_powered);
        println!("    Remote Wakeup: {}", config.remote_wakeup);

        for iface in &config.interfaces {
            println!();
            println!(
                "    Interface {} (alt setting {}):",
                iface.interface_number, iface.setting_number
            );
            println!(
                "      Class:     {} ({:02x}:{:02x}:{:02x})",
                format_class(iface.class_code, iface.sub_class_code),
                iface.class_code,
                iface.sub_class_code,
                iface.protocol_code
            );
            println!("      Endpoints: {}", iface.num_endpoints);

            for ep in &iface.endpoints {
                let dir = match ep.direction {
                    UsbDirection::In => "IN",
                    UsbDirection::Out => "OUT",
                };
                let transfer = match ep.transfer_type {
                    UsbTransferType::Control => "Control",
                    UsbTransferType::Isochronous => "Isochronous",
                    UsbTransferType::Bulk => "Bulk",
                    UsbTransferType::Interrupt => "Interrupt",
                };
                println!(
                    "        EP 0x{:02x}: {} {} (max {} bytes, interval {})",
                    ep.address, dir, transfer, ep.max_packet_size, ep.interval
                );
            }
        }
    }
}

pub fn print_transfer_result(resp: &TransferResponse) {
    if resp.success {
        println!("Transfer OK: {} bytes transferred", resp.bytes_transferred);
        if !resp.data.is_empty() {
            println!("{}", hex_dump(&resp.data));
        }
    } else {
        println!(
            "Transfer FAILED: {}",
            resp.error_message.as_deref().unwrap_or("Unknown error")
        );
    }
}

pub fn hex_dump(data: &[u8]) -> String {
    let mut result = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let offset = i * 16;
        result.push_str(&format!("{offset:08x}  "));

        // Hex bytes
        for (j, byte) in chunk.iter().enumerate() {
            if j == 8 {
                result.push(' ');
            }
            result.push_str(&format!("{byte:02x} "));
        }

        // Padding for short lines
        let remaining = 16 - chunk.len();
        for j in 0..remaining {
            if chunk.len() + j == 8 {
                result.push(' ');
            }
            result.push_str("   ");
        }

        result.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                result.push(*byte as char);
            } else {
                result.push('.');
            }
        }
        result.push('|');
        result.push('\n');
    }
    result
}

fn format_class(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x00, _) => "Per-Iface",
        (0x01, _) => "Audio",
        (0x02, _) => "CDC",
        (0x03, _) => "HID",
        (0x05, _) => "Physical",
        (0x06, _) => "Image",
        (0x07, _) => "Printer",
        (0x08, _) => "Storage",
        (0x09, _) => "Hub",
        (0x0A, _) => "CDC-Data",
        (0x0B, _) => "SmartCard",
        (0x0D, _) => "ContentSec",
        (0x0E, _) => "Video",
        (0x0F, _) => "Healthcare",
        (0x10, _) => "AV",
        (0x11, _) => "Billboard",
        (0xDC, _) => "Diagnostic",
        (0xE0, _) => "Wireless",
        (0xEF, _) => "Misc",
        (0xFE, _) => "App-Spec",
        (0xFF, _) => "Vendor",
        _ => "Unknown",
    }
}

pub fn parse_hex_data(hex_str: &str) -> anyhow::Result<Vec<u8>> {
    let hex_str = hex_str.replace([' ', ':'], "");
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(&hex_str);

    if !hex_str.len().is_multiple_of(2) {
        anyhow::bail!("Hex string must have even number of characters");
    }

    (0..hex_str.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex_str[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex at position {i}: {e}"))
        })
        .collect()
}
