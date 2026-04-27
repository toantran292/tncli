#!/bin/bash
set -e

REPO="toantran292/tncli"
INSTALL_DIR="/usr/local/bin"

# Check required tools
MISSING=""
command -v curl >/dev/null 2>&1 || MISSING="$MISSING curl"
command -v tmux >/dev/null 2>&1 || MISSING="$MISSING tmux"
command -v zsh >/dev/null 2>&1  || MISSING="$MISSING zsh"
command -v tar >/dev/null 2>&1  || MISSING="$MISSING tar"

if [ -n "$MISSING" ]; then
  echo "error: missing required dependencies:$MISSING"
  echo ""
  echo "Install them first:"
  echo "  macOS:        brew install$MISSING"
  echo "  Ubuntu/Debian: sudo apt install$MISSING"
  echo "  Arch:         sudo pacman -S$MISSING"
  exit 1
fi

# Detect OS + architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  darwin) OS_NAME="darwin" ;;
  linux)  OS_NAME="linux" ;;
  *)
    echo "error: unsupported OS: $OS"
    echo "tncli supports: macOS (darwin), Linux"
    exit 1
    ;;
esac

case "$ARCH" in
  arm64|aarch64) ARCH_NAME="arm64" ;;
  x86_64|amd64)  ARCH_NAME="amd64" ;;
  *)
    echo "error: unsupported architecture: $ARCH"
    echo "tncli supports: arm64 (Apple Silicon/aarch64), amd64 (x86_64)"
    exit 1
    ;;
esac

PLATFORM="${OS_NAME}-${ARCH_NAME}"
echo "Detected platform: $PLATFORM"

# Get latest release tag
echo "Fetching latest version..."
TAG=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
if [ -z "$TAG" ]; then
  echo "error: could not fetch latest release from github.com/$REPO"
  exit 1
fi
echo "Latest version: $TAG"

# Download
URL="https://github.com/$REPO/releases/download/$TAG/tncli-${PLATFORM}.tar.gz"
TMPDIR=$(mktemp -d)
echo "Downloading tncli-${PLATFORM}..."
HTTP_CODE=$(curl -sL -w "%{http_code}" "$URL" -o "$TMPDIR/tncli.tar.gz")
if [ "$HTTP_CODE" != "200" ]; then
  echo "error: download failed (HTTP $HTTP_CODE)"
  echo "URL: $URL"
  echo ""
  echo "Available platforms: darwin-arm64, darwin-amd64, linux-amd64, linux-arm64"
  rm -rf "$TMPDIR"
  exit 1
fi

# Extract
tar xzf "$TMPDIR/tncli.tar.gz" -C "$TMPDIR"
BINARY="$TMPDIR/tncli-${PLATFORM}"

if [ ! -f "$BINARY" ]; then
  echo "error: binary not found in archive"
  rm -rf "$TMPDIR"
  exit 1
fi

# Remove macOS quarantine flag
if [ "$OS_NAME" = "darwin" ]; then
  xattr -d com.apple.quarantine "$BINARY" 2>/dev/null || true
fi
chmod +x "$BINARY"

# Verify
if ! "$BINARY" --version >/dev/null 2>&1; then
  echo "error: binary verification failed"
  rm -rf "$TMPDIR"
  exit 1
fi

# Install
echo "Installing to $INSTALL_DIR (may require sudo)..."
sudo mkdir -p "$INSTALL_DIR"
sudo cp "$BINARY" "$INSTALL_DIR/tncli"
sudo chmod +x "$INSTALL_DIR/tncli"

# Cleanup
rm -rf "$TMPDIR"

VERSION=$("$INSTALL_DIR/tncli" --version 2>/dev/null || echo "$TAG")
echo ""
echo "$VERSION installed successfully!"
echo "Run 'tncli --help' to get started."
