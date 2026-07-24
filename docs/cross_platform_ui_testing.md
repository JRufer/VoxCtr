# Cross-Platform UI Testing Strategy

VoxCtrl's UI (Settings/History windows, tray icon, and the always-on-top
overlay HUD) was built and tuned against KDE Plasma (Wayland and X11). This
doc lays out how to validate the UI on environments the team doesn't use
day-to-day — **Hyprland** and **Windows** in particular — plus how to triage
and fix what breaks.

## Why KDE-tuned code is a real risk here

Grepping the codebase turns up several places where behavior was written
around specific KDE Plasma (KWin) quirks rather than the Wayland/X11/Win32
protocols in general. None of these are bugs on KDE — they're the reason the
overlay works there — but they're exactly the kind of code that silently
regresses on a different compositor:

1. **Overlay always-on-top + positioning (`src-tauri/src/overlay.rs`).**
   The overlay is a plain winit toplevel window (`WindowLevel::AlwaysOnTop`),
   not a `wlr-layer-shell` surface. The code comments (`overlay.rs:1144-1166`,
   `1642-1650`, `1672-1680`) explicitly reason about **KWin and Mutter**
   behavior: re-asserting the "above" level on a heartbeat because X11
   stacking isn't sticky, and keeping the surface mapped forever because KWin
   grabs keyboard focus on every remap. Hyprland and Sway (wlroots-based) are
   never mentioned and were presumably never tested.
   - **Biggest concrete risk:** Hyprland tiles windows by default. A regular
     toplevel with no float/no-focus hint can get tiled into the workspace
     grid instead of floating untouched in a corner, which would break the
     entire HUD concept (position, click-through, always-on-top).
   - The overlay window has no explicit Wayland `app_id` set, so users can't
     even write a targeted Hyprland/Sway `windowrule`/`window rule` to force
     it to float — it inherits whatever default winit assigns.
2. **Tray icon.** Built on `tauri`'s `tray-icon` feature, which uses the
   StatusNotifierItem (SNI) protocol on Linux (via
   `libayatana-appindicator3`). SNI is native on KDE Plasma and works on
   Hyprland *if* the bar in use (Waybar, etc.) has an SNI host module enabled
   — it is not guaranteed out of the box on every Hyprland config.
3. **Desktop notifications** (`voxctrl-inject/src/lib.rs`, `notify-rust`).
   Requires a running notification daemon (`org.freedesktop.Notifications`).
   KDE and GNOME ship one; a minimal Hyprland setup may not (needs `mako` or
   `dunst`) — the call fails silently (fire-and-forget thread), so a missing
   daemon looks like "notifications are broken" rather than "not installed."
4. **Text injection** (`inject_linux`). `wtype` requires the compositor to
   support the `virtual-keyboard`/`input-method` Wayland protocols. Hyprland
   and Sway support this; behavior should be consistent, but it's untested
   outside KDE Wayland. The `xdotool`/clipboard-paste fallback is X11-only
   and should behave identically under XWayland on any compositor.
5. **Global hotkeys** (`voxctrl-hotkeys::linux`) use raw `evdev`, bypassing
   the desktop environment/compositor entirely. This is the one part of the
   UI-adjacent stack that should be **compositor-agnostic by construction**
   — worth confirming, but it's the lowest-risk item on Linux.
6. **Windows.** `windows_build.md` already flags the text-injection path
   (clipboard + PowerShell `SendKeys`) as a stopgap "planned to improve
   reliability"; treat it as a known weak point, not a KDE-parity question.
   WebView2 rendering (Settings/History windows) and the Slint overlay's
   winit backend on Win32 are the two things that have had materially less
   real-world use than the Linux/KDE path.

## Test environment matrix

