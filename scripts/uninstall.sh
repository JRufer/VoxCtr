#!/usr/bin/env bash
# VoxCtrl uninstaller.
#
# Reverts everything VoxCtrl's installer (`--install` / the in-app "Setup
# System Integration" button) and the app itself create at runtime, returning
# the system to its pre-VoxCtrl state:
#
#   System (needs sudo):
#     - /etc/udev/rules.d/99-voxctrl.rules (+ legacy 99-voxctl / 99-voxctr)
#     - udev rules reload
#     - membership of the current user in the `input` group
#     - (opt-in via --remove-packages) host packages the installer added
#
#   User:
#     - ~/.local/share/applications/ai.voxctrl.app.desktop   (menu launcher,
#       plus the older voxctrl.desktop name)
#     - ~/.local/share/icons/hicolor/128x128/apps/voxctrl.png
#     - ~/.config/voxctrl/                            (config.json, targets.toml, bindings.toml)
#     - ~/.local/share/voxctrl/                       (whisper models, piper engine+voices,
#                                                      pocket-tts voices, startup_errors.log)
#     - ~/.local/share/ai.voxctrl.app/ and ~/.cache/ai.voxctrl.app/  (WebKit profile)
#     - kyutai pocket-tts entries in the HuggingFace cache
#     - /tmp/voxctrl-mcp.sock
#     - (optional, prompted) the AppImage file itself
#
# Usage:
#   ./uninstall.sh                   interactive uninstall
#   ./uninstall.sh --yes             no prompts (keeps the AppImage file)
#   ./uninstall.sh --remove-appimage also delete the .AppImage file itself
#   ./uninstall.sh --remove-packages also remove packages the installer added
#
# Package removal is opt-in because those packages (wtype, xdotool,
# wl-clipboard, xclip, espeak-ng, portaudio, webkit2gtk, ...) are shared
# system libraries: they may have been present before VoxCtrl, and other
# applications may depend on them. The package manager will refuse to remove
# anything another installed package still requires, but review the list it
# prints before confirming.

set -u

BOLD=$(tput bold 2>/dev/null || true); NC=$(tput sgr0 2>/dev/null || true)
ok()   { echo "  [ok]   $*"; }
skip() { echo "  [--]   $*"; }
act()  { echo "${BOLD}== $*${NC}"; }

ASSUME_YES=false
REMOVE_PACKAGES=false
REMOVE_APPIMAGE=false
for arg in "$@"; do
    case "$arg" in
        --yes|-y) ASSUME_YES=true ;;
        --remove-packages) REMOVE_PACKAGES=true ;;
        --remove-appimage) REMOVE_APPIMAGE=true ;;
        -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown argument: $arg (see --help)"; exit 1 ;;
    esac
done

confirm() {
    $ASSUME_YES && return 0
    read -r -p "$1 [y/N] " reply
    [[ "$reply" =~ ^[Yy]$ ]]
}

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this script as your normal user, not root — it uses sudo only where needed."
    exit 1
fi

# ── 1. Stop running instances ─────────────────────────────────────────────────
act "Stopping VoxCtrl"
if pkill -x voxctrl 2>/dev/null || pkill -x voxctrl-overlay 2>/dev/null; then
    sleep 1
    pkill -9 -x voxctrl 2>/dev/null || true
    pkill -9 -x voxctrl-overlay 2>/dev/null || true
    ok "Stopped running VoxCtrl processes"
else
    skip "No running VoxCtrl processes"
fi
rm -f /tmp/voxctrl-mcp.sock && skip "Removed MCP socket (if present)"

# ── 2. System-level changes (sudo) ────────────────────────────────────────────
act "Reverting system integration (sudo required)"

UDEV_RULES=(/etc/udev/rules.d/99-voxctrl.rules /etc/udev/rules.d/99-voxctl.rules /etc/udev/rules.d/99-voxctr.rules)
FOUND_RULES=()
for f in "${UDEV_RULES[@]}"; do [ -e "$f" ] && FOUND_RULES+=("$f"); done

IN_INPUT_GROUP=false
if id -Gn "$USER" | tr ' ' '\n' | grep -qx input; then IN_INPUT_GROUP=true; fi

