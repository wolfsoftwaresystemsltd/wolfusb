// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

//! Linux kernel USB/IP protocol (what vhci_hcd speaks over the TCP socket).
//! Reference: https://docs.kernel.org/usb/usbip_protocol.html
//!
//! All fields are big-endian. Headers are fixed-size 48 bytes for CMD/RET,
//! 48 bytes for UNLINK. Payloads follow for OUT transfers (data to device)
//! on CMD_SUBMIT, and for IN transfers (data from device) on RET_SUBMIT.

use bytes::{Buf, BufMut, BytesMut};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// USB/IP command codes (set in the 4-byte command field of each header)
pub const USBIP_CMD_SUBMIT: u32 = 0x00000001;
pub const USBIP_RET_SUBMIT: u32 = 0x00000003;
pub const USBIP_CMD_UNLINK: u32 = 0x00000002;
pub const USBIP_RET_UNLINK: u32 = 0x00000004;

/// USB/IP direction: 0 = OUT (host → device), 1 = IN (device → host)
pub const USBIP_DIR_OUT: u32 = 0;
pub const USBIP_DIR_IN: u32 = 1;

/// Fixed header size for SUBMIT/UNLINK commands and their responses
pub const HEADER_SIZE: usize = 48;

/// Setup packet size (control transfers only)
pub const SETUP_SIZE: usize = 8;

/// USB/IP common header present on all PDUs
#[derive(Debug, Clone, Copy)]
pub struct UsbipHeader {
    pub command: u32,
    pub seqnum: u32,
    /// Device ID — busnum << 16 | devnum
    pub devid: u32,
    /// Direction (0 = OUT, 1 = IN)
    pub direction: u32,
    /// Endpoint number (0-15)
    pub ep: u32,
}

impl UsbipHeader {
    pub fn new(command: u32, seqnum: u32, devid: u32, direction: u32, ep: u32) -> Self {
        Self { command, seqnum, devid, direction, ep }
    }
}

/// CMD_SUBMIT: client asks server to execute a USB transfer
#[derive(Debug, Clone)]
pub struct CmdSubmit {
    pub header: UsbipHeader,
    /// URB transfer flags (kernel URB_* flags)
    pub transfer_flags: u32,
    /// Requested transfer length (bytes)
    pub transfer_buffer_length: i32,
    /// Start frame (for isoc); -1 for non-isoc
    pub start_frame: i32,
    /// Number of isoc packets; 0xFFFFFFFF for non-isoc
    pub number_of_packets: i32,
    /// Interval (for interrupt/isoc transfers)
    pub interval: i32,
    /// Setup packet (8 bytes, control transfers only; zeroes otherwise)
    pub setup: [u8; SETUP_SIZE],
    /// Transfer data (for OUT transfers; empty for IN)
    pub data: Vec<u8>,
}

/// RET_SUBMIT: server responds with the result of a USB transfer
#[derive(Debug, Clone)]
pub struct RetSubmit {
    pub header: UsbipHeader,
    /// Status (0 = success, negative errno on failure)
    pub status: i32,
    /// Actual number of bytes transferred
    pub actual_length: i32,
    /// Start frame (isoc only)
    pub start_frame: i32,
    /// Number of isoc packets transferred
    pub number_of_packets: i32,
    /// Number of URBs that couldn't complete (isoc only)
    pub error_count: i32,
    /// Transfer data (for IN transfers; empty for OUT)
    pub data: Vec<u8>,
}

/// CMD_UNLINK: client cancels a previously submitted URB
#[derive(Debug, Clone)]
pub struct CmdUnlink {
    pub header: UsbipHeader,
    /// The seqnum of the URB to cancel
    pub unlink_seqnum: u32,
}

/// RET_UNLINK: server responds to an unlink request
#[derive(Debug, Clone)]
pub struct RetUnlink {
    pub header: UsbipHeader,
    /// Status (0 = unlinked, -ECONNRESET = already completed)
    pub status: i32,
}

/// Any USB/IP PDU that can appear on the wire
#[derive(Debug, Clone)]
pub enum UsbipPdu {
    CmdSubmit(CmdSubmit),
    RetSubmit(RetSubmit),
    CmdUnlink(CmdUnlink),
    RetUnlink(RetUnlink),
}

// ─── Encoding ────────────────────────────────────────────────────────────────

fn encode_header(buf: &mut BytesMut, h: &UsbipHeader) {
    buf.put_u32(h.command);
    buf.put_u32(h.seqnum);
    buf.put_u32(h.devid);
    buf.put_u32(h.direction);
    buf.put_u32(h.ep);
}

impl CmdSubmit {
    pub fn encode(&self, buf: &mut BytesMut) {
        encode_header(buf, &self.header);
        buf.put_u32(self.transfer_flags);
        buf.put_i32(self.transfer_buffer_length);
        buf.put_i32(self.start_frame);
        buf.put_i32(self.number_of_packets);
        buf.put_i32(self.interval);
        buf.put_slice(&self.setup);
        buf.put_slice(&self.data);
    }
}