| Environment | How to get it | What it exercises |
|---|---|---|
| **KDE Plasma (Wayland)** | Existing baseline | Control group — must not regress |
| **KDE Plasma (X11)** | Existing baseline | `xdotool` path, X11 stacking |
| **Hyprland** | VM or spare machine (Arch/CachyOS + `hyprland` package), or a live ISO (e.g. JaKooLit's Hyprland-Dots image, or plain Arch + `pacman -S hyprland waybar`) | wlroots layer-shell/tiling behavior, `wtype`, SNI tray via Waybar |
| **Sway** | `pacman -S sway` / `apt install sway` in a VM | Second wlroots reference point — if a bug reproduces on both Hyprland and Sway, it's a wlroots-generic issue, not Hyprland-specific; if it reproduces on Hyprland only, it's likely a tiling-policy/windowrule issue |
| **GNOME (Wayland)** | Ubuntu/Fedora Workstation VM | Bonus non-KDE reference; catches SNI-tray-not-supported-by-default and Mutter-specific stacking, since GNOME and Hyprland/Sway share "not KWin" but differ from each other too |
| **Windows 10/11** | VM (VMware/VirtualBox/Hyper-V) or physical machine | WebView2 rendering, Win32 always-on-top, `SendKeys` injection, WASAPI audio |

Use VMs for everything except the KDE baseline — Hyprland/Sway need real (or
virtual) GPU-accelerated Wayland sessions; nested-compositor tricks
(`WAYLAND_DISPLAY` inside `WAYLAND_DISPLAY`) are unreliable for always-on-top
and input-focus testing specifically, which is the area most likely to
regress, so don't rely on them for anything beyond a quick smoke check.

## Manual test checklist (run per environment above)

Treat this as the release checklist — copy it into the PR/issue when
validating a platform and check off each row.

**Windows & chrome**
- [ ] Settings window opens, resizes, and closes normally
- [ ] Tray icon → "⚙ Settings" and double-click both raise/focus the
      Settings window if it's open but hidden behind others
- [ ] History window opens and displays entries

**Overlay HUD**
- [ ] Overlay appears in the configured position (Top/Center/Bottom) on
      first dictation
- [ ] Overlay stays **floating**, not tiled into the workspace (Hyprland/Sway
      specific — this is the top risk item)
- [ ] Overlay stays **on top** of a maximized/fullscreen window during a
      second and third dictation (not just the first)
- [ ] Overlay is click-through (mouse events pass to the window underneath)
- [ ] Overlay never steals keyboard focus from the app being dictated into
- [ ] Switching overlay style in Visual tab hot-swaps correctly
- [ ] Overlay follows a monitor-preference change and fails over gracefully
      if that monitor is unplugged
- [ ] Overlay behaves correctly with just one connected monitor and with 2+

**Tray**
- [ ] Tray icon is visible at all (flag if the bar/shell doesn't host SNI)
- [ ] Icon state changes on record/processing/speaking

**Global hotkeys**
- [ ] Hold-to-talk, toggle, double-tap, double-tap-and-hold, and chord
      gestures all fire (Linux: confirms evdev path is DE-agnostic;
      Windows: exercises `voxctrl-hotkeys::windows`)
- [ ] Hotkeys still work when a non-privileged app has focus vs. when a
      privileged/elevated app (Windows) or a Wayland-secured app has focus

**Text injection**
- [ ] Dictate into: a terminal, a browser text field, and an Electron app
      (each is a distinct focus/input model)
- [ ] Confirm which path was used (wtype / xdotool / clipboard-paste /
      Windows SendKeys) via logs, and that the clipboard-paste fallback
      still restores/doesn't clobber prior clipboard content unexpectedly

**Notifications**
- [ ] Transcription-complete notification appears; if it silently doesn't,
      check whether a notification daemon is even running before filing a
      bug

**Config/installer**
- [ ] AppImage `--install` flow completes and detects the right package
      manager (Linux only)
- [ ] Windows NSIS/MSI installer completes and WebView2 prompts correctly
      when missing

## Automated coverage: what exists vs. what's realistic to add

- **Already covered:** Svelte components (`tests/svelte/*.test.ts`, vitest)
  and pure Rust logic (`voxctrl-config`, `voxctrl-routing`, `voxctrl-hotkeys`
  gesture parsing, `voxctrl-app`) run in CI on Ubuntu and Windows
  (`.github/workflows/ci.yml`). None of this touches a real compositor/WM.
- **Not realistic to fully automate:** always-on-top behavior, click-through,
  focus-stealing, and tiling-vs-floating are properties of the interaction
  between the windowing toolkit and a specific compositor/WM. CI runners
  don't have Hyprland/Sway/GNOME sessions, and headless/nested Wayland
  compositors don't reproduce real stacking/tiling policy reliably — a green
  CI run here would be false confidence, not a substitute for the manual
  checklist above.
- **Worth adding regardless:**
  - A tiny CLI/debug flag or log line in `overlay.rs` that prints which
    window level / position it just applied and why (heartbeat re-raise,
    idle, monitor failover) — turns "overlay looks wrong" bug reports into
    "log shows it thinks it's at (x, y) with level Y," which is far faster
    to triage across an environment you don't have hands-on.
  - A short manual QA template (the checklist above, saved as an issue
    template) filled out once per platform per release, not per PR.

## Triage & fix workflow for issues found

1. **Reproduce and classify** using the matrix above: KDE-only-fine vs.
   fails-on-Hyprland-only vs. fails-on-both-wlroots vs. fails-on-Windows.
   Hyprland-only strongly suggests a tiling/windowrule problem; both-wlroots
   suggests a layer-shell/protocol gap; Windows is its own code path
   entirely (`#[cfg(target_os = "windows")]` in `voxctrl-inject`,
   `voxctrl-hotkeys`).
2. **Prefer protocol-correct fixes over WM-specific hacks.** E.g. for the
   Hyprland-tiling risk, the durable fix is making the overlay a real
   `wlr-layer-shell` surface (correct on every wlroots compositor and
   ignored gracefully elsewhere) rather than special-casing Hyprland. Where
   that's too large a change short-term, a documented interim fix is fine:
   set an explicit Wayland `app_id` on the overlay window and publish a
   recommended `windowrule`/`windowrulev2 = float, class:^(voxctrl-overlay)$`
   snippet in `docs/installation.md`, the same way the AppImage installer
   already documents distro-specific setup steps.
3. **Gate genuinely environment-specific behavior at runtime**, not compile
   time, using `XDG_CURRENT_DESKTOP` / `XDG_SESSION_TYPE` / `WAYLAND_DISPLAY`
   (the injection code already does this for Wayland vs. X11) — never add a
   new Linux `#[cfg]` split for something that's actually a desktop-specific
   runtime difference.
4. **Regression-test on the full matrix before merging a fix**, not just the
   platform that reported the bug — the KWin-specific comments in
   `overlay.rs` exist because a naive fix for one environment previously
   broke another; assume the same risk applies to any change here.
5. **Update this doc's checklist** if a new class of bug is found, so the
   next release's manual pass catches it too.

---

## macOS Port & Build

Porting to macOS is **feasible but not free** — it is a real port, not just a
new build target. Much of the stack is already macOS-friendly, but three
pieces of core functionality have no macOS code path today and will fail (or
fail to compile) until they're written. Read this before promising a Mac
build to anyone.

### What already works on macOS (no changes needed)

These dependencies and code paths are macOS-native or already have a macOS
branch:

- **Whisper inference** (`whisper-rs`) — builds with Apple **Metal**
  acceleration by default on macOS (or CPU).
- **Audio capture** (`cpal`) — uses CoreAudio.
- **Clipboard** (`arboard`) — native macOS support.
- **WebView UI** (Settings/History) — Tauri uses `WKWebView` on macOS.
- **Tray icon** — Tauri's `tray-icon` renders in the macOS menu bar.
- **Notifications** — `voxctrl-inject/src/lib.rs:114` already gates
  `notify-rust` with `#[cfg(any(target_os = "linux", target_os = "macos"))]`.
- **Overlay window** — the winit backend has a
  `#[cfg(not(any(windows, linux)))]` fallback (`overlay.rs:1465`), so it
  compiles on macOS; the always-on-top HUD will need the same real-device
  testing as Hyprland (macOS `WindowLevel` and Spaces/full-screen behavior
  differ from both X11 and Wayland).
- **App icon** — `src-tauri/icons/icon.icns` already exists.

### What must be written before a Mac build is usable

1. **Text injection** (`voxctrl-inject/src/lib.rs`). The current code
   `bail!`s on any non-Linux/Windows OS (`lib.rs:13-14`). macOS needs its own
   `#[cfg(target_os = "macos")]` path — the pragmatic approach mirrors
   Windows: write to the clipboard via `arboard`, then synthesize **Cmd+V**
   with a `CGEvent` (or via the `enigo` crate). Requires the **Accessibility**
   TCC permission (see below).
2. **Global hotkeys** (`voxctrl-hotkeys`). Only `linux` (evdev) and `windows`
   (rdev) branches exist (`lib.rs:33-41`); macOS currently falls through to a
   "not supported" warning. `rdev` (already a dependency for Windows) **also
   supports macOS** via `CGEventTap`, so the cleanest first cut is to widen
   the Windows branch to `#[cfg(any(target_os = "windows", target_os = "macos"))]`
   and reuse `windows.rs`'s rdev logic. Requires the **Input Monitoring** and
   **Accessibility** TCC permissions.
3. **MCP server socket** (`voxctrl-mcp/src/lib.rs:84-149`). The Unix-socket
   path is gated `#[cfg(target_os = "linux")]` and the Windows path uses named
   pipes — macOS matches **neither**, so the crate won't compile on macOS as
   written. macOS is Unix, so the fix is small: change the Unix-socket gate
   from `target_os = "linux"` to `unix` (or `any(linux, macos)`) so macOS
   reuses the Unix-domain-socket implementation.
4. **DBus target** (`voxctrl-dbus`, and the `dbus` delivery type in routing).
   Already stubbed out on non-Linux (`lib.rs:141`), so it compiles — but the
   `dbus` output target simply won't function on macOS. That's acceptable
   (document it as Linux-only); don't try to emulate it.

### macOS permissions (TCC)

macOS gates the exact capabilities VoxCtrl relies on behind per-app user
consent (TCC). Distribution builds need usage-description strings in the
app's `Info.plist` (Tauri injects these via `bundle.macOS` config) and users
must grant, under **System Settings → Privacy & Security**:

- **Microphone** — recording (`NSMicrophoneUsageDescription`).
- **Accessibility** — synthesizing Cmd+V keystrokes for injection.
- **Input Monitoring** — the global hotkey event tap.

Unsigned/un-notarized builds make this worse: macOS **Gatekeeper** will
refuse to open the app normally, and TCC permission grants are keyed to the
app's code signature, so an unsigned app can have its granted permissions
silently invalidated on rebuild. For anything beyond local dev, plan on an
Apple **Developer ID** certificate ($99/yr) plus **notarization**.

### Building on GitHub Actions

GitHub-hosted macOS runners make a Mac build possible **without owning a
Mac** for the compile step (you'll still want real hardware for the UI
testing in the matrix above). Runner images:

- `macos-14` → **Apple Silicon (arm64)** — the primary target for modern
  Macs.
- `macos-13` → **Intel (x86_64)** — for older Macs, if you want to ship both.

Unlike the Linux job, macOS needs **no `apt` system dependencies** — the
SDK, Metal, and CoreAudio ship with the runner. A minimal job to add to the
`build` matrix in `.github/workflows/release.yml`:

```yaml
# ── macOS (Apple Silicon) ────────────────────────────────────────────
- name: macos-arm64
  os: macos-14
  features: ""
  artifact_label: macos-arm64
  target: aarch64-apple-darwin

# ── macOS (Intel), optional ──────────────────────────────────────────
- name: macos-x86_64
  os: macos-13
  features: ""
  artifact_label: macos-x86_64
  target: x86_64-apple-darwin
```

with matching steps (guarded by `runner.os == 'macOS'`):

```yaml
- name: Add Rust target (macOS)
  if: runner.os == 'macOS'
  run: rustup target add ${{ matrix.target }}

- name: Build .app + .dmg (macOS)
  if: runner.os == 'macOS'
  run: npx tauri build --target ${{ matrix.target }} -- --features moonshine

- name: Collect macOS artifacts
  if: runner.os == 'macOS'
  run: |
    mkdir -p upload
    find . -path '*/bundle/dmg/*.dmg' -exec cp {} \
      "upload/VoxCtrl-${{ matrix.artifact_label }}.dmg" \;
    ls -lh upload/
```

Notes for whoever wires this up:

- **Order of operations matters.** Do the code work (injection, hotkeys, the
  MCP `#[cfg(unix)]` fix) *first* and get it compiling locally or via a
  throwaway `cargo check` job on a macOS runner. Adding the release-matrix
  rows before the crate compiles on macOS just turns the release build red.
- **Prove it compiles before you bundle it.** Mirror the existing CI pattern:
  add `macos-14` to the `cargo-check` matrix in `.github/workflows/ci.yml`
  (it currently only checks `ubuntu-22.04` and `windows-latest`) so macOS
  build breakage is caught on every PR, not only at release time.
- **Signing/notarization in CI** needs the Developer ID cert (base64) and an
  App Store Connect API key stored as repo secrets, consumed via Tauri's
  `APPLE_CERTIFICATE`, `APPLE_SIGNING_IDENTITY`, and notarization env vars.
  Ship **unsigned** first to validate the port, then layer signing on once
  the build itself is green. Don't put certificates or keys in the workflow
  file — repo secrets only.
- **Version-sync gate.** The release workflow already fails fast if
  `package.json` / `tauri.conf.json` / `Cargo.toml` versions disagree; adding
  macOS rows doesn't change that, but remember to update the "Downloads"
  table in the `publish` job so the `.dmg` files are listed for users.

### Where macOS fits the testing matrix

Treat macOS as a **second first-class desktop target alongside Windows**, and
run the same manual QA checklist above against it — with extra attention to:
the overlay's always-on-top behavior across Spaces and full-screen apps; the
TCC permission prompts appearing (and the app degrading gracefully if the
user denies them); and the injection path working in Terminal, a browser, and
a native Cocoa app. A `macos-14` runner is enough to catch **compile and
bundle** regressions in CI; **behavioral** UI testing still needs a real Mac,
exactly as Hyprland needs a real (or virtualized) wlroots session.
