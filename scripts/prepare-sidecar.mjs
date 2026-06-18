#!/usr/bin/env node
// Stage the freshly-built `voxctrl-overlay` binary as a Tauri sidecar so it
// gets bundled *inside* the AppImage / deb / installer next to the main app.
//
// Tauri's `externalBin` mechanism expects each sidecar to be named with the
// host target triple suffix (e.g. `voxctrl-overlay-x86_64-unknown-linux-gnu`).
// At bundle time Tauri strips the triple and drops `voxctrl-overlay` alongside
// the main `voxctrl` binary, which is exactly where `get_overlay_path()` looks
// first. Without this the overlay binary is missing from packaged builds and
// the app silently falls back to a stale dev binary (or none at all).
//
// Chicken-and-egg note: `voxctrl-overlay` lives in the same crate that declares
// `externalBin`, and `tauri-build`'s build script validates that the sidecar
// file exists on *every* compile of that crate — including the `cargo build
// --bin voxctrl-overlay` we run to produce the binary in the first place. So
// this script runs in two phases:
//   --ensure : guarantee the triple-named file exists before the overlay is
//              built. Copies the real binary if it is already present (e.g.
//              warm cache), otherwise writes a tiny placeholder just so the
//              build-script validation passes.
//   (default): require the freshly-built release binary and copy it over,
//              replacing any placeholder, before Tauri bundles.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, copyFileSync, writeFileSync, chmodSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ensureOnly = process.argv.includes('--ensure');

// Resolve the host target triple from rustc (e.g. "x86_64-unknown-linux-gnu").
function hostTriple() {
  const out = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const match = out.match(/^host:\s*(\S+)$/m);
  if (!match) {
    throw new Error('Could not determine host triple from `rustc -vV`');
  }
  return match[1];
}

const triple = hostTriple();
const isWindows = process.platform === 'win32';
const exeSuffix = isWindows ? '.exe' : '';

const destDir = join(repoRoot, 'src-tauri', 'binaries');
const destBinary = join(destDir, `voxctrl-overlay-${triple}${exeSuffix}`);

// The overlay binary may live under either profile depending on how it was
// built. Prefer release (what packaged builds use) but fall back to debug for
// the dev flow.
const candidates = [
  join(repoRoot, 'target', 'release', `voxctrl-overlay${exeSuffix}`),
  join(repoRoot, 'target', 'debug', `voxctrl-overlay${exeSuffix}`),
];
const srcBinary = candidates.find((p) => existsSync(p));

mkdirSync(destDir, { recursive: true });

if (srcBinary) {
  copyFileSync(srcBinary, destBinary);
  if (!isWindows) {
    chmodSync(destBinary, 0o755);
  }
  console.log(`[prepare-sidecar] staged ${srcBinary} -> ${destBinary}`);
} else if (ensureOnly) {
  // Build-script validation only checks that the path exists; the real binary
  // is staged by the second (default) invocation once it has been compiled.
  if (!existsSync(destBinary)) {
    writeFileSync(destBinary, '');
    if (!isWindows) {
      chmodSync(destBinary, 0o755);
    }
    console.log(`[prepare-sidecar] wrote placeholder ${destBinary} (overlay not built yet)`);
  }
} else {
  throw new Error(
    `Overlay binary not found in ${candidates.join(' or ')}.\n` +
      'Run `cargo build --bin voxctrl-overlay --release` before staging the sidecar.'
  );
}
