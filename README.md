# wolfusb

[![Sponsor](https://img.shields.io/badge/Sponsor-%E2%9D%A4-pink?style=for-the-badge&logo=github)](https://github.com/sponsors/wolfsoftwaresystemsltd)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/wolfsoftwaresystemsltd/wolfusb/ci.yml?style=for-the-badge&label=CI)](https://github.com/wolfsoftwaresystemsltd/wolfusb/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/wolfsoftwaresystemsltd/wolfusb?style=for-the-badge)](https://github.com/wolfsoftwaresystemsltd/wolfusb/releases/latest)

Share USB devices over IP. A server exposes locally connected USB devices over TCP, and clients on any machine perform remote USB operations -- listing, attaching, and executing control, bulk, and interrupt transfers.

## Installation

### Quick install (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/setup.sh | bash
```

This automatically detects your platform, downloads the latest release binary, and installs it.

Options:

```bash
# Install a specific version
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/setup.sh | bash -s -- --version v0.1.0

# Install to a custom directory
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/setup.sh | bash -s -- --install-dir /opt/bin
```

### Download from releases

Pre-built binaries for all platforms are available on the [Releases](https://github.com/wolfsoftwaresystemsltd/wolfusb/releases) page:

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `wolfusb-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `wolfusb-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Linux ARMv7 (Raspberry Pi) | `wolfusb-<version>-armv7-unknown-linux-gnueabihf.tar.gz` |
| macOS Intel | `wolfusb-<version>-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `wolfusb-<version>-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `wolfusb-<version>-x86_64-pc-windows-msvc.zip` |

### Install as a Linux systemd service

```bash
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/install-service.sh -o install-service.sh
chmod +x install-service.sh
sudo ./install-service.sh --build --port 3240 --key "my-secret"
```

See `sudo ./install-service.sh --help` for all options, or `sudo ./install-service.sh --uninstall` to remove.

### Build from source

Requires [Rust](https://rustup.rs/) 1.85+ and libusb development headers:

```bash
# Debian/Ubuntu
sudo apt install libusb-1.0-0-dev pkg-config

# Fedora
sudo dnf install libusb1-devel

# Arch
sudo pacman -S libusb

# macOS
brew install libusb
```

Windows: install [libusb](https://libusb.info/) and ensure the device has a WinUSB-compatible driver (use [Zadig](https://zadig.akeo.ie/)).

```bash
git clone https://github.com/wolfsoftwaresystemsltd/wolfusb.git
cd wolfusb
cargo build --release
```

The binary is at `target/release/wolfusb`.

### Runtime dependencies

The pre-built binaries require libusb to be installed at runtime:

```bash
# Debian/Ubuntu
sudo apt install libusb-1.0-0

# Fedora
sudo dnf install libusb1

# Arch
sudo pacman -S libusb

# macOS
brew install libusb
```

## Quick start

**1. Start the server** on the machine with USB devices:

```bash
wolfusb server
```

This binds to `0.0.0.0:3240` by default.

**2. List devices** from any machine on the network:

```bash
wolfusb list --server 192.168.1.100:3240
```

Output:

```
Bus:Addr VID:PID   Speed  Class      Manufacturer             Product                  Serial
----------------------------------------------------------------------------------------------------
1:2      046d:c52b 12M    Per-Iface  Logitech                 USB Receiver             -
1:5      8087:0026 12M    Wireless   Intel Corp.              AX201 Bluetooth          -
2:1      0bda:5411 480M   Hub        -                        -                        -
```

**3. Inspect a device** in detail:

```bash
wolfusb info --server 192.168.1.100:3240 --bus 1 --addr 2
```

**4. Attach** to claim exclusive access:

```bash
wolfusb attach --server 192.168.1.100:3240 --bus 1 --addr 2
```

Output:

```
Attached to 1:2, session_id = 1
```

**5. Perform USB transfers** using the session ID:

```bash
# Read the device descriptor (18 bytes, standard GET_DESCRIPTOR)
wolfusb control --server 192.168.1.100:3240 \
    --session-id 1 --bus 1 --addr 2 \
    --request-type 0x80 --request 0x06 \
    --value 0x0100 --index 0x00 --length 18
```

**6. Detach** when finished:

```bash
wolfusb detach --server 192.168.1.100:3240 \
    --bus 1 --addr 2 --session-id 1
```

## Commands

### server

Start the wolfusb server.

```
wolfusb server [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-b, --bind <ADDR>` | `0.0.0.0` | Address to bind to |
| `-p, --port <PORT>` | `3240` | TCP port |
| `--key <KEY>` | none | Pre-shared authentication key |

### list

List USB devices available on the remote server.

```
wolfusb list --server <HOST:PORT> [--key <KEY>]
```

### info

Show the full descriptor tree for a device: device descriptor, configurations, interfaces, and endpoints.

```
wolfusb info --server <HOST:PORT> --bus <BUS> --addr <ADDR> [--key <KEY>]
```

### attach

Claim exclusive access to a remote USB device. Returns a `session_id` required for all transfer commands. Only one client can attach to a device at a time. On Linux, kernel drivers are automatically detached.

```
wolfusb attach --server <HOST:PORT> --bus <BUS> --addr <ADDR> [--key <KEY>]
```

### detach

Release a previously attached device. Kernel drivers are reattached on Linux.

```
wolfusb detach --server <HOST:PORT> --bus <BUS> --addr <ADDR> --session-id <ID> [--key <KEY>]
```

If a client disconnects without detaching, the server automatically cleans up.

### control

Perform a USB control transfer. Supports both IN (read) and OUT (write) directions, determined by bit 7 of `--request-type`.

```
wolfusb control --server <HOST:PORT> --session-id <ID> --bus <BUS> --addr <ADDR> \
    --request-type <HEX> --request <HEX> --value <HEX> --index <HEX> \
    [--length <N>] [--data <HEX>] [--timeout <MS>] [--key <KEY>]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--request-type` | required | bmRequestType (hex). Bit 7: 0=OUT, 1=IN |
| `--request` | required | bRequest code (hex) |
| `--value` | required | wValue (hex) |
| `--index` | required | wIndex (hex) |
| `--length` | `0` | Bytes to read for IN transfers |
| `--data` | none | Hex payload for OUT transfers |
| `--timeout` | `5000` | Timeout in milliseconds |

Hex values accept `0x` prefix or plain hex digits.

**Common USB standard requests:**

```bash
# GET_DESCRIPTOR - Device (type 0x01)
wolfusb control -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --request-type 0x80 --request 0x06 --value 0x0100 --index 0x00 --length 18

# GET_DESCRIPTOR - Configuration (type 0x02)
wolfusb control -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --request-type 0x80 --request 0x06 --value 0x0200 --index 0x00 --length 255

# GET_DESCRIPTOR - String #1
wolfusb control -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --request-type 0x80 --request 0x06 --value 0x0301 --index 0x0409 --length 255

# GET_STATUS
wolfusb control -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --request-type 0x80 --request 0x00 --value 0x0000 --index 0x00 --length 2
```

### bulk

Perform a USB bulk transfer. Direction is determined by bit 7 of the endpoint address.

```
wolfusb bulk --server <HOST:PORT> --session-id <ID> --bus <BUS> --addr <ADDR> \
    --endpoint <HEX> [--length <N>] [--data <HEX>] [--timeout <MS>] [--key <KEY>]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--endpoint` | required | Endpoint address (hex). Bit 7: 0=OUT, 0x80=IN |
| `--length` | `0` | Bytes to read for IN transfers |
| `--data` | none | Hex payload for OUT transfers |
| `--timeout` | `5000` | Timeout in milliseconds |

```bash
# Read 512 bytes from bulk IN endpoint 0x81
wolfusb bulk -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --endpoint 0x81 --length 512

# Write data to bulk OUT endpoint 0x02
wolfusb bulk -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --endpoint 0x02 --data "48656c6c6f"
```

### interrupt

Perform a USB interrupt transfer. Same interface as `bulk`.

```
wolfusb interrupt --server <HOST:PORT> --session-id <ID> --bus <BUS> --addr <ADDR> \
    --endpoint <HEX> [--length <N>] [--data <HEX>] [--timeout <MS>] [--key <KEY>]
```

```bash
# Read 8 bytes from interrupt IN endpoint 0x81
wolfusb interrupt -s host:3240 --session-id 1 --bus 1 --addr 2 \
    --endpoint 0x81 --length 8
```

## Authentication

wolfusb supports optional HMAC-SHA256 pre-shared key authentication. When a key is set, the client and server perform a challenge-response handshake before any operations are allowed.

Set the key via `--key` flag or `WOLFUSB_KEY` environment variable:

```bash
# Server
export WOLFUSB_KEY="my-secret-key"
wolfusb server

# Client
export WOLFUSB_KEY="my-secret-key"
wolfusb list --server host:3240
```

Or per-command:

```bash
wolfusb server --key "my-secret-key"
wolfusb list --server host:3240 --key "my-secret-key"
```

Without a key, the server accepts all connections (suitable for trusted networks only).

## TLS Encryption

wolfusb supports TLS to encrypt all traffic between client and server.

### Generate a self-signed certificate (for testing)

```bash
openssl req -x509 -newkey rsa:4096 -keyout server.key -out server.crt \
    -days 365 -nodes -subj "/CN=wolfusb"
```

### Server with TLS

```bash
wolfusb server --tls-cert server.crt --tls-key server.key
```

### Client with TLS

```bash
# Trust a specific CA/self-signed cert
wolfusb list --server host:3240 --tls --tls-ca server.crt

# Skip verification (testing only)
wolfusb list --server host:3240 --tls --tls-insecure

# Use system CA roots (for proper certificates)
wolfusb list --server host:3240 --tls
```

### Combined with authentication

```bash
# Server: TLS + auth key
wolfusb server --tls-cert server.crt --tls-key server.key --key "secret"

# Client: TLS + auth key
wolfusb list --server host:3240 --tls --tls-ca server.crt --key "secret"
```

## Logging

wolfusb uses `env_logger`. Control verbosity with the `RUST_LOG` environment variable:

```bash
# Default (info level)
wolfusb server

# Debug logging
RUST_LOG=debug wolfusb server

# Trace logging (very verbose)
RUST_LOG=trace wolfusb server

# Only show warnings and errors
RUST_LOG=warn wolfusb server
```

## USB permissions

### Linux

By default, accessing USB devices requires root. To avoid running as root, create a udev rule:

```bash
# /etc/udev/rules.d/99-wolfusb.rules
# Allow all USB devices for the "plugdev" group
SUBSYSTEM=="usb", MODE="0666", GROUP="plugdev"
```

Then reload:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

For specific devices only:

```bash
# Allow a specific VID:PID
SUBSYSTEM=="usb", ATTR{idVendor}=="046d", ATTR{idProduct}=="c52b", MODE="0666"
```

### macOS

libusb generally works without extra permissions for most devices. Some devices may require disabling the macOS kernel driver by creating a codeless kext or using `kextunload`.

### Windows

Each device must have a WinUSB-compatible driver. Use [Zadig](https://zadig.akeo.ie/) to replace the default driver with WinUSB, libusb-win32, or libusbK.

## Protocol

wolfusb uses a custom binary protocol over TCP (default port 3240).

### Wire format

Each message is framed as:

```
[4 bytes: payload length (big-endian u32)][N bytes: bincode-encoded message]
```

Maximum frame size is 16 MiB.

### Message types

| Message | Direction | Description |
|---------|-----------|-------------|
| `Hello` / `HelloResponse` | C -> S / S -> C | Protocol handshake and authentication |
| `ListDevices` / `DeviceList` | C -> S / S -> C | Enumerate available USB devices |
| `GetDescriptors` / `DescriptorData` | C -> S / S -> C | Full descriptor tree for a device |
| `Attach` / `AttachResult` | C -> S / S -> C | Claim exclusive access to a device |
| `Detach` / `DetachResult` | C -> S / S -> C | Release a device |
| `ControlTransfer` / `TransferResult` | C -> S / S -> C | USB control transfer |
| `BulkTransfer` / `TransferResult` | C -> S / S -> C | USB bulk transfer |
| `InterruptTransfer` / `TransferResult` | C -> S / S -> C | USB interrupt transfer |
| `ClaimInterface` / `ClaimInterfaceResult` | C -> S / S -> C | Claim a USB interface |
| `ReleaseInterface` / `ReleaseInterfaceResult` | C -> S / S -> C | Release a USB interface |
| `SetConfiguration` / `SetConfigurationResult` | C -> S / S -> C | Set active USB configuration |
| `Ping` / `Pong` | C -> S / S -> C | Keepalive |
| `Error` | S -> C | Error response |

### Connection lifecycle

```
Client                          Server
  |--- Hello ------------------>|
  |<-- HelloResponse -----------|  (HMAC-SHA256 challenge-response if key set)
  |                             |
  |--- ListDevices ------------>|
  |<-- DeviceList --------------|
  |                             |
  |--- Attach ----------------->|  (server detaches kernel drivers)
  |<-- AttachResult ------------|  (returns session_id)
  |                             |
  |--- ControlTransfer -------->|  (proxied to physical USB device)
  |<-- TransferResult ----------|
  |                             |
  |--- Detach ----------------->|  (server reattaches kernel drivers)
  |<-- DetachResult ------------|
  |                             |
  |--- [disconnect] ----------->|  (server auto-cleans up any attached devices)
```

## Architecture

```
src/
  main.rs                   CLI entry point (clap subcommands)
  lib.rs                    Library root
  error.rs                  Error types (thiserror)
  protocol/
    mod.rs
    types.rs                Wire-safe USB descriptor types
    messages.rs             Protocol message definitions
    codec.rs                Length-prefixed bincode frame codec
  server/
    mod.rs
    listener.rs             TCP accept loop
    connection.rs           Per-client message dispatch
    device_manager.rs       USB device enumeration and lifecycle
    transfer.rs             USB transfer execution
  client/
    mod.rs
    session.rs              TCP connection and handshake
    commands.rs             CLI command implementations
    display.rs              Pretty-printing and hex dump
```

## Platform notes

| Feature | Linux | macOS | Windows |
|---------|-------|-------|---------|
| Device enumeration | Yes | Yes | Yes |
| Kernel driver detach | Yes | No (not supported by libusb) | No (not applicable) |
| Kernel driver reattach | Yes | No | No |
| All transfer types | Yes | Yes | Yes (with WinUSB driver) |
| Requires root/admin | No (with udev rules) | No (usually) | No (with WinUSB) |

On macOS and Windows, `detach_kernel_driver` failures are handled gracefully -- the operation is skipped without error.

## License

MIT

(C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
