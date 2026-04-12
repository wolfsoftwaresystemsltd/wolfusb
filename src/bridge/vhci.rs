// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Interface to the Linux kernel's vhci_hcd driver via sysfs.
//!
//! vhci_hcd creates "virtual" USB host controllers. By writing a socket FD
//! and device info to sysfs attach, the kernel treats whatever USB/IP protocol
//! appears on that socket as a real USB device.
//!
//! Sysfs layout (single vhci_hcd.0 controller):
//! - `/sys/devices/platform/vhci_hcd.0/status` — list of ports (free/in-use)
//! - `/sys/devices/platform/vhci_hcd.0/attach` — write `<port> <sockfd> <devid> <speed>`
//! - `/sys/devices/platform/vhci_hcd.0/detach` — write `<port>` to release
//!
//! Newer kernels may have `vhci_hcd.N` for multiple controllers — we try each.

use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;

/// USB speed codes used by vhci_hcd (match Linux USB core definitions)
#[repr(u8)]
pub enum Speed {
    Low = 1,
    Full = 2,
    High = 3,
    Wireless = 4,
    Super = 5,
    SuperPlus = 6,
}

impl Speed {
    pub fn from_rusb(s: rusb::Speed) -> Self {
        match s {
            rusb::Speed::Low => Speed::Low,
            rusb::Speed::Full => Speed::Full,
            rusb::Speed::High => Speed::High,
            rusb::Speed::Super => Speed::Super,
            rusb::Speed::SuperPlus => Speed::SuperPlus,
            _ => Speed::High, // reasonable default for unknown
        }
    }
    pub fn as_u8(&self) -> u8 {
        match self {
            Speed::Low => 1,
            Speed::Full => 2,
            Speed::High => 3,
            Speed::Wireless => 4,
            Speed::Super => 5,
            Speed::SuperPlus => 6,
        }
    }
}

/// Find the first available vhci_hcd controller path
fn find_vhci_path() -> io::Result<String> {
    for i in 0..32 {
        let p = format!("/sys/devices/platform/vhci_hcd.{}", i);
        if Path::new(&p).is_dir() {
            return Ok(p);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "vhci_hcd not found — ensure kernel module is loaded (modprobe vhci-hcd)",
    ))
}

/// Ensure vhci-hcd kernel module is loaded. No-op if already loaded.
pub fn ensure_module_loaded() -> io::Result<()> {
    if Path::new("/sys/devices/platform/vhci_hcd.0").is_dir() {
        return Ok(());
    }
    let status = Command::new("modprobe").arg("vhci-hcd").status()?;
    if !status.success() {
        // Try alternative name
        let _ = Command::new("modprobe").arg("vhci_hcd").status();
    }
    if !Path::new("/sys/devices/platform/vhci_hcd.0").is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Failed to load vhci-hcd kernel module. Install with: \
             dnf install kernel-modules-extra (Fedora/RHEL) \
             or apt install linux-modules-extra-$(uname -r) (Debian/Ubuntu)",
        ));
    }
    Ok(())
}

/// Parse vhci_hcd status file to find the first free port.
/// Status format (one line per port), header row first:
///   hub port sta spd dev      sockfd local_busid
///   hs  0000 004 000 00000000 000000 0-0
///   ss  0008 004 000 00000000 000000 0-0
/// Columns: hub (hs/ss), port, status (004=free), speed, dev, sockfd, busid
/// Status code 004 = VDEV_ST_NULL (free), others = busy.
/// For bridge use, we prefer HS (high-speed 2.0) ports since most USB devices
/// are happy there; SS ports are for USB 3.x.
fn find_free_port(vhci_path: &str) -> io::Result<u32> {
    let status_path = format!("{}/status", vhci_path);
    let content = fs::read_to_string(&status_path)?;
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        // Skip header row (starts with "hub")
        if fields[0] == "hub" {
            continue;
        }
        // fields[0] = hub type (hs/ss), fields[1] = port, fields[2] = status
        let port: u32 = match fields[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let status: u32 = match fields[2].parse() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if status == 4 {
            return Ok(port);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "No free vhci_hcd ports available",
    ))
}

/// Attach an authenticated socket to vhci_hcd, creating a virtual USB device.
///
/// The kernel reads USB/IP protocol from the socket FD. When the device is
/// detached (via `detach()` or `wolfusb mount` exit), the kernel releases the port.
///
/// Returns the port number that was assigned.
pub fn attach<F: AsRawFd>(socket: &F, devid: u32, speed: Speed) -> io::Result<u32> {
    ensure_module_loaded()?;
    let vhci_path = find_vhci_path()?;
    let port = find_free_port(&vhci_path)?;

    let attach_path = format!("{}/attach", vhci_path);
    let sockfd = socket.as_raw_fd();
    let cmd = format!("{} {} {} {}", port, sockfd, devid, speed.as_u8());

    // The sysfs attach file must be written in a single write syscall
    let mut f = fs::OpenOptions::new().write(true).open(&attach_path)?;
    f.write_all(cmd.as_bytes())?;
    Ok(port)
}

/// Detach a port, removing the virtual USB device.
pub fn detach(port: u32) -> io::Result<()> {
    let vhci_path = find_vhci_path()?;
    let detach_path = format!("{}/detach", vhci_path);
    let mut f = fs::OpenOptions::new().write(true).open(&detach_path)?;
    f.write_all(port.to_string().as_bytes())?;
    Ok(())
}

/// Check whether vhci_hcd is available on this system.
pub fn is_available() -> bool {
    Path::new("/sys/devices/platform/vhci_hcd.0").is_dir()
}
