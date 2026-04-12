// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Server-side USB/IP export using the Linux kernel's `usbip_host` module.
//!
//! Why not the Rust `usbip` crate: the crate's URB handlers run in userspace
//! and must re-implement async iso transfers, per-packet status, alt-setting
//! management, composite-device descriptor passthrough, concurrent URB
//! queueing, and more. Upstream is incomplete on most of these — iso transfers
//! are a stub (empty response), class-specific descriptors are dropped on the
//! nusb path, and a single mutex serialises all URBs. That breaks UVC
//! webcams, USB audio, USB TV tuners, and anything with more than one
//! concurrent transfer.
//!
//! Instead: use the battle-tested in-kernel USB/IP server. The kernel already
//! implements the full USB/IP protocol, every transfer type (including iso
//! with proper per-packet handling), alt settings, and concurrent URB queues.
//!
//! The wolfusb server still owns the TCP socket — we just do our auth
//! handshake, then send `OP_REP_IMPORT` and hand the socket fd to the kernel
//! via sysfs. From that moment the kernel reads/writes USB/IP protocol bytes
//! on the socket and our process just keeps the fd alive until disconnect.
//!
//! Sysfs interface (see Linux drivers/usb/usbip/stub_dev.c):
//!   - /sys/bus/usb/drivers/usbip-host/match_busid  (allow listed busid)
//!   - /sys/bus/usb/drivers/usbip-host/bind         (detach + export)
//!   - /sys/bus/usb/drivers/usbip-host/unbind       (release)
//!   - /sys/bus/usb/devices/<busid>/usbip_sockfd    (hand fd to kernel)
//!   - /sys/bus/usb/devices/<busid>/usbip_status    (0=idle 1=avail 2=in use)

use std::fs;
use std::io::Write;
use std::os::fd::RawFd;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

/// Ensure the `usbip_host` kernel module is loaded. `modprobe` is idempotent.
pub fn ensure_module_loaded() -> Result<()> {
    if Path::new("/sys/bus/usb/drivers/usbip-host").is_dir() {
        return Ok(());
    }
    let ok = Command::new("modprobe")
        .arg("usbip_host")
        .status()
        .context("failed to spawn modprobe")?
        .success()
        || Command::new("modprobe")
            .arg("usbip-host")
            .status()
            .ok()
            .map(|s| s.success())
            .unwrap_or(false);
    if !ok || !Path::new("/sys/bus/usb/drivers/usbip-host").is_dir() {
        return Err(anyhow!(
            "failed to load usbip_host kernel module — on Fedora/RHEL install \
             kernel-modules-extra, on Debian/Ubuntu linux-modules-extra-$(uname -r)"
        ));
    }
    Ok(())
}

/// Compute the Linux USB sysfs bus-id for a device at the given bus/port path.
/// Example: bus=1, port_path="2" → "1-2";  bus=1, port_path="2.3" → "1-2.3".
pub fn sysfs_busid(bus: u8, port_path: &str) -> String {
    format!("{}-{}", bus, port_path)
}

/// Get the port path component for a device by enumerating sysfs.
/// rusb gives us bus_number and address (dynamic), but sysfs keys by
/// bus-port_path (static). Find the sysfs entry whose devnum matches.
pub fn port_path_for_address(bus: u8, address: u8) -> Result<String> {
    let entries = fs::read_dir("/sys/bus/usb/devices").context("reading /sys/bus/usb/devices")?;
    for e in entries.flatten() {
        let p = e.path();
        let name = match p.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip interface entries (contain ':') and root hubs (usbN)
        if name.contains(':') || !name.contains('-') {
            continue;
        }
        let busnum = read_sysfs_u32(&p.join("busnum")).unwrap_or(0);
        let devnum = read_sysfs_u32(&p.join("devnum")).unwrap_or(0);
        if busnum == bus as u32 && devnum == address as u32 {
            // name is e.g. "1-2" or "1-2.3"; strip the bus prefix
            if let Some(rest) = name.strip_prefix(&format!("{}-", bus)) {
                return Ok(rest.to_string());
            }
        }
    }
    Err(anyhow!(
        "no sysfs device matches bus={} address={}",
        bus,
        address
    ))
}