impl RetSubmit {
    pub fn encode(&self, buf: &mut BytesMut) {
        encode_header(buf, &self.header);
        buf.put_i32(self.status);
        buf.put_i32(self.actual_length);
        buf.put_i32(self.start_frame);
        buf.put_i32(self.number_of_packets);
        buf.put_i32(self.error_count);
        // Reserved padding: header (20) + fields (20) = 40 bytes; header size is 48
        buf.put_slice(&[0u8; SETUP_SIZE]);
        buf.put_slice(&self.data);
    }
}

impl CmdUnlink {
    pub fn encode(&self, buf: &mut BytesMut) {
        encode_header(buf, &self.header);
        buf.put_u32(self.unlink_seqnum);
        // Padding to fill 48-byte header
        buf.put_slice(&[0u8; 24]);
    }
}

impl RetUnlink {
    pub fn encode(&self, buf: &mut BytesMut) {
        encode_header(buf, &self.header);
        buf.put_i32(self.status);
        // Padding
        buf.put_slice(&[0u8; 24]);
    }
}

// ─── Async reading/writing ───────────────────────────────────────────────────

/// Read one USB/IP PDU from the stream. Blocks until full PDU is received.
pub async fn read_pdu<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<UsbipPdu> {
    let mut hdr = [0u8; HEADER_SIZE];
    r.read_exact(&mut hdr).await?;

    let mut b = &hdr[..];
    let command = b.get_u32();
    let seqnum = b.get_u32();
    let devid = b.get_u32();
    let direction = b.get_u32();
    let ep = b.get_u32();
    let header = UsbipHeader { command, seqnum, devid, direction, ep };

    match command {
        USBIP_CMD_SUBMIT => {
            let transfer_flags = b.get_u32();
            let transfer_buffer_length = b.get_i32();
            let start_frame = b.get_i32();
            let number_of_packets = b.get_i32();
            let interval = b.get_i32();
            let mut setup = [0u8; SETUP_SIZE];
            setup.copy_from_slice(&b[..SETUP_SIZE]);

            // For OUT transfers, read the data payload
            let data = if direction == USBIP_DIR_OUT && transfer_buffer_length > 0 {
                let mut d = vec![0u8; transfer_buffer_length as usize];
                r.read_exact(&mut d).await?;
                d
            } else {
                Vec::new()
            };

            Ok(UsbipPdu::CmdSubmit(CmdSubmit {
                header, transfer_flags, transfer_buffer_length,
                start_frame, number_of_packets, interval, setup, data,
            }))
        }
        USBIP_RET_SUBMIT => {
            let status = b.get_i32();
            let actual_length = b.get_i32();
            let start_frame = b.get_i32();
            let number_of_packets = b.get_i32();
            let error_count = b.get_i32();
            // Skip 8 bytes of padding (setup field in RET)
            // For IN transfers, read the data payload
            let data = if direction == USBIP_DIR_IN && actual_length > 0 {
                let mut d = vec![0u8; actual_length as usize];
                r.read_exact(&mut d).await?;
                d
            } else {
                Vec::new()
            };
            Ok(UsbipPdu::RetSubmit(RetSubmit {
                header, status, actual_length,
                start_frame, number_of_packets, error_count, data,
            }))
        }
        USBIP_CMD_UNLINK => {
            let unlink_seqnum = b.get_u32();
            Ok(UsbipPdu::CmdUnlink(CmdUnlink { header, unlink_seqnum }))
        }
        USBIP_RET_UNLINK => {
            let status = b.get_i32();
            Ok(UsbipPdu::RetUnlink(RetUnlink { header, status }))
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("Unknown USB/IP command: 0x{:08x}", command))),
    }
}

/// Write a USB/IP PDU to the stream.
pub async fn write_pdu<W: AsyncWrite + Unpin>(w: &mut W, pdu: &UsbipPdu) -> io::Result<()> {
    let mut buf = BytesMut::with_capacity(HEADER_SIZE + 4096);
    match pdu {
        UsbipPdu::CmdSubmit(p) => p.encode(&mut buf),
        UsbipPdu::RetSubmit(p) => p.encode(&mut buf),
        UsbipPdu::CmdUnlink(p) => p.encode(&mut buf),
        UsbipPdu::RetUnlink(p) => p.encode(&mut buf),
    }
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// Compute USB/IP devid from bus and device numbers
pub fn make_devid(bus_number: u8, device_number: u8) -> u32 {
    ((bus_number as u32) << 16) | (device_number as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_cmd_submit() {
        let original = CmdSubmit {
            header: UsbipHeader::new(USBIP_CMD_SUBMIT, 42, 0x00010002, USBIP_DIR_IN, 1),
            transfer_flags: 0,
            transfer_buffer_length: 64,
            start_frame: -1,
            number_of_packets: -1,
            interval: 0,
            setup: [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00],
            data: Vec::new(),
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf);
        assert_eq!(buf.len(), HEADER_SIZE); // header + fields + setup = exactly 48
    }

    #[test]
    fn devid_encoding() {
        assert_eq!(make_devid(1, 2), 0x00010002);
        assert_eq!(make_devid(255, 255), 0x00FF00FF);
    }
}
