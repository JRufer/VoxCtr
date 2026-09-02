#! /usr/bin/env bash
# AppRun hook: host-first fallback libraries.
#
# Libraries under usr/lib/fallback are ones the bundled WebKitGTK needs but
# that must NOT shadow a host copy when the host has one: the host's own
# libraries link against them (Arch's libmount links libsystemd, and needs
# LIBSYSTEMD_251, newer than the copy the ubuntu-22.04 build host bundles),
# so a bundled copy on LD_LIBRARY_PATH aborts the host library at startup.
# They can't simply be stripped either: a host may not ship them at all, and
# WebKitGTK would then fail to load — non-systemd distributions have no
# libsystemd/libudev, and libgstgl-1.0.so.0 lives in libgstreamer-gl1.0-0,
# which gstreamer1.0-plugins-base does not pull in, so a desktop with no
# WebKit of its own can be missing it entirely.
#
# So for each fallback library, expose our copy only when the host has no
# library of that soname. The symlinks live in a per-user runtime directory
# because the AppImage mount is read-only.
#
# This file is installed into the AppDir by the AppImage slimming step in
# build_appimage.sh and .github/workflows/release.yml, and sourced by AppRun
# before it hands over to the linuxdeploy AppRun, which prepends the AppDir
# library directories to whatever LD_LIBRARY_PATH we export here.

fallback_src="${APPDIR:-$(dirname "$(realpath "$0")")}/usr/lib/fallback"
if [ -d "$fallback_src" ]; then
    fallback_dir="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/voxctrl-fallback-libs-$(id -u)"
    rm -rf "$fallback_dir" 2>/dev/null || true
    if mkdir -p "$fallback_dir" 2>/dev/null; then
        host_libs="$(ldconfig -p 2>/dev/null || /sbin/ldconfig -p 2>/dev/null || true)"
        for lib in "$fallback_src"/*.so*; do
            [ -e "$lib" ] || continue
            name="$(basename "$lib")"
            host_has=false
            if printf '%s\n' "$host_libs" | grep -qF "$name"; then
                host_has=true
            else
                for dir in /usr/lib /usr/lib64 /lib /lib64 \
                           /usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu; do
                    if [ -e "$dir/$name" ]; then host_has=true; break; fi
                done
            fi
            if [ "$host_has" = false ]; then
                ln -sf "$lib" "$fallback_dir/$name"
            fi
        done
        export LD_LIBRARY_PATH="$fallback_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
fi
