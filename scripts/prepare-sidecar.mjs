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

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

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

const srcBinary = join(repoRoot, 'target', 'release', `voxctrl-overlay${exeSuffix}`);
if (!existsSync(srcBinary)) {
  throw new Error(
    `Overlay binary not found at ${srcBinary}.\n` +
      'Run `cargo build --bin voxctrl-overlay --release` before staging the sidecar.'
  );
}

const destDir = join(repoRoot, 'src-tauri', 'binaries');
mkdirSync(destDir, { recursive: true });
const destBinary = join(destDir, `voxctrl-overlay-${triple}${exeSuffix}`);

copyFileSync(srcBinary, destBinary);
if (!isWindows) {
  chmodSync(destBinary, 0o755);
}

console.log(`[prepare-sidecar] staged ${srcBinary} -> ${destBinary}`);
