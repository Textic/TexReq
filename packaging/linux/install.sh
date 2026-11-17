#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_PNG_DIR="${HOME}/.local/share/icons/hicolor/512x512/apps"
ICON_SVG_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_PNG_DIR" "$ICON_SVG_DIR"
install -m 755 "$SCRIPT_DIR/texreq" "$BIN_DIR/texreq"
install -m 644 "$SCRIPT_DIR/assets/texreq.png" "$ICON_PNG_DIR/texreq.png"
install -m 644 "$SCRIPT_DIR/assets/texreq.svg" "$ICON_SVG_DIR/texreq.svg"
install -m 644 "$SCRIPT_DIR/texreq.desktop" "$APP_DIR/texreq.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" || true
fi

echo "Installed TexReq to $BIN_DIR/texreq"
echo "Make sure $BIN_DIR is in your PATH."
