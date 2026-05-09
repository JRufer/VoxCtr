#!/bin/bash
# VoxCtl AppImage Installer
#
# Installs the AppImage to ~/.local/bin/voxctl and handles all system-level
# dependencies that cannot be bundled inside the AppImage:
#   - Runtime C libraries (portaudio, dbus)
#   - Text injection binaries (wtype, xdotool)
#   - Clipboard tools (wl-clipboard, xclip)
#   - Audio playback (alsa-utils)
#   - TTS engine (piper, espeak-ng)
#   - Optional: socat (MCP/Claude Desktop bridge)
#   - Optional: python3-pyatspi (AT-SPI2 accessibility)
#   - udev rules + user groups for global hotkeys

APP_NAME="VoxCtl"
APPIMAGE_FILE="VoxCtl-x86_64.AppImage"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"

# ── Colours ──────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Installing $APP_NAME${NC}"
echo -e "${BOLD}==========================================${NC}"

# ──────────────────────────────────────────────────────
# 0. Detect distro and package manager
# ──────────────────────────────────────────────────────
detect_pkg_manager() {
    if command -v apt-get &>/dev/null; then   echo "apt"
    elif command -v pacman &>/dev/null;       then echo "pacman"
    elif command -v dnf &>/dev/null;          then echo "dnf"
    elif command -v zypper &>/dev/null;       then echo "zypper"
    else                                           echo "unknown"
    fi
}

PKG_MGR=$(detect_pkg_manager)
info "Detected package manager: $PKG_MGR"

# Resolve a package name for the current distro.
# Args: <apt-name> <pacman-name> [<dnf-name> [<zypper-name>]]
_resolve_pkg() {
    local apt="$1" pac="$2" dnf="${3:-$1}" zyp="${4:-$1}"
    case "$PKG_MGR" in
        apt)    echo "$apt" ;;
        pacman) echo "$pac" ;;
        dnf)    echo "$dnf" ;;
        zypper) echo "$zyp" ;;
        *)      echo "$apt" ;;
    esac
}

# Install one package via the detected package manager. Non-fatal on failure.
_pkg_install() {
    local pkg="$1"
    if [ "$PKG_MGR" = "unknown" ]; then
        warn "Unknown package manager — please install '$pkg' manually."
        return 1
    fi
    case "$PKG_MGR" in
        apt)    sudo apt-get install -y "$pkg" ;;
        pacman) sudo pacman -S --noconfirm "$pkg" ;;
        dnf)    sudo dnf install -y "$pkg" ;;
        zypper) sudo zypper install -y "$pkg" ;;
    esac
}

# ──────────────────────────────────────────────────────
# 1. Pre-flight: AppImage must exist
# ──────────────────────────────────────────────────────
step "Pre-flight check"
if [ ! -f "$APPIMAGE_FILE" ]; then
    fail "$APPIMAGE_FILE not found in the current directory."
    echo "       Build it first:  bash scripts/build_appimage.sh"
    exit 1
fi
ok "Found $APPIMAGE_FILE"

# ──────────────────────────────────────────────────────
# 2. Required system dependencies
#
# The AppImage bundles all Python packages, but it cannot bundle:
#   • libportaudio2  — PyAudio's C extension links to this at runtime
#   • libdbus-1      — dbus-python's C extension links to this at runtime
#   • External binaries (wtype, xdotool, wl-copy, aplay, piper, etc.)
# ──────────────────────────────────────────────────────
step "Required system dependencies"

# portaudio — required for all audio capture (PyAudio)
info "Checking libportaudio (required for audio capture)..."
if ldconfig -p 2>/dev/null | grep -q "libportaudio"; then
    ok "libportaudio found"
else
    pkg=$(_resolve_pkg "libportaudio2" "portaudio" "portaudio-devel" "libportaudio2")
    info "Installing $pkg..."
    _pkg_install "$pkg" && ok "$pkg installed" \
        || warn "Failed to install $pkg — audio capture will not work."
fi

# alsa-utils — provides 'aplay' used by the TTS engine for audio output
info "Checking aplay (alsa-utils, required for TTS audio playback)..."
if command -v aplay &>/dev/null; then
    ok "aplay available"
