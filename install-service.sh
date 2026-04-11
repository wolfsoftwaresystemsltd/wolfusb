#!/usr/bin/env bash
# (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
#
# Install wolfusb as a systemd service on Linux.
#
# Usage:
#   sudo ./install-service.sh [OPTIONS]
#   sudo ./install-service.sh --uninstall
#
# Options:
#   --bind <ADDR>       Address to bind to        (default: 0.0.0.0)
#   --port <PORT>       TCP port                   (default: 3240)
#   --key <KEY>         Pre-shared auth key         (default: none)
#   --binary <PATH>     Path to wolfusb binary     (default: ./target/release/wolfusb)
#   --user <USER>       Service user               (default: wolfusb)
#   --install-dir <DIR> Binary install directory   (default: /usr/local/bin)
#   --build             Build release binary first
#   --uninstall         Remove the service and user
#   --help              Show this help message

set -euo pipefail

# --- Defaults ---
BIND="0.0.0.0"
PORT="3240"
KEY=""
BINARY="./target/release/wolfusb"
SERVICE_USER="wolfusb"
INSTALL_DIR="/usr/local/bin"
BUILD=false
UNINSTALL=false

SERVICE_NAME="wolfusb"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
ENV_FILE="/etc/wolfusb/wolfusb.env"

# --- Colours ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

usage() {
    sed -n '3,/^$/s/^# \?//p' "$0"
    exit 0
}

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --bind)       BIND="$2";        shift 2 ;;
        --port)       PORT="$2";        shift 2 ;;
        --key)        KEY="$2";         shift 2 ;;
        --binary)     BINARY="$2";      shift 2 ;;
        --user)       SERVICE_USER="$2"; shift 2 ;;
        --install-dir) INSTALL_DIR="$2"; shift 2 ;;
        --build)      BUILD=true;       shift ;;
        --uninstall)  UNINSTALL=true;   shift ;;
        --help|-h)    usage ;;
        *)
            error "Unknown option: $1"
            echo "Run with --help for usage."
            exit 1
            ;;
    esac
done

# --- Root check ---
if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root (use sudo)."
    exit 1
fi

# --- Uninstall ---
if $UNINSTALL; then
    info "Uninstalling wolfusb service..."

    if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
        info "Stopping service..."
        systemctl stop "$SERVICE_NAME"
    fi

    if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
        info "Disabling service..."
        systemctl disable "$SERVICE_NAME"
    fi

    if [[ -f "$SERVICE_FILE" ]]; then
        info "Removing service file..."
        rm -f "$SERVICE_FILE"
        systemctl daemon-reload
    fi

    if [[ -f "${INSTALL_DIR}/wolfusb" ]]; then
        info "Removing binary..."
        rm -f "${INSTALL_DIR}/wolfusb"
    fi

    if [[ -d /etc/wolfusb ]]; then
        info "Removing configuration..."
        rm -rf /etc/wolfusb
    fi

    if id "$SERVICE_USER" &>/dev/null; then
        info "Removing user ${SERVICE_USER}..."
        userdel "$SERVICE_USER" 2>/dev/null || true
    fi

    info "wolfusb service uninstalled."
    exit 0
fi

# --- Build if requested ---
if $BUILD; then
    info "Building release binary..."
    if ! command -v cargo &>/dev/null; then
        error "cargo not found. Install Rust: https://rustup.rs/"
        exit 1
    fi
    # Build as the calling user, not root
    SUDO_USER_HOME=$(eval echo "~${SUDO_USER:-$USER}")
    sudo -u "${SUDO_USER:-$USER}" \
        env HOME="$SUDO_USER_HOME" \
        cargo build --release
    BINARY="./target/release/wolfusb"
fi

# --- Validate binary ---
if [[ ! -f "$BINARY" ]]; then
    error "Binary not found: $BINARY"
    echo "  Build first with: cargo build --release"
    echo "  Or pass --build to build automatically."
    exit 1
fi

if [[ ! -x "$BINARY" ]]; then
    chmod +x "$BINARY"
fi

# --- Create service user ---
if ! id "$SERVICE_USER" &>/dev/null; then
    info "Creating service user: ${SERVICE_USER}"
    useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# --- Install binary ---
info "Installing binary to ${INSTALL_DIR}/wolfusb"
install -m 755 "$BINARY" "${INSTALL_DIR}/wolfusb"

# --- Create config directory and environment file ---
info "Creating configuration in /etc/wolfusb/"
mkdir -p /etc/wolfusb
cat > "$ENV_FILE" <<EOF
# wolfusb service configuration
# (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
#
# Edit this file to change service parameters, then restart:
#   sudo systemctl restart wolfusb

# Address to bind to
WOLFUSB_BIND=${BIND}

# TCP port
WOLFUSB_PORT=${PORT}

# Pre-shared authentication key (leave empty for no auth)
WOLFUSB_KEY=${KEY}

# Log level: error, warn, info, debug, trace
RUST_LOG=info
EOF

chmod 640 "$ENV_FILE"
chown root:"$SERVICE_USER" "$ENV_FILE"

# --- Create systemd service ---
info "Creating systemd service: ${SERVICE_FILE}"
cat > "$SERVICE_FILE" <<EOF
# wolfusb - USB over IP sharing service
# (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
#
# Configuration: /etc/wolfusb/wolfusb.env

[Unit]
Description=wolfusb - Share USB devices over IP
Documentation=https://wolf.uk.com
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
EnvironmentFile=${ENV_FILE}

ExecStart=${INSTALL_DIR}/wolfusb server --bind \${WOLFUSB_BIND} --port \${WOLFUSB_PORT}

# Restart on failure
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictNamespaces=true
RestrictRealtime=true
MemoryDenyWriteExecute=true

# Allow USB device access
SupplementaryGroups=plugdev

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=wolfusb

[Install]
WantedBy=multi-user.target
EOF

# --- Reload and enable ---
info "Reloading systemd..."
systemctl daemon-reload

info "Enabling service..."
systemctl enable "$SERVICE_NAME"

info "Starting service..."
systemctl start "$SERVICE_NAME"

# --- Verify ---
sleep 1
if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "wolfusb service is running."
else
    warn "Service may have failed to start. Check logs:"
    echo "  sudo journalctl -u wolfusb -n 20"
fi

echo ""
info "Installation complete."
echo ""
echo "  Service status:   sudo systemctl status wolfusb"
echo "  View logs:        sudo journalctl -u wolfusb -f"
echo "  Edit config:      sudo nano /etc/wolfusb/wolfusb.env"
echo "  Restart service:  sudo systemctl restart wolfusb"
echo "  Stop service:     sudo systemctl stop wolfusb"
echo "  Uninstall:        sudo ./install-service.sh --uninstall"
echo ""
echo "  Listening on:     ${BIND}:${PORT}"
if [[ -n "$KEY" ]]; then
    echo "  Authentication:   enabled (key set)"
else
    echo "  Authentication:   disabled (no key set)"
fi
echo ""
echo "  Note: The service user '${SERVICE_USER}' needs USB access."
echo "  Add a udev rule to grant access:"
echo "    echo 'SUBSYSTEM==\"usb\", MODE=\"0666\", GROUP=\"plugdev\"' | \\"
echo "      sudo tee /etc/udev/rules.d/99-wolfusb.rules"
echo "    sudo udevadm control --reload-rules && sudo udevadm trigger"
