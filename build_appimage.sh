#!/usr/bin/env bash
# VoxCtr AppImage Compilation Script
#
# Automates the entire compilation, packaging, and bundling pipeline
# to produce a fully portable standalone AppImage in the workspace root.

set -euo pipefail

# Parse command line options
FORCE_CPU_FLAG=false
NO_MOONSHINE=false
VERBOSE_FLAG=false
HIDE_SHADOW_LIBS=false
for arg in "$@"; do
    case "$arg" in
        --cpu) FORCE_CPU_FLAG=true ;;
        # Build the whisper-cpp-only AppImage (the pre-Moonshine build). This
        # drops the `moonshine` cargo feature so the compile no longer downloads
        # a prebuilt ONNX Runtime from cdn.pyke.io — useful for offline builds or
        # when that download host is unreachable/blocked.
        --no-moonshine) NO_MOONSHINE=true ;;
        # Pass --verbose to `tauri build` and crank up linuxdeploy's own
        # logging. Tauri otherwise collapses a bundling failure into a bare
        # "failed to run linuxdeploy"; this surfaces linuxdeploy's real error.
        --verbose|-v) VERBOSE_FLAG=true ;;
        # Automatically move any third-party GLib/GTK shadow libraries (e.g.
        # Insync's /usr/lib/insync) out of linuxdeploy's search path for the
        # duration of the build and restore them afterwards. Needs sudo. Without
        # this the script only *warns* about them (see the detection in step 1).
        --hide-shadow-libs|--fix-shadow-libs) HIDE_SHADOW_LIBS=true ;;
    esac
done

# Env override: MOONSHINE=0 is equivalent to --no-moonshine.
if [ "${MOONSHINE:-1}" = "0" ]; then
    NO_MOONSHINE=true
fi

# ── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# Shadow-library hiding (see --hide-shadow-libs). SHADOW_HIDE_MAP holds
# "original|hidden" entries for directories we moved out of linuxdeploy's search
# path; restore_shadow_libs() puts them back and is wired to run on any exit
# (normal, error, or Ctrl-C) so a third-party app like Insync is never left
# displaced if the build fails midway.
SHADOW_HIDE_MAP=()
restore_shadow_libs() {
    local entry orig hidden
    for entry in "${SHADOW_HIDE_MAP[@]}"; do
        orig="${entry%%|*}"; hidden="${entry#*|}"
        if [ -e "$hidden" ] && [ ! -e "$orig" ]; then
            if sudo mv "$hidden" "$orig"; then
                info "Restored $orig"
            else
                warn "Could not restore $orig — run manually: sudo mv \"$hidden\" \"$orig\""
            fi
        fi
    done
}
trap restore_shadow_libs EXIT INT TERM

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

# Verify the GTK/WebKit development libraries Tauri links against are present.
# Without these, the Rust compile fails deep in the build with a cryptic
# pkg-config error (e.g. "Package gdk-3.0 was not found") rather than a clear
# up-front message. We check via pkg-config: gtk+-3.0 covers the overlay's gdk
# dependency, webkit2gtk-4.1 covers the Tauri webview.
if ! command -v pkg-config &>/dev/null; then
    fail "'pkg-config' is not installed on your system!"
    info "Building the Tauri application requires pkg-config to locate GTK/WebKit."
    info "👉 Please install it via your package manager:"
    info "   - Arch:   sudo pacman -S pkgconf"
    info "   - Ubuntu: sudo apt install pkg-config"
    info "   - Fedora: sudo dnf install pkgconf-pkg-config"
    echo ""
    exit 1
fi

MISSING_DEV_LIBS=()
for pc in gtk+-3.0 webkit2gtk-4.1; do
    if ! pkg-config --exists "$pc" 2>/dev/null; then
        MISSING_DEV_LIBS+=("$pc")
    fi