else
    pkg=$(_resolve_pkg "alsa-utils" "alsa-utils" "alsa-utils" "alsa-utils")
    info "Installing $pkg..."
    _pkg_install "$pkg" && ok "$pkg installed" \
        || warn "Failed to install $pkg — TTS audio playback will not work."
fi

# Text injection — wtype (Wayland) and/or xdotool (X11).
# The app prefers wtype on Wayland and falls back to xdotool on X11.
info "Checking text injection tools (wtype / xdotool)..."
HAVE_WTYPE=false; HAVE_XDOTOOL=false
command -v wtype   &>/dev/null && HAVE_WTYPE=true   && ok "wtype available (Wayland injection)"
command -v xdotool &>/dev/null && HAVE_XDOTOOL=true && ok "xdotool available (X11 injection)"

if ! $HAVE_WTYPE; then
    pkg=$(_resolve_pkg "wtype" "wtype" "wtype" "wtype")
    info "Installing $pkg (Wayland text injection)..."
    _pkg_install "$pkg" && ok "$pkg installed" || warn "wtype not available — Wayland text injection may not work."
fi
if ! $HAVE_XDOTOOL; then
    pkg=$(_resolve_pkg "xdotool" "xdotool" "xdotool" "xdotool")
    info "Installing $pkg (X11 text injection fallback)..."
    _pkg_install "$pkg" && ok "$pkg installed" || warn "xdotool not available — X11 text injection may not work."
fi

# Clipboard — wl-clipboard (Wayland) and xclip (X11)
info "Checking clipboard tools (wl-clipboard / xclip)..."
if command -v wl-copy &>/dev/null; then
    ok "wl-clipboard available (Wayland clipboard)"
else
    pkg=$(_resolve_pkg "wl-clipboard" "wl-clipboard" "wl-clipboard" "wl-clipboard")
    info "Installing $pkg..."
    _pkg_install "$pkg" && ok "$pkg installed" || warn "wl-clipboard not available — Wayland clipboard output may not work."
fi

if command -v xclip &>/dev/null; then
    ok "xclip available (X11 clipboard)"
else
    pkg=$(_resolve_pkg "xclip" "xclip" "xclip" "xclip")
    info "Installing $pkg..."
    _pkg_install "$pkg" && ok "$pkg installed" || warn "xclip not available — X11 clipboard output may not work."
fi

# espeak-ng — TTS fallback when piper is not installed or a model hasn't loaded yet
info "Checking espeak-ng (TTS fallback engine)..."
if command -v espeak-ng &>/dev/null; then
    ok "espeak-ng available"
else
    pkg=$(_resolve_pkg "espeak-ng" "espeak-ng" "espeak-ng" "espeak-ng")
    info "Installing $pkg..."
    _pkg_install "$pkg" && ok "$pkg installed" || warn "espeak-ng not available — TTS will only work if piper is installed."
fi

# ──────────────────────────────────────────────────────
# 3. Optional system dependencies (prompt user)
# ──────────────────────────────────────────────────────
step "Optional system dependencies"

# socat — bridges the MCP Unix socket to Claude Desktop's stdio transport
if command -v socat &>/dev/null; then
    ok "socat available (MCP / Claude Desktop bridge enabled)"
else
    echo ""
    echo "    socat enables the MCP server that lets Claude Desktop call VoxCtl tools"
    echo "    (transcribe_voice, speak_text, get_status) directly from Claude Desktop."
    read -rp "  Install socat for Claude Desktop / MCP integration? [y/N] " yn
    if [[ "$yn" =~ ^[Yy]$ ]]; then
        pkg=$(_resolve_pkg "socat" "socat" "socat" "socat")
        _pkg_install "$pkg" && ok "socat installed" \
            || warn "socat install failed — MCP bridge will not work."
    else
        warn "socat skipped — MCP server / Claude Desktop integration will be disabled."
    fi
fi

# pyatspi — system package (python3-pyatspi); NOT pip-installable on Python 3.11+.
# The AppImage's AppRun adds the host site-packages to PYTHONPATH so the app finds it.
if python3 -c "import pyatspi" 2>/dev/null; then
    ok "python3-pyatspi available — AT-SPI2 context-aware transcription enabled"
