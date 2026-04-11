// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::sync::Arc;

use log::info;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::connection::Connection;
use super::device_manager::DeviceManager;
use crate::error::Result;

pub async fn run_server(bind_addr: &str, port: u16, shared_key: Option<Vec<u8>>) -> Result<()> {
    let device_manager = Arc::new(Mutex::new(DeviceManager::new()?));
    let addr = format!("{bind_addr}:{port}");
    let listener = TcpListener::bind(&addr).await?;

    info!("wolfusb server listening on {addr}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let dm = device_manager.clone();
        let key = shared_key.clone();

        tokio::spawn(async move {
            let mut conn = Connection::new(stream, dm, peer_addr, key);
            conn.run().await;
        });
    }
}
