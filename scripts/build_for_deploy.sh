#!/bin/bash
# VoxCtl Deploy Bundle Builder
#
# Builds the AppImage and packages it with everything a user needs to install
# VoxCtl: the AppImage, install.sh, and supporting files.
#
# Usage:
#   bash scripts/build_for_deploy.sh [--output <path>]
#
# Output:
#   VoxCtl-deploy-<version>.zip  (default: project root)
#
# Unzip on target machine, then run:
#   cd VoxCtl-deploy-<version>/
#   bash install.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Colours ──────────────────────────────────────────────────────────────────
BOLD='\033[1m'; GREEN='\033[0;32m'; BLUE='\033[0;34m'
YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; exit 1; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ── Parse args ────────────────────────────────────────────────────────────────
OUTPUT_DIR="${PROJECT_ROOT}"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output) OUTPUT_DIR="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: bash scripts/build_for_deploy.sh [--output <dir>]"
            exit 0 ;;
        *) fail "Unknown argument: $1" ;;
    esac
done

cd "${PROJECT_ROOT}"

echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  VoxCtl Deploy Bundle Builder${NC}"
echo -e "${BOLD}==========================================${NC}"

# ── Determine version ─────────────────────────────────────────────────────────
VERSION=$(git describe --tags --always --dirty 2>/dev/null || echo "dev")
BUNDLE_NAME="VoxCtl-deploy-${VERSION}"
APPIMAGE_FILE="VoxCtl-x86_64.AppImage"
ZIP_FILE="${OUTPUT_DIR}/${BUNDLE_NAME}.zip"

info "Version   : ${VERSION}"
info "Bundle    : ${ZIP_FILE}"

# ── Step 1: Build the AppImage ────────────────────────────────────────────────
step "Building AppImage"

if [[ -f "${APPIMAGE_FILE}" ]]; then
    warn "Existing ${APPIMAGE_FILE} found — rebuilding..."
fi

bash scripts/build_appimage.sh
ok "AppImage built: ${APPIMAGE_FILE}"

# ── Step 2: Assemble staging directory ────────────────────────────────────────
step "Assembling deploy bundle"

STAGE_DIR=$(mktemp -d)
BUNDLE_DIR="${STAGE_DIR}/${BUNDLE_NAME}"
mkdir -p "${BUNDLE_DIR}"

# Core: AppImage + installer
cp "${APPIMAGE_FILE}"   "${BUNDLE_DIR}/"
cp "install.sh"         "${BUNDLE_DIR}/"
chmod +x "${BUNDLE_DIR}/install.sh"

# Scripts needed by install.sh at runtime
mkdir -p "${BUNDLE_DIR}/scripts"
cp "scripts/setup-permissions.sh" "${BUNDLE_DIR}/scripts/"
chmod +x "${BUNDLE_DIR}/scripts/setup-permissions.sh"

# Icon (used by install.sh for the desktop entry)
mkdir -p "${BUNDLE_DIR}/assets"
cp "assets/app_icon.png" "${BUNDLE_DIR}/assets/"

# Desktop entry template (install.sh writes it programmatically, but include it
# so advanced users can inspect it)
cp "voxctl.desktop" "${BUNDLE_DIR}/" 2>/dev/null || true

ok "Staged to ${BUNDLE_DIR}"

# ── Step 3: Verify the bundle looks complete ──────────────────────────────────
step "Verifying bundle contents"

REQUIRED_FILES=(
    "${APPIMAGE_FILE}"
    "install.sh"
    "scripts/setup-permissions.sh"
    "assets/app_icon.png"
)

for f in "${REQUIRED_FILES[@]}"; do
    if [[ -f "${BUNDLE_DIR}/${f}" ]]; then
        ok "  ${f}"
    else
        fail "Missing expected file: ${f}"
    fi
done

# ── Step 4: Create the zip ────────────────────────────────────────────────────
step "Creating zip archive"

mkdir -p "${OUTPUT_DIR}"
rm -f "${ZIP_FILE}"

(cd "${STAGE_DIR}" && zip -r "${ZIP_FILE}" "${BUNDLE_NAME}/")
rm -rf "${STAGE_DIR}"

ZIP_SIZE=$(du -sh "${ZIP_FILE}" | cut -f1)
ok "Archive: ${ZIP_FILE}  (${ZIP_SIZE})"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Bundle ready!${NC}"
echo -e "${BOLD}==========================================${NC}"
echo ""
echo "  ${ZIP_FILE}"
echo ""
echo "  Share this zip with users. On their machine:"
echo ""
echo "    unzip ${BUNDLE_NAME}.zip"
echo "    cd ${BUNDLE_NAME}"
echo "    bash install.sh"
echo ""
