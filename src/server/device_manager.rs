// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use log::{debug, info, warn};
use rusb::{Context, DeviceHandle, UsbContext};

use crate::error::{Result, WolfUsbError};
use crate::protocol::types::*;

struct AttachmentInfo {
    handle: DeviceHandle<Context>,
    detached_kernel_drivers: Vec<u8>,
    claimed_interfaces: Vec<u8>,
    client_addr: SocketAddr,
}

pub struct DeviceManager {
    context: Context,
    attachments: HashMap<DeviceId, AttachmentInfo>,
    next_session_id: u64,
    session_to_device: HashMap<u64, DeviceId>,
}

impl DeviceManager {
    pub fn new() -> Result<Self> {
        let context = Context::new()?;
        Ok(Self {
            context,
            attachments: HashMap::new(),
            next_session_id: 1,
            session_to_device: HashMap::new(),
        })
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let devices = self.context.devices()?;
        let mut result = Vec::new();

        for device in devices.iter() {
            let desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(e) => {
                    debug!("Skipping device: cannot read descriptor: {e}");
                    continue;
                }
            };

            let device_id = DeviceId {
                bus_number: device.bus_number(),
                address: device.address(),
            };

            // Try to read string descriptors (requires opening the device)
            let (manufacturer, product, serial_number) = match device.open() {
                Ok(handle) => {
                    let timeout = Duration::from_millis(500);
                    let mfr = handle.read_manufacturer_string_ascii(&desc).ok();
                    let prod = handle.read_product_string_ascii(&desc).ok();
                    let serial = handle.read_serial_number_string_ascii(&desc).ok();
                    let _ = timeout; // used conceptually for context
                    (mfr, prod, serial)
                }
                Err(_) => (None, None, None),
            };

            let version = desc.usb_version();
            let dev_version = desc.device_version();

            result.push(DeviceInfo {
                device_id,
                vendor_id: desc.vendor_id(),
                product_id: desc.product_id(),
                class_code: desc.class_code(),
                sub_class_code: desc.sub_class_code(),
                protocol_code: desc.protocol_code(),
                max_packet_size_ep0: desc.max_packet_size(),
                num_configurations: desc.num_configurations(),
                usb_version: (version.major(), version.minor(), version.sub_minor()),
                device_version: (
                    dev_version.major(),
                    dev_version.minor(),
                    dev_version.sub_minor(),
                ),
                speed: device.speed().into(),
                port_number: device.port_number(),
                manufacturer,
                product,
                serial_number,
            });
        }

