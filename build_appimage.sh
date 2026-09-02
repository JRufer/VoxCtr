#!/usr/bin/env bash
# VoxCtr AppImage Compilation Script
#
# Automates the entire compilation, packaging, and bundling pipeline
# to produce a fully portable standalone AppImage in the workspace root.

set -euo pipefail

# Parse command line options
FORCE_CPU_FLAG=false
FORCE_VULKAN_FLAG=false
for arg in "$@"; do
    case "$arg" in
        --cpu) FORCE_CPU_FLAG=true ;;
        --vulkan) FORCE_VULKAN_FLAG=true ;;
    esac
done

# ── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ══════════════════════════════════════════════════════════════════════════════
# 1. Verification of appimagetool Wrapper
# ══════════════════════════════════════════════════════════════════════════════
step "Checking AppImage Compiler Toolchain"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

# Parse application metadata from tauri.conf.json
TAURI_CONF="src-tauri/tauri.conf.json"
if [ ! -f "$TAURI_CONF" ]; then
    fail "Could not find Tauri configuration file at $TAURI_CONF"
    exit 1
fi

if command -v jq &>/dev/null; then
    APP_NAME=$(jq -r '.productName' "$TAURI_CONF")
    APP_VERSION=$(jq -r '.version' "$TAURI_CONF")
else
    APP_NAME=$(grep -oP '"productName":\s*"\K[^"]+' "$TAURI_CONF" || echo "VoxCtr")
    APP_VERSION=$(grep -oP '"version":\s*"\K[^"]+' "$TAURI_CONF" || echo "0.1.0")
fi

# Ensure the raw binary is renamed
if [ -f "./appimagetool" ] && [ ! -f "./appimagetool.bin" ]; then
    info "Found raw appimagetool binary. Restructuring into wrapper setup..."
    mv appimagetool appimagetool.bin
    chmod +x appimagetool.bin
fi

# Create the wrapper script
info "Creating headless FUSE-bypass wrapper script..."
cat > ./appimagetool <<'EOF'
#!/usr/bin/env bash
export QT_QPA_PLATFORM=offscreen
exec "$(dirname "$0")/appimagetool.bin" --appimage-extract-and-run "$@"
EOF
chmod +x ./appimagetool

# Verify unsquashfs is installed (required for FUSE-less extraction of AppImage builders)
if ! command -v unsquashfs &>/dev/null; then
    fail "The 'unsquashfs' utility is not installed on your system!"
    info "Building or running AppImages in FUSE-less mode requires squashfs-tools."
    info "👉 Please run './install.sh' to install it automatically, or run:"
    info "   - Arch:   sudo pacman -S squashfs-tools"
    info "   - Ubuntu: sudo apt install squashfs-tools"
    info "   - Fedora: sudo dnf install squashfs-tools"
    echo ""
    exit 1
fi

# Verify npm is installed (required for Svelte/Vite frontend assets)
if ! command -v npm &>/dev/null; then
    fail "The Node Package Manager 'npm' is not installed on your system!"
    info "Building the Svelte frontend requires Node.js and npm."
    info "👉 Please install it via your package manager:"
    info "   - Arch:   sudo pacman -S npm"
    info "   - Ubuntu: sudo apt install npm"
    info "   - Fedora: sudo dnf install npm"
    echo ""
    exit 1
fi

# Verify cargo is installed and functional
if ! command -v cargo &>/dev/null || ! cargo --version &>/dev/null; then
    # If cargo is missing or not functional, check if rustup is available to configure it
    if command -v rustup &>/dev/null; then
        info "rustup is installed but no default toolchain is configured."
        info "Attempting to initialize rustup stable toolchain..."
        rustup default stable || true
        if [ -f "$HOME/.cargo/env" ]; then
            source "$HOME/.cargo/env" || true
        fi
    fi

    # Check cargo again after trying to initialize
    if ! command -v cargo &>/dev/null || ! cargo --version &>/dev/null; then
        fail "The Rust compiler toolchain 'cargo' is not installed or not configured on your system!"
        info "Building the Tauri application requires Cargo and the Rust toolchain."
        info "👉 Please install it via rustup (recommended) or your package manager:"
        info "   - rustup (Recommended): curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        info "   - Arch:   sudo pacman -S rustup && rustup default stable"
        info "   - Ubuntu: sudo apt install cargo"
        info "   - Fedora: sudo dnf install cargo"
        echo ""
        exit 1
    fi