if [ ${#FOUND_RULES[@]} -gt 0 ] || $IN_INPUT_GROUP; then
    SUDO_SCRIPT=""
    if [ ${#FOUND_RULES[@]} -gt 0 ]; then
        SUDO_SCRIPT+="rm -f ${FOUND_RULES[*]} && udevadm control --reload-rules && udevadm trigger"
    fi
    if $IN_INPUT_GROUP; then
        [ -n "$SUDO_SCRIPT" ] && SUDO_SCRIPT+=" ; "
        SUDO_SCRIPT+="gpasswd -d '$USER' input"
    fi
    if sudo sh -c "$SUDO_SCRIPT"; then
        [ ${#FOUND_RULES[@]} -gt 0 ] && ok "Removed udev rule(s): ${FOUND_RULES[*]}"
        if $IN_INPUT_GROUP; then
            ok "Removed $USER from the 'input' group (log out and back in to apply)"
            echo "         Note: if you were in the 'input' group before installing VoxCtrl"
            echo "         for another tool, re-add yourself: sudo usermod -aG input $USER"
        fi
    else
        echo "  [!!]   sudo step failed or was cancelled — system integration NOT reverted"
    fi
else
    skip "No udev rules found and $USER is not in the 'input' group"
fi

# Optional: remove the host packages the installer added.
if $REMOVE_PACKAGES; then
    act "Removing host packages installed by VoxCtrl setup"
    echo "  These are shared packages — your package manager will refuse anything"
    echo "  still required by other software, but review the transaction carefully."
    if command -v pacman >/dev/null 2>&1; then
        sudo pacman -Rns wtype xdotool wl-clipboard xclip portaudio espeak-ng || true
    elif command -v apt-get >/dev/null 2>&1; then
        sudo apt-get remove wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng || true
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf remove wtype xdotool wl-clipboard xclip portaudio espeak-ng || true
    elif command -v zypper >/dev/null 2>&1; then
        sudo zypper remove wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng || true
    else
        skip "No known package manager found"
    fi
    echo "  Note: webkit2gtk / openssl / libayatana-appindicator are left installed —"
    echo "  they are core desktop libraries that many other applications use."
fi

# ── 3. Desktop integration ────────────────────────────────────────────────────
act "Removing desktop integration and registered global shortcuts"
LAUNCHER="$HOME/.local/share/applications/ai.voxctrl.app.desktop"
# Installs from before the entry was renamed to match the portal app id.
LEGACY_LAUNCHER="$HOME/.local/share/applications/voxctrl.desktop"
ICON="$HOME/.local/share/icons/hicolor/128x128/apps/voxctrl.png"

# Unregister global shortcuts from KDE kglobalaccel if running
for qdbus_cmd in qdbus6 qdbus-qt6 qdbus; do
    if command -v "$qdbus_cmd" >/dev/null 2>&1; then
        for comp in "ai.voxctrl.app" "ai.voxctrl.app.desktop" "voxctrl" "voxctrl.desktop"; do
            action_output=$($qdbus_cmd --literal org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.allActionsForComponent "$comp" 2>/dev/null || true)
            if [ -n "$action_output" ]; then
                shortcut_ids=$(echo "$action_output" | grep -o '"voxctrl_[^"]*"' | tr -d '"' | sort -u || true)
                for sc in $shortcut_ids; do
                    $qdbus_cmd org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.unregister "$comp" "$sc" >/dev/null 2>&1 || true
                done
            fi
        done
        break
    fi
done

# Clean shortcut configuration from kglobalshortcutsrc if present
KGLOBAL_SRC="$HOME/.config/kglobalshortcutsrc"
if [ -f "$KGLOBAL_SRC" ]; then
    if grep -qE '\[(ai\.voxctrl\.app|voxctrl)' "$KGLOBAL_SRC" 2>/dev/null; then
        python3 -c '
import sys, re
path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8", errors="ignore") as f:
        content = f.read()
    cleaned = re.sub(r"\[(ai\.voxctrl\.app[^\]]*|voxctrl[^\]]*)\][^\[]*", "", content)
    with open(path, "w", encoding="utf-8") as f:
        f.write(cleaned)
except Exception:
    pass
' "$KGLOBAL_SRC" 2>/dev/null || true
        ok "Cleaned registered shortcuts from $KGLOBAL_SRC"
    fi
fi

# Remember where the AppImage lives (from the launcher) before deleting it.
APPIMAGE_PATH=""
for candidate in "$LAUNCHER" "$LEGACY_LAUNCHER"; do
    if [ -f "$candidate" ]; then
        APPIMAGE_PATH=$(sed -n 's/^Exec=//p' "$candidate" | head -1)
        break
    fi
done

for f in "$LAUNCHER" "$LEGACY_LAUNCHER" "$ICON"; do
    if [ -e "$f" ]; then rm -f "$f" && ok "Removed $f"; else skip "Not present: $f"; fi
done
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null

# ── 4. Configuration and data ─────────────────────────────────────────────────
act "Removing configuration and data"
USER_DIRS=(
    "$HOME/.config/voxctrl"            # config.json, targets.toml, bindings.toml (+ backups)
    "$HOME/.local/share/voxctrl"       # whisper models, piper engine + voices, pocket-tts voices, logs
    "$HOME/.local/share/ai.voxctrl.app" # Tauri/WebKit profile data
    "$HOME/.cache/ai.voxctrl.app"      # Tauri/WebKit cache
)
for d in "${USER_DIRS[@]}"; do
    if [ -d "$d" ]; then rm -rf "$d" && ok "Removed $d"; else skip "Not present: $d"; fi
done

# Pocket-TTS models live in the shared HuggingFace cache; remove only the
# repos VoxCtrl downloads, never the whole cache.
HF_HUB="$HOME/.cache/huggingface/hub"
for repo in models--kyutai--pocket-tts models--kyutai--pocket-tts-without-voice-cloning models--kyutai--tts-voices; do
    if [ -d "$HF_HUB/$repo" ]; then
        rm -rf "$HF_HUB/$repo" && ok "Removed HuggingFace cache entry: $repo"
    fi
done

# ── 5. The AppImage itself (optional) ─────────────────────────────────────────
act "AppImage file"
if [ -n "$APPIMAGE_PATH" ] && [ -f "$APPIMAGE_PATH" ]; then
    # Deleted only with the explicit --remove-appimage flag, or on an
    # interactive "yes" — never silently under --yes alone.
    if $REMOVE_APPIMAGE; then
        rm -f "$APPIMAGE_PATH" && ok "Removed $APPIMAGE_PATH"
    elif $ASSUME_YES; then
        skip "Kept $APPIMAGE_PATH (pass --remove-appimage to delete it)"
    elif confirm "Delete the AppImage itself ($APPIMAGE_PATH)?"; then
        rm -f "$APPIMAGE_PATH" && ok "Removed $APPIMAGE_PATH"
    else
        skip "Kept $APPIMAGE_PATH"
    fi
else
    skip "AppImage location unknown (launcher already gone?) — delete the .AppImage file manually if desired"
fi

echo
echo "${BOLD}VoxCtrl has been uninstalled.${NC}"
echo "Log out and back in for the 'input' group removal to take effect."
