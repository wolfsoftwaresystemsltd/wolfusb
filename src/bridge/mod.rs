// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Bridge mode — translates between wolfusb's authenticated connection and
//! the Linux kernel's vhci_hcd driver, making a remote USB device appear as
//! a real local USB device (visible in lsusb, usable by containers/VMs).
//!
//! Architecture:
//! - Client-side: hands the TCP socket FD to `/sys/devices/platform/vhci_hcd.0/attach`.
//!   The kernel then drives USB enumeration and speaks USB/IP protocol on that socket.
//! - Server-side: reads USB/IP PDUs from the socket, translates to rusb calls on the
//!   actual USB device, and writes USB/IP responses back.
//!
//! We bypass the usbipd daemon and usbip CLI entirely — wolfusb handles auth
//! (TLS + HMAC), then hands the authenticated socket to the kernel driver.

pub mod usbip;
pub mod vhci;
