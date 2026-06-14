#!/usr/bin/env bash
set -euo pipefail

AIKD_VERSION="2.0.0"
INSTALL_DIR="${AIKD_INSTALL_DIR:-$HOME/.local/bin}"
REPO="your-org/aikd"

detect_platform() {
    local OS ARCH
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        linux*)
            case "$ARCH" in
                x86_64)  echo "x86_64-unknown-linux-gnu" ;;
                aarch64) echo "aarch64-unknown-linux-gnu" ;;
                *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
            esac ;;
        darwin*)
            case "$ARCH" in
                x86_64)  echo "x86_64-apple-darwin" ;;
                arm64)   echo "aarch64-apple-darwin" ;;
                *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
            esac ;;
        *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
    esac
}

TARGET=$(detect_platform)
URL="https://github.com/$REPO/releases/download/v$AIKD_VERSION/aikd-$TARGET"

echo "Installing AIKD v$AIKD_VERSION for $TARGET..."
mkdir -p "$INSTALL_DIR"
curl -sSfL "$URL" -o "$INSTALL_DIR/aikd"
chmod +x "$INSTALL_DIR/aikd"

echo "Installed to $INSTALL_DIR/aikd"
echo "Run: aikd init"
