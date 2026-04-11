// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use futures::{SinkExt, StreamExt};
use log::debug;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use crate::error::{Result, WolfUsbError};
use crate::protocol::codec::WolfUsbCodec;
use crate::protocol::messages::*;
use crate::protocol::types::*;

pub struct Session {
    framed: Framed<TcpStream, WolfUsbCodec>,
    pub server_name: String,
}

impl Session {
    pub async fn connect(
        addr: &str,
        client_name: &str,
        shared_key: Option<&[u8]>,
    ) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let mut framed = Framed::new(stream, WolfUsbCodec);

        // Generate auth nonce
        let mut auth_nonce = [0u8; 32];
        use rand::RngCore;
        rand::rng().fill_bytes(&mut auth_nonce);

        // Send Hello
        let hello = Message::Hello(HelloRequest {
            protocol_version: PROTOCOL_VERSION,
            client_name: client_name.to_string(),
            auth_nonce,
        });
        framed.send(hello).await?;

        // Receive HelloResponse
        let response = framed
            .next()
            .await
            .ok_or(WolfUsbError::ConnectionClosed)?
            .map_err(|e| WolfUsbError::ProtocolError(e.to_string()))?;

        let hello_resp = match response {
            Message::HelloResponse(resp) => resp,
            _ => {
                return Err(WolfUsbError::UnexpectedMessage {
                    expected: "HelloResponse".to_string(),
                    got: format!("{response:?}"),
                })
            }
        };

        if !hello_resp.auth_accepted {
            return Err(WolfUsbError::AuthenticationFailed(
                hello_resp
                    .error_message
                    .unwrap_or_else(|| "Unknown auth error".to_string()),
            ));
        }

        // Verify server's HMAC if we have a shared key
        if let Some(key) = shared_key {
            use hmac::{Hmac, Mac};
            use sha2::Sha256;

            type HmacSha256 = Hmac<Sha256>;
            let mut mac =
                HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            mac.update(&auth_nonce);
            mac.update(b"wolfusb-server");
            if mac
                .verify_slice(&hello_resp.auth_challenge_response)
                .is_err()
            {
                return Err(WolfUsbError::AuthenticationFailed(
                    "Server HMAC verification failed".to_string(),
                ));
            }
        }

        if hello_resp.protocol_version != PROTOCOL_VERSION {
            return Err(WolfUsbError::VersionMismatch {
                local: PROTOCOL_VERSION,
                remote: hello_resp.protocol_version,
            });
        }

        debug!("Connected to server: {}", hello_resp.server_name);

        Ok(Self {
            framed,
            server_name: hello_resp.server_name,
        })
    }

    async fn send_and_recv(&mut self, msg: Message) -> Result<Message> {
        self.framed.send(msg).await?;
        self.framed
            .next()
            .await
            .ok_or(WolfUsbError::ConnectionClosed)?
    }

    pub async fn list_devices(&mut self) -> Result<Vec<DeviceInfo>> {
        let response = self.send_and_recv(Message::ListDevices).await?;
        match response {
            Message::DeviceList(resp) => Ok(resp.devices),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "DeviceList".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn get_descriptors(
        &mut self,
        device_id: DeviceId,
    ) -> Result<DeviceDescriptorTree> {
        let response = self
            .send_and_recv(Message::GetDescriptors(GetDescriptorsRequest {
                device_id,
            }))
            .await?;
        match response {
            Message::DescriptorData(resp) => Ok(resp.descriptors),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "DescriptorData".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn attach(&mut self, device_id: DeviceId) -> Result<u64> {
        let response = self
            .send_and_recv(Message::Attach(AttachRequest { device_id }))
            .await?;
        match response {
            Message::AttachResult(resp) if resp.success => Ok(resp.session_id.unwrap()),
            Message::AttachResult(resp) => Err(WolfUsbError::ProtocolError(
                resp.error_message.unwrap_or_else(|| "Attach failed".to_string()),
            )),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "AttachResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn detach(&mut self, device_id: DeviceId, session_id: u64) -> Result<()> {
        let response = self
            .send_and_recv(Message::Detach(DetachRequest {
                device_id,
                session_id,
            }))
            .await?;
        match response {
            Message::DetachResult(resp) if resp.success => Ok(()),
            Message::DetachResult(resp) => Err(WolfUsbError::ProtocolError(
                resp.error_message.unwrap_or_else(|| "Detach failed".to_string()),
            )),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "DetachResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn control_transfer(
        &mut self,
        req: ControlTransferRequest,
    ) -> Result<TransferResponse> {
        let response = self
            .send_and_recv(Message::ControlTransfer(req))
            .await?;
        match response {
            Message::TransferResult(resp) => Ok(resp),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "TransferResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn bulk_transfer(
        &mut self,
        req: BulkTransferRequest,
    ) -> Result<TransferResponse> {
        let response = self.send_and_recv(Message::BulkTransfer(req)).await?;
        match response {
            Message::TransferResult(resp) => Ok(resp),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "TransferResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn interrupt_transfer(
        &mut self,
        req: InterruptTransferRequest,
    ) -> Result<TransferResponse> {
        let response = self
            .send_and_recv(Message::InterruptTransfer(req))
            .await?;
        match response {
            Message::TransferResult(resp) => Ok(resp),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "TransferResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn claim_interface(
        &mut self,
        session_id: u64,
        device_id: DeviceId,
        interface_number: u8,
    ) -> Result<()> {
        let response = self
            .send_and_recv(Message::ClaimInterface(ClaimInterfaceRequest {
                session_id,
                device_id,
                interface_number,
            }))
            .await?;
        match response {
            Message::ClaimInterfaceResult(resp) if resp.success => Ok(()),
            Message::ClaimInterfaceResult(resp) => Err(WolfUsbError::ProtocolError(
                resp.error_message
                    .unwrap_or_else(|| "Claim interface failed".to_string()),
            )),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "ClaimInterfaceResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn release_interface(
        &mut self,
        session_id: u64,
        device_id: DeviceId,
        interface_number: u8,
    ) -> Result<()> {
        let response = self
            .send_and_recv(Message::ReleaseInterface(ReleaseInterfaceRequest {
                session_id,
                device_id,
                interface_number,
            }))
            .await?;
        match response {
            Message::ReleaseInterfaceResult(resp) if resp.success => Ok(()),
            Message::ReleaseInterfaceResult(resp) => Err(WolfUsbError::ProtocolError(
                resp.error_message
                    .unwrap_or_else(|| "Release interface failed".to_string()),
            )),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "ReleaseInterfaceResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn set_configuration(
        &mut self,
        session_id: u64,
        device_id: DeviceId,
        configuration: u8,
    ) -> Result<()> {
        let response = self
            .send_and_recv(Message::SetConfiguration(SetConfigurationRequest {
                session_id,
                device_id,
                configuration,
            }))
            .await?;
        match response {
            Message::SetConfigurationResult(resp) if resp.success => Ok(()),
            Message::SetConfigurationResult(resp) => Err(WolfUsbError::ProtocolError(
                resp.error_message
                    .unwrap_or_else(|| "Set configuration failed".to_string()),
            )),
            Message::Error(e) => Err(WolfUsbError::ProtocolError(e.message)),
            other => Err(WolfUsbError::UnexpectedMessage {
                expected: "SetConfigurationResult".to_string(),
                got: format!("{other:?}"),
            }),
        }
    }
}
