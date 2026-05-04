#!/bin/bash
# Whisper Wayland AppImage Build Script
# This script creates a portable AppImage bundling Python and dependencies.

# Exit on error
set -e

APP_NAME="Whisper-Wayland"
APP_DIR="AppDir"
PYTHON_VERSION=$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')

echo "Building AppImage for ${APP_NAME} (Python ${PYTHON_VERSION})..."

# 1. Clean and Setup Structure
rm -rf ${APP_DIR} *.AppImage
mkdir -p ${APP_DIR}/usr/bin
mkdir -p ${APP_DIR}/usr/share/whisper-wayland
mkdir -p ${APP_DIR}/usr/share/metainfo
mkdir -p ${APP_DIR}/usr/share/applications
mkdir -p ${APP_DIR}/usr/share/icons/hicolor/256x256/apps

# 2. Copy Application Files
echo "[*] Copying application source and assets..."
cp -r src/ ${APP_DIR}/usr/share/whisper-wayland/
cp -r assets/ ${APP_DIR}/usr/share/whisper-wayland/
cp whisper-wayland.desktop ${APP_DIR}/usr/share/applications/

# Update desktop file for AppImage
sed -i "s|^Exec=.*|Exec=whisper-wayland|" ${APP_DIR}/usr/share/applications/whisper-wayland.desktop
sed -i "s|^Icon=.*|Icon=whisper-wayland|" ${APP_DIR}/usr/share/applications/whisper-wayland.desktop

# Icons
cp assets/app_icon.png ${APP_DIR}/usr/share/icons/hicolor/256x256/apps/whisper-wayland.png
cp assets/app_icon.png ${APP_DIR}/whisper-wayland.png
ln -sf usr/share/applications/whisper-wayland.desktop ${APP_DIR}/whisper-wayland.desktop

# 3. Install Dependencies using a virtualenv
echo "[*] Creating virtual environment inside AppDir..."
python3 -m venv ${APP_DIR}/usr/venv

echo "[*] Installing dependencies into AppDir venv..."
${APP_DIR}/usr/venv/bin/pip install --upgrade pip
${APP_DIR}/usr/venv/bin/pip install -r requirements.txt

# 4. Set up AppRun
echo "[*] Setting up AppRun..."
cat > ${APP_DIR}/AppRun <<EOF
#!/bin/bash
HERE="\$(dirname "\$(readlink -f "\${0}")")"
USR_DIR="\${HERE}/usr"
VENV_DIR="\${USR_DIR}/venv"

# Export library paths
export LD_LIBRARY_PATH="\${USR_DIR}/lib:\${USR_DIR}/lib/x86_64-linux-gnu:\${LD_LIBRARY_PATH}"

# Wayland / Desktop integration
export XDG_DATA_DIRS="\${USR_DIR}/share:\${XDG_DATA_DIRS}"

# Launch using the bundled venv's python
export PYTHONPATH="\${USR_DIR}/share/whisper-wayland:\${PYTHONPATH}"
exec "\${VENV_DIR}/bin/python3" "\${USR_DIR}/share/whisper-wayland/src/main.py" "\$@"
EOF
chmod +x ${APP_DIR}/AppRun

# 5. AppImage Creation Tool
if ! command -v appimagetool &> /dev/null; then
    if [ ! -f "appimagetool" ]; then
        echo "[*] appimagetool not found, downloading..."
        wget -c https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -O appimagetool
        chmod +x appimagetool
    fi
    TOOL="./appimagetool"
else
    TOOL="appimagetool"
fi

# 6. Build
echo "[*] Packaging AppImage..."
export ARCH=x86_64
$TOOL ${APP_DIR} ${APP_NAME}-x86_64.AppImage

echo "Success! ${APP_NAME}-x86_64.AppImage created."
