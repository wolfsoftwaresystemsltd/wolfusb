#!/usr/bin/env bash
# (C) Copyright Wolf Software Systems Ltd - https://wolf.uk.com
#
# Universal installer for wolfusb.
# Downloads the latest release binary for your platform and installs it.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/wolfusb/main/setup.sh | bash
#
#   Or download and run manually:
#     chmod +x setup.sh
#     ./setup.sh [OPTIONS]
#
# Options:
#   --version <TAG>     Install a specific version (default: latest)
#   --install-dir <DIR> Where to install (default: /usr/local/bin or ~/.local/bin)
#   --help              Show this help message

set -euo pipefail

REPO="wolfsoftwaresystemsltd/wolfusb"
VERSION=""
INSTALL_DIR=""

# --- Colours ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

usage() {
    echo "wolfusb installer"
    echo ""
    echo "Usage: setup.sh [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --version <TAG>      Install a specific version (e.g. v0.1.0)"
    echo "  --install-dir <DIR>  Install directory (default: /usr/local/bin or ~/.local/bin)"
    echo "  --help               Show this help"
    exit 0
}

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)     VERSION="$2";     shift 2 ;;
        --install-dir) INSTALL_DIR="$2"; shift 2 ;;
        --help|-h)     usage ;;
        *)
            error "Unknown option: $1"
            echo "Run with --help for usage."
            exit 1
            ;;
    esac
done

# --- Detect platform ---
detect_platform() {
    local os arch target

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64|amd64)    target="x86_64-unknown-linux-gnu" ;;
                aarch64|arm64)   target="aarch64-unknown-linux-gnu" ;;
                armv7l|armhf)    target="armv7-unknown-linux-gnueabihf" ;;
                *)
                    error "Unsupported Linux architecture: $arch"
                    exit 1
                    ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64|amd64)    target="x86_64-apple-darwin" ;;
                aarch64|arm64)   target="aarch64-apple-darwin" ;;
                *)
                    error "Unsupported macOS architecture: $arch"
                    exit 1
                    ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            target="x86_64-pc-windows-msvc"
            ;;
        *)
            error "Unsupported operating system: $os"
            exit 1
            ;;
    esac

    echo "$target"
}

# --- Detect archive extension ---
archive_ext() {
    case "$1" in
        *windows*) echo "zip" ;;
        *)         echo "tar.gz" ;;
    esac
}

# --- Choose install directory ---
choose_install_dir() {
    if [[ -n "$INSTALL_DIR" ]]; then
        echo "$INSTALL_DIR"
        return
    fi

    if [[ $EUID -eq 0 ]]; then
        echo "/usr/local/bin"
    elif [[ -w /usr/local/bin ]]; then
        echo "/usr/local/bin"
    else
        local dir="${HOME}/.local/bin"
        mkdir -p "$dir"
        echo "$dir"
    fi
}

# --- Fetch latest version tag ---
fetch_latest_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest"
    local tag

    if command -v curl &>/dev/null; then
        tag=$(curl -fsSL "$url" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
    elif command -v wget &>/dev/null; then
        tag=$(wget -qO- "$url" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
    else
        error "Neither curl nor wget found. Install one and try again."
        exit 1
    fi

    if [[ -z "$tag" ]]; then
        error "Could not determine latest version. Specify one with --version."
        exit 1
    fi

    echo "$tag"
}

# --- Download file ---
download() {
    local url="$1"
    local dest="$2"

    info "Downloading: $url"
    if command -v curl &>/dev/null; then
        curl -fsSL -o "$dest" "$url"
    elif command -v wget &>/dev/null; then
        wget -qO "$dest" "$url"
    fi
}

# --- Check dependencies ---
check_deps() {
    local os="$(uname -s)"

    case "$os" in
        Linux)
            if ! ldconfig -p 2>/dev/null | grep -q libusb-1.0; then
                warn "libusb-1.0 not found. wolfusb requires libusb to run."
                echo ""
                echo "  Install it with:"
                echo "    Debian/Ubuntu:  sudo apt install libusb-1.0-0"
                echo "    Fedora:         sudo dnf install libusb1"
                echo "    Arch:           sudo pacman -S libusb"
                echo ""
            fi
            ;;
        Darwin)
            if ! brew list libusb &>/dev/null 2>&1; then
                warn "libusb not found. wolfusb requires libusb to run."
                echo "  Install it with: brew install libusb"
                echo ""
            fi
            ;;
    esac
}