fi

# Verify cmake is installed (required for whisper-rs compilation)
if ! command -v cmake &>/dev/null; then
    fail "The 'cmake' build tool is not installed on your system!"
    info "Building the application requires CMake to compile whisper-rs."
    info "👉 Please install it via your package manager:"
    info "   - Arch:   sudo pacman -S cmake"
    info "   - Ubuntu: sudo apt install cmake"
    info "   - Fedora: sudo dnf install cmake"
    echo ""
    exit 1
fi

ok "AppImage toolchain wrapper is verified and ready."

# ══════════════════════════════════════════════════════════════════════════════
# 2. Build Frontend (Vite / Svelte)
# ══════════════════════════════════════════════════════════════════════════════
step "Building Svelte Frontend Assets"

if [ ! -d "node_modules" ]; then
    info "Installing frontend node packages..."
    npm install
fi

info "Compiling frontend bundle..."
npm run build
ok "Frontend compiled successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 3. Compile & Bundle Tauri / Rust App
# ══════════════════════════════════════════════════════════════════════════════
step "Compiling & Packaging Tauri Application"

# Inject our root folder into PATH so Tauri's bundler uses our wrapper
export PATH="$ROOT_DIR:$PATH"
export QT_QPA_PLATFORM=offscreen
export APPIMAGE_EXTRACT_AND_RUN=1
export NO_STRIP=true

# Check if an NVIDIA GPU is present on the system
HAS_NVIDIA_GPU=false
if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null; then
    HAS_NVIDIA_GPU=true
fi

# Detect if CUDA toolkit is present (only used to warn/explain behavior to the user)
CUDA_FOUND=false
if [ -d "/opt/cuda" ] || [ -d "/usr/local/cuda" ]; then
    CUDA_FOUND=true
fi

# Check if Vulkan development headers are installed (required to compile ggml-vulkan)
HAS_VULKAN_HEADERS=false
if echo "#include <vulkan/vulkan.h>" | cc -E - &>/dev/null; then
    HAS_VULKAN_HEADERS=true
fi

detect_pkg_manager() {
    if command -v pacman &>/dev/null;    then echo "pacman"
    elif command -v apt-get &>/dev/null; then echo "apt"
    elif command -v dnf &>/dev/null;     then echo "dnf"
    elif command -v zypper &>/dev/null;  then echo "zypper"
    else                                      echo "unknown"
    fi
}

install_vulkan_deps() {
    local PKG_MGR
    PKG_MGR=$(detect_pkg_manager)
    info "Installing Vulkan build dependencies (requires sudo)..."
    case "$PKG_MGR" in
        apt)
            sudo apt-get update -y
            sudo apt-get install -y libvulkan-dev shaderc
            ;;
        pacman)
            sudo pacman -S --noconfirm --needed vulkan-headers shaderc
            ;;
        dnf)
            sudo dnf install -y vulkan-headers shaderc
            ;;
        zypper)
            sudo zypper install -y vulkan-headers shaderc
            ;;
        *)
            fail "Unable to auto-install Vulkan dependencies for package manager: $PKG_MGR"
            info "👉 Please install Vulkan headers (e.g. libvulkan-dev) and shaderc (glslc) manually."
            exit 1
            ;;
    esac
}

# Determine build mode (vulkan or cpu)
BUILD_MODE="cpu"
if [ "$FORCE_CPU_FLAG" = "true" ] || [ "${FORCE_CPU:-0}" = "1" ]; then
    BUILD_MODE="cpu"
