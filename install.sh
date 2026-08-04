#!/usr/bin/env bash
# VoxCtrl Installer & Host Setup Script
#
# Configures the host environment to run the portable AppImage natively:
# 1. Installs system runtime dependencies (PortAudio, WebKitGTK, espeak-ng, tools).
# 2. Ensures the portable AppImage exists (compiling if missing).
# 3. Establishes hardware udev permissions for evdev global hotkeys.
# 4. Integrates the AppImage into the desktop launcher (~/.local/share/applications/).

set -euo pipefail

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ── Package Manager Detection ────────────────────────────────────────────────
detect_pkg_manager() {
    if command -v pacman &>/dev/null;    then echo "pacman"
    elif command -v apt-get &>/dev/null; then echo "apt"
    elif command -v dnf &>/dev/null;     then echo "dnf"
    elif command -v zypper &>/dev/null;  then echo "zypper"
    else                                      echo "unknown"
    fi
}

PKG_MGR=$(detect_pkg_manager)
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

# Verify npm is installed as a dependency at start
if ! command -v npm &>/dev/null; then
    fail "The Node Package Manager 'npm' is not installed, which is required as a dependency."
    info "👉 Please install Node.js and npm via your package manager first:"
    info "   - Arch:   sudo pacman -S npm"
    info "   - Ubuntu: sudo apt install npm"
    info "   - Fedora: sudo dnf install npm"
    echo ""
    exit 1
fi

# ══════════════════════════════════════════════════════════════════════════════
# 1. Install Host Runtime Dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Installing Host Runtime Dependencies"

info "Detected Package Manager: $PKG_MGR"

case "$PKG_MGR" in
    pacman)
        info "Installing portable runtime packages via pacman..."
        sudo pacman -S --noconfirm --needed \
            webkit2gtk-4.1 openssl libayatana-appindicator \
            wtype xdotool wl-clipboard xclip portaudio squashfs-tools \
            espeak-ng
        ;;
    apt)
        info "Installing portable runtime packages via apt..."
        sudo apt-get update -y
        sudo apt-get install -y \
            libwebkit2gtk-4.1-0 libssl3 libayatana-appindicator3-1 \
            wtype xdotool wl-clipboard xclip libportaudio2 squashfs-tools \
            espeak-ng
        ;;
    dnf)
        info "Installing portable runtime packages via dnf..."
        sudo dnf install -y \
            webkit2gtk4.1 openssl libayatana-appindicator3 \
            wtype xdotool wl-clipboard xclip portaudio squashfs-tools \
            espeak-ng
        ;;
    zypper)
        info "Installing portable runtime packages via zypper..."
        sudo zypper install -y \
            libwebkit2gtk-4_1-0 libopenssl3 libayatana-appindicator3-1 \
            wtype xdotool wl-clipboard xclip libportaudio2 squashfs-tools \
            espeak-ng
        ;;
    *)
        warn "Unsupported package manager. Please ensure you have these runtimes installed manually:"
        echo "  - webkit2gtk-4.1 libraries"
        echo "  - openssl (libssl)"
        echo "  - libayatana-appindicator3"
        echo "  - portaudio libraries"
        echo "  - espeak-ng (TTS fallback)"
        echo "  - wtype (Wayland keystrokes) / xdotool (X11 keystrokes)"
        echo "  - wl-clipboard (Wayland clipboard) / xclip (X11 clipboard)"
        ;;
esac

ok "Host runtime dependencies installed."

# ══════════════════════════════════════════════════════════════════════════════
# 2. Retrieve / Compile AppImage
# ══════════════════════════════════════════════════════════════════════════════
step "Retrieving Portable AppImage"

