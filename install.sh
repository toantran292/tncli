#!/bin/bash
set -e

BIN="./tncli"
INSTALL_DIR="/usr/local/bin"

if [[ ! -f "$BIN" ]]; then
  echo "error: tncli binary not found in current directory"
  echo "run this script from the tncli folder"
  exit 1
fi

# Remove quarantine flag (macOS blocks unsigned downloads)
xattr -d com.apple.quarantine "$BIN" 2>/dev/null || true

# Make executable
chmod +x "$BIN"

# Install to PATH
echo "Installing tncli to $INSTALL_DIR ..."
sudo cp "$BIN" "$INSTALL_DIR/tncli"
echo "Done! Run 'tncli' from anywhere."