else
    # Check if Vulkan is requested or if a GPU is present
    if [ "$FORCE_VULKAN_FLAG" = "true" ] || [ "$HAS_NVIDIA_GPU" = true ] || command -v vulkaninfo &>/dev/null; then
        BUILD_MODE="vulkan"
        
        # Check if we need to install dependencies
        if ! command -v glslc &>/dev/null || [ "$HAS_VULKAN_HEADERS" = false ]; then
            info "Vulkan compiler (glslc) or development headers (vulkan/vulkan.h) are missing."
            install_vulkan_deps
            
            # Re-verify after installation attempt
            HAS_VULKAN_HEADERS=false
            if echo "#include <vulkan/vulkan.h>" | cc -E - &>/dev/null; then
                HAS_VULKAN_HEADERS=true
            fi
            
            if ! command -v glslc &>/dev/null || [ "$HAS_VULKAN_HEADERS" = false ]; then
                fail "Vulkan build requirements (glslc and vulkan/vulkan.h) are still missing after package installation attempt."
                exit 1
            fi
        fi
    else
        BUILD_MODE="cpu"
    fi
fi

info "Running Tauri release compiler with headless PATH..."
# Set up a cleanup trap to restore /usr/lib/insync if it was hidden
cleanup() {
    if [ -d "/tmp/insync-build-temp" ]; then
        info "Restoring /usr/lib/insync..."
        sudo mv /tmp/insync-build-temp /usr/lib/insync || true
    fi
}
trap cleanup EXIT

# Temporarily hide /usr/lib/insync to prevent linuxdeploy from scanning it
# by moving it completely outside of the /usr/lib directory
if [ -d "/usr/lib/insync" ]; then
    info "Temporarily hiding /usr/lib/insync during the build (requires sudo)..."
    sudo mv /usr/lib/insync /tmp/insync-build-temp
fi

# The Moonshine ONNX backend is always compiled in so both speech engines
# (whisper-cpp and Moonshine) are selectable in every AppImage. It links ONNX
# Runtime, fetched at build time, so this step needs network access.
if [ "$BUILD_MODE" = "vulkan" ]; then
    info "Compiling with Vulkan GPU support (whisper-cpp + Moonshine + Inflect)..."
    if [ "$CUDA_FOUND" = true ]; then
        warn "CUDA Toolkit was detected, but AppImages cannot bundle CUDA support"
        warn "because linuxdeploy attempts to bundle the massive CUDA libraries, which fails."
        warn "We are compiling with Vulkan GPU acceleration instead."
    fi
    npx tauri build --verbose -- --features vulkan
else
    info "Compiling for CPU only (whisper-cpp + Moonshine + Inflect)..."
    # Moonshine and Inflect-Micro are default features; only GPU backends and
    # `custom-protocol` need naming here.
    npx tauri build --verbose
fi

ok "Compilation finished successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 4. Relocate & Expose Portable AppImage
# ══════════════════════════════════════════════════════════════════════════════
step "Exposing Portable AppImage to Root"

# Locate compiled AppImage files in target bundle directories
BUNDLE_DIR="./target/release/bundle/appimage"
if [ ! -d "$BUNDLE_DIR" ]; then
    # Fallback to local cargo/tauri target configurations
    BUNDLE_DIR="./src-tauri/target/release/bundle/appimage"
fi

FOUND_APPIMAGES=( $(find "$BUNDLE_DIR" -maxdepth 1 -name "*.AppImage" 2>/dev/null || true) )

