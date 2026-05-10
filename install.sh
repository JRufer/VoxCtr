#!/usr/bin/env bash
# VoxCtl Source-Based Installer
#
# Installs VoxCtl from source (git clone) with all system and Python
# dependencies. Designed to be piped from GitHub or run manually.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/JRufer/VoxCtr/master/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/JRufer/VoxCtr/master/install.sh | bash -s -- --branch v0.5
#   bash install.sh                     # interactive, prompts for optional deps
#   bash install.sh --branch dev        # pull a specific branch
#   bash install.sh --dev               # dev mode: use CWD, incremental deps
#   bash install.sh --uninstall         # remove VoxCtl
#   bash install.sh --help

set -euo pipefail

# ── Constants ────────────────────────────────────────────────────────────────
REPO_URL="https://github.com/JRufer/VoxCtr.git"
INSTALL_DIR="$HOME/.local/share/voxctl"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
PIPER_VERSION="v0.0.2"
PIPER_URL="https://github.com/rhasspy/piper/releases/download/${PIPER_VERSION}/piper_linux_x86_64.tar.gz"
MIN_PYTHON_MAJOR=3
MIN_PYTHON_MINOR=10

# Directories preserved across nuke-and-replace
PRESERVE_DIRS=("piper" "piper-voices")

# ── Defaults ─────────────────────────────────────────────────────────────────
BRANCH="master"
DEV_MODE=false
UNINSTALL=false
INTERACTIVE=true
PYTHON=""

# Detect non-interactive (piped) execution
[[ -t 0 ]] || INTERACTIVE=false

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ── Cleanup trap ─────────────────────────────────────────────────────────────
CLEANUP_DIR=""
cleanup() {
    if [[ -n "$CLEANUP_DIR" && -d "$CLEANUP_DIR" ]]; then
        rm -rf "$CLEANUP_DIR"
    fi
}
trap cleanup EXIT

# ── Argument parsing ─────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: bash install.sh [OPTIONS]

Install VoxCtl from source with all dependencies.

Options:
  --branch <name>   Git branch/tag to install (default: master)
  --dev             Dev mode: use current directory, skip clone/nuke,
                    incremental pip install only
  --uninstall       Remove VoxCtl installation
  -h, --help        Show this help message

Examples:
  bash install.sh                          # install from master
  bash install.sh --branch v0.5            # install specific branch
  bash install.sh --dev                    # dev mode (fast, no clone)
  curl -fsSL <url>/install.sh | bash       # pipe-to-bash install
  curl ... | bash -s -- --branch dev       # pipe-to-bash with branch
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --branch)
            [[ -z "${2:-}" ]] && { fail "--branch requires a value"; exit 1; }
            BRANCH="$2"; shift 2 ;;
        --dev)
            DEV_MODE=true; shift ;;
        --uninstall)
            UNINSTALL=true; shift ;;
        -h|--help)
            usage ;;
        *)
            fail "Unknown option: $1"; echo "Run with --help for usage."; exit 1 ;;
    esac
done

# Validate flag combinations
if $DEV_MODE && [[ "$BRANCH" != "master" ]]; then
    fail "--dev and --branch are incompatible. In dev mode, manage your branch with git."
    exit 1
fi

# ── Uninstall ────────────────────────────────────────────────────────────────
if $UNINSTALL; then
    step "Uninstalling VoxCtl"
    rm -f "$BIN_DIR/voxctl"
    rm -f "$DESKTOP_DIR/voxctl.desktop"
    rm -f "$ICON_DIR/voxctl.png"
    if [[ -d "$INSTALL_DIR" ]]; then
        rm -rf "$INSTALL_DIR"
        ok "Removed $INSTALL_DIR"
    fi
    ok "VoxCtl uninstalled."
    info "Config preserved at ~/.config/voxctl/ (delete manually if desired)"
    info "Model caches preserved at ~/.cache/voxctl/ and ~/.cache/moonshine_voice/"
    exit 0
fi

# ── Banner ───────────────────────────────────────────────────────────────────
echo -e "${BOLD}==========================================${NC}"
if $DEV_MODE; then
    echo -e "${BOLD}  Installing VoxCtl (dev mode)${NC}"
else
    echo -e "${BOLD}  Installing VoxCtl${NC}"
fi
echo -e "${BOLD}==========================================${NC}"
$DEV_MODE && info "Dev mode: using current directory, incremental deps"
$INTERACTIVE || info "Non-interactive mode: installing all optional deps"
info "Branch: $BRANCH"

