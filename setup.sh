#!/usr/bin/env bash
# (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
#
# WolfUSB installer — Linux only, static musl binaries, distro-agnostic.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/setup.sh | bash
#
# Options:
#   --install-dir <DIR>  Where to install (default: /usr/local/bin if root, else ~/.local/bin)
#   --help               Show this help

set -euo pipefail

REPO="wolfsoftwaresystemsltd/wolfusb"
INSTALL_DIR=""

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-dir) INSTALL_DIR="$2"; shift 2 ;;
        --help|-h)
            echo "WolfUSB installer"
            echo ""
            echo "Usage: setup.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --install-dir <DIR>  Install directory (default: /usr/local/bin or ~/.local/bin)"
            echo "  --help               Show this help"
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# --- Platform check ---
if [[ "$(uname -s)" != "Linux" ]]; then
    error "WolfUSB supports Linux only (needs vhci_hcd kernel module)"
    exit 1
fi

case "$(uname -m)" in
    x86_64|amd64)   ARCH="x86_64" ;;
    aarch64|arm64)  ARCH="aarch64" ;;
    *)
        error "Unsupported architecture: $(uname -m) (only x86_64 and aarch64 are supported)"
        exit 1
        ;;
esac

# --- Install dir ---
if [[ -z "$INSTALL_DIR" ]]; then
    if [[ $EUID -eq 0 ]]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
    fi
fi
mkdir -p "$INSTALL_DIR"

info "Installing wolfusb for Linux $ARCH to $INSTALL_DIR..."

# --- Download from /releases/latest/download/ (follows redirect to newest tag) ---
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/wolfusb-${ARCH}"
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

if ! curl -fsSL --connect-timeout 15 --max-time 300 --retry 2 -o "$TMPFILE" "$DOWNLOAD_URL"; then
    error "Failed to download $DOWNLOAD_URL"
    error "Check https://github.com/${REPO}/releases for available builds"
    exit 1
fi

# Verify the file is a binary (not an HTML error page)
if ! file "$TMPFILE" 2>/dev/null | grep -q "ELF"; then
    if [[ $(head -c 200 "$TMPFILE" 2>/dev/null) == *"<html"* ]]; then
        error "Download returned HTML (release not found?). URL: $DOWNLOAD_URL"
    else
        error "Downloaded file is not a valid Linux binary"
    fi
    exit 1
fi

# --- Install ---
if [[ -w "$INSTALL_DIR" ]]; then
    install -m 0755 "$TMPFILE" "$INSTALL_DIR/wolfusb"
else
    info "Need sudo to write to $INSTALL_DIR"
    sudo install -m 0755 "$TMPFILE" "$INSTALL_DIR/wolfusb"
fi

# --- Verify ---
if "$INSTALL_DIR/wolfusb" --version >/dev/null 2>&1; then
    info "Installed: $("$INSTALL_DIR/wolfusb" --version)"
else
    warn "Binary installed but --version failed. Check: $INSTALL_DIR/wolfusb"
fi

echo ""
echo "Quick start:"
echo "  wolfusb server                              # start server"
echo "  wolfusb list --server <host>:3240           # list remote devices"
echo "  wolfusb mount --server <host>:3240 \\"
echo "                --bus 1 --addr 2              # mount as virtual USB device"
echo ""
echo "To install as a systemd service:"
echo "  sudo curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install-service.sh | bash"