if [ ${#FOUND_APPIMAGES[@]} -eq 0 ]; then
    fail "Could not locate compiled AppImage bundle in target outputs!"
    exit 1
fi

LATEST_BUNDLE="${FOUND_APPIMAGES[0]}"

# Slim the AppImage by stripping graphics, Wayland, and input libraries so it falls through
# to using the host system's native ones at runtime (resolves overlay UI and hotkey bugs).
info "Found compiled bundle: $LATEST_BUNDLE"
info "Slimming AppImage (stripping graphics, Wayland, and input libraries)..."

# Extract the AppImage
work=$(mktemp -d)
cp "$LATEST_BUNDLE" "$work/in.AppImage"
chmod +x "$work/in.AppImage"
( cd "$work" && ./in.AppImage --appimage-extract >/dev/null )

root="$work/squashfs-root"

# Bundle WebKitGTK 4.1 helper processes (WebKitNetworkProcess, WebKitWebProcess)
helper_dir=""
for dir in "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1" "/usr/lib/webkit2gtk-4.1" "/usr/lib64/webkit2gtk-4.1"; do
    if [ -d "$dir" ]; then
        helper_dir="$dir"
        break
    fi
done

if [ -n "$helper_dir" ]; then
    info "Found WebKitGTK helper processes in $helper_dir, bundling into AppImage..."
    mkdir -p "$root/usr/lib/webkit2gtk-4.1" \
             "$root/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1" \
             "$root/lib/webkit2gtk-4.1"
    cp -r "$helper_dir"/* "$root/usr/lib/webkit2gtk-4.1/"
    cp -r "$helper_dir"/* "$root/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/"
    cp -r "$helper_dir"/* "$root/lib/webkit2gtk-4.1/"
fi

# Append WEBKIT_EXEC_PATH export into linuxdeploy GTK apprun hook and AppRun
for script in "$root/AppRun" "$root/apprun-hooks/linuxdeploy-plugin-gtk.sh"; do
    if [ -f "$script" ] && ! grep -q "WEBKIT_EXEC_PATH" "$script"; then
        sed -i '2i export WEBKIT_EXEC_PATH="${APPDIR}/usr/lib/webkit2gtk-4.1:${APPDIR}/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1"' "$script"
        sed -i '3i export WEBKIT_INJECTED_BUNDLE_PATH="${APPDIR}/usr/lib/webkit2gtk-4.1/injected-bundle:${APPDIR}/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/injected-bundle"' "$script"
    fi
done

# Strip graphics, Wayland, input libraries, and host-dependent/security libraries
# so the AppImage falls through to the host system's native versions at runtime.
for pat in 'libwayland-*.so*' 'libEGL.so*' 'libGL.so*' 'libGLX.so*' \
           'libGLdispatch.so*' 'libOpenGL.so*' 'libglapi.so*' \
           'libgbm.so*' 'libdrm.so*' \
           'libxkbcommon.so*' 'libxkbcommon-x11.so*' \
           'libgstreamer-*.so*' 'libgst*.so*' \
           'libvulkan.so*' 'libssl.so*' 'libcrypto.so*' \
           'libcanberra-gtk3.so*' 'libcanberra.so*' \
           'libcanberra-gtk-module.so' 'libcanberra-gtk3-module.so' \
           'libcolorreload-gtk-module.so' 'libwindow-decorations-gtk-module.so'; do
    find "$root" -name "$pat" -print -delete 2>/dev/null || true
done

# Repackage the AppImage
rm -f "$LATEST_BUNDLE"
ARCH=x86_64 ./appimagetool.bin "$root" "$LATEST_BUNDLE" >/dev/null

rm -rf "$work"
ok "AppImage successfully slimmed."

if [ "$BUILD_MODE" = "vulkan" ]; then
    PORTABLE_PATH="./${APP_NAME}_${APP_VERSION}_amd64-linux-x86_64-vulkan.appimage"
else
    PORTABLE_PATH="./${APP_NAME}_${APP_VERSION}_amd64-linux-x86_64.appimage"
fi
SYMLINK_PATH="./${APP_NAME}-latest-x86_64.AppImage"

info "Moving and exposing portable versioned AppImage to root..."
rm -f "$PORTABLE_PATH"
cp "$LATEST_BUNDLE" "$PORTABLE_PATH"
chmod +x "$PORTABLE_PATH"

# Establish a latest symlink to maintain compatibility for local scripts/runners
ln -sf "$(basename "$PORTABLE_PATH")" "$SYMLINK_PATH"
info "Created latest symlink: $SYMLINK_PATH -> $PORTABLE_PATH"

echo ""
echo -e "${BOLD}==================================================${NC}"
echo -e "${BOLD}  Portable AppImage Compiled Successfully!${NC}"
echo -e "${BOLD}==================================================${NC}"
echo ""
echo "  Your fully standalone, portable application is ready:"
echo -e "    👉 ${GREEN}${PORTABLE_PATH}${NC} ($(du -sh "$PORTABLE_PATH" | cut -f1))"
echo -e "    👉 Symlink: ${GREEN}${SYMLINK_PATH}${NC}"
echo ""
echo "  To launch and test the application directly, run:"
echo "    $PORTABLE_PATH"
echo ""
