#!/usr/bin/env bash
# Bumps VoxCtrl's version number in every file that declares one, in a single
# atomic step. There is no single source of truth for the app version:
#
#   - src-tauri/tauri.conf.json  ("version")      — what Tauri's getVersion()
#     returns, what the About tab shows, and what build_appimage.sh /
#     release.yml derive the release tag and filenames from.
#   - package.json                ("version")      — npm/frontend package
#     version; cosmetic today, but easy to let drift (it did once: v0.1.8
#     shipped with package.json still at 0.1.7).
#   - Cargo.toml                  ([workspace.package] version) — the Cargo
#     version inherited by every crate via `version.workspace = true`.
#
# Nothing enforces these match each other, and nothing enforces the release
# tag matches them either (release.yml lets a manual workflow_dispatch
# override the tag with an arbitrary string). Use this script instead of
# hand-editing version fields so all three stay in lockstep; CI additionally
# verifies they agree before every release build (see release.yml).
#
# Usage:
#   ./scripts/bump_version.sh 0.2.8

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [ $# -ne 1 ]; then
    echo "Usage: $0 <new-version>   (e.g. $0 0.2.8)" >&2
    exit 1
fi

NEW_VERSION="$1"
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
    echo "Error: '$NEW_VERSION' doesn't look like a version number (expected e.g. 1.2.3)" >&2
    exit 1
fi

if ! command -v jq &>/dev/null; then
    echo "Error: this script requires 'jq'. Install it (e.g. sudo pacman -S jq / sudo apt install jq)." >&2
    exit 1
fi

update_json() {
    local file="$1"
    local tmp
    tmp=$(mktemp)
    # jq's default output already ends with exactly one trailing newline
    # (regardless of whether the input file had one) — don't add another.
    jq --indent 2 --arg v "$NEW_VERSION" '.version = $v' "$file" > "$tmp"
    mv "$tmp" "$file"
    echo "  Updated $file"
}

echo "Bumping VoxCtrl version to $NEW_VERSION..."

update_json "package.json"
update_json "src-tauri/tauri.conf.json"

# Cargo.toml is TOML, not JSON: patch the version line under [workspace.package].
if ! grep -q '^\[workspace\.package\]' Cargo.toml; then
    echo "Error: could not find [workspace.package] in Cargo.toml" >&2
    exit 1
fi
sed -i "/^\[workspace\.package\]/,/^\[/ s/^version = \".*\"/version = \"${NEW_VERSION}\"/" Cargo.toml
echo "  Updated Cargo.toml"

echo ""
echo "Done. Verify with:"
echo "  jq -r .version package.json src-tauri/tauri.conf.json"
echo "  grep -A1 '\\[workspace.package\\]' Cargo.toml"
echo ""
echo "Cargo.lock package entries (voxctrl-*) will pick up the new version on"
echo "the next 'cargo build' / 'cargo check'; run one before committing."
