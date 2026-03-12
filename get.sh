#!/usr/bin/env bash
# woven — one-line installer
# curl -fsSL https://raw.githubusercontent.com/viewerofall/woven/main/get.sh | bash

set -euo pipefail

REPO="viewerofall/woven"
BRANCH="main"
TARBALL="v1.tar.gz"
TARBALL_URL="https://raw.githubusercontent.com/${REPO}/${BRANCH}/${TARBALL}"
TMP_DIR="$(mktemp -d)"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "${GREEN}  →${RESET} $*"; }
warn()    { echo -e "${YELLOW}  ⚠${RESET}  $*"; }
error()   { echo -e "${RED}  ✗${RESET}  $*" >&2; exit 1; }
section() { echo -e "\n${BOLD}$*${RESET}"; }

cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

# ── preflight ─────────────────────────────────────────────────────────────────
section "woven installer"

command -v curl &>/dev/null || error "curl is required but not installed."
command -v tar  &>/dev/null || error "tar is required but not installed."

if [[ -z "${WAYLAND_DISPLAY:-}" && -z "${WAYLAND_SOCKET:-}" ]]; then
    warn "No active Wayland session detected — install will continue."
fi

# ── download & extract ────────────────────────────────────────────────────────
section "Downloading woven"
info "From: $TARBALL_URL"

curl -fsSL --progress-bar "$TARBALL_URL" -o "$TMP_DIR/$TARBALL" \
    || error "Download failed. Check https://github.com/${REPO}"

info "Extracting..."
tar -xzf "$TMP_DIR/$TARBALL" -C "$TMP_DIR"

# Tarball layout:
#   exec/   woven  woven-ctrl
#   lua/    runtime/  woven.lua
#   sysd/   woven.service

SRC="$TMP_DIR"
[[ -f "$SRC/exec/woven" && -f "$SRC/exec/woven-ctrl" ]] \
    || error "exec/ directory missing or incomplete — tarball may be corrupt."
[[ -d "$SRC/lua/runtime" ]] \
    || error "lua/runtime/ missing — tarball may be corrupt."
[[ -f "$SRC/sysd/woven.service" ]] \
    || error "sysd/woven.service missing — tarball may be corrupt."

# ── binaries ──────────────────────────────────────────────────────────────────
section "Installing binaries"
sudo install -Dm755 "$SRC/exec/woven"      /usr/local/bin/woven
sudo install -Dm755 "$SRC/exec/woven-ctrl" /usr/local/bin/woven-ctrl
info "woven       → /usr/local/bin/woven"
info "woven-ctrl  → /usr/local/bin/woven-ctrl"

# ── lua runtime ───────────────────────────────────────────────────────────────
section "Installing Lua runtime"
sudo mkdir -p /usr/local/share/woven
sudo cp -r "$SRC/lua/runtime" /usr/local/share/woven/runtime
info "runtime     → /usr/local/share/woven/runtime/"

# ── config ────────────────────────────────────────────────────────────────────
section "Installing config"
mkdir -p "$HOME/.config/woven"
if [[ -f "$HOME/.config/woven/woven.lua" ]]; then
    warn "Config exists — skipping. Delete ~/.config/woven/woven.lua to restore defaults."
else
    cp "$SRC/lua/woven.lua" "$HOME/.config/woven/woven.lua"
    info "config      → ~/.config/woven/woven.lua"
fi

# ── WOVEN_ROOT ────────────────────────────────────────────────────────────────
section "Setting WOVEN_ROOT"

SHELL_NAME="$(basename "${SHELL:-bash}")"
case "$SHELL_NAME" in
    zsh)  RC_FILE="$HOME/.zshrc" ;;
    fish) RC_FILE="$HOME/.config/fish/config.fish" ;;
    bash) RC_FILE="$HOME/.bashrc" ;;
    *)    RC_FILE="$HOME/.profile" ;;
esac

if [[ "$SHELL_NAME" == "fish" ]]; then
    WRITE_LINE="set -gx WOVEN_ROOT /usr/local/share/woven"
else
    WRITE_LINE="export WOVEN_ROOT=/usr/local/share/woven"
fi

if grep -qF "WOVEN_ROOT" "$RC_FILE" 2>/dev/null; then
    warn "WOVEN_ROOT already in $RC_FILE — skipping."
else
    { echo ""; echo "# woven workspace overlay"; echo "$WRITE_LINE"; } >> "$RC_FILE"
    info "WOVEN_ROOT  → $RC_FILE"
    info "Run: source $RC_FILE  (or open a new terminal)"
fi
export WOVEN_ROOT=/usr/local/share/woven

# ── systemd user service ──────────────────────────────────────────────────────
section "Installing systemd user service"

if command -v systemctl &>/dev/null; then
    mkdir -p "$HOME/.config/systemd/user"
    cp "$SRC/sysd/woven.service" "$HOME/.config/systemd/user/woven.service"
    systemctl --user daemon-reload
    # enable only — do NOT start yet, compositor keybind isn't set up
    systemctl --user enable woven.service
    info "service     → ~/.config/systemd/user/woven.service"
    info "Enabled (will start automatically on next login)."
else
    warn "systemctl not found — add 'woven &' to your compositor autostart manually."
fi

# ── compositor keybind ────────────────────────────────────────────────────────
section "Compositor setup"
echo "  Add a keybind to your compositor config before starting woven."
echo ""
echo "  Hyprland (~/.config/hypr/hyprland.conf):"
echo -e "    ${BOLD}exec-once = woven${RESET}"
echo -e "    ${BOLD}bind = SUPER, grave, exec, woven-ctrl --toggle${RESET}"
echo ""
echo "  Niri (~/.config/niri/config.kdl):"
echo -e "    ${BOLD}spawn-at-startup \"woven\"${RESET}"
echo -e "    ${BOLD}Super+Grave { spawn \"woven-ctrl\" \"--toggle\"; }${RESET}"
echo ""
echo "  Sway (~/.config/sway/config):"
echo -e "    ${BOLD}exec woven${RESET}"
echo -e "    ${BOLD}bindsym Super+grave exec woven-ctrl --toggle${RESET}"
echo ""
echo "  After adding the keybind, reload your compositor or log out and back in."

# ── first launch ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}Open woven-ctrl now?${RESET}"
echo    "  Launches the GUI so you can configure your theme before first use."
echo    "  (Does not start the woven daemon — do that after setting your keybind.)"
echo -n "  [y/N] "
read -r REPLY </dev/tty
echo ""

if [[ "${REPLY,,}" == "y" ]]; then
    WOVEN_ROOT=/usr/local/share/woven woven-ctrl --setup &
    info "woven-ctrl setup wizard launched."
else
    info "Run 'woven-ctrl --setup' anytime to configure."
fi

echo ""
echo -e "${GREEN}${BOLD}Installation complete.${RESET}"
echo    "  Add your compositor keybind, then log out and back in to start woven."