fn read_sysfs_u32(p: &Path) -> Option<u32> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn read_sysfs_string(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// Returns true if the device is currently bound to the usbip-host driver.
pub fn is_bound(busid: &str) -> bool {
    let p = format!("/sys/bus/usb/devices/{}/driver", busid);
    match fs::read_link(&p) {
        Ok(target) => target
            .file_name()
            .map(|n| n == "usbip-host")
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Bind the device to usbip-host, detaching its current kernel drivers.
/// Idempotent: if already bound, returns Ok.
///
/// This follows the procedure in Linux kernel's `tools/usb/usbip/libsrc/
/// usbip_host_common.c:usbip_bind_device()`:
///   1. Unbind the device from its current device-level driver (usb).
///   2. Unbind each interface from its interface-level driver.
///   3. Add the busid to usbip-host's match_busid allow-list.
///   4. Trigger kernel driver re-probe via /sys/bus/usb/drivers_probe.
///
/// We can't simply `echo busid > /sys/bus/usb/drivers/usbip-host/bind`
/// because that probes while other drivers still hold the interfaces,
/// returning EBUSY.
pub fn bind(busid: &str) -> Result<()> {
    if is_bound(busid) {
        return Ok(());
    }
    ensure_module_loaded()?;

    // This mirrors `usbip bind -b <busid>` from Linux kernel tools
    // (tools/usb/usbip/libsrc/usbip_host_common.c:usbip_bind_device):
    //   1. Add busid to usbip-host's match_busid allow-list.
    //   2. Unbind the current device-level driver (usually `usb`).
    //      The kernel cascades this into unbinding all interfaces too.
    //   3. Directly bind the device to usbip-host.

    // Step 1: allow-list
    let match_path = "/sys/bus/usb/drivers/usbip-host/match_busid";
    if let Ok(mut f) = fs::OpenOptions::new().write(true).open(match_path) {
        let _ = f.write_all(format!("add {}", busid).as_bytes());
    }

    // Step 2: find the current device-level driver and unbind the device
    // from it. We don't hard-code "usb" because some devices may be bound
    // to a vendor-specific device driver.
    let cur_drv_link = format!("/sys/bus/usb/devices/{}/driver", busid);
    if let Ok(target) = fs::read_link(&cur_drv_link) {
        if let Some(drv_name) = target.file_name().and_then(|n| n.to_str()) {
            if drv_name != "usbip-host" {
                let unbind = format!("/sys/bus/usb/drivers/{}/unbind", drv_name);
                fs::OpenOptions::new()
                    .write(true)
                    .open(&unbind)
                    .and_then(|mut f| f.write_all(busid.as_bytes()))
                    .with_context(|| {
                        format!(
                            "unbinding {} from {} (needed before usbip-host can claim it)",
                            busid, drv_name
                        )
                    })?;
            }
        }
    }

    // Step 3: bind directly to usbip-host.
    let bind_path = "/sys/bus/usb/drivers/usbip-host/bind";
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(bind_path)
        .with_context(|| format!("opening {}", bind_path))?;
    f.write_all(busid.as_bytes())
        .with_context(|| format!("binding {} to usbip-host", busid))?;
    drop(f);

    // Give the kernel a moment.
    for _ in 0..20 {
        if is_bound(busid) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(anyhow!("{} did not bind to usbip-host", busid))
}

/// Unbind the device from usbip-host and let normal drivers (uvcvideo,
/// hid, btusb, snd-usb-audio, etc.) claim it again.
pub fn unbind(busid: &str) -> Result<()> {
    if is_bound(busid) {
        let unbind_path = "/sys/bus/usb/drivers/usbip-host/unbind";
        if let Ok(mut f) = fs::OpenOptions::new().write(true).open(unbind_path) {
            let _ = f.write_all(busid.as_bytes());
        }
    }
    // Remove the busid from the allow-list so normal drivers can claim it.
    let match_path = "/sys/bus/usb/drivers/usbip-host/match_busid";
    if let Ok(mut f) = fs::OpenOptions::new().write(true).open(match_path) {
        let _ = f.write_all(format!("del {}", busid).as_bytes());
    }
    // Ask the kernel to re-probe so normal drivers bind.
    let probe_path = "/sys/bus/usb/drivers_probe";
    if let Ok(mut f) = fs::OpenOptions::new().write(true).open(probe_path) {
        let _ = f.write_all(busid.as_bytes());
    }
    Ok(())
}

/// Information the kernel needs in the 312-byte `usb_device` struct
/// that follows the OP_REP_IMPORT header.
pub struct DeviceInfo {
    pub busid: String,
    pub bus_num: u32,
    pub dev_num: u32,
    pub speed: u32, // kernel USB_SPEED_* value
    pub vendor_id: u16,
    pub product_id: u16,
    pub bcd_device: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub configuration_value: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
}

impl DeviceInfo {
    /// Read device info from sysfs for a bound usbip-host device.
    pub fn from_sysfs(busid: &str) -> Result<Self> {
        let root = Path::new("/sys/bus/usb/devices").join(busid);
        let hex = |p: &Path| -> Result<u32> {
            let s = read_sysfs_string(p).ok_or_else(|| anyhow!("missing {}", p.display()))?;
            u32::from_str_radix(s.trim_start_matches("0x"), 16)
                .with_context(|| format!("parsing {}", p.display()))
        };
        let dec = |p: &Path| -> Result<u32> {
            read_sysfs_u32(p).ok_or_else(|| anyhow!("missing {}", p.display()))
        };
        // /sys/.../speed is a string like "480" (mbps). Translate.
        let speed_str =
            read_sysfs_string(&root.join("speed")).ok_or_else(|| anyhow!("missing speed"))?;
        let speed = match speed_str.as_str() {
            "1.5" => 1,   // LOW
            "12" => 2,    // FULL
            "480" => 3,   // HIGH
            "5000" => 5,  // SUPER
            "10000" => 6, // SUPER_PLUS
            _ => 0,       // UNKNOWN
        };
        Ok(Self {
            busid: busid.to_string(),
            bus_num: dec(&root.join("busnum"))?,
            dev_num: dec(&root.join("devnum"))?,
            speed,
            vendor_id: hex(&root.join("idVendor"))? as u16,
            product_id: hex(&root.join("idProduct"))? as u16,
            bcd_device: {
                // bcdDevice is like "6.01" — convert to 0x0601
                let s = read_sysfs_string(&root.join("bcdDevice")).unwrap_or_default();
                let mut parts = s.split('.');
                let maj: u16 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let min_str = parts.next().unwrap_or("00");
                let min: u16 = min_str.parse().unwrap_or(0);
                (maj << 8) | (min & 0xFF)
            },
            device_class: hex(&root.join("bDeviceClass"))? as u8,
            device_subclass: hex(&root.join("bDeviceSubClass"))? as u8,
            device_protocol: hex(&root.join("bDeviceProtocol"))? as u8,
            configuration_value: dec(&root.join("bConfigurationValue"))? as u8,
            num_configurations: dec(&root.join("bNumConfigurations"))? as u8,
            num_interfaces: dec(&root.join("bNumInterfaces"))? as u8,
        })
    }

    /// Serialise to the 312-byte on-wire usb_device struct.
    pub fn to_wire_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(312);
        // path (256 bytes) — sysfs path, NUL-terminated
        let path = format!("/sys/devices/pci0000:00/usb{}/{}", self.bus_num, self.busid);
        let mut pbuf = [0u8; 256];
        let bytes = path.as_bytes();
        pbuf[..bytes.len().min(256)].copy_from_slice(&bytes[..bytes.len().min(256)]);
        out.extend_from_slice(&pbuf);
        // busid (32 bytes)
        let mut bbuf = [0u8; 32];
        let bytes = self.busid.as_bytes();
        bbuf[..bytes.len().min(32)].copy_from_slice(&bytes[..bytes.len().min(32)]);
        out.extend_from_slice(&bbuf);
        // busnum, devnum, speed, ids, classes
        out.extend_from_slice(&self.bus_num.to_be_bytes());
        out.extend_from_slice(&self.dev_num.to_be_bytes());
        out.extend_from_slice(&self.speed.to_be_bytes());
        out.extend_from_slice(&self.vendor_id.to_be_bytes());
        out.extend_from_slice(&self.product_id.to_be_bytes());
        out.extend_from_slice(&self.bcd_device.to_be_bytes());
        out.push(self.device_class);
        out.push(self.device_subclass);
        out.push(self.device_protocol);
        out.push(self.configuration_value);
        out.push(self.num_configurations);
        out.push(self.num_interfaces);
        debug_assert_eq!(out.len(), 312, "usb_device wire size");
        out
    }
}

/// Hand the socket fd to the kernel. From this point the kernel owns the
/// socket and drives all USB/IP traffic; we must not read/write it ourselves.
pub fn attach_socket(busid: &str, fd: RawFd) -> Result<()> {
    let path = format!("/sys/bus/usb/devices/{}/usbip_sockfd", busid);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path))?;
    f.write_all(fd.to_string().as_bytes())
        .with_context(|| format!("writing fd {} to {}", fd, path))?;
    Ok(())
}

/// Wait until the kernel releases the socket (device detached / client gone).
/// Polls `usbip_status`: 0=idle, 1=avail (bound but no client), 2=in use.
pub fn wait_until_detached(busid: &str) {
    let path = format!("/sys/bus/usb/devices/{}/usbip_status", busid);
    loop {
        let status = read_sysfs_u32(Path::new(&path)).unwrap_or(0);
        if status != 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
