#!/usr/bin/env bash
# Fetch the uruntime AppImage runtime.
#
# appimagetool's stock type-2 runtime statically links libfuse 2 and looks for a
# `fusermount` binary on $PATH. Ubuntu 22.04 / Linux Mint 21 and newer ship
# fuse3 (`fusermount3`) and no longer install libfuse2, so an AppImage built
# with that runtime aborts on a stock desktop with
#
#   Error: No suitable fusermount binary found on the $PATH
#
# uruntime (https://github.com/VHSgunzo/uruntime) is a drop-in replacement that
# speaks FUSE3 and, when no fusermount is usable at all, extracts and runs
# itself instead of aborting.
#
# Usage: fetch-uruntime.sh <destination-path>
#
# Exits non-zero (without leaving a usable file behind) if the runtime could not
# be fetched or does not look like a working runtime, so callers can fall back
# to appimagetool's built-in runtime rather than failing the build.
set -euo pipefail

dest="${1:?usage: fetch-uruntime.sh <destination-path>}"
url="https://github.com/VHSgunzo/uruntime/releases/latest/download/uruntime-appimage-squashfs-x86_64"

mkdir -p "$(dirname "$dest")"

if ! curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 30 -o "$dest" "$url"; then
    echo "fetch-uruntime: download failed ($url)" >&2
    rm -f "$dest"
    exit 1
fi
chmod +x "$dest"

# It must be a self-contained x86-64 ELF: anything else (an HTML error page, a
# redirect stub, a wrong-architecture asset) would produce an AppImage that
# cannot start.
if ! file -b "$dest" | grep -q 'ELF 64-bit.*x86-64'; then
    echo "fetch-uruntime: downloaded file is not an x86-64 ELF binary" >&2
    rm -f "$dest"
    exit 1
fi

# And it must answer as an AppImage runtime. Either flag is enough; the point is
# to catch a file that downloaded cleanly but does not behave like a runtime.
if ! "$dest" --appimage-version >/dev/null 2>&1 \
   && ! "$dest" --appimage-help >/dev/null 2>&1; then
    echo "fetch-uruntime: downloaded runtime did not answer --appimage-version/--appimage-help" >&2
    rm -f "$dest"
    exit 1
fi

echo "fetch-uruntime: using $url" >&2
