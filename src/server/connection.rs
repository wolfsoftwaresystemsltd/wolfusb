// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::server::TlsStream;
use tokio_util::codec::Framed;

use super::device_manager::DeviceManager;
use super::transfer;
use crate::protocol::codec::WolfUsbCodec;
use crate::protocol::messages::*;
use crate::protocol::types::DeviceId;

/// The connection's underlying stream. Bridge mode requires a raw `TcpStream`
/// because the kernel `usbip_host` driver needs a kernel-managed fd; a TLS
/// session can't be handed off.
pub enum ServerStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for ServerStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ServerStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_flush(cx),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

pub struct Connection {
    framed: Framed<ServerStream, WolfUsbCodec>,
    device_manager: Arc<Mutex<DeviceManager>>,
    peer_addr: SocketAddr,
    authenticated: bool,
    shared_key: Option<Vec<u8>>,
    sessions: HashMap<u64, DeviceId>,
}

/// Action returned by handle_message — either send a reply, or switch to bridge mode
enum Action {
    Reply(Message),
    Bridge { reply: Message, target: DeviceId },
}

impl Connection {
    pub fn new(
        stream: ServerStream,
        device_manager: Arc<Mutex<DeviceManager>>,
        peer_addr: SocketAddr,
        shared_key: Option<Vec<u8>>,
    ) -> Self {
        Self {
            framed: Framed::new(stream, WolfUsbCodec),
            device_manager,
            peer_addr,
            authenticated: false,
            shared_key,
            sessions: HashMap::new(),
        }
    }

    pub async fn run(mut self) {
        info!("Client connected: {}", self.peer_addr);

        let bridge_handoff = loop {
            let msg = match self.framed.next().await {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => {
                    error!("Protocol error from {}: {e}", self.peer_addr);
                    break None;
                }
                None => {
                    info!("Client disconnected: {}", self.peer_addr);
                    break None;
                }
            };

            match self.handle_message(msg).await {
                Action::Reply(resp) => {
                    if let Err(e) = self.framed.send(resp).await {
                        error!("Failed to send response to {}: {e}", self.peer_addr);
                        break None;
                    }
                }
                Action::Bridge { reply, target } => {
                    if let Err(e) = self.framed.send(reply).await {
                        error!("Failed to send BridgeAccepted to {}: {e}", self.peer_addr);
                        break None;
                    }
                    if let Err(e) = self.framed.flush().await {
                        error!("Failed to flush before bridge mode: {e}");
                        break None;
                    }
                    break Some(target);
                }
            }
        };

        if let Some(target) = bridge_handoff {
            // Hand raw TCP stream to the kernel usbip_host bridge.
            // TLS sessions can't be bridged — the kernel needs a raw fd.
            match self.framed.into_inner() {
                ServerStream::Plain(tcp) => {
                    super::bridge::run_bridge(tcp, target).await;
                    info!("Bridge closed: {}", self.peer_addr);
                }
                ServerStream::Tls(_) => {
                    warn!(
                        "Bridge rejected for {}: TLS sessions cannot be bridged \
                         (kernel needs raw TCP fd). Reconnect without TLS.",
                        self.peer_addr
                    );
                }
            }
            return;
        }

        self.cleanup().await;
    }

