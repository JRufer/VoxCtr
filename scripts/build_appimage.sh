#!/bin/bash
# VoxCtl AppImage Build Script
#
# Bundles the Python application and all pip dependencies into a portable AppImage.
#
# What IS bundled:
#   All packages from requirements.txt (PyQt6, faster-whisper, onnxruntime, PyAudio
#   Python wrapper, dbus-python, evdev, noisereduce, scipy, numpy, websockets, mcp,
#   and 50+ more).
#
# Note: pyatspi is NOT bundled — it is a system package (python3-pyatspi) not
#   available on PyPI for Python 3.11+. AppRun adds the host site-packages to
#   PYTHONPATH so the app finds it if the user installed python3-pyatspi.
#
# What is NOT bundled (must be present on the host — installed by install.sh):
#   • libportaudio2  — PyAudio's C extension links to this at runtime
#   • libdbus-1      — dbus-python's C extension links to this at runtime
#   • wtype          — Wayland text injection binary
#   • xdotool        — X11 text injection binary
#   • wl-clipboard   — Wayland clipboard tools (wl-copy / wl-paste)
#   • xclip          — X11 clipboard tool
#   • alsa-utils     — provides aplay for TTS audio output
#   • piper          — neural TTS binary (installed to /opt/piper by install.sh)
#   • espeak-ng      — TTS fallback binary
#   • socat          — optional MCP/Claude Desktop bridge (stdio transport)
#   • AT-SPI2 daemon — pyatspi is bundled, but the host AT-SPI registry must be running

set -e

APP_NAME="VoxCtl"
APP_DIR="AppDir"
PYTHON_VERSION=$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')

echo "Building AppImage for ${APP_NAME} (Python ${PYTHON_VERSION})..."

# 1. Clean and set up AppDir structure
rm -rf "${APP_DIR}" ./*.AppImage
mkdir -p "${APP_DIR}/usr/bin"
mkdir -p "${APP_DIR}/usr/share/voxctl"
mkdir -p "${APP_DIR}/usr/share/metainfo"
mkdir -p "${APP_DIR}/usr/share/applications"
mkdir -p "${APP_DIR}/usr/share/icons/hicolor/256x256/apps"

# 2. Copy application files
echo "[*] Copying application source and assets..."
cp -r src/    "${APP_DIR}/usr/share/voxctl/"
cp -r assets/ "${APP_DIR}/usr/share/voxctl/"

cp voxctl.desktop "${APP_DIR}/usr/share/applications/"
sed -i "s|^Exec=.*|Exec=voxctl|"   "${APP_DIR}/usr/share/applications/voxctl.desktop"
sed -i "s|^Icon=.*|Icon=voxctl|"   "${APP_DIR}/usr/share/applications/voxctl.desktop"

cp assets/app_icon.png "${APP_DIR}/usr/share/icons/hicolor/256x256/apps/voxctl.png"
cp assets/app_icon.png "${APP_DIR}/voxctl.png"
ln -sf usr/share/applications/voxctl.desktop "${APP_DIR}/voxctl.desktop"

# 3. Install all pip dependencies into a self-contained venv
echo "[*] Creating virtual environment inside AppDir..."
python3 -m venv "${APP_DIR}/usr/venv"

echo "[*] Installing dependencies into AppDir venv..."
"${APP_DIR}/usr/venv/bin/pip" install --upgrade pip --quiet
"${APP_DIR}/usr/venv/bin/pip" install -r requirements.txt

# 4. Write AppRun (keep in sync with repo-root AppRun)
echo "[*] Writing AppRun..."
cat > "${APP_DIR}/AppRun" <<'EOF'
#!/bin/bash
HERE="$(dirname "$(readlink -f "${0}")")"
USR_DIR="${HERE}/usr"
VENV_DIR="${USR_DIR}/venv"

# Make PyAudio/dbus-python C extensions findable (host libs: libportaudio2, libdbus-1)
export LD_LIBRARY_PATH="${USR_DIR}/lib:${USR_DIR}/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH}"

# Wayland / XDG desktop integration
export XDG_DATA_DIRS="${USR_DIR}/share:${XDG_DATA_DIRS}"

# pyatspi is a system package (python3-pyatspi) that cannot be bundled via pip.
# Add the host system site-packages so pyatspi is found if installed on the host.
SYS_SITE=$(python3 -c "import site; print(site.getsitepackages()[0])" 2>/dev/null || true)
export PYTHONPATH="${USR_DIR}/share/voxctl:${SYS_SITE}:${PYTHONPATH}"

exec "${VENV_DIR}/bin/python3" "${USR_DIR}/share/voxctl/src/main.py" "$@"
EOF
chmod +x "${APP_DIR}/AppRun"

# 5. Locate or download appimagetool
if command -v appimagetool &>/dev/null; then
    TOOL="appimagetool"
elif [ -f "./appimagetool" ]; then
    TOOL="./appimagetool"
else
    echo "[*] appimagetool not found — downloading..."
    wget -c "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
         -O appimagetool
    chmod +x appimagetool
    TOOL="./appimagetool"
fi

# 6. Package the AppImage
echo "[*] Packaging AppImage..."
export ARCH=x86_64
$TOOL "${APP_DIR}" "${APP_NAME}-x86_64.AppImage"

echo ""
echo "Success!  ${APP_NAME}-x86_64.AppImage created."
echo ""
echo "To install:  bash install.sh"