# Helper function to dynamically scan and resolve the best local AppImage
resolve_local_appimage() {
    # Scan for any semver-compliant versioned AppImages (e.g. VoxCtrl-0.1.0-x86_64.AppImage)
    local found=( $(find . -maxdepth 1 -name "VoxCtrl-*-x86_64.AppImage" 2>/dev/null | sort -V || true) )
    if [ ${#found[@]} -gt 0 ]; then
        echo "${found[-1]}"
    elif [ -f "./VoxCtrl-latest-x86_64.AppImage" ]; then
        echo "./VoxCtrl-latest-x86_64.AppImage"
    elif [ -f "./VoxCtrl-x86_64.AppImage" ]; then
        echo "./VoxCtrl-x86_64.AppImage"
    else
        echo ""
    fi
}

PORTABLE_APPIMAGE=$(resolve_local_appimage)

if [ -n "$PORTABLE_APPIMAGE" ] && [ -f "$PORTABLE_APPIMAGE" ]; then
    ok "Portable AppImage found in workspace: $PORTABLE_APPIMAGE"
else
    # Default target filename for downloads
    PORTABLE_APPIMAGE="./VoxCtrl-latest-x86_64.AppImage"
    info "Portable AppImage not found in root. Attempting to fetch pre-compiled binary..."
    
    DOWNLOAD_URL="https://github.com/JRufer/VoxCtrl/releases/latest/download/VoxCtrl-latest-x86_64.AppImage"
    FETCHED=0
    
    if command -v curl &>/dev/null; then
        info "Downloading latest AppImage via curl..."
        if curl -s -L -f -o "$PORTABLE_APPIMAGE" "$DOWNLOAD_URL"; then
            FETCHED=1
        fi
    elif command -v wget &>/dev/null; then
        info "Downloading latest AppImage via wget..."
        if wget -q -O "$PORTABLE_APPIMAGE" "$DOWNLOAD_URL"; then
            FETCHED=1
        fi
    fi
    
    if [ $FETCHED -eq 1 ]; then
        chmod +x "$PORTABLE_APPIMAGE"
        ok "Fetched latest pre-compiled AppImage successfully!"
    else
        warn "Could not fetch pre-compiled binary (it may not be released yet or network is offline)."
        info "Falling back to compiling AppImage from source..."
        
        # Check for build toolchain dependencies
        MISSING_BUILD_TOOLS=0
        if ! command -v cargo &>/dev/null || ! cargo --version &>/dev/null; then MISSING_BUILD_TOOLS=1; fi
        if ! command -v npm &>/dev/null;   then MISSING_BUILD_TOOLS=1; fi
        if ! command -v cmake &>/dev/null; then MISSING_BUILD_TOOLS=1; fi
        
        # Check if an NVIDIA GPU is present
        HAS_NVIDIA_GPU=false
        if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null; then
            HAS_NVIDIA_GPU=true
        fi

        # If NVIDIA GPU is present but CUDA toolkit is not in standard paths, treat as missing build tools
        INSTALL_CUDA=false
        if [ "$HAS_NVIDIA_GPU" = true ]; then
            if [ ! -d "/opt/cuda" ] && [ ! -d "/usr/local/cuda" ]; then
                MISSING_BUILD_TOOLS=1
                INSTALL_CUDA=true
            fi
        fi
        
        if [ $MISSING_BUILD_TOOLS -eq 1 ]; then
            info "Compiler tools or dependencies are missing. Installing build toolchain dependencies..."
            case "$PKG_MGR" in
                pacman)
                    if [ "$INSTALL_CUDA" = true ]; then
                        info "NVIDIA GPU detected. Adding 'cuda' toolkit to installation..."
                        sudo pacman -S --noconfirm --needed base-devel rustup nodejs npm pkgconf cuda cmake
                    else
                        sudo pacman -S --noconfirm --needed base-devel rustup nodejs npm pkgconf cmake
                    fi
                    ;;
                apt)
                    if [ "$INSTALL_CUDA" = true ]; then
                        info "NVIDIA GPU detected. Adding 'nvidia-cuda-toolkit' to installation..."
                        sudo apt-get install -y build-essential curl nodejs npm pkg-config nvidia-cuda-toolkit cmake
                    else
                        sudo apt-get install -y build-essential curl nodejs npm pkg-config cmake
                    fi
                    ;;
                dnf)
                    sudo dnf groupinstall -y "Development Tools"
                    if [ "$INSTALL_CUDA" = true ]; then
                        info "NVIDIA GPU detected. Adding 'cuda-toolkit' to installation..."
                        sudo dnf install -y curl nodejs npm pkgconf-pkg-config cuda-toolkit cmake
                    else
                        sudo dnf install -y curl nodejs npm pkgconf-pkg-config cmake
                    fi
                    ;;
                zypper)
                    sudo zypper install -t pattern -y devel_basis
                    if [ "$INSTALL_CUDA" = true ]; then
                        info "NVIDIA GPU detected. Adding 'cuda' toolkit to installation..."
                        sudo zypper install -y curl nodejs npm pkg-config cuda cmake
                    else
                        sudo zypper install -y curl nodejs npm pkg-config cmake
                    fi
                    ;;
            esac
            
            # Initialize rustup if needed
            if ! command -v cargo &>/dev/null && command -v rustup &>/dev/null; then
                rustup default stable
                source "$HOME/.cargo/env" || true
            fi
        fi
        
        # Run the compiler script
        info "Executing build_appimage.sh to compile portable package..."
        ./build_appimage.sh
        ok "AppImage compiled successfully from source."
        
        # Re-resolve since a new file was built
        PORTABLE_APPIMAGE=$(resolve_local_appimage)
    fi
