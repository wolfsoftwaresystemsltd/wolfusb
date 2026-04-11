// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures::SinkExt;
use log::{error, info};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::codec::Framed;

use super::device_manager::DeviceManager;
use super::transfer;
use crate::protocol::codec::WolfUsbCodec;
use crate::protocol::messages::*;
use crate::protocol::types::DeviceId;

use futures::StreamExt;

pub struct Connection {
    framed: Framed<TcpStream, WolfUsbCodec>,
    device_manager: Arc<Mutex<DeviceManager>>,
    peer_addr: SocketAddr,
    authenticated: bool,
    shared_key: Option<Vec<u8>>,
    sessions: HashMap<u64, DeviceId>,
}

impl Connection {
    pub fn new(
        stream: TcpStream,
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

    pub async fn run(&mut self) {
        info!("Client connected: {}", self.peer_addr);

        loop {
            let msg = match self.framed.next().await {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => {
                    error!("Protocol error from {}: {e}", self.peer_addr);
                    break;
                }
                None => {
                    info!("Client disconnected: {}", self.peer_addr);
                    break;
                }
            };

            let response = self.handle_message(msg).await;

            if let Some(resp) = response
                && let Err(e) = self.framed.send(resp).await
            {
                error!("Failed to send response to {}: {e}", self.peer_addr);
                break;
            }
        }

        // Cleanup: detach all devices held by this connection
        self.cleanup().await;
    }

    async fn handle_message(&mut self, msg: Message) -> Option<Message> {
        match msg {
            Message::Hello(req) => Some(self.handle_hello(req)),
            Message::Ping => Some(Message::Pong),

            // All other messages require authentication (if key is set)
            _ if self.shared_key.is_some() && !self.authenticated => {
                Some(Message::Error(ErrorResponse {
                    code: ErrorCode::AuthenticationFailed,
                    message: "Not authenticated. Send Hello first.".to_string(),
                }))
            }

            Message::ListDevices => Some(self.handle_list_devices().await),
            Message::GetDescriptors(req) => Some(self.handle_get_descriptors(req).await),
            Message::Attach(req) => Some(self.handle_attach(req).await),
            Message::Detach(req) => Some(self.handle_detach(req).await),
            Message::ControlTransfer(req) => Some(self.handle_control_transfer(req).await),
            Message::BulkTransfer(req) => Some(self.handle_bulk_transfer(req).await),
            Message::InterruptTransfer(req) => Some(self.handle_interrupt_transfer(req).await),
            Message::ClaimInterface(req) => Some(self.handle_claim_interface(req).await),
            Message::ReleaseInterface(req) => Some(self.handle_release_interface(req).await),
            Message::SetConfiguration(req) => Some(self.handle_set_configuration(req).await),

            _ => Some(Message::Error(ErrorResponse {
                code: ErrorCode::InternalError,
                message: "Unexpected message type".to_string(),
            })),
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

        let (auth_accepted, challenge_response) = if let Some(ref key) = self.shared_key {
            use hmac::{Hmac, Mac};
            use sha2::Sha256;

            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            mac.update(&req.auth_nonce);
            mac.update(b"wolfusb-server");
            let result = mac.finalize().into_bytes().to_vec();
            (true, result)
        } else {
            (true, Vec::new())
        };

        self.authenticated = auth_accepted;
        info!(
            "Hello from {} (client: {}), auth={}",
            self.peer_addr, req.client_name, auth_accepted
        );

        Message::HelloResponse(HelloResponse {
            protocol_version: PROTOCOL_VERSION,
            server_name: "wolfusb".to_string(),
            auth_accepted,
            auth_challenge_response: challenge_response,
            error_message: None,
        })
    }

    async fn handle_list_devices(&self) -> Message {
        let manager = self.device_manager.lock().await;
        match manager.list_devices() {
            Ok(devices) => Message::DeviceList(DeviceListResponse { devices }),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::UsbError,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_get_descriptors(&self, req: GetDescriptorsRequest) -> Message {
        let manager = self.device_manager.lock().await;
        match manager.get_descriptors(req.device_id) {
            Ok(descriptors) => Message::DescriptorData(DescriptorDataResponse {
                device_id: req.device_id,
                descriptors,
            }),
            Err(e) => Message::Error(ErrorResponse {
                code: ErrorCode::DeviceNotFound,
                message: e.to_string(),
            }),
        }
    }

    async fn handle_attach(&mut self, req: AttachRequest) -> Message {
        let mut manager = self.device_manager.lock().await;
        match manager.attach(req.device_id, self.peer_addr) {
            Ok(session_id) => {
                self.sessions.insert(session_id, req.device_id);
                Message::AttachResult(AttachResponse {
                    device_id: req.device_id,
                    success: true,
                    error_message: None,
                    session_id: Some(session_id),
                })
            }
            Err(e) => Message::AttachResult(AttachResponse {
                device_id: req.device_id,
                success: false,
                error_message: Some(e.to_string()),
                session_id: None,
            }),
        }
    }

    async fn handle_detach(&mut self, req: DetachRequest) -> Message {
        let mut manager = self.device_manager.lock().await;
        match manager.detach(req.device_id, req.session_id) {
            Ok(()) => {
                self.sessions.remove(&req.session_id);
                Message::DetachResult(DetachResponse {
                    device_id: req.device_id,
                    success: true,
                    error_message: None,
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
        let manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            });
        }

        match manager.get_handle(req.device_id) {
            Ok(handle) => {
                let resp = transfer::execute_control_transfer(handle, &req);
                Message::TransferResult(resp)
            }
            Err(e) => Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_bulk_transfer(&self, req: BulkTransferRequest) -> Message {
        let manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            });
        }

        match manager.get_handle(req.device_id) {
            Ok(handle) => {
                let resp = transfer::execute_bulk_transfer(handle, &req);
                Message::TransferResult(resp)
            }
            Err(e) => Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_interrupt_transfer(&self, req: InterruptTransferRequest) -> Message {
        let manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            });
        }

        match manager.get_handle(req.device_id) {
            Ok(handle) => {
                let resp = transfer::execute_interrupt_transfer(handle, &req);
                Message::TransferResult(resp)
            }
            Err(e) => Message::TransferResult(TransferResponse {
                session_id: req.session_id,
                device_id: req.device_id,
                success: false,
                data: Vec::new(),
                bytes_transferred: 0,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_claim_interface(&self, req: ClaimInterfaceRequest) -> Message {
        let mut manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            });
        }

        match manager.claim_interface(req.device_id, req.interface_number) {
            Ok(()) => Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: true,
                error_message: None,
            }),
            Err(e) => Message::ClaimInterfaceResult(ClaimInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_release_interface(&self, req: ReleaseInterfaceRequest) -> Message {
        let mut manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            });
        }

        match manager.release_interface(req.device_id, req.interface_number) {
            Ok(()) => Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: true,
                error_message: None,
            }),
            Err(e) => Message::ReleaseInterfaceResult(ReleaseInterfaceResponse {
                success: false,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn handle_set_configuration(&self, req: SetConfigurationRequest) -> Message {
        let mut manager = self.device_manager.lock().await;
        if let Err(e) = manager.validate_session(req.session_id, req.device_id) {
            return Message::SetConfigurationResult(SetConfigurationResponse {
                success: false,
                error_message: Some(e.to_string()),
            });
        }

        match manager.set_configuration(req.device_id, req.configuration) {
            Ok(()) => Message::SetConfigurationResult(SetConfigurationResponse {
                success: true,
                error_message: None,
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
        let mut manager = self.device_manager.lock().await;
        manager.detach_all_for_sessions(&session_ids);
        self.sessions.clear();
        info!(
            "Cleaned up {} sessions for {}",
            session_ids.len(),
            self.peer_addr
        );
    }
}
