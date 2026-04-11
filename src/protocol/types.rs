// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum UsbSpeed {
    Unknown,
    Low,
    Full,
    High,
    Super,
    SuperPlus,
}

impl From<rusb::Speed> for UsbSpeed {
    fn from(s: rusb::Speed) -> Self {
        match s {
            rusb::Speed::Low => UsbSpeed::Low,
            rusb::Speed::Full => UsbSpeed::Full,
            rusb::Speed::High => UsbSpeed::High,
            rusb::Speed::Super => UsbSpeed::Super,
            rusb::Speed::SuperPlus => UsbSpeed::SuperPlus,
            _ => UsbSpeed::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum UsbTransferType {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

impl From<rusb::TransferType> for UsbTransferType {
    fn from(t: rusb::TransferType) -> Self {
        match t {
            rusb::TransferType::Control => UsbTransferType::Control,
            rusb::TransferType::Isochronous => UsbTransferType::Isochronous,
            rusb::TransferType::Bulk => UsbTransferType::Bulk,
            rusb::TransferType::Interrupt => UsbTransferType::Interrupt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum UsbDirection {
    In,
    Out,
}

impl From<rusb::Direction> for UsbDirection {
    fn from(d: rusb::Direction) -> Self {
        match d {
            rusb::Direction::In => UsbDirection::In,
            rusb::Direction::Out => UsbDirection::Out,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub struct DeviceId {
    pub bus_number: u8,
    pub address: u8,
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.bus_number, self.address)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class_code: u8,
    pub sub_class_code: u8,
    pub protocol_code: u8,
    pub max_packet_size_ep0: u8,
    pub num_configurations: u8,
    pub usb_version: (u8, u8, u8),
    pub device_version: (u8, u8, u8),
    pub speed: UsbSpeed,
    pub port_number: u8,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct EndpointInfo {
    pub address: u8,
    pub number: u8,
    pub direction: UsbDirection,
    pub transfer_type: UsbTransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterfaceInfo {
    pub interface_number: u8,
    pub setting_number: u8,
    pub class_code: u8,
    pub sub_class_code: u8,
    pub protocol_code: u8,
    pub num_endpoints: u8,
    pub description: Option<String>,
    pub endpoints: Vec<EndpointInfo>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ConfigurationInfo {
    pub number: u8,
    pub num_interfaces: u8,
    pub max_power_ma: u16,
    pub self_powered: bool,
    pub remote_wakeup: bool,
    pub description: Option<String>,
    pub interfaces: Vec<InterfaceInfo>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DeviceDescriptorTree {
    pub device: DeviceInfo,
    pub configurations: Vec<ConfigurationInfo>,
}