else
    echo ""
    echo "    python3-pyatspi enables AT-SPI2 context-aware transcription (direct text"
    echo "    insertion into focused applications without using the clipboard)."
    read -rp "  Install python3-pyatspi for AT-SPI2 features? [y/N] " yn
    if [[ "$yn" =~ ^[Yy]$ ]]; then
        pkg=$(_resolve_pkg "python3-pyatspi" "python-pyatspi" "python3-pyatspi" "python3-pyatspi")
        _pkg_install "$pkg" && ok "$pkg installed — AT-SPI2 features enabled" \
            || warn "$pkg install failed — AT-SPI2 context-aware transcription will be unavailable."
    else
        warn "python3-pyatspi skipped — AT-SPI2 context-aware transcription will be unavailable."
    fi
fi

# ──────────────────────────────────────────────────────
# 4. GPU detection and transcription backend guidance
# ──────────────────────────────────────────────────────
step "GPU / transcription backend"

# Moonshine is the recommended default: faster + more accurate than Whisper,
# works on any hardware.  It is bundled in the AppImage via requirements.txt.
if python3 -c "import moonshine_voice" 2>/dev/null; then
    ok "moonshine-voice available — Moonshine backend active (fastest, most accurate)."
    info "Moonshine Medium beats Whisper Large-v3 accuracy at 1/6 the parameters."
else
    warn "moonshine-voice not found in Python path — will fall back to Whisper backends."
    info "To enable Moonshine: pip install moonshine-voice"
fi

if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null 2>&1; then
    ok "NVIDIA GPU detected — faster-whisper (CUDA) available as fallback backend."
    info "Ensure NVIDIA drivers are current for best performance."
elif command -v vulkaninfo &>/dev/null && vulkaninfo 2>/dev/null | grep -qi "GPU id"; then
    info "AMD/Intel GPU with Vulkan detected."
    info "For Vulkan-accelerated Whisper fallback, install whisper-cpp:"
    echo "         Arch:  yay -S whisper-cpp-vulkan"
    echo "         Other: build from source with -DGGML_VULKAN=ON"
else
    info "No discrete GPU detected — Moonshine or faster-whisper CPU will be used."
fi

# ──────────────────────────────────────────────────────
# 5. Install the AppImage
# ──────────────────────────────────────────────────────
step "Installing AppImage"
mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"

info "Copying AppImage to $BIN_DIR/voxctl..."
cp "$APPIMAGE_FILE" "$BIN_DIR/voxctl"
chmod +x "$BIN_DIR/voxctl"
ok "AppImage installed at $BIN_DIR/voxctl"

# ──────────────────────────────────────────────────────
# 6. Icon + desktop entry
# ──────────────────────────────────────────────────────
step "Desktop integration"
if [ -f "assets/app_icon.png" ]; then
    cp assets/app_icon.png "$ICON_DIR/voxctl.png"
    ok "Icon installed"
fi

# Remove every existing voxctl.desktop from all known locations so that
# re-running install.sh never accumulates duplicate app launcher entries.
DESKTOP_CANDIDATES=(
    "$HOME/.local/share/applications/voxctl.desktop"
    "/usr/local/share/applications/voxctl.desktop"
    "/usr/share/applications/voxctl.desktop"
)
for candidate in "${DESKTOP_CANDIDATES[@]}"; do
    if [ -f "$candidate" ]; then
        if [ -w "$candidate" ]; then
            rm -f "$candidate"
            info "Removed old desktop entry: $candidate"
        else
            sudo rm -f "$candidate"
            info "Removed old desktop entry (sudo): $candidate"
        fi
    fi
done

cat > "$DESKTOP_DIR/voxctl.desktop" <<EOF
[Desktop Entry]
Name=VoxCtl
Comment=Native, on-device voice-to-text pipeline
Exec=$BIN_DIR/voxctl
Icon=voxctl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
Keywords=moonshine;whisper;voice;dictation;wayland;
EOF
ok "Desktop entry created at $DESKTOP_DIR/voxctl.desktop"
UPDATED_DESKTOP_DIR="$DESKTOP_DIR"

command -v update-desktop-database &>/dev/null && update-desktop-database "$UPDATED_DESKTOP_DIR" || true

