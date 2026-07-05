#!/usr/bin/env bash
# VoxCtr AppImage Compilation Script
#
# Automates the entire compilation, packaging, and bundling pipeline
# to produce a fully portable standalone AppImage in the workspace root.

set -euo pipefail

# Parse command line options
FORCE_CPU_FLAG=false
for arg in "$@"; do
    case "$arg" in
        --cpu) FORCE_CPU_FLAG=true ;;
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

# Create the wrapper if missing
if [ ! -f "./appimagetool" ]; then
    info "Creating headless FUSE-bypass wrapper script..."
    cat > ./appimagetool <<'EOF'
#!/usr/bin/env bash
export QT_QPA_PLATFORM=offscreen
exec "$(dirname "$0")/appimagetool.bin" --appimage-extract-and-run "$@"
EOF
    chmod +x ./appimagetool
fi

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

# Detect and inject common CUDA paths to PATH for CMake nvcc detection in non-interactive shells
for cuda_dir in "/opt/cuda/bin" "/usr/local/cuda/bin"; do
    if [ -d "$cuda_dir" ]; then
        export PATH="$cuda_dir:$PATH"
    fi
done

# Set CUDA home variables if found on the system
CUDA_FOUND=false
if [ -d "/opt/cuda" ]; then
    export CUDA_PATH="/opt/cuda"
    export CUDA_TOOLKIT_ROOT_DIR="/opt/cuda"
    export CUDAToolkit_ROOT="/opt/cuda"
    export CUDACXX="/opt/cuda/bin/nvcc"
    export LD_LIBRARY_PATH="/opt/cuda/lib64:${LD_LIBRARY_PATH:-}"
    export LIBRARY_PATH="/opt/cuda/lib64:${LIBRARY_PATH:-}"
    CUDA_FOUND=true
elif [ -d "/usr/local/cuda" ]; then
    export CUDA_PATH="/usr/local/cuda"
    export CUDA_TOOLKIT_ROOT_DIR="/usr/local/cuda"
    export CUDAToolkit_ROOT="/usr/local/cuda"
    export CUDACXX="/usr/local/cuda/bin/nvcc"
    export LD_LIBRARY_PATH="/usr/local/cuda/lib64:${LD_LIBRARY_PATH:-}"
    export LIBRARY_PATH="/usr/local/cuda/lib64:${LIBRARY_PATH:-}"
    CUDA_FOUND=true
fi

# Check if an NVIDIA GPU is present on the system
HAS_NVIDIA_GPU=false
if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null; then
    HAS_NVIDIA_GPU=true
fi

if [ "$CUDA_FOUND" = false ] && [ "$HAS_NVIDIA_GPU" = true ]; then
    # NVIDIA GPU is present but toolkit is missing. Fail unless forced to compile for CPU.
    if [ "${FORCE_CPU_FLAG}" = "true" ] || [ "${FORCE_CPU:-0}" = "1" ]; then
        warn "NVIDIA GPU detected but CUDA Toolkit is missing. Forcing CPU-only compilation as requested."
    else
        fail "NVIDIA GPU detected, but the CUDA Toolkit (nvcc) was not found in standard paths (/opt/cuda or /usr/local/cuda)!"
        info "Building with CUDA (GPU) support requires the CUDA Toolkit."
        info "👉 Please install the CUDA Toolkit package for your distribution:"
        info "   - Arch:   sudo pacman -S cuda"
        info "   - Ubuntu: sudo apt install nvidia-cuda-toolkit"
        info "   - Fedora: sudo dnf install cuda-toolkit"
        echo ""
        info "If you wish to bypass this check and compile for CPU-only instead, run:"
        info "   FORCE_CPU=1 ./build_appimage.sh  or  ./build_appimage.sh --cpu"
        echo ""
        exit 1
    fi
fi

info "Running Tauri release compiler with headless PATH and CUDA injection..."
# The Moonshine ONNX backend is always compiled in so both speech engines
# (whisper-cpp and Moonshine) are selectable in every AppImage. It links ONNX
# Runtime, fetched at build time, so this step needs network access.
if [ "$CUDA_FOUND" = true ]; then
    info "CUDA detected. Compiling with GPU support (whisper-cpp + Moonshine)..."
    npx tauri build -- --features cuda,moonshine
else
    info "CUDA not detected. Compiling for CPU only (whisper-cpp + Moonshine)..."
    npx tauri build -- --features moonshine
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
PORTABLE_PATH="./${APP_NAME}-${APP_VERSION}-x86_64.AppImage"
SYMLINK_PATH="./${APP_NAME}-latest-x86_64.AppImage"

info "Found compiled bundle: $LATEST_BUNDLE"
info "Moving and exposing portable versioned AppImage to root..."
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