        Ok(result)
    }

    pub fn get_descriptors(&self, device_id: DeviceId) -> Result<DeviceDescriptorTree> {
        let device = self.find_device(device_id)?;
        let desc = device.device_descriptor()?;

        let (manufacturer, product, serial_number) = match device.open() {
            Ok(handle) => {
                let mfr = handle.read_manufacturer_string_ascii(&desc).ok();
                let prod = handle.read_product_string_ascii(&desc).ok();
                let serial = handle.read_serial_number_string_ascii(&desc).ok();
                (mfr, prod, serial)
            }
            Err(_) => (None, None, None),
        };

        let version = desc.usb_version();
        let dev_version = desc.device_version();

        let device_info = DeviceInfo {
            device_id,
            vendor_id: desc.vendor_id(),
            product_id: desc.product_id(),
            class_code: desc.class_code(),
            sub_class_code: desc.sub_class_code(),
            protocol_code: desc.protocol_code(),
            max_packet_size_ep0: desc.max_packet_size(),
            num_configurations: desc.num_configurations(),
            usb_version: (version.major(), version.minor(), version.sub_minor()),
            device_version: (
                dev_version.major(),
                dev_version.minor(),
                dev_version.sub_minor(),
            ),
            speed: device.speed().into(),
            port_number: device.port_number(),
            manufacturer,
            product,
            serial_number,
        };

        let mut configurations = Vec::new();
        for cfg_idx in 0..desc.num_configurations() {
            let config = match device.config_descriptor(cfg_idx) {
                Ok(c) => c,
                Err(e) => {
                    debug!("Cannot read config descriptor {cfg_idx}: {e}");
                    continue;
                }
            };

            let mut interfaces = Vec::new();
            for iface in config.interfaces() {
                for setting in iface.descriptors() {
                    let mut endpoints = Vec::new();
                    for ep in setting.endpoint_descriptors() {
                        endpoints.push(EndpointInfo {
                            address: ep.address(),
                            number: ep.number(),
                            direction: ep.direction().into(),
                            transfer_type: ep.transfer_type().into(),
                            max_packet_size: ep.max_packet_size(),
                            interval: ep.interval(),
                        });
                    }

                    interfaces.push(InterfaceInfo {
                        interface_number: setting.interface_number(),
                        setting_number: setting.setting_number(),
                        class_code: setting.class_code(),
                        sub_class_code: setting.sub_class_code(),
                        protocol_code: setting.protocol_code(),
                        num_endpoints: setting.num_endpoints(),
                        description: None, // Would need open handle + string desc index
                        endpoints,
                    });
                }
            }

            configurations.push(ConfigurationInfo {
                number: config.number(),
                num_interfaces: config.num_interfaces(),
                max_power_ma: config.max_power() as u16 * 2,
                self_powered: false,
                remote_wakeup: false,
                description: None,
                interfaces,
            });
        }

        Ok(DeviceDescriptorTree {
            device: device_info,
            configurations,
        })
    }

    pub fn attach(&mut self, device_id: DeviceId, client_addr: SocketAddr) -> Result<u64> {
        if self.attachments.contains_key(&device_id) {
            return Err(WolfUsbError::DeviceAlreadyAttached);
        }

        let device = self.find_device(device_id)?;
        let handle = device.open()?;

        // Try to detach kernel drivers on all interfaces
        let desc = device.device_descriptor()?;
        let mut detached_drivers = Vec::new();

        if let Ok(config) = device.active_config_descriptor() {
            for iface in config.interfaces() {
                let iface_num = iface.number();
                match handle.detach_kernel_driver(iface_num) {
                    Ok(()) => {
                        info!("Detached kernel driver from {device_id} interface {iface_num}");
                        detached_drivers.push(iface_num);
                    }
                    Err(rusb::Error::NotSupported) => {
                        // macOS/Windows: kernel driver detach not supported
                    }
                    Err(rusb::Error::NotFound) => {
                        // No kernel driver was attached
                    }
                    Err(e) => {
                        warn!(
                            "Failed to detach kernel driver from {device_id} interface {iface_num}: {e}"
                        );
                    }
                }
            }
        }
        let _ = desc; // suppress unused warning

        let session_id = self.next_session_id;
        self.next_session_id += 1;

        self.session_to_device.insert(session_id, device_id);
        self.attachments.insert(
            device_id,
            AttachmentInfo {
                handle,
                detached_kernel_drivers: detached_drivers,
                claimed_interfaces: Vec::new(),
                client_addr,
            },
        );

        info!("Attached {device_id} for {client_addr} (session {session_id})");
        Ok(session_id)
    }

    pub fn detach(&mut self, device_id: DeviceId, session_id: u64) -> Result<()> {
        let attachment = self
            .attachments
            .remove(&device_id)
            .ok_or(WolfUsbError::DeviceNotAttached)?;

        // Release claimed interfaces
        for iface_num in &attachment.claimed_interfaces {
            if let Err(e) = attachment.handle.release_interface(*iface_num) {
                warn!("Failed to release interface {iface_num} on {device_id}: {e}");
            }
        }

        // Reattach kernel drivers
        for iface_num in &attachment.detached_kernel_drivers {
            if let Err(e) = attachment.handle.attach_kernel_driver(*iface_num) {
                warn!("Failed to reattach kernel driver on {device_id} interface {iface_num}: {e}");
            }
        }

        self.session_to_device.remove(&session_id);
        info!(
            "Detached {device_id} from {} (session {session_id})",
            attachment.client_addr
        );
        Ok(())
    }

    pub fn detach_all_for_sessions(&mut self, session_ids: &[u64]) {
        for &session_id in session_ids {
            if let Some(&device_id) = self.session_to_device.get(&session_id)
                && let Err(e) = self.detach(device_id, session_id)
            {
                warn!("Cleanup detach failed for session {session_id}: {e}");
            }
        }
    }

    pub fn claim_interface(&mut self, device_id: DeviceId, interface_number: u8) -> Result<()> {
        let attachment = self
            .attachments
            .get_mut(&device_id)
            .ok_or(WolfUsbError::DeviceNotAttached)?;

        attachment.handle.claim_interface(interface_number)?;
        attachment.claimed_interfaces.push(interface_number);
        info!("Claimed interface {interface_number} on {device_id}");
        Ok(())
    }

    pub fn release_interface(&mut self, device_id: DeviceId, interface_number: u8) -> Result<()> {
        let attachment = self
            .attachments
            .get_mut(&device_id)
            .ok_or(WolfUsbError::DeviceNotAttached)?;

        attachment.handle.release_interface(interface_number)?;
        attachment
            .claimed_interfaces
            .retain(|&n| n != interface_number);
        info!("Released interface {interface_number} on {device_id}");
        Ok(())
    }

    pub fn set_configuration(&mut self, device_id: DeviceId, configuration: u8) -> Result<()> {
        let attachment = self
            .attachments
            .get_mut(&device_id)
            .ok_or(WolfUsbError::DeviceNotAttached)?;

        attachment.handle.set_active_configuration(configuration)?;
        info!("Set configuration {configuration} on {device_id}");
        Ok(())
    }

    pub fn get_handle(&self, device_id: DeviceId) -> Result<&DeviceHandle<Context>> {
        self.attachments
            .get(&device_id)
            .map(|a| &a.handle)
            .ok_or(WolfUsbError::DeviceNotAttached)
    }

    pub fn validate_session(&self, session_id: u64, device_id: DeviceId) -> Result<()> {
        match self.session_to_device.get(&session_id) {
            Some(&id) if id == device_id => Ok(()),
            Some(_) => Err(WolfUsbError::InvalidSession(session_id)),
            None => Err(WolfUsbError::InvalidSession(session_id)),
        }
    }

    fn find_device(&self, device_id: DeviceId) -> Result<rusb::Device<Context>> {
        let devices = self.context.devices()?;
        devices
            .iter()
            .find(|d| d.bus_number() == device_id.bus_number && d.address() == device_id.address)
            .ok_or(WolfUsbError::DeviceNotFound {
                bus: device_id.bus_number,
                addr: device_id.address,
            })
    }
}