fi

# Extract dynamic variables from the resolved AppImage path for brand consistency
FILENAME=$(basename "$PORTABLE_APPIMAGE")
if [[ "$FILENAME" =~ VoxCtrl-(.*)-x86_64.AppImage ]]; then
    APP_NAME="VoxCtrl"
    APP_VERSION="${BASH_REMATCH[1]}"
else
    APP_NAME="VoxCtrl"
    APP_VERSION="latest"
fi

# ══════════════════════════════════════════════════════════════════════════════
# 3. Udev Rules Setup for evdev Hotkeys
# ══════════════════════════════════════════════════════════════════════════════
step "Configuring Hardware Permissions (udev)"

UDEV_RULE_PATH="/etc/udev/rules.d/99-voxctrl.rules"

# The `uaccess` tag is what removes the "log out and log back in" step:
# systemd-logind grants the user of the active local session an ACL on these
# devices as soon as the rules are reloaded and the devices re-triggered.
# The rule is rewritten unconditionally so an upgrade from a VoxCtrl version
# that shipped the group-only rule actually picks up the new behaviour.
info "Setting up udev rules for global hotkeys (requires sudo)..."
sudo tee "$UDEV_RULE_PATH" > /dev/null <<'EOF'
# Installed by VoxCtrl — global hotkey and input access.
#
# uaccess grants the user of the ACTIVE local session an ACL on these devices,
# applied immediately by systemd-logind on `udevadm trigger`. This is why
# VoxCtrl does not require logging out after setup, and it is narrower than
# permanent `input` group membership: access follows the seat, not the account.
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"

# Virtual keyboard device used for synthetic keystroke injection.
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput", TAG+="uaccess"
EOF
sudo chmod 0644 "$UDEV_RULE_PATH"

info "Reloading udev rules..."
sudo udevadm control --reload-rules || true
sudo udevadm trigger --subsystem-match=input --action=change || true
sudo udevadm trigger --subsystem-match=misc --action=change || true
ok "udev rules configured successfully."

# Fallback for systems without logind, where the ACL above is not applied.
if ! id -Gn "$USER" | tr ' ' '\n' | grep -qx input; then
    info "Adding user '$USER' to 'input' group as a fallback..."
    sudo usermod -aG input "$USER" || true
else
    ok "User is already in the 'input' group."
fi

# Report what is actually true rather than assuming the worst.
HOTKEYS_LIVE=false
for dev in /dev/input/event*; do
    [ -r "$dev" ] && HOTKEYS_LIVE=true && break
done
if [ "$HOTKEYS_LIVE" = true ]; then
    ok "Keyboard access is live — no logout required."
else
    warn "Keyboard access is not active in this shell yet. VoxCtrl picks it up on"
    warn "its next start; only if that also fails do you need to log out."
fi

# Remove legacy rules if they exist to keep system clean
if [ -f "/etc/udev/rules.d/99-voxctr.rules" ]; then
    info "Removing legacy udev rule path (99-voxctr.rules)..."
    sudo rm -f "/etc/udev/rules.d/99-voxctr.rules"