done
if [ ${#MISSING_DEV_LIBS[@]} -gt 0 ]; then
    fail "Missing GTK/WebKit development libraries: ${MISSING_DEV_LIBS[*]}"
    info "Tauri links against these at build time; the Rust compile will fail without them."
    info "👉 Please install the development packages for your distribution:"
    info "   - Arch:   sudo pacman -S gtk3 webkit2gtk-4.1"
    info "   - Ubuntu: sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev"
    info "   - Fedora: sudo dnf install gtk3-devel webkit2gtk4.1-devel"
    echo ""
    exit 1
fi

# Verify the tools Tauri's AppImage bundler shells out to. Tauri downloads and
# runs `linuxdeploy` for the final packaging step; linuxdeploy in turn requires
# `patchelf` (to rewrite library rpaths) and `file` (to classify binaries). When
# either is missing the Rust compile succeeds but bundling dies late with a
# generic "failed to run linuxdeploy" and no obvious cause. Check them up front.
MISSING_BUNDLE_TOOLS=()
for tool in patchelf file; do
    if ! command -v "$tool" &>/dev/null; then
        MISSING_BUNDLE_TOOLS+=("$tool")
    fi
done
if [ ${#MISSING_BUNDLE_TOOLS[@]} -gt 0 ]; then
    fail "Missing AppImage bundling tools: ${MISSING_BUNDLE_TOOLS[*]}"
    info "Tauri's AppImage packager runs 'linuxdeploy', which needs these to succeed."
    info "Without them the build fails late with 'failed to run linuxdeploy'."
    info "👉 Please install them via your package manager:"
    info "   - Arch:   sudo pacman -S patchelf file"
    info "   - Ubuntu: sudo apt install patchelf file"
    info "   - Fedora: sudo dnf install patchelf file"
    echo ""
    exit 1
fi

# Warn about third-party directories under /usr/lib (or /lib) that ship their own
# copies of the core GLib/GTK libraries. linuxdeploy's GTK plugin deploys that
# stack by globbing library names across these prefixes and *their subdirectories*
# — not via the loader, the ld.so cache, or the environment — so it sweeps in any
# stray copy it finds and then fails to bundle it when it needs a library the host
# lacks. The classic case is Insync: /usr/lib/insync/libgio-2.0.so.0 needs
# libselinux.so.1, absent on Arch/CachyOS. That surfaces late as a cryptic
# "failed to run linuxdeploy", so flag it up front with the correct remediation.
#
# Note: the shadow directory must be moved *out* of the scanned prefix entirely —
# renaming it in place (e.g. to insync.disabled) or renaming the files (…so.0.bak)
# does NOT help, because the glob still matches anything under /usr/lib.
# Note: /usr/lib64 and /lib are commonly symlinks to /usr/lib (Arch/CachyOS), so
# the same physical directory can be reached under several prefixes. Canonicalize
# each candidate and dedupe, otherwise the same shadow dir is processed twice and
# the second hide collides with the first.
SHADOW_LIB_DIRS=()
declare -A _shadow_seen=()
shopt -s nullglob
for _prefix in /usr/lib /usr/lib64 /lib /lib64; do
    [ -d "$_prefix" ] || continue
    for _sub in "$_prefix"/*/; do
        _sub="${_sub%/}"
        # Skip the standard multiarch dir (e.g. /usr/lib/x86_64-linux-gnu).
        case "$_sub" in */*-linux-gnu) continue ;; esac
        if compgen -G "$_sub/libgio-2.0.so*"        >/dev/null 2>&1 \
           || compgen -G "$_sub/libgobject-2.0.so*" >/dev/null 2>&1 \
           || compgen -G "$_sub/libglib-2.0.so*"    >/dev/null 2>&1 \
           || compgen -G "$_sub/libgdk_pixbuf-2.0.so*" >/dev/null 2>&1 \
           || compgen -G "$_sub/libgtk-3.so*"       >/dev/null 2>&1; then
            _canon="$(readlink -f "$_sub" 2>/dev/null || echo "$_sub")"
            if [ -z "${_shadow_seen[$_canon]:-}" ]; then
                _shadow_seen[$_canon]=1
                SHADOW_LIB_DIRS+=("$_canon")
            fi
        fi
    done
done
shopt -u nullglob
if [ ${#SHADOW_LIB_DIRS[@]} -gt 0 ]; then
    warn "Found third-party copies of the GLib/GTK libraries under /usr/lib:"
    for _d in "${SHADOW_LIB_DIRS[@]}"; do warn "    $_d"; done
    warn "linuxdeploy's GTK plugin globs those library names across /usr/lib and will"
    warn "try to bundle these instead of the system copies, then fail on their extra"
    warn "dependencies (e.g. Insync's libgio-2.0.so.0 needs libselinux.so.1) — the"
    warn "late, cryptic 'failed to run linuxdeploy'."
    if [ "$HIDE_SHADOW_LIBS" = true ]; then
        info "--hide-shadow-libs is set: these will be moved aside for the build and"
        info "restored automatically when it finishes."
    else
        info "Re-run with --hide-shadow-libs to move them aside automatically (needs sudo):"
        info "   ./build_appimage.sh --hide-shadow-libs"
        info "…or do it by hand — move each directory OUT of /usr/lib (a sibling path, not"
        info "a rename in place; the glob still matches anything under /usr/lib), then"
        info "restore it afterwards. For example:"
        for _d in "${SHADOW_LIB_DIRS[@]}"; do
            _hide="$(dirname "$(dirname "$_d")")/$(basename "$_d").buildhide"
            info "   sudo mv \"$_d\" \"$_hide\"    # …build…    then: sudo mv \"$_hide\" \"$_d\""
        done
    fi
    echo ""
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

# Assemble the cargo feature list for the release build.
#   - cuda      : GPU acceleration (only when a CUDA toolkit was found).
#   - moonshine : the Moonshine ONNX speech engine (default on; opt out with
#                 --no-moonshine / MOONSHINE=0). This feature makes the compile
#                 download a prebuilt ONNX Runtime from cdn.pyke.io, so it is the
#                 one part of the build that requires network access.
BUILD_FEATURES=""
if [ "$CUDA_FOUND" = true ]; then
    BUILD_FEATURES="cuda"
fi
if [ "$NO_MOONSHINE" = false ]; then
    BUILD_FEATURES="${BUILD_FEATURES:+$BUILD_FEATURES,}moonshine"
fi

# The Moonshine backend fetches a prebuilt ONNX Runtime from cdn.pyke.io at
# compile time. If that host is unreachable the build only fails after a long
# compile, so probe it up front and point users at the offline escape hatch.
if [ "$NO_MOONSHINE" = false ] && command -v curl &>/dev/null; then
    info "Checking reachability of the ONNX Runtime download host (cdn.pyke.io)..."
    # Without -f, curl succeeds as long as it *connected* (even on an HTTP 4xx),
    # and only fails on connection-level problems: DNS, connect timeout, or a
    # proxy/egress policy rejecting the CONNECT (403/407). That is exactly the
    # condition that dooms the ONNX Runtime download, so key off the exit status.
    if ! curl -s -o /dev/null --connect-timeout 10 --max-time 20 https://cdn.pyke.io/ 2>/dev/null; then
        warn "Could not reach cdn.pyke.io."
        warn "The Moonshine build downloads a prebuilt ONNX Runtime from there at compile"
        warn "time; if it stays unreachable the build will fail after a long compile."
        info "To build the whisper-cpp-only AppImage without this download, re-run with:"
        info "   ./build_appimage.sh --no-moonshine     (or  MOONSHINE=0 ./build_appimage.sh )"
        echo ""
    fi
fi

# In verbose mode, ask Tauri and linuxdeploy to print what they are doing so a
# bundling failure shows linuxdeploy's real error instead of just Tauri's
# generic "failed to run linuxdeploy" wrapper.
TAURI_VERBOSE=()
if [ "$VERBOSE_FLAG" = true ]; then
    TAURI_VERBOSE=(--verbose)
    export VERBOSE=2          # linuxdeploy log verbosity (0=error … 2=debug)
    info "Verbose mode enabled: streaming Tauri + linuxdeploy output."
fi

# With --hide-shadow-libs, move any detected third-party GLib/GTK directories out
# of linuxdeploy's search path now, recording each move so the EXIT trap restores
# them once the build finishes (or fails). They are staged in a sibling of the
# scanned prefix (e.g. /usr/lib/insync -> /usr/insync.buildhide) — outside
# /usr/lib so the plugin's glob can't reach them, and on the same filesystem so
# the move is an instant rename.
if [ "$HIDE_SHADOW_LIBS" = true ] && [ ${#SHADOW_LIB_DIRS[@]} -gt 0 ]; then
    step "Hiding third-party shadow libraries from linuxdeploy"
    for _d in "${SHADOW_LIB_DIRS[@]}"; do
        _hide="$(dirname "$(dirname "$_d")")/$(basename "$_d").buildhide"
        if [ -e "$_hide" ]; then
            fail "Staging path $_hide already exists; refusing to overwrite it."
            info "Move it out of the way and re-run, or restore it manually."
            exit 1
        fi
        info "Moving $_d -> $_hide (restored automatically when the build finishes)..."
        if sudo mv "$_d" "$_hide"; then
            SHADOW_HIDE_MAP+=("$_d|$_hide")
        else
            fail "Could not move $_d out of the way (sudo required)."
            exit 1
        fi
    done
    ok "Shadow libraries hidden; they will be restored automatically on exit."
fi

info "Running Tauri release compiler with headless PATH and CUDA injection..."
if [ -n "$BUILD_FEATURES" ]; then
    info "Compiling with features: ${BUILD_FEATURES}"
    npx tauri build "${TAURI_VERBOSE[@]}" -- --features "$BUILD_FEATURES"
else
    info "Compiling with default features (whisper-cpp only)..."
    npx tauri build "${TAURI_VERBOSE[@]}"
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
