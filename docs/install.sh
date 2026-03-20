#!/bin/bash
# polyplugc installer - downloads the CLI binary for your platform
# Usage: curl -fsSL https://polyplug.github.io/install.sh | bash

set -e

REPO="polyplug/polyplug"
BINARY_NAME="polyplugc"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

get_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)
            case "$ARCH" in
                x86_64|amd64) echo "linux-x64" ;;
                aarch64|arm64) echo "linux-arm64" ;;
                *) error "Unsupported architecture: $ARCH" ;;
            esac
            ;;
        Darwin)
            case "$ARCH" in
                x86_64|amd64) echo "macos-x64" ;;
                aarch64|arm64) echo "macos-arm64" ;;
                *) error "Unsupported architecture: $ARCH" ;;
            esac
            ;;
        *) error "Unsupported OS: $OS" ;;
    esac
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"v([^"]+)".*/\1/'
}

download_binary() {
    local version="$1"
    local platform="$2"
    local url="https://github.com/$REPO/releases/download/v$version/${BINARY_NAME}-${platform}"

    info "Downloading polyplugc v$version for $platform..."
    info "URL: $url"

    local tmp_file="/tmp/${BINARY_NAME}-${platform}"

    if ! curl -fsSL --progress-bar -o "$tmp_file" "$url"; then
        error "Failed to download binary from $url"
    fi

    echo "$tmp_file"
}

install_binary() {
    local tmp_file="$1"

    mkdir -p "$INSTALL_DIR"
    chmod +x "$tmp_file"
    mv "$tmp_file" "$INSTALL_DIR/$BINARY_NAME"

    info "Installed $BINARY_NAME to $INSTALL_DIR"
}

add_to_path() {
    local shell_rc=""

    if [ -n "$ZSH_VERSION" ]; then
        shell_rc="$HOME/.zshrc"
    elif [ -n "$BASH_VERSION" ]; then
        shell_rc="$HOME/.bashrc"
    fi

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if ! grep -q "$INSTALL_DIR" "$shell_rc" 2>/dev/null; then
            echo "" >> "$shell_rc"
            echo "# Added by polyplugc installer" >> "$shell_rc"
            echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$shell_rc"
            info "Added $INSTALL_DIR to PATH in $shell_rc"
            info "Run 'source $shell_rc' or start a new shell to use polyplugc"
        fi
    fi
}

main() {
    info "Installing polyplugc..."

    local platform
    platform=$(get_platform)
    info "Detected platform: $platform"

    local version
    version=$(get_latest_version)
    info "Latest version: $version"

    local tmp_file
    tmp_file=$(download_binary "$version" "$platform")

    install_binary "$tmp_file"

    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        add_to_path
    fi

    info ""
    info "Installation complete!"
    info ""
    info "Run 'polyplugc --help' to get started."
    info ""
    info "If 'polyplugc' is not found, run:"
    info "  export PATH=\"\$PATH:$INSTALL_DIR\""
}

main "$@"