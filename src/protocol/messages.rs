// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use bincode::{Decode, Encode};

use super::types::*;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Encode, Decode)]
pub enum Message {
    // Handshake
    Hello(HelloRequest),
    HelloResponse(HelloResponse),

    // Device enumeration
    ListDevices,
    DeviceList(DeviceListResponse),

    GetDescriptors(GetDescriptorsRequest),
    DescriptorData(DescriptorDataResponse),

    // Device attachment
    Attach(AttachRequest),
    AttachResult(AttachResponse),

    Detach(DetachRequest),
    DetachResult(DetachResponse),

    // USB Transfers
    ControlTransfer(ControlTransferRequest),
    BulkTransfer(BulkTransferRequest),
    InterruptTransfer(InterruptTransferRequest),
    TransferResult(TransferResponse),

    // Interface management
    ClaimInterface(ClaimInterfaceRequest),
    ClaimInterfaceResult(ClaimInterfaceResponse),
    ReleaseInterface(ReleaseInterfaceRequest),
    ReleaseInterfaceResult(ReleaseInterfaceResponse),
    SetConfiguration(SetConfigurationRequest),
    SetConfigurationResult(SetConfigurationResponse),

    // Virtual USB bridge (vhci_hcd mode) — after auth, client requests bridge mode;
    // once server sends BridgeAccepted, the wire protocol switches to raw USB/IP
    // kernel protocol bytes (no more bincode framing).
    Bridge(BridgeRequest),
    BridgeAccepted(BridgeAcceptedResponse),
    BridgeRejected(BridgeRejectedResponse),

    // Error
    Error(ErrorResponse),

    // Keepalive
    Ping,
    Pong,
}

// --- Handshake ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct HelloRequest {
    pub protocol_version: u32,
    pub client_name: String,
    pub auth_nonce: [u8; 32],
    /// HMAC(key, nonce || "wolfusb-client") -- proves client knows the key
    pub auth_proof: Vec<u8>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct HelloResponse {
    pub protocol_version: u32,
    pub server_name: String,
    pub auth_accepted: bool,
    pub auth_challenge_response: Vec<u8>,
    pub error_message: Option<String>,
}

// --- Device Enumeration ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GetDescriptorsRequest {
    pub device_id: DeviceId,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DescriptorDataResponse {
    pub device_id: DeviceId,
    pub descriptors: DeviceDescriptorTree,
}

// --- Attach/Detach ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct AttachRequest {
    pub device_id: DeviceId,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AttachResponse {
    pub device_id: DeviceId,
    pub success: bool,
    pub error_message: Option<String>,
    pub session_id: Option<u64>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DetachRequest {
    pub device_id: DeviceId,
    pub session_id: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DetachResponse {
    pub device_id: DeviceId,
    pub success: bool,
    pub error_message: Option<String>,
}

// --- USB Transfers ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct ControlTransferRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub data: Vec<u8>,
    pub length: u16,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct BulkTransferRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub endpoint: u8,
    pub data: Vec<u8>,
    pub length: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterruptTransferRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub endpoint: u8,
    pub data: Vec<u8>,
    pub length: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct TransferResponse {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub success: bool,
    pub data: Vec<u8>,
    pub bytes_transferred: u32,
    pub error_message: Option<String>,
}

// --- Interface/Config Management ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct ClaimInterfaceRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub interface_number: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ClaimInterfaceResponse {
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ReleaseInterfaceRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub interface_number: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ReleaseInterfaceResponse {
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetConfigurationRequest {
    pub session_id: u64,
    pub device_id: DeviceId,
    pub configuration: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetConfigurationResponse {
    pub success: bool,
    pub error_message: Option<String>,
}

// --- Virtual USB Bridge ---
// After the client sends Bridge and receives BridgeAccepted, the connection
// switches mode: all further bytes on the wire are raw USB/IP kernel protocol.
// The client hands the underlying socket FD to /sys/.../vhci_hcd.0/attach
// which lets the Linux kernel treat the remote device as a local USB device.

#[derive(Debug, Clone, Encode, Decode)]
pub struct BridgeRequest {
    pub device_id: DeviceId,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct BridgeAcceptedResponse {
    pub device_id: DeviceId,
    /// USB/IP "devid" — encoded bus/device numbers used by the kernel
    pub devid: u32,
    /// Device speed (matches rusb::Speed ordinal)
    pub speed: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device bcdDevice
    pub bcd_device: u16,
    /// Device class codes
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    /// Number of configurations
    pub num_configurations: u8,
    /// Number of interfaces in active config
    pub num_interfaces: u8,
    /// Active configuration value (bConfigurationValue)
    pub config_value: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct BridgeRejectedResponse {
    pub error_message: String,
}

// --- Error ---

#[derive(Debug, Clone, Encode, Decode)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum ErrorCode {
    ProtocolVersionMismatch,
    AuthenticationFailed,
    DeviceNotFound,
    DeviceAlreadyAttached,
    DeviceNotAttached,
    InvalidSession,
    TransferFailed,
    UsbError,
    InternalError,
}