# ──────────────────────────────────────────────────────
# 7. Piper TTS engine
# ──────────────────────────────────────────────────────
step "Piper TTS engine"
_install_piper() {
    local tmp_dir tarball
    tmp_dir=$(mktemp -d)
    tarball="$tmp_dir/piper_amd64.tar.gz"

    info "Downloading piper v0.0.2..."
    if ! curl -fsSL -o "$tarball" \
        "https://github.com/rhasspy/piper/releases/download/v0.0.2/piper_amd64.tar.gz"; then
        warn "piper download failed — TTS will fall back to espeak-ng."
        rm -rf "$tmp_dir"
        return 1
    fi

    tar -xzf "$tarball" -C "$tmp_dir"
    sudo mkdir -p /opt/piper
    sudo cp -r "$tmp_dir/piper/." /opt/piper/

    # Wrapper script keeps the binary next to its bundled .so files
    printf '#!/bin/sh\nexec /opt/piper/piper "$@"\n' \
        | sudo tee /usr/local/bin/piper > /dev/null
    sudo chmod +x /usr/local/bin/piper

    # Register piper's bundled shared libraries with the dynamic linker
    echo /opt/piper | sudo tee /etc/ld.so.conf.d/piper.conf > /dev/null
    sudo ldconfig

    rm -rf "$tmp_dir"
    ok "piper installed to /opt/piper"
}

if command -v piper &>/dev/null; then
    ok "piper already installed at $(command -v piper) — skipping."
else
    _install_piper || true
fi

# ──────────────────────────────────────────────────────
# 8. Hardware permissions (evdev / uinput)
# ──────────────────────────────────────────────────────
step "Hardware permissions (evdev / uinput)"
info "Creating udev rules for global hotkeys (requires sudo)..."
if [ -f "scripts/setup-permissions.sh" ]; then
    sudo bash scripts/setup-permissions.sh
else
    warn "scripts/setup-permissions.sh not found — global hotkeys may not work."
fi

# ──────────────────────────────────────────────────────
# 9. PATH check
# ──────────────────────────────────────────────────────
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo ""
    warn "\$HOME/.local/bin is not in your PATH."
    echo "       Add this line to ~/.bashrc or ~/.zshrc:"
    echo ""
    echo "         export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo "       Then reload your shell:  source ~/.bashrc"
fi

# ──────────────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Installation Complete!${NC}"
echo -e "${BOLD}==========================================${NC}"
echo ""
echo "  Installed:"
echo "    AppImage   →  $BIN_DIR/voxctl"
echo "    Desktop    →  $DESKTOP_DIR/voxctl.desktop"
echo "    Icon       →  $ICON_DIR/voxctl.png"
command -v piper &>/dev/null && echo "    TTS        →  piper (neural) + espeak-ng (fallback)"
echo ""
echo "  What the AppImage bundles (no extra pip install needed):"
echo "    • All Python packages: PyQt6, moonshine-voice, faster-whisper, onnxruntime,"
echo "      PyAudio, dbus-python, evdev, noisereduce, scipy, numpy, websockets, mcp,"
echo "      and 50+ more"
echo "  Note: python3-pyatspi is a system package (installed above if selected);"
echo "        the AppImage reads it from the host site-packages at runtime."
echo ""
echo "  What must be on the host (installed above):"
echo "    • libportaudio2  — audio capture C library"
echo "    • alsa-utils     — aplay for TTS audio output"
echo "    • wtype          — Wayland text injection"
echo "    • xdotool        — X11 text injection fallback"
echo "    • wl-clipboard   — Wayland clipboard"
echo "    • xclip          — X11 clipboard fallback"
echo "    • espeak-ng      — TTS fallback engine"
echo "    • piper          — neural TTS engine (installed to /opt/piper)"
echo ""
echo "  Next steps:"
echo "    1. LOG OUT and LOG BACK IN (required for hotkey group permissions)"
echo "    2. Run: voxctl"
echo "       (or launch from your application menu)"
echo ""
echo "  On first launch VoxCtl will:"
echo "    • Download the Moonshine model automatically (~270 MB for 'medium')"
echo "    • Let you configure hotkeys, output targets, and TTS voice"
echo "    • (Whisper model only downloaded if you switch to faster-whisper or whisper-cpp)"
echo ""
echo "  TTS voice models are downloaded on demand from Settings → TTS."
echo ""
