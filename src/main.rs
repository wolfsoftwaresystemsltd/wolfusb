// (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wolfusb", version, about = "Share USB devices over IP")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    },

    /// List remote USB devices
    List {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
        /// Pre-shared authentication key
        #[arg(long, env = "WOLFUSB_KEY")]
        key: Option<String>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { bind, port, key } => {
            let shared_key = key.map(|k| k.into_bytes());
            wolfusb::server::listener::run_server(&bind, port, shared_key).await?;
        }

        Commands::List { server, key } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
            wolfusb::client::commands::cmd_list(&mut session).await?;
        }

        Commands::Info {
            server,
            bus,
            addr,
            key,
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
            wolfusb::client::commands::cmd_info(&mut session, bus, addr).await?;
        }

        Commands::Attach {
            server,
            bus,
            addr,
            key,
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
            wolfusb::client::commands::cmd_attach(&mut session, bus, addr).await?;
        }

        Commands::Detach {
            server,
            bus,
            addr,
            session_id,
            key,
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
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
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
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
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
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
        } => {
            let key_bytes = key.as_deref().map(|k| k.as_bytes());
            let mut session =
                wolfusb::client::session::Session::connect(&server, "wolfusb-cli", key_bytes)
                    .await?;
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
