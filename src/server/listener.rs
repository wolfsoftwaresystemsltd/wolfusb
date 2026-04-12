// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::sync::Arc;

use log::{info, warn};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_rustls::TlsAcceptor;

use super::connection::{Connection, ServerStream};
use super::device_manager::DeviceManager;
use crate::error::Result;

pub async fn run_server(
    bind_addr: &str,
    port: u16,
    shared_key: Option<Vec<u8>>,
    tls_acceptor: Option<TlsAcceptor>,
) -> Result<()> {
    let device_manager = Arc::new(Mutex::new(DeviceManager::new()?));
    let addr = format!("{bind_addr}:{port}");
    let listener = TcpListener::bind(&addr).await?;

    let tls_mode = if tls_acceptor.is_some() {
        "TLS"
    } else {
        "plain"
    };
    info!("wolfusb server listening on {addr} ({tls_mode})");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let dm = device_manager.clone();
        let key = shared_key.clone();
        let acceptor = tls_acceptor.clone();

        tokio::spawn(async move {
            if let Some(acceptor) = acceptor {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        let conn = Connection::new(
                            ServerStream::Tls(Box::new(tls_stream)),
                            dm,
                            peer_addr,
                            key,
                        );
                        conn.run().await;
                    }
                    Err(e) => {
                        warn!("TLS handshake failed for {peer_addr}: {e}");
                    }
                }
            } else {
                let conn = Connection::new(ServerStream::Plain(stream), dm, peer_addr, key);
                conn.run().await;
            }
        });
    }
}
