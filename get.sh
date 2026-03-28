#!/bin/sh
# woven installer
# Usage: curl -fsSL https://raw.githubusercontent.com/viewerofall/woven/main/get.sh | sh

set -e

REPO="viewerofall/woven"
TARBALL="v2-alpha.tar.gz"
TMP=$(mktemp -d)

cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "==> Downloading woven..."
curl -fsSL "https://github.com/$REPO/releases/latest/download/$TARBALL" -o "$TMP/$TARBALL" \
  || curl -fsSL "https://raw.githubusercontent.com/$REPO/main/$TARBALL" -o "$TMP/$TARBALL"

echo "==> Extracting..."
tar -xzf "$TMP/$TARBALL" -C "$TMP"

# Find extracted dir (handles varying names)
SRC=$(find "$TMP" -maxdepth 1 -mindepth 1 -type d | head -1)
[ -z "$SRC" ] && SRC="$TMP"

echo "==> Installing config and runtime..."
mkdir -p ~/.config/woven
# Don't overwrite existing config
[ -f ~/.config/woven/woven.lua ] || cp "$SRC/woven.lua" ~/.config/woven/woven.lua
cp -r "$SRC/runtime" ~/.config/woven/

echo "==> Installing systemd user service..."
mkdir -p ~/.config/systemd/user
cp "$SRC/woven.service" ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable woven.service

echo "==> Installing binaries..."
BINDIR="$HOME/.local/bin"
mkdir -p "$BINDIR"
# Build from source if no prebuilt binaries found
if [ -f "$SRC/exec/woven" ] && [ -f "$SRC/exec/woven-ctrl" ]; then
    cp "$SRC/exec/woven" "$SRC/exec/woven-ctrl" "$BINDIR/"
else
    echo "==> No prebuilt binaries found — building from source (requires Rust)..."
    cargo build --release --manifest-path "$SRC/Cargo.toml"
    cp "$SRC/target/release/woven" "$SRC/target/release/woven-ctrl" "$BINDIR/"
fi
chmod +x "$BINDIR/woven" "$BINDIR/woven-ctrl"

echo "==> Setting WOVEN_ROOT..."
# Write to /etc/profile.d for system-wide login shells
if [ -d /etc/profile.d ] && [ -w /etc/profile.d ]; then
    echo 'export WOVEN_ROOT="$HOME/.config/woven"' | sudo tee /etc/profile.d/woven.sh > /dev/null
    echo "    Written to /etc/profile.d/woven.sh"
else
    # Fall back to user shell configs
    for RC in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
        if [ -f "$RC" ] && ! grep -q "WOVEN_ROOT" "$RC"; then
            echo 'export WOVEN_ROOT="$HOME/.config/woven"' >> "$RC"
            echo "    Added to $RC"
        fi
    done
fi

# Set for current session regardless
export WOVEN_ROOT="$HOME/.config/woven"

echo ""
echo "==> woven installed."
echo ""
echo "    Add your compositor keybind:"
echo "    Hyprland: bind = SUPER, grave, exec, woven-ctrl --toggle"
echo "    Niri:     Super+Grave { spawn \"woven-ctrl\" \"--toggle\"; }"
echo "    Sway:     bindsym Super+grave exec woven-ctrl --toggle"
echo ""
echo "    Start now:"
echo "    systemctl --user start woven.service"
echo ""
echo "    Or run directly:"
echo "    woven &"
echo ""
