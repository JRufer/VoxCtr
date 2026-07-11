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
// Run from beforeBuildCommand/beforeDevCommand *after* the overlay binary has
// been compiled. The crate's build.rs writes a placeholder so plain cargo
// builds still compile; this script replaces that placeholder with the real
// binary before Tauri bundles.

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

import { statSync } from 'node:fs';

export function selectBinary(candidates, existsSyncFn, statSyncFn) {
  const existingCandidates = candidates.filter((p) => existsSyncFn(p));
  if (existingCandidates.length === 0) {
    throw new Error(
      `Overlay binary not found in ${candidates.join(' or ')}.\n` +
        'Run `cargo build --bin voxctrl-overlay` before staging the sidecar.'
    );
  }
  let srcBinary = existingCandidates[0];
  if (existingCandidates.length > 1) {
    const stats = existingCandidates.map((p) => ({ path: p, mtime: statSyncFn(p).mtimeMs }));
    stats.sort((a, b) => b.mtime - a.mtime);
    srcBinary = stats[0].path;
  }
  return srcBinary;
}

const isDirectRun = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (isDirectRun) {
  const triple = hostTriple();
  const isWindows = process.platform === 'win32';
  const exeSuffix = isWindows ? '.exe' : '';

  const candidates = [
    join(repoRoot, 'target', 'release', `voxctrl-overlay${exeSuffix}`),
    join(repoRoot, 'target', 'debug', `voxctrl-overlay${exeSuffix}`),
  ];

  const srcBinary = selectBinary(candidates, existsSync, statSync);

  const destDir = join(repoRoot, 'src-tauri', 'binaries');
  mkdirSync(destDir, { recursive: true });
  const destBinary = join(destDir, `voxctrl-overlay-${triple}${exeSuffix}`);

  copyFileSync(srcBinary, destBinary);
  if (!isWindows) {
    chmodSync(destBinary, 0o755);
  }

  console.log(`[prepare-sidecar] staged ${srcBinary} -> ${destBinary}`);
}
