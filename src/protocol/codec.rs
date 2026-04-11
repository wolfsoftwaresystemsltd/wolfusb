// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::messages::Message;
use crate::error::WolfUsbError;

const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024; // 16 MiB
const LENGTH_PREFIX_SIZE: usize = 4;
const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub struct WolfUsbCodec;

impl Decoder for WolfUsbCodec {
    type Item = Message;
    type Error = WolfUsbError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>, WolfUsbError> {
        // Need at least 4 bytes for the length prefix
        if src.len() < LENGTH_PREFIX_SIZE {
            return Ok(None);
        }

        // Peek at the length without consuming
        let length = u32::from_be_bytes([src[0], src[1], src[2], src[3]]);

        if length > MAX_FRAME_SIZE {
            return Err(WolfUsbError::FrameTooLarge {
                size: length,
                max: MAX_FRAME_SIZE,
            });
        }

        let total_frame_size = LENGTH_PREFIX_SIZE + length as usize;

        // Wait for the full frame
        if src.len() < total_frame_size {
            src.reserve(total_frame_size - src.len());
            return Ok(None);
        }

        // Consume the length prefix
        src.advance(LENGTH_PREFIX_SIZE);

        // Take the payload bytes
        let payload = src.split_to(length as usize);

        // Decode the message
        let (message, _) = bincode::decode_from_slice(&payload, BINCODE_CONFIG)?;

        Ok(Some(message))
    }
}

impl Encoder<Message> for WolfUsbCodec {
    type Error = WolfUsbError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), WolfUsbError> {
        let payload = bincode::encode_to_vec(&item, BINCODE_CONFIG)?;
        let length = payload.len() as u32;

        if length > MAX_FRAME_SIZE {
            return Err(WolfUsbError::FrameTooLarge {
                size: length,
                max: MAX_FRAME_SIZE,
            });
        }

        dst.reserve(LENGTH_PREFIX_SIZE + payload.len());
        dst.put_u32(length);
        dst.extend_from_slice(&payload);

        Ok(())
    }
}
