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
