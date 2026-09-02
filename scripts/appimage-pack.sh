#!/usr/bin/env bash
# Package an AppDir into an AppImage, preferring the uruntime runtime.
#
# Why not just call appimagetool: its built-in runtime needs a libfuse 2
# `fusermount` binary that Ubuntu 22.04 / Linux Mint 21 and newer do not install
# (see scripts/fetch-uruntime.sh). uruntime speaks FUSE3 and falls back to
# extract-and-run, so the AppImage starts on a stock desktop.
#
# Usage: appimage-pack.sh <AppDir> <output.AppImage> [appimagetool]
#
# If uruntime cannot be fetched, or the AppImage built with it does not verify,
# this falls back to appimagetool's built-in runtime — the packaging run is
# never left worse off than before.
set -euo pipefail

appdir="${1:?usage: appimage-pack.sh <AppDir> <output.AppImage> [appimagetool]}"
out="${2:?usage: appimage-pack.sh <AppDir> <output.AppImage> [appimagetool]}"
appimagetool="${3:-./appimagetool.bin}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

pack() { # $1: runtime file to embed, or "" for appimagetool's built-in one
    rm -f "$out"
    if [ -n "${1:-}" ]; then
        ARCH=x86_64 "$appimagetool" --runtime-file "$1" "$appdir" "$out"
    else
        ARCH=x86_64 "$appimagetool" "$appdir" "$out"
    fi
}

# A packaged AppImage has to unpack itself and carry a launcher. --appimage-extract
# needs no FUSE, so this works on CI runners and in containers.
verify() {
    local img work ok=1
    img="$(readlink -f "$out")"
    [ -f "$img" ] || return 1
    chmod +x "$img"
    work="$(mktemp -d)"
    ( cd "$work" && "$img" --appimage-extract >/dev/null 2>&1 ) || ok=0
    # The extraction directory is squashfs-root for both runtimes; accept any
    # extracted AppDir so a future runtime that names it differently still
    # verifies instead of silently falling back.
    [ -n "$(find "$work" -maxdepth 2 -name AppRun -type f -print -quit)" ] || ok=0
    rm -rf "$work"
    [ "$ok" = 1 ]
}

runtime_dir="$(mktemp -d)"
trap 'rm -rf "$runtime_dir"' EXIT

if "$script_dir/fetch-uruntime.sh" "$runtime_dir/uruntime"; then
    echo "appimage-pack: packaging with uruntime"
    if pack "$runtime_dir/uruntime" && verify; then
        echo "appimage-pack: packaged and verified $out (uruntime)"
        exit 0
    fi
    echo "appimage-pack: WARNING uruntime packaging did not verify; falling back to appimagetool's built-in runtime" >&2
else
    echo "appimage-pack: WARNING uruntime unavailable; using appimagetool's built-in runtime" >&2
fi

pack ""
if ! verify; then
    echo "appimage-pack: ERROR packaged AppImage failed verification" >&2
    exit 1
fi
echo "appimage-pack: packaged and verified $out (appimagetool runtime)"
