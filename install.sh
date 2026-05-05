#!/bin/bash
# Whisper-Wayland AppImage Installer

set -e

APP_NAME="Whisper-Wayland"
APPIMAGE_FILE="Whisper-Wayland-x86_64.AppImage"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"

echo "=========================================="
echo " Installing $APP_NAME "
echo "=========================================="

# Check if AppImage exists
if [ ! -f "$APPIMAGE_FILE" ]; then
    echo "Error: $APPIMAGE_FILE not found in the current directory."
    echo "Please build the AppImage first or place this script next to it."
    exit 1
fi

# Ensure target directories exist
mkdir -p "$BIN_DIR"
mkdir -p "$DESKTOP_DIR"
mkdir -p "$ICON_DIR"

# 1. Install the AppImage
echo "[*] Copying AppImage to $BIN_DIR..."
cp "$APPIMAGE_FILE" "$BIN_DIR/whisper-wayland"
chmod +x "$BIN_DIR/whisper-wayland"

# 2. Install the icon
echo "[*] Installing icon..."
if [ -f "assets/app_icon.png" ]; then
    cp assets/app_icon.png "$ICON_DIR/whisper-wayland.png"
fi

# 3. Create the desktop entry
echo "[*] Creating desktop entry..."
cat > "$DESKTOP_DIR/whisper-wayland.desktop" <<EOF
[Desktop Entry]
Name=Whisper Wayland
Comment=Native, on-device voice-to-text pipeline
Exec=$BIN_DIR/whisper-wayland
Icon=whisper-wayland
Terminal=false
Type=Application
Categories=Utility;Audio;
EOF

# 4. Install piper TTS (binary + bundled shared libraries)
echo ""
echo "[*] Installing piper TTS engine..."
_install_piper() {
    local tmp_dir
    tmp_dir=$(mktemp -d)
    local tarball="$tmp_dir/piper_amd64.tar.gz"

    echo "    Downloading piper v0.0.2..."
    if ! curl -fsSL -o "$tarball" \
        "https://github.com/rhasspy/piper/releases/download/v0.0.2/piper_amd64.tar.gz"; then
        echo "    Warning: piper download failed. TTS will fall back to espeak-ng."
        rm -rf "$tmp_dir"
        return 1
    fi

    tar -xzf "$tarball" -C "$tmp_dir"

    sudo mkdir -p /opt/piper
    sudo cp -r "$tmp_dir/piper/." /opt/piper/

    # Wrapper keeps the binary next to its bundled .so files
    printf '#!/bin/sh\nexec /opt/piper/piper "$@"\n' | sudo tee /usr/local/bin/piper > /dev/null
    sudo chmod +x /usr/local/bin/piper

    # Register bundled libs with the dynamic linker
    echo /opt/piper | sudo tee /etc/ld.so.conf.d/piper.conf > /dev/null
    sudo ldconfig

    rm -rf "$tmp_dir"
    echo "    piper installed to /opt/piper"
}

if command -v piper &>/dev/null; then
    echo "    piper already installed at $(command -v piper) — skipping."
else
    _install_piper
fi

# 5. Set up permissions (evdev)
echo ""
echo "[*] Setting up hardware permissions (evdev) for global hotkeys..."
echo "This step requires root privileges to create udev rules."
if [ -f "scripts/setup-permissions.sh" ]; then
    sudo bash scripts/setup-permissions.sh
else
    echo "Warning: scripts/setup-permissions.sh not found. Hotkeys may not work globally."
fi

# 6. Update desktop database
echo "[*] Updating desktop database..."
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR"
fi

echo "=========================================="
echo " Installation Complete!"
echo " "
echo " Note: Make sure '$BIN_DIR' is in your PATH."
echo " You may need to log out and log back in for the permission changes to take effect."
echo "=========================================="
