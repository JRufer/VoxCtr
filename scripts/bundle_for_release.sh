#!/bin/bash
# VoxCtl Release Bundle Script
#
# Packages the AppImage and all files needed by install.sh into a single zip
# that the user can extract anywhere and run:
#
#   unzip VoxCtl-x86_64.zip
#   cd VoxCtl
#   bash install.sh

set -e

APPIMAGE="VoxCtl-x86_64.AppImage"
BUNDLE_DIR="VoxCtl"
OUT_ZIP="VoxCtl-x86_64.zip"

# ── Pre-flight ────────────────────────────────────────────────────────────────

if [ ! -f "$APPIMAGE" ]; then
    echo "[FAIL] $APPIMAGE not found in the current directory."
    echo "       Build it first:  bash scripts/build_appimage.sh"
    exit 1
fi

if ! command -v zip &>/dev/null; then
    echo "[FAIL] 'zip' is not installed. Install it and try again."
    exit 1
fi

# ── Assemble bundle directory ─────────────────────────────────────────────────

echo "[*] Assembling release bundle..."

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/scripts"
mkdir -p "$BUNDLE_DIR/assets"

cp "$APPIMAGE"                   "$BUNDLE_DIR/"
cp install.sh                    "$BUNDLE_DIR/"
cp scripts/setup-permissions.sh  "$BUNDLE_DIR/scripts/"
cp assets/app_icon.png           "$BUNDLE_DIR/assets/"

chmod +x "$BUNDLE_DIR/$APPIMAGE"
chmod +x "$BUNDLE_DIR/install.sh"
chmod +x "$BUNDLE_DIR/scripts/setup-permissions.sh"

# ── Zip ───────────────────────────────────────────────────────────────────────

rm -f "$OUT_ZIP"
zip -r "$OUT_ZIP" "$BUNDLE_DIR"
rm -rf "$BUNDLE_DIR"

echo ""
echo "[OK] Created $OUT_ZIP"
echo ""
echo "     Users can install with:"
echo "       unzip $OUT_ZIP"
echo "       cd $BUNDLE_DIR"
echo "       bash install.sh"
