// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wolfusb", version, about = "Share USB devices over IP")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Common TLS options for client commands.
#[derive(clap::Args, Clone)]
struct ClientTlsArgs {
    /// Enable TLS encryption
    #[arg(long)]
    tls: bool,
    /// Path to CA certificate PEM file for TLS verification
    #[arg(long)]
    tls_ca: Option<PathBuf>,
    /// Skip TLS certificate verification (insecure)
    #[arg(long)]
    tls_insecure: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the wolfusb server
    Server {
        /// Address to bind to
        #[arg(short, long, default_value = "0.0.0.0")]
        bind: String,
        /// Port to listen on
        #[arg(short, long, default_value_t = 3240)]
        port: u16,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        /// Path to TLS certificate PEM file
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        /// Path to TLS private key PEM file
        #[arg(long)]
        tls_key: Option<PathBuf>,
    },

    /// List remote USB devices
    List {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Show detailed device descriptors
    Info {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Attach to a remote USB device
    Attach {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Detach from a remote USB device
    Detach {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// Session ID from attach
        #[arg(long)]
        session_id: u64,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Perform a USB control transfer
    Control {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// Session ID from attach
        #[arg(long)]
        session_id: u64,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// bmRequestType byte
        #[arg(long)]
        request_type: String,
        /// bRequest byte
        #[arg(long)]
        request: String,
        /// wValue
        #[arg(long)]
        value: String,
        /// wIndex
        #[arg(long)]
        index: String,
        /// Max bytes to read (IN transfers)
        #[arg(long, default_value = "0")]
        length: u16,
        /// Hex data to send (OUT transfers)
        #[arg(long)]
        data: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "5000")]
        timeout: u64,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Perform a USB bulk transfer
    Bulk {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// Session ID from attach
        #[arg(long)]
        session_id: u64,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// Endpoint address (bit 7 = direction: 0x80=IN)
        #[arg(long)]
        endpoint: String,
        /// Max bytes to read (IN transfers)
        #[arg(long, default_value = "0")]
        length: u32,
        /// Hex data to send (OUT transfers)
        #[arg(long)]
        data: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "5000")]
        timeout: u64,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },

    /// Perform a USB interrupt transfer
    Interrupt {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// Session ID from attach
        #[arg(long)]
        session_id: u64,
        /// USB bus number
        #[arg(long)]
        bus: u8,
        /// USB device address
        #[arg(long)]
        addr: u8,
        /// Endpoint address (bit 7 = direction: 0x80=IN)
        #[arg(long)]
        endpoint: String,
        /// Max bytes to read (IN transfers)
        #[arg(long, default_value = "0")]
        length: u32,
        /// Hex data to send (OUT transfers)
        #[arg(long)]
        data: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "5000")]
        timeout: u64,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
        #[command(flatten)]
        tls_args: ClientTlsArgs,
    },
}

/// Parse a value that may be hex (0x prefix) or decimal.
fn parse_u8_hex(s: &str) -> anyhow::Result<u8> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Ok(u8::from_str_radix(hex, 16)?)
    } else {
        Ok(s.parse::<u8>()?)
    }
}

fn parse_u16_hex(s: &str) -> anyhow::Result<u16> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Ok(u16::from_str_radix(hex, 16)?)
    } else {
        Ok(s.parse::<u16>()?)
    }
}

fn build_tls_connector(
    tls_args: &ClientTlsArgs,
) -> anyhow::Result<Option<tokio_rustls::TlsConnector>> {
    if !tls_args.tls && tls_args.tls_ca.is_none() && !tls_args.tls_insecure {
        return Ok(None);
    }
    Ok(Some(wolfusb::tls::client_connector(
        tls_args.tls_ca.as_deref(),
        tls_args.tls_insecure,
    )?))
}

async fn client_session(
    server: &str,
    key: Option<&String>,
    tls_args: &ClientTlsArgs,
) -> anyhow::Result<wolfusb::client::session::Session> {
    let key_bytes = key.map(|k| k.as_bytes());
    let tls = build_tls_connector(tls_args)?;
    Ok(wolfusb::client::session::Session::connect(server, "wolfusb-cli", key_bytes, tls).await?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server {
            bind,
            port,
            key,
            tls_cert,
            tls_key,
        } => {
            let shared_key = key.map(|k| k.into_bytes());
            let tls_acceptor = match (tls_cert, tls_key) {
                (Some(cert), Some(key)) => Some(wolfusb::tls::server_acceptor(&cert, &key)?),
                (None, None) => None,
                _ => anyhow::bail!("Both --tls-cert and --tls-key must be provided together"),
            };
            wolfusb::server::listener::run_server(&bind, port, shared_key, tls_acceptor).await?;
        }

        Commands::List {
            server,
            key,
            json,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_list(&mut session, json).await?;
        }

        Commands::Info {
            server,
            bus,
            addr,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_info(&mut session, bus, addr).await?;
        }

        Commands::Attach {
            server,
            bus,
            addr,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_attach(&mut session, bus, addr).await?;
        }

        Commands::Detach {
            server,
            bus,
            addr,
            session_id,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_detach(&mut session, bus, addr, session_id).await?;
        }

        Commands::Control {
            server,
            session_id,
            bus,
            addr,
            request_type,
            request,
            value,
            index,
            length,
            data,
            timeout,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_control(
                &mut session,
                session_id,
                bus,
                addr,
                parse_u8_hex(&request_type)?,
                parse_u8_hex(&request)?,
                parse_u16_hex(&value)?,
                parse_u16_hex(&index)?,
                length,
                data.as_deref(),
                timeout,
            )
            .await?;
        }

        Commands::Bulk {
            server,
            session_id,
            bus,
            addr,
            endpoint,
            length,
            data,
            timeout,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_bulk(
                &mut session,
                session_id,
                bus,
                addr,
                parse_u8_hex(&endpoint)?,
                length,
                data.as_deref(),
                timeout,
            )
            .await?;
        }

        Commands::Interrupt {
            server,
            session_id,
            bus,
            addr,
            endpoint,
            length,
            data,
            timeout,
            key,
            tls_args,
        } => {
            let mut session = client_session(&server, key.as_ref(), &tls_args).await?;
            wolfusb::client::commands::cmd_interrupt(
                &mut session,
                session_id,
                bus,
                addr,
                parse_u8_hex(&endpoint)?,
                length,
                data.as_deref(),
                timeout,
            )
            .await?;
        }
    }

    Ok(())
}