# --- Main ---
main() {
    echo -e "${CYAN}"
    echo "  wolfusb installer"
    echo "  (C) Copyright Wolf Software Systems Ltd"
    echo "  https://wolf.uk.com"
    echo -e "${NC}"

    local target
    target=$(detect_platform)
    info "Detected platform: $target"

    local ext
    ext=$(archive_ext "$target")

    if [[ -z "$VERSION" ]]; then
        info "Fetching latest version..."
        VERSION=$(fetch_latest_version)
    fi
    info "Version: $VERSION"

    local install_dir
    install_dir=$(choose_install_dir)
    info "Install directory: $install_dir"

    # Download
    local archive_name="wolfusb-${VERSION}-${target}.${ext}"
    local download_url="https://github.com/${REPO}/releases/download/${VERSION}/${archive_name}"
    WOLFUSB_TMPDIR=$(mktemp -d)
    trap 'rm -rf "$WOLFUSB_TMPDIR"' EXIT
    local tmpdir="$WOLFUSB_TMPDIR"

    download "$download_url" "${tmpdir}/${archive_name}"

    # Extract
    info "Extracting..."
    case "$ext" in
        tar.gz)
            tar xzf "${tmpdir}/${archive_name}" -C "$tmpdir"
            ;;
        zip)
            if command -v unzip &>/dev/null; then
                unzip -q "${tmpdir}/${archive_name}" -d "$tmpdir"
            elif command -v 7z &>/dev/null; then
                7z x -o"$tmpdir" "${tmpdir}/${archive_name}" > /dev/null
            else
                error "No unzip or 7z found to extract archive."
                exit 1
            fi
            ;;
    esac

    # Install
    local binary_name="wolfusb"
    if [[ "$target" == *windows* ]]; then
        binary_name="wolfusb.exe"
    fi

    if [[ ! -f "${tmpdir}/${binary_name}" ]]; then
        error "Binary not found in archive."
        exit 1
    fi

    info "Installing to ${install_dir}/${binary_name}"
    mkdir -p "$install_dir"

    if [[ -w "$install_dir" ]]; then
        install -m 755 "${tmpdir}/${binary_name}" "${install_dir}/${binary_name}"
    else
        info "Requesting sudo to install to ${install_dir}..."
        sudo install -m 755 "${tmpdir}/${binary_name}" "${install_dir}/${binary_name}"
    fi

    # Verify
    if "${install_dir}/${binary_name}" --version &>/dev/null; then
        local ver
        ver=$("${install_dir}/${binary_name}" --version)
        info "Installed: $ver"
    else
        info "Installed: ${install_dir}/${binary_name}"
    fi

    # Check PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$install_dir"; then
        warn "${install_dir} is not in your PATH."
        echo ""
        echo "  Add it with:"
        echo "    export PATH=\"${install_dir}:\$PATH\""
        echo ""
        echo "  Or add that line to your ~/.bashrc / ~/.zshrc / ~/.config/fish/config.fish"
        echo ""
    fi

    # Check runtime dependencies
    check_deps

    echo ""
    info "Installation complete."
    echo ""
    echo "  Quick start:"
    echo "    wolfusb server                              # start server"
    echo "    wolfusb list --server <host>:3240           # list remote devices"
    echo "    wolfusb attach --server <host>:3240 --bus 1 --addr 2"
    echo ""
    echo "  Full documentation: https://github.com/${REPO}"
    echo ""

    # Offer service install on Linux
    if [[ "$(uname -s)" == "Linux" ]] && command -v systemctl &>/dev/null; then
        echo "  To install as a systemd service:"
        echo "    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install-service.sh -o install-service.sh"
        echo "    chmod +x install-service.sh"
        echo "    sudo ./install-service.sh --binary ${install_dir}/wolfusb"
        echo ""
    fi
}

main