# ══════════════════════════════════════════════════════════════════════════════
# 1. Pre-flight checks
# ══════════════════════════════════════════════════════════════════════════════
step "Pre-flight checks"

# ── Python 3.10+ ─────────────────────────────────────────────────────────────
check_python() {
    local py ver major minor
    for py in python3 python; do
        if command -v "$py" &>/dev/null; then
            ver=$("$py" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")' 2>/dev/null) || continue
            major=${ver%%.*}
            minor=${ver#*.}
            if [[ $major -ge $MIN_PYTHON_MAJOR && $minor -ge $MIN_PYTHON_MINOR ]]; then
                PYTHON="$py"
                return 0
            fi
        fi
    done
    return 1
}

if check_python; then
    ok "Python: $($PYTHON --version) ($PYTHON)"
else
    fail "Python ${MIN_PYTHON_MAJOR}.${MIN_PYTHON_MINOR}+ is required."
    info "Found: $(python3 --version 2>&1 || echo 'python3 not found')"
    info "Install Python 3.10+ from your package manager or https://www.python.org"
    exit 1
fi

# ── Git ──────────────────────────────────────────────────────────────────────
if ! $DEV_MODE; then
    if command -v git &>/dev/null; then
        ok "git: $(git --version)"
    else
        fail "git is required for installation."
        info "Install with: sudo apt install git  (or equivalent)"
        exit 1
    fi
fi

# ── curl (for piper download) ────────────────────────────────────────────────
if command -v curl &>/dev/null; then
    ok "curl available"
else
    warn "curl not found — piper TTS download may fail"
fi

# ── venv module ──────────────────────────────────────────────────────────────
if $PYTHON -m venv --help &>/dev/null; then
    ok "Python venv module available"
else
    fail "Python venv module is missing."
    info "Install with: sudo apt install python3-venv  (or equivalent)"
    exit 1
fi

# ══════════════════════════════════════════════════════════════════════════════
# 2. Package manager detection
# ══════════════════════════════════════════════════════════════════════════════
detect_pkg_manager() {
    if command -v apt-get &>/dev/null; then   echo "apt"
    elif command -v pacman &>/dev/null;  then echo "pacman"
    elif command -v dnf &>/dev/null;     then echo "dnf"
    elif command -v zypper &>/dev/null;  then echo "zypper"
    else                                      echo "unknown"
    fi
}

PKG_MGR=$(detect_pkg_manager)
info "Package manager: $PKG_MGR"

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
    if [[ "$PKG_MGR" == "unknown" ]]; then
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

# ══════════════════════════════════════════════════════════════════════════════
# 3. Required system dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Required system dependencies"

install_if_missing_lib() {
    local lib_name="$1" pkg_apt="$2" pkg_pac="$3" pkg_dnf="${4:-$2}" pkg_zyp="${5:-$2}"
    info "Checking $lib_name..."
    # Try pkg-config first (reliable on Arch), fall back to ldconfig
    if pkg-config --exists "${lib_name}" 2>/dev/null \
       || ldconfig -p 2>/dev/null | grep -q "$lib_name"; then
        ok "$lib_name found"
    else
        local pkg
        pkg=$(_resolve_pkg "$pkg_apt" "$pkg_pac" "$pkg_dnf" "$pkg_zyp")
        info "Installing $pkg..."
        _pkg_install "$pkg" && ok "$pkg installed" \
            || warn "Failed to install $pkg"
    fi
}

install_if_missing_cmd() {
    local cmd="$1" desc="$2" pkg_apt="$3" pkg_pac="$4" pkg_dnf="${5:-$3}" pkg_zyp="${6:-$3}"
    info "Checking $cmd ($desc)..."
    if command -v "$cmd" &>/dev/null; then
        ok "$cmd available"
    else
        local pkg
        pkg=$(_resolve_pkg "$pkg_apt" "$pkg_pac" "$pkg_dnf" "$pkg_zyp")
        info "Installing $pkg..."
        _pkg_install "$pkg" && ok "$pkg installed" \
            || warn "Failed to install $pkg — $desc will not work."
    fi
}

# portaudio — required for all audio capture (PyAudio)
install_if_missing_lib "portaudio-2.0" "libportaudio2" "portaudio" "portaudio-devel" "libportaudio2"

# alsa-utils — provides 'aplay' for TTS audio output
install_if_missing_cmd "aplay" "TTS audio playback" "alsa-utils" "alsa-utils"

# Text injection — wtype (Wayland) and xdotool (X11)
install_if_missing_cmd "wtype" "Wayland text injection" "wtype" "wtype"
install_if_missing_cmd "xdotool" "X11 text injection" "xdotool" "xdotool"

# Clipboard — wl-clipboard (Wayland) and xclip (X11)
if ! command -v wl-copy &>/dev/null; then
    info "Installing wl-clipboard (Wayland clipboard)..."
    local_pkg=$(_resolve_pkg "wl-clipboard" "wl-clipboard" "wl-clipboard" "wl-clipboard")
    _pkg_install "$local_pkg" && ok "wl-clipboard installed" \
        || warn "wl-clipboard not available"
else
    ok "wl-clipboard available"
fi

install_if_missing_cmd "xclip" "X11 clipboard" "xclip" "xclip"

# espeak-ng — TTS fallback engine
install_if_missing_cmd "espeak-ng" "TTS fallback engine" "espeak-ng" "espeak-ng"

# ══════════════════════════════════════════════════════════════════════════════
# 4. Optional system dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Optional system dependencies"

# Helper: install optional dep (auto-yes if non-interactive)
install_optional() {
    local cmd="$1" desc="$2" pkg_apt="$3" pkg_pac="$4" pkg_dnf="${5:-$3}" pkg_zyp="${6:-$3}"
    if command -v "$cmd" &>/dev/null; then
        ok "$cmd available ($desc)"
        return 0
    fi

    if $INTERACTIVE; then
        echo ""
        echo "    $desc"
        read -rp "  Install $cmd? [y/N] " yn
        if [[ ! "$yn" =~ ^[Yy]$ ]]; then
            warn "$cmd skipped"
            return 0
        fi
    else
        info "Installing $cmd ($desc) [non-interactive]..."
    fi

    local pkg
    pkg=$(_resolve_pkg "$pkg_apt" "$pkg_pac" "$pkg_dnf" "$pkg_zyp")
    _pkg_install "$pkg" && ok "$pkg installed" \
        || warn "$pkg install failed"
}

install_optional "socat" \
    "socat enables the MCP server for Claude Desktop integration" \
    "socat" "socat"

# pyatspi — system package, check via python import
if $PYTHON -c "import pyatspi" 2>/dev/null; then
    ok "python3-pyatspi available (AT-SPI2 context-aware transcription)"
else
    local pyatspi_desc="python3-pyatspi enables AT-SPI2 context-aware transcription"
    if $INTERACTIVE; then
        echo ""
        echo "    $pyatspi_desc"
        read -rp "  Install python3-pyatspi? [y/N] " yn
        if [[ "$yn" =~ ^[Yy]$ ]]; then
            local pkg
            pkg=$(_resolve_pkg "python3-pyatspi" "python-pyatspi" "python3-pyatspi" "python3-pyatspi")
            _pkg_install "$pkg" && ok "$pkg installed" \
                || warn "$pkg install failed"
        else
            warn "python3-pyatspi skipped"
        fi
    else
        info "Installing python3-pyatspi [non-interactive]..."
        local pkg
        pkg=$(_resolve_pkg "python3-pyatspi" "python-pyatspi" "python3-pyatspi" "python3-pyatspi")
        _pkg_install "$pkg" && ok "$pkg installed" \
            || warn "$pkg install failed"
    fi
fi

# ══════════════════════════════════════════════════════════════════════════════
# 5. Source install
# ══════════════════════════════════════════════════════════════════════════════
step "Source install"

if $DEV_MODE; then
    # ── Dev mode: use CWD ────────────────────────────────────────────────────
    INSTALL_DIR="$(pwd)"

    if [[ ! -f "$INSTALL_DIR/requirements.txt" ]]; then
        fail "requirements.txt not found in current directory."
        info "Dev mode expects to be run from the VoxCtl repo root."
        exit 1
    fi
    ok "Using dev directory: $INSTALL_DIR"

    # Create venv if missing (reuse if present)
    if [[ -d "$INSTALL_DIR/.venv" ]]; then
        ok "Reusing existing .venv/"
    else
        info "Creating .venv/..."
        $PYTHON -m venv "$INSTALL_DIR/.venv"
        ok ".venv/ created"
    fi

    # Incremental pip install
    info "Installing/updating Python dependencies (incremental)..."
    "$INSTALL_DIR/.venv/bin/pip" install --upgrade pip --quiet 2>/dev/null
    "$INSTALL_DIR/.venv/bin/pip" install -r "$INSTALL_DIR/requirements.txt" --quiet
    ok "Python dependencies up to date"

else
    # ── Normal mode: nuke-and-replace ────────────────────────────────────────
    CLEANUP_DIR=$(mktemp -d)

    # Preserve data dirs
    if [[ -d "$INSTALL_DIR" ]]; then
        info "Existing install found — preserving user data..."
        for d in "${PRESERVE_DIRS[@]}"; do
            if [[ -d "$INSTALL_DIR/$d" ]]; then
                mv "$INSTALL_DIR/$d" "$CLEANUP_DIR/$d"
                ok "Preserved $d/"
            fi
        done

        info "Removing old install..."
        rm -rf "$INSTALL_DIR"
        ok "Old install removed"
    fi

    # Fresh clone
    info "Cloning VoxCtl (branch: $BRANCH)..."
    git clone --depth 1 --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
    ok "Source cloned to $INSTALL_DIR"

    # Restore preserved dirs
    for d in "${PRESERVE_DIRS[@]}"; do
        if [[ -d "$CLEANUP_DIR/$d" ]]; then
            mv "$CLEANUP_DIR/$d" "$INSTALL_DIR/$d"
            ok "Restored $d/"
        fi
    done
    rm -rf "$CLEANUP_DIR"
    CLEANUP_DIR=""

    # Create venv
    info "Creating .venv/..."
    $PYTHON -m venv "$INSTALL_DIR/.venv"
    ok ".venv/ created"

    # Install pip dependencies
    info "Installing Python dependencies..."
    "$INSTALL_DIR/.venv/bin/pip" install --upgrade pip --quiet 2>/dev/null
    "$INSTALL_DIR/.venv/bin/pip" install -r "$INSTALL_DIR/requirements.txt"
    ok "Python dependencies installed"
fi

# ══════════════════════════════════════════════════════════════════════════════
# 6. Piper TTS engine (user-local)
# ══════════════════════════════════════════════════════════════════════════════
step "Piper TTS engine"

PIPER_DIR="$INSTALL_DIR/piper"

_install_piper_local() {
    local tmp_dir tarball
    tmp_dir=$(mktemp -d)

    info "Downloading piper ${PIPER_VERSION}..."
    if ! curl -fsSL -o "$tmp_dir/piper.tar.gz" "$PIPER_URL"; then
        warn "piper download failed — TTS will fall back to espeak-ng."
        rm -rf "$tmp_dir"
        return 1
    fi

    info "Extracting to $PIPER_DIR..."
    tar -xzf "$tmp_dir/piper.tar.gz" -C "$tmp_dir"

    # The tarball extracts to a "piper" subdirectory
    mkdir -p "$PIPER_DIR"
    cp -r "$tmp_dir/piper/." "$PIPER_DIR/"
    chmod +x "$PIPER_DIR/piper"

    rm -rf "$tmp_dir"
    ok "piper installed to $PIPER_DIR"
}

if [[ -x "$PIPER_DIR/piper" ]]; then
    ok "piper already installed at $PIPER_DIR/piper"
elif command -v piper &>/dev/null; then
    ok "piper found on system PATH: $(command -v piper)"
else
    _install_piper_local || true
fi

# ══════════════════════════════════════════════════════════════════════════════
# 7. Launcher script
# ══════════════════════════════════════════════════════════════════════════════
step "Launcher script"

mkdir -p "$BIN_DIR"

cat > "$BIN_DIR/voxctl" <<LAUNCHER
#!/bin/bash
VOXCTL_DIR="$INSTALL_DIR"
export PYTHONPATH="\$VOXCTL_DIR/src:\$PYTHONPATH"
export PATH="\$VOXCTL_DIR/piper:\$PATH"
export LD_LIBRARY_PATH="\$VOXCTL_DIR/piper:\$LD_LIBRARY_PATH"
# Expose host system site-packages so pyatspi (system-only package) is found
SYS_SITE=\$(python3 -c "import site; print(site.getsitepackages()[0])" 2>/dev/null || true)
[ -n "\$SYS_SITE" ] && export PYTHONPATH="\$PYTHONPATH:\$SYS_SITE"
exec "\$VOXCTL_DIR/.venv/bin/python3" "\$VOXCTL_DIR/src/main.py" "\$@"
LAUNCHER
chmod +x "$BIN_DIR/voxctl"
ok "Launcher installed at $BIN_DIR/voxctl"

# ══════════════════════════════════════════════════════════════════════════════
# 8. Desktop integration
# ══════════════════════════════════════════════════════════════════════════════
step "Desktop integration"

mkdir -p "$DESKTOP_DIR" "$ICON_DIR"

# Install icon
if [[ -f "$INSTALL_DIR/assets/app_icon.png" ]]; then
    cp "$INSTALL_DIR/assets/app_icon.png" "$ICON_DIR/voxctl.png"
    ok "Icon installed"
fi

# Desktop entry — update existing or create new
EXISTING_DESKTOP=""
for candidate in \
    "$HOME/.local/share/applications/voxctl.desktop" \
    "/usr/local/share/applications/voxctl.desktop" \
    "/usr/share/applications/voxctl.desktop"; do
    if [[ -f "$candidate" ]]; then
        EXISTING_DESKTOP="$candidate"
        break
    fi
done

if [[ -n "$EXISTING_DESKTOP" ]]; then
    info "Updating existing desktop entry: $EXISTING_DESKTOP"
    sed -i "s|^Exec=.*|Exec=$BIN_DIR/voxctl|" "$EXISTING_DESKTOP"
    ok "Desktop entry updated"
    UPDATED_DESKTOP_DIR="$(dirname "$EXISTING_DESKTOP")"
else
    cat > "$DESKTOP_DIR/voxctl.desktop" <<DESKTOP
[Desktop Entry]
Name=VoxCtl
Comment=Native, on-device voice-to-text pipeline
Exec=$BIN_DIR/voxctl
Icon=voxctl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
Keywords=whisper;voice;dictation;wayland;
DESKTOP
    ok "Desktop entry created at $DESKTOP_DIR/voxctl.desktop"
    UPDATED_DESKTOP_DIR="$DESKTOP_DIR"
fi

command -v update-desktop-database &>/dev/null && update-desktop-database "$UPDATED_DESKTOP_DIR" 2>/dev/null || true

# ══════════════════════════════════════════════════════════════════════════════
# 9. Hardware permissions (evdev / uinput)
# ══════════════════════════════════════════════════════════════════════════════
step "Hardware permissions (evdev / uinput)"

SETUP_PERMS="$INSTALL_DIR/scripts/setup-permissions.sh"
if [[ -f "$SETUP_PERMS" ]]; then
    info "Setting up udev rules for global hotkeys (requires sudo)..."
    # Pass real username — $USER becomes root under sudo
    sudo REAL_USER="$(whoami)" bash "$SETUP_PERMS"
else
    warn "setup-permissions.sh not found — global hotkeys may not work."
fi

# ══════════════════════════════════════════════════════════════════════════════
# 10. GPU detection
# ══════════════════════════════════════════════════════════════════════════════
step "GPU / transcription backend"

if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null 2>&1; then
    ok "NVIDIA GPU detected — faster-whisper (CUDA) backend available."
elif command -v vulkaninfo &>/dev/null && vulkaninfo 2>/dev/null | grep -qi "GPU id"; then
    warn "AMD/Intel GPU with Vulkan detected."
    info "For GPU-accelerated transcription install whisper-cpp with Vulkan support."
else
    info "No discrete GPU detected — CPU transcription will be used."
    info "Recommended model size for CPU: 'base' or 'small'."
fi

# ══════════════════════════════════════════════════════════════════════════════
# 11. PATH check
# ══════════════════════════════════════════════════════════════════════════════
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo ""
    warn "\$HOME/.local/bin is not in your PATH."
    echo "       Add this line to ~/.bashrc or ~/.zshrc:"
    echo ""
    echo "         export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo "       Then reload your shell:  source ~/.bashrc"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Summary
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Installation Complete!${NC}"
echo -e "${BOLD}==========================================${NC}"
echo ""
echo "  Installed:"
echo "    Source     →  $INSTALL_DIR"
echo "    Venv       →  $INSTALL_DIR/.venv/"
echo "    Launcher   →  $BIN_DIR/voxctl"
echo "    Desktop    →  ${UPDATED_DESKTOP_DIR:-$DESKTOP_DIR}/voxctl.desktop"
echo "    Icon       →  $ICON_DIR/voxctl.png"
if [[ -x "$PIPER_DIR/piper" ]]; then
    echo "    Piper TTS  →  $PIPER_DIR/piper"
elif command -v piper &>/dev/null; then
    echo "    Piper TTS  →  $(command -v piper) (system)"
fi
echo ""
echo "  User data (preserved across reinstalls):"
echo "    Config     →  ~/.config/voxctl/config.json"
echo "    Models     →  ~/.cache/voxctl/ (faster-whisper)"
echo "               →  ~/.cache/moonshine_voice/ (moonshine)"
echo "    Voices     →  $INSTALL_DIR/piper-voices/"
echo ""
echo "  Next steps:"
echo "    1. LOG OUT and LOG BACK IN (required for hotkey group permissions)"
echo "       Tip: 'exec newgrp input' activates the group in the current terminal."
echo "    2. Run: voxctl"
echo ""
