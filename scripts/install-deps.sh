#!/bin/bash
# Install Python dependencies for VoxCtl.
#
# On modern distros (Ubuntu 23.04+, Debian 12+, Arch, etc.) the system Python is
# "externally managed" and plain `pip install` is blocked. This script handles
# that by preferring a virtual environment. Pass --system to force a system-wide
# install instead (requires --break-system-packages).
#
# Usage:
#   ./scripts/install-deps.sh                  # install into .venv/ (CPU-only torch)
#   ./scripts/install-deps.sh --cuda           # install into .venv/ + torch with CUDA
#   ./scripts/install-deps.sh --system         # install system-wide (not recommended)
#   ./scripts/install-deps.sh --system --cuda  # system-wide + torch with CUDA

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REQUIREMENTS="$REPO_ROOT/requirements.txt"
VENV_DIR="$REPO_ROOT/.venv"
USE_SYSTEM=false
WITH_CUDA=false

for arg in "$@"; do
    case "$arg" in
        --system) USE_SYSTEM=true ;;
        --cuda)   WITH_CUDA=true ;;
        *) echo "Unknown argument: $arg" >&2; exit 1 ;;
    esac
done

if [ ! -f "$REQUIREMENTS" ]; then
    echo "Error: requirements.txt not found at $REQUIREMENTS" >&2
    exit 1
fi

# Resolve which pip to use
_pip() {
    if $USE_SYSTEM; then
        pip install --break-system-packages "$@"
    elif [ -n "${VIRTUAL_ENV:-}" ]; then
        pip install "$@"
    else
        "$VENV_DIR/bin/pip" install "$@"
    fi
}

# --- Set up environment ---

if ! $USE_SYSTEM; then
    if [ -n "${VIRTUAL_ENV:-}" ]; then
        echo "Active venv detected: $VIRTUAL_ENV"
    else
        if [ ! -d "$VENV_DIR" ]; then
            echo "Creating virtual environment at $VENV_DIR ..."
            python3 -m venv "$VENV_DIR"
        fi
        echo "Using venv: $VENV_DIR"
        _pip --upgrade pip --quiet
    fi
else
    echo "Installing system-wide (--break-system-packages)..."
fi

# --- Core dependencies ---

echo ""
echo "Installing core dependencies from requirements.txt ..."
_pip install -r "$REQUIREMENTS"

# --- PyAudio system lib hint ---
# PyAudio requires portaudio. If it failed, give a clear hint.
if ! python3 -c "import pyaudio" 2>/dev/null; then
    echo ""
    echo "WARNING: PyAudio failed to install. You likely need the portaudio dev library:"
    echo "  Ubuntu/Debian: sudo apt install portaudio19-dev"
    echo "  Arch:          sudo pacman -S portaudio"
    echo "  Fedora:        sudo dnf install portaudio-devel"
    echo "Then re-run this script."
fi

# --- Torch (+ optional CUDA) ---

echo ""
if $WITH_CUDA; then
    # Detect installed CUDA version via nvidia-smi
    CUDA_VER=""
    if command -v nvidia-smi &>/dev/null; then
        CUDA_VER=$(nvidia-smi | grep -oP "CUDA Version: \K[0-9]+\.[0-9]+" | head -1)
    fi

    if [ -z "$CUDA_VER" ]; then
        echo "WARNING: --cuda requested but nvidia-smi not found or returned no version."
        echo "Falling back to CUDA 12.1 wheel. If this is wrong, edit the URL below."
        CUDA_VER="12.1"
    fi

    # Map X.Y → cuXYZ wheel tag used by PyTorch
    CUDA_MAJOR="${CUDA_VER%%.*}"
    CUDA_MINOR="${CUDA_VER#*.}"
    CUDA_MINOR="${CUDA_MINOR%%.*}"  # strip any patch

    # PyTorch ships wheels for cu118, cu121, cu124, cu126, cu128
    if   [ "$CUDA_MAJOR" -eq 11 ] && [ "$CUDA_MINOR" -ge 8 ]; then CU_TAG="cu118"
    elif [ "$CUDA_MAJOR" -eq 12 ] && [ "$CUDA_MINOR" -lt 1 ]; then CU_TAG="cu118"
    elif [ "$CUDA_MAJOR" -eq 12 ] && [ "$CUDA_MINOR" -lt 4 ]; then CU_TAG="cu121"
    elif [ "$CUDA_MAJOR" -eq 12 ] && [ "$CUDA_MINOR" -lt 6 ]; then CU_TAG="cu124"
    elif [ "$CUDA_MAJOR" -eq 12 ] && [ "$CUDA_MINOR" -lt 8 ]; then CU_TAG="cu126"
    else CU_TAG="cu128"
    fi

    TORCH_INDEX="https://download.pytorch.org/whl/${CU_TAG}"
    echo "Detected CUDA ${CUDA_VER} — installing torch with ${CU_TAG} support..."
    echo "  Index URL: $TORCH_INDEX"
    _pip install torch torchvision torchaudio --index-url "$TORCH_INDEX"
else
    echo "Installing CPU-only torch (pass --cuda to get GPU-accelerated build)..."
    _pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cpu
fi

# --- Summary ---

echo ""
echo "=========================================="
echo " All dependencies installed successfully."
echo "=========================================="

if ! $USE_SYSTEM && [ -z "${VIRTUAL_ENV:-}" ]; then
    echo ""
    echo "Activate the environment before running VoxCtl:"
    echo "  source $VENV_DIR/bin/activate"
fi