fi
if [ -f "/etc/udev/rules.d/99-voxctl.rules" ]; then
    info "Removing legacy udev rule path (99-voxctl.rules)..."
    sudo rm -f "/etc/udev/rules.d/99-voxctl.rules"
fi

# ══════════════════════════════════════════════════════════════════════════════
# 4. Desktop Integration launcher
# ══════════════════════════════════════════════════════════════════════════════
step "Registering Desktop Launcher & Application Icon"

ICON_DEST_DIR="$HOME/.local/share/icons/hicolor/128x128/apps"
LAUNCHER_DEST_DIR="$HOME/.local/share/applications"

mkdir -p "$ICON_DEST_DIR"
mkdir -p "$LAUNCHER_DEST_DIR"

ICON_DEST_PATH="$ICON_DEST_DIR/voxctrl.png"
ICON_COPIED=0

# Install high-res desktop icon
if [ -f "./src-tauri/icons/128x128.png" ]; then
    cp "./src-tauri/icons/128x128.png" "$ICON_DEST_PATH"
    ICON_COPIED=1
    ok "Application icon installed from source tree: $ICON_DEST_PATH"
else
    # Try to extract the icon dynamically from the AppImage (resolves raw deployment bug)
    info "Attempting to extract application icon from portable AppImage..."
    if command -v unsquashfs &>/dev/null; then
        if "$PORTABLE_APPIMAGE" --appimage-extract usr/share/icons/hicolor/128x128/apps/voxctrl.png &>/dev/null; then
            cp squashfs-root/usr/share/icons/hicolor/128x128/apps/voxctrl.png "$ICON_DEST_PATH"
            rm -rf squashfs-root
            ICON_COPIED=1
            ok "Application icon extracted and installed successfully: $ICON_DEST_PATH"
        elif "$PORTABLE_APPIMAGE" --appimage-extract usr/share/icons/hicolor/512x512/apps/voxctrl.png &>/dev/null; then
            cp squashfs-root/usr/share/icons/hicolor/512x512/apps/voxctrl.png "$ICON_DEST_PATH"
            rm -rf squashfs-root
            ICON_COPIED=1
            ok "Application icon extracted (512px) and installed successfully: $ICON_DEST_PATH"
        fi
    fi
fi

if [ $ICON_COPIED -eq 0 ]; then
    warn "Could not extract or copy a custom high-res icon. Using desktop fallbacks."
fi

# Write desktop entry linked directly to the portable AppImage
LAUNCHER_PATH="$LAUNCHER_DEST_DIR/voxctrl.desktop"
ABS_APPIMAGE_PATH="$(readlink -f "$PORTABLE_APPIMAGE")"

cat > "$LAUNCHER_PATH" <<EOF
[Desktop Entry]
Name=VoxCtrl
Comment=Private Global Voice Dictation Gateway
Exec=$ABS_APPIMAGE_PATH
Icon=voxctrl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
StartupWMClass=ai.voxctrl.app
Keywords=whisper;voice;dictation;wayland;
EOF

chmod +x "$LAUNCHER_PATH"
ok "Desktop launcher integrated successfully: $LAUNCHER_PATH"

# Clean up legacy launcher if it exists
if [ -f "$LAUNCHER_DEST_DIR/voxctrl.desktop" ]; then
    rm -f "$LAUNCHER_DEST_DIR/voxctrl.desktop"
fi

echo ""
echo -e "${BOLD}==================================================${NC}"
echo -e "${BOLD}  Setup & Integration Complete!${NC}"
echo -e "${BOLD}==================================================${NC}"
echo ""
echo "  VoxCtrl ($APP_VERSION) is now fully integrated into your desktop environment!"
echo "  You can launch it directly from your applications menu or run:"
echo -e "    ${GREEN}$PORTABLE_APPIMAGE${NC}"
echo ""
if [ "$HOTKEYS_LIVE" = true ]; then
    echo "  ✅ Global hotkeys are ready to use — no logout needed."
else
    echo "  ℹ️  Keyboard access was configured but is not visible from this shell."
    echo "  Start VoxCtrl: it restarts itself once to pick the permissions up, and"
    echo "  tells you in-app if anything is still missing."
fi
echo ""
