# Building VoxCtrl on macOS

> **Status: in progress.** macOS is a new target. The core paths (UI, audio,
> whisper inference, clipboard, tray, notifications, MCP server, text injection,
> global hotkeys) have macOS code, but the port has not yet had the same
> real-hardware soak testing as Linux/KDE. Treat macOS builds as preview
> quality and file issues for anything that misbehaves. See
> [Cross-Platform UI Testing](./cross_platform_ui_testing.md) for the testing
> strategy this port follows.

## Supported hardware

- **Apple Silicon (arm64)** — the primary target. CI builds it on `macos-14`.
- **Intel (x86_64)** — buildable on `macos-13`, not shipped by default. Add a
  matching matrix row if you need it.

Whisper inference uses **Metal** acceleration automatically on macOS (via
`whisper-rs`/whisper.cpp); no CUDA/Vulkan flags apply.

---

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust (via rustup) | 1.75+ | `stable-aarch64-apple-darwin` on Apple Silicon |
| Node.js | 18+ | Frontend build (Vite/Svelte) |
| Xcode Command Line Tools | current | `xcode-select --install` — provides `clang`, Metal, and the SDK |
| Tauri CLI | 2.x | `cargo install tauri-cli`, or use the bundled `npx tauri` |

No Homebrew system libraries are required for a standard build — unlike Linux,
there is no WebKitGTK/PortAudio/appindicator to install. macOS provides
`WKWebView`, CoreAudio, and the menu-bar tray natively.

---

## Development build

```bash
npm install
npm run tauri dev
```

## Production build

```bash
npm install
npx tauri build --bundles app,dmg
```

Artifacts land in `src-tauri/target/release/bundle/`:
- `macos/VoxCtrl.app` — the application bundle
- `dmg/VoxCtrl_<version>_aarch64.dmg` — the disk image

To also compile the Moonshine ONNX backend (fetches ONNX Runtime at build
time, so it needs network access during the build):

```bash
npx tauri build --bundles app,dmg -- --features moonshine
```

---

## Permissions (TCC)

macOS gates the capabilities VoxCtrl relies on behind per-app user consent.
The bundle ships usage-description strings (`src-tauri/Info.plist`) so the
prompts appear; the user must grant, under **System Settings → Privacy &
Security**:

| Permission | Why VoxCtrl needs it |
|---|---|
| **Microphone** | Recording audio for transcription |
| **Accessibility** | Synthesizing the paste keystroke that inserts dictated text |
| **Input Monitoring** | The global hotkey listener (`rdev` event tap) |

If dictation types nothing, or hotkeys never fire, an ungranted permission is
the first thing to check.

---

## Platform notes / current limitations

- **Text injection** copies the text to the pasteboard and synthesizes
  **Cmd+V** via AppleScript (`osascript` → System Events). This mirrors the
  clipboard-paste fallback used elsewhere and requires the Accessibility
  permission. A native `CGEvent`/`SendInput`-equivalent path may replace it
  later for speed/reliability.
- **Global hotkeys** use the `rdev` event tap (the same backend as Windows),
  which requires Input Monitoring + Accessibility. The raw-evdev Linux path
  does not apply.
- **MCP server** listens on the same Unix domain socket as Linux
  (`/tmp/voxctrl-mcp.sock`).
- **DBus** output targets are **Linux-only** and are inert on macOS; use
  `exec`, `http`, `webhook`, `socket`, or `pipe` instead for automation.

---

## Signing & notarization (for distribution)

Unsigned builds are fine for local testing but Gatekeeper blocks them on other
machines, and TCC permission grants are tied to the code signature (so an
unsigned app can lose granted permissions on rebuild). For distribution:

1. Obtain an Apple **Developer ID Application** certificate (Apple Developer
   Program, $99/yr).
2. Add the certificate and an App Store Connect API key to the repo secrets,
   and wire Tauri's signing/notarization env vars
   (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
   `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`) in the release
   job.
3. Add an `entitlements.plist` (with `com.apple.security.device.audio-input`)
   and reference it from `bundle.macOS.entitlements` in `tauri.conf.json`.

See the [Tauri macOS signing docs](https://tauri.app/distribute/sign/macos/).

---

## Building on GitHub Actions

Two workflows cover macOS:

- **`.github/workflows/macos-build.yml` — "macOS Build (preview)".** A
  standalone build that runs on pushes to the `claude/macos-port-build`
  branch (or any `macos-**` branch), on PRs touching macOS-relevant files, and
  via manual dispatch. It uploads the `.app` (zipped) and `.dmg` as run
  artifacts. **This is how the port is validated without merging to master.**
- **`.github/workflows/release.yml`.** The macOS row (`macos-14`) is part of
  the release matrix and produces `VoxCtrl-macos-arm64.dmg` alongside the Linux
  and Windows artifacts. It only runs on pushes to `master`.

The `cargo check` CI job (`.github/workflows/ci.yml`) also runs on `macos-14`,
so macOS compile breakage is caught on every PR.

### Troubleshooting

- **`No .dmg found under bundle/dmg`** — the bundle step failed earlier; scroll
  up in the build step log. A common cause is a Rust compile error in a
  macOS-gated code path.
- **App won't open ("damaged / unidentified developer")** — expected for
  unsigned builds. Right-click → Open, or
  `xattr -dr com.apple.quarantine /Applications/VoxCtrl.app`.
- **Overlay sidecar missing at runtime** — the `beforeBuildCommand` stages
  `voxctrl-overlay` for the host triple; ensure the build ran it (a native
  build where the runner arch matches the target handles this automatically).
