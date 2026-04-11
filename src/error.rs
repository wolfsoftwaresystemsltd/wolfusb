// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use thiserror::Error;

#[derive(Error, Debug)]
pub enum WolfUsbError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Protocol version mismatch: local={local}, remote={remote}")]
    VersionMismatch { local: u32, remote: u32 },

    #[error("Serialization error: {0}")]
    SerializationError(#[from] bincode::error::EncodeError),

    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] bincode::error::DecodeError),

    #[error("USB error: {0}")]
    UsbError(#[from] rusb::Error),

    #[error("Device not found: bus={bus} addr={addr}")]
    DeviceNotFound { bus: u8, addr: u8 },

    #[error("Device already attached by another client")]
    DeviceAlreadyAttached,

    #[error("Device not attached (no active session)")]
    DeviceNotAttached,

    #[error("Invalid session ID: {0}")]
    InvalidSession(u64),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: u32, max: u32 },

    #[error("Unexpected message: expected {expected}, got {got}")]
    UnexpectedMessage { expected: String, got: String },

    #[error("Connection closed")]
    ConnectionClosed,
}

pub type Result<T> = std::result::Result<T, WolfUsbError>;
