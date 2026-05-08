#!/bin/bash
# Install Python dependencies for VoxCtl.
#
# On modern distros (Ubuntu 23.04+, Debian 12+, Arch, etc.) the system Python is
# "externally managed" and plain `pip install` is blocked. This script handles
# that by preferring a virtual environment. Pass --system to force a system-wide
# install instead (requires --break-system-packages).
#
# Usage:
#   ./scripts/install-deps.sh            # install into .venv/
#   ./scripts/install-deps.sh --system   # install system-wide (not recommended)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REQUIREMENTS="$REPO_ROOT/requirements.txt"
VENV_DIR="$REPO_ROOT/.venv"
USE_SYSTEM=false

for arg in "$@"; do
    case "$arg" in
        --system) USE_SYSTEM=true ;;
        *) echo "Unknown argument: $arg" >&2; exit 1 ;;
    esac
done

if [ ! -f "$REQUIREMENTS" ]; then
    echo "Error: requirements.txt not found at $REQUIREMENTS" >&2
    exit 1
fi

if $USE_SYSTEM; then
    echo "Installing system-wide (--break-system-packages)..."
    pip install --break-system-packages -r "$REQUIREMENTS"
    echo "Done."
    exit 0
fi

# --- Virtual environment path ---

# If we're already inside a venv, install there directly.
if [ -n "${VIRTUAL_ENV:-}" ]; then
    echo "Active venv detected: $VIRTUAL_ENV"
    echo "Installing into active venv..."
    pip install -r "$REQUIREMENTS"
    echo "Done."
    exit 0
fi

# Create the venv if it doesn't exist.
if [ ! -d "$VENV_DIR" ]; then
    echo "Creating virtual environment at $VENV_DIR ..."
    python3 -m venv "$VENV_DIR"
fi

echo "Installing dependencies into $VENV_DIR ..."
"$VENV_DIR/bin/pip" install --upgrade pip --quiet
"$VENV_DIR/bin/pip" install -r "$REQUIREMENTS"

echo ""
echo "Done. Activate the environment with:"
echo "  source $VENV_DIR/bin/activate"