    async fn handle_message(&mut self, msg: Message) -> Action {
        match msg {
            Message::Hello(req) => Action::Reply(self.handle_hello(req)),
            Message::Ping => Action::Reply(Message::Pong),

            _ if self.shared_key.is_some() && !self.authenticated => {
                Action::Reply(Message::Error(ErrorResponse {
                    code: ErrorCode::AuthenticationFailed,
                    message: "Not authenticated. Send Hello first.".to_string(),
                }))
            }

            Message::ListDevices => Action::Reply(self.handle_list_devices().await),
            Message::GetDescriptors(req) => Action::Reply(self.handle_get_descriptors(req).await),
            Message::Attach(req) => Action::Reply(self.handle_attach(req).await),
            Message::Detach(req) => Action::Reply(self.handle_detach(req).await),
            Message::ControlTransfer(req) => Action::Reply(self.handle_control_transfer(req).await),
            Message::BulkTransfer(req) => Action::Reply(self.handle_bulk_transfer(req).await),
            Message::InterruptTransfer(req) => {
                Action::Reply(self.handle_interrupt_transfer(req).await)
            }
            Message::ClaimInterface(req) => Action::Reply(self.handle_claim_interface(req).await),
            Message::ReleaseInterface(req) => {
                Action::Reply(self.handle_release_interface(req).await)
            }
            Message::SetConfiguration(req) => {
                Action::Reply(self.handle_set_configuration(req).await)
            }
            Message::Bridge(req) => self.handle_bridge(req).await,

            _ => Action::Reply(Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: "Unexpected message type".to_string(),
            })),
        }
    }

    async fn handle_bridge(&mut self, req: BridgeRequest) -> Action {
        // Compute the busid the client must send in OP_REQ_IMPORT. The `usbip`
        // crate's rusb backend formats bus_id as "{bus}-{addr}-{port_number}"
        // (see usbip/src/lib.rs:244-249). The client can't derive port_number
        // on its own, so we look up the device and return the correct string.
        let target_bus = req.device_id.bus_number;
        let target_addr = req.device_id.address;
        let busid = tokio::task::spawn_blocking(move || {
            let devices = rusb::devices().ok()?;
            for d in devices.iter() {
                if d.bus_number() == target_bus && d.address() == target_addr {
                    return Some(format!(
                        "{}-{}-{}",
                        d.bus_number(),
                        d.address(),
                        d.port_number()
                    ));
                }
            }
            None
        })
        .await
        .ok()
        .flatten();

        let busid = match busid {
            Some(b) => b,
            None => {
                return Action::Reply(Message::BridgeRejected(BridgeRejectedResponse {
                    error_message: format!(
                        "Device not found at bus={} addr={}",
                        target_bus, target_addr
                    ),
                }));
            }
        };

        info!(
            "Bridge: accepting device bus={} addr={} as busid={}",
            target_bus, target_addr, busid
        );

        Action::Bridge {
            reply: Message::BridgeAccepted(BridgeAcceptedResponse {
                device_id: req.device_id,
                busid,
                // Remaining fields are informational only; real device info
                // comes from OP_REP_IMPORT once the client does the USB/IP
                // import handshake on the raw stream.
                devid: 0,
                speed: 0,
                vendor_id: 0,
                product_id: 0,
                bcd_device: 0,
                device_class: 0,
                device_subclass: 0,
                device_protocol: 0,
                num_configurations: 0,
                num_interfaces: 0,
                config_value: 0,
            }),
            target: req.device_id,
        }
    }

    fn handle_hello(&mut self, req: HelloRequest) -> Message {
        if req.protocol_version != PROTOCOL_VERSION {
            return Message::HelloResponse(HelloResponse {
                protocol_version: PROTOCOL_VERSION,
                server_name: "wolfusb".to_string(),
                auth_accepted: false,
                auth_challenge_response: Vec::new(),
                error_message: Some(format!(
                    "Protocol version mismatch: server={}, client={}",
                    PROTOCOL_VERSION, req.protocol_version
                )),
            });
        }

        if let Some(ref key) = self.shared_key {
            use hmac::{Hmac, Mac};
            use sha2::Sha256;

            type HmacSha256 = Hmac<Sha256>;

            // Verify client proof
            let mut client_mac =
                HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            client_mac.update(&req.auth_nonce);
            client_mac.update(b"wolfusb-client");

            if client_mac.verify_slice(&req.auth_proof).is_err() {
                warn!(
                    "Authentication failed for {} (client: {})",
                    self.peer_addr, req.client_name
                );
                return Message::HelloResponse(HelloResponse {
                    protocol_version: PROTOCOL_VERSION,
                    server_name: "wolfusb".to_string(),
                    auth_accepted: false,
                    auth_challenge_response: Vec::new(),
                    error_message: Some("Authentication failed".to_string()),
                });
            }

            // Server proof so client can verify us
            let mut server_mac =
                HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            server_mac.update(&req.auth_nonce);
            server_mac.update(b"wolfusb-server");
            let server_proof = server_mac.finalize().into_bytes().to_vec();

            self.authenticated = true;
            info!(
                "Hello from {} (client: {}), auth=ok",
                self.peer_addr, req.client_name
            );

            Message::HelloResponse(HelloResponse {
                protocol_version: PROTOCOL_VERSION,
                server_name: "wolfusb".to_string(),
                auth_accepted: true,
                auth_challenge_response: server_proof,
                error_message: None,
            })
        } else {
            self.authenticated = true;
            info!(
                "Hello from {} (client: {}), auth=none",
                self.peer_addr, req.client_name
            );

            Message::HelloResponse(HelloResponse {
                protocol_version: PROTOCOL_VERSION,
                server_name: "wolfusb".to_string(),
                auth_accepted: true,
                auth_challenge_response: Vec::new(),
                error_message: None,
            })
        }
    }

    async fn handle_list_devices(&self) -> Message {
        let dm = self.device_manager.clone();
        match tokio::task::spawn_blocking(move || dm.blocking_lock().list_devices()).await {
            Ok(Ok(devices)) => Message::DeviceList(DeviceListResponse { devices }),
            Ok(Err(e)) => Message::Error(ErrorResponse {
                code: ErrorCode::UsbError,
                message: e.to_string(),
            }),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_get_descriptors(&self, req: GetDescriptorsRequest) -> Message {
        let dm = self.device_manager.clone();
        let did = req.device_id;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().get_descriptors(did)).await {
            Ok(Ok(descriptors)) => Message::DescriptorData(DescriptorDataResponse {
                device_id: req.device_id,
                descriptors,
            }),
            Ok(Err(e)) => Message::Error(ErrorResponse {
                code: ErrorCode::DeviceNotFound,
                message: e.to_string(),
            }),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_attach(&mut self, req: AttachRequest) -> Message {
        let dm = self.device_manager.clone();
        let did = req.device_id;
        let addr = self.peer_addr;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().attach(did, addr)).await {
            Ok(Ok(session_id)) => {
                self.sessions.insert(session_id, req.device_id);
                Message::AttachResult(AttachResponse {
                    device_id: req.device_id,
                    success: true,
                    error_message: None,
                    session_id: Some(session_id),
                })
            }
            Ok(Err(e)) => Message::AttachResult(AttachResponse {
                device_id: req.device_id,
                success: false,
                error_message: Some(e.to_string()),
                session_id: None,
            }),
            Err(e) => Message::AttachResult(AttachResponse {
                device_id: req.device_id,
                success: false,
                error_message: Some(e.to_string()),
                session_id: None,
            }),
        }
    }

    async fn handle_detach(&mut self, req: DetachRequest) -> Message {
        // Verify this session belongs to this connection
        if !self.sessions.contains_key(&req.session_id) {
            return Message::DetachResult(DetachResponse {
                device_id: req.device_id,
                success: false,
                error_message: Some("Session does not belong to this connection".to_string()),
            });
        }

        let dm = self.device_manager.clone();
        let did = req.device_id;
        let sid = req.session_id;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().detach(did, sid)).await {
            Ok(Ok(())) => {
                self.sessions.remove(&req.session_id);
                Message::DetachResult(DetachResponse {
                    device_id: req.device_id,
                    success: true,
                    error_message: None,
                })
            }
            Ok(Err(e)) => {
                // Clean up stale session even on error
                self.sessions.remove(&req.session_id);
                Message::DetachResult(DetachResponse {
                    device_id: req.device_id,
                    success: false,
                    error_message: Some(e.to_string()),
                })
            }
            Err(e) => Message::DetachResult(DetachResponse {
                device_id: req.device_id,
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_control_transfer(&self, req: ControlTransferRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return transfer_error(&req.session_id, &req.device_id, "Invalid session");
        }

        let handle = {
            let manager = self.device_manager.lock().await;
            if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
                return transfer_error(&req.session_id, &req.device_id, &e.to_string());
            }
            match manager.get_handle(req.device_id) {
                Ok(h) => h,
                Err(e) => return transfer_error(&req.session_id, &req.device_id, &e.to_string()),
            }
        };

        match tokio::task::spawn_blocking(move || transfer::execute_control_transfer(&handle, &req))
            .await
        {
            Ok(resp) => Message::TransferResult(resp),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_bulk_transfer(&self, req: BulkTransferRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return transfer_error(&req.session_id, &req.device_id, "Invalid session");
        }

        let handle = {
            let manager = self.device_manager.lock().await;
            if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
                return transfer_error(&req.session_id, &req.device_id, &e.to_string());
            }
            match manager.get_handle(req.device_id) {
                Ok(h) => h,
                Err(e) => return transfer_error(&req.session_id, &req.device_id, &e.to_string()),
            }
        };

        match tokio::task::spawn_blocking(move || transfer::execute_bulk_transfer(&handle, &req))
            .await
        {
            Ok(resp) => Message::TransferResult(resp),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_interrupt_transfer(&self, req: InterruptTransferRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return transfer_error(&req.session_id, &req.device_id, "Invalid session");
        }

        let handle = {
            let manager = self.device_manager.lock().await;
            if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
                return transfer_error(&req.session_id, &req.device_id, &e.to_string());
            }
            match manager.get_handle(req.device_id) {
                Ok(h) => h,
                Err(e) => return transfer_error(&req.session_id, &req.device_id, &e.to_string()),
            }
        };

        match tokio::task::spawn_blocking(move || {
            transfer::execute_interrupt_transfer(&handle, &req)
        })
        .await
        {
            Ok(resp) => Message::TransferResult(resp),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_claim_interface(&self, req: ClaimInterfaceRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: false,
                error_message: Some("Invalid session".to_string()),
            });
        }

        let dm = self.device_manager.clone();
        let did = req.device_id;
        let iface = req.interface_number;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().claim_interface(did, iface))
            .await
        {
            Ok(Ok(())) => Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: true,
                error_message: None,
            }),
            Ok(Err(e)) => Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
            Err(e) => Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_release_interface(&self, req: ReleaseInterfaceRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: false,
                error_message: Some("Invalid session".to_string()),
            });
        }

        let dm = self.device_manager.clone();
        let did = req.device_id;
        let iface = req.interface_number;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().release_interface(did, iface))
            .await
        {
            Ok(Ok(())) => Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: true,
                error_message: None,
            }),
            Ok(Err(e)) => Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
            Err(e) => Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_set_configuration(&self, req: SetConfigurationRequest) -> Message {
        if !self.sessions.contains_key(&req.session_id) {
            return Message::SetConfigurationResult(SetConfigurationResponse {
                success: false,
                error_message: Some("Invalid session".to_string()),
            });
        }

        let dm = self.device_manager.clone();
        let did = req.device_id;
        let config = req.configuration;
        match tokio::task::spawn_blocking(move || dm.blocking_lock().set_configuration(did, config))
            .await
        {
            Ok(Ok(())) => Message::SetConfigurationResult(SetConfigurationResponse {
                success: true,
                error_message: None,
            }),
            Ok(Err(e)) => Message::SetConfigurationResult(SetConfigurationResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
            Err(e) => Message::SetConfigurationResult(SetConfigurationResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn cleanup(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let session_ids: Vec<u64> = self.sessions.keys().copied().collect();
        let dm = self.device_manager.clone();
        tokio::task::spawn_blocking(move || {
            dm.blocking_lock().detach_all_for_sessions(&session_ids);
        })
        .await
        .ok();
        self.sessions.clear();
        info!("Cleaned up sessions for {}", self.peer_addr);
    }
}

fn transfer_error(session_id: &u64, device_id: &DeviceId, msg: &str) -> Message {
    Message::TransferResult(TransferResponse {
        session_id: *session_id,
        device_id: *device_id,
        success: false,
        data: Vec::new(),
        bytes_transferred: 0,
        error_message: Some(msg.to_string()),
    })
}
