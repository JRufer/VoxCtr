# Porting VoxCtrl to Windows 11 — Development Plan

**Status:** proposal · **Target:** Windows 11 (x64 + ARM64), Windows 10 22H2 as a
best-effort floor · **Baseline:** VoxCtrl 0.4.0

---

## 1. Executive summary

VoxCtrl is *already* written in a portable stack — Rust + Tauri 2 + Svelte +
Slint — and it already contains Windows code paths. That makes the port look
closer to done than it is. The honest verdict from auditing the tree:

> **Windows is currently a compile target, not a supported platform.**
> The Windows-specific code that exists has never been built end to end, never
> been run, and three of its core paths are provably wrong by inspection.
> Global hotkeys cannot fire at all on Windows today, dictated text is
> corrupted on the way out, and the Hotkeys tab shows a padlock claiming
> VoxCtrl does not read the keyboard on a backend that reads all of it.

Two pieces of CI configuration say this out loud:

- `.github/workflows/ci.yml:62` — the Windows `cargo check` job runs
  `--workspace --exclude voxctrl-inference --exclude voxctrl-app`. The two
  crates excluded are the speech engine and the entire application. Everything
  Windows-specific in `src-tauri/` has therefore never been type-checked.
- `.github/workflows/release.yml:99-113` — the `windows-cpu` row of the release
  matrix is commented out, with the note *"nobody can test what it produces at
  the moment"*.

So the plan below is not "add Windows support to a cross-platform app". It is:

1. **Make it build.** Turn on the excluded crates in CI and fix what falls out.
2. **Fix the broken seams** — hotkeys and text injection, which make the app
   non-functional rather than merely degraded, and a privacy indicator that
   currently tells Windows users the opposite of the truth.
3. **Close the parity gaps** — TTS install, notifications, autostart, overlay,
   IPC, updater.
4. **Refactor the seams** from scattered `#[cfg]` blocks into a small platform
   trait layer, so parity does not rot again.
5. **Ship it** — signed installer, release matrix, docs, and a Windows CI lane
   that would have caught all of the above.

Estimated effort: **~8–11 engineer-weeks**, sequenced into five milestones.
Milestones 0–1 (~2 weeks) get to "it launches and dictates correctly on
Windows 11"; the remaining three are parity, shipping, and hardening.

---

## 2. Why this is a seams problem, not a rewrite

The workspace is 13 crates and ~30k lines of Rust. Only a small fraction is
platform-bound. Counting `target_os` occurrences across the tree gives 104,
with over 70% of them in six files. The portable majority — config, routing model,
gesture engine, text post-processing, LLM client, update version logic, the
whole Svelte UI — needs nothing.

The platform-bound surface is exactly seven seams:

| # | Seam | Linux mechanism | Windows mechanism |
|---|---|---|---|
| 1 | Global hotkeys | XDG portal / XInput2 / evdev | Low-level keyboard hook |
| 2 | Text injection | `wtype` / `xdotool` / clipboard | `SendInput` |
| 3 | Overlay window | X11 input-shape, `_NET_WM_STATE_ABOVE` | `WS_EX_TRANSPARENT`/`LAYERED`/`NOACTIVATE` |
| 4 | Local IPC | Unix socket, D-Bus | Named pipe, (no D-Bus) |
| 5 | Notifications | `notify-rust` → libnotify | WinRT toast w/ registered AUMID |
| 6 | Desktop integration | `.desktop` file, XDG dirs | Start Menu shortcut, registry, known folders |
| 7 | Packaging / update | AppImage + `.deb`, in-place replace | NSIS `setup.exe`, re-run installer |

Everything else is already portable or trivially so. **The plan's central
architectural recommendation is to make these seven seams explicit** — one
module per seam with a platform-neutral interface — instead of letting
`#[cfg(target_os)]` spread further through business logic (it is currently in
`targets.rs`, `commands.rs`, `overlay.rs`, `pipeline.rs`, and `stop_key.rs`).

---

## 3. Audit: what actually works today

Legend: **✅** works · **⚠️** degraded/partial · **❌** broken or absent ·
**🐧** Linux-only by nature

### 3.1 Blockers — must be fixed before any Windows build ships

#### ❌ B1. Global hotkeys never fire (`crates/voxctrl-hotkeys/src/windows.rs:87`)

The Windows backend translates `rdev` keys into binding names with:

```rust
fn key_name(key: &rdev::Key) -> String {
    format!("KEY_{key:?}").to_ascii_uppercase()
}
```

Bindings are stored in **evdev vocabulary** — that is the canonical naming
across the whole app. `crates/voxctrl-routing/src/loader.rs:408` seeds the
default binding as `["KEY_LEFTMETA", "KEY_SPACE"]`, and the settings UI
generates the same vocabulary from browser key codes
(`src/lib/Settings/HotkeysTab.svelte:433`, `mapBrowserKeyToEvdev`).

`rdev`'s `Key` enum uses a different vocabulary entirely. The `Debug` names are
`MetaLeft`, `ControlLeft`, `KeyA`, `Num1`, `Escape`. So the function produces:

| Physical key | `key_name()` emits | Bindings contain | Match? |
|---|---|---|---|
| Left Super | `KEY_METALEFT` | `KEY_LEFTMETA` | ❌ |
| Left Ctrl | `KEY_CONTROLLEFT` | `KEY_LEFTCTRL` | ❌ |
| A | `KEY_KEYA` | `KEY_A` | ❌ |
| 1 | `KEY_NUM1` | `KEY_1` | ❌ |
| Escape | `KEY_ESCAPE` | `KEY_ESC` | ❌ |
| Space | `KEY_SPACE` | `KEY_SPACE` | ✅ |

Space and a handful of others collide by luck. **Every modifier and every
letter is wrong**, so the shipped default hotkey (Super+Space) can never
activate, and neither can almost anything a user records. The gesture engine,
`KeyMatcher`, and health reporting are all fine — they are fed garbage.

This is also invisible in testing: `ListenerHealth` is marked healthy
(`windows.rs:26-30`) as soon as the thread starts, so the UI reports the hotkey
backend as working while no key ever matches.

#### ⚠️ B2. Text injection mangles ordinary text (`crates/voxctrl-routing/src/targets.rs:142-172`)

The `inject`/type target on Windows shells out to PowerShell and calls
`System.Windows.Forms.SendKeys::SendWait` on the transcribed text.

The code base64-encodes the payload — carefully, and with a good comment —
so that no PowerShell metacharacter can escape the string. That defends against
*PowerShell* parsing. It does nothing about **SendKeys' own escape syntax**,
which is applied to the decoded string. In SendKeys, `+` `^` `%` `~` `(` `)`
`{` `}` `[` `]` are all metacharacters:

| Dictated | SendKeys types |
|---|---|
| `50% of users` | `50` then Alt-chords the rest |
| `f(x) = a + b` | `f`, `x`, ` = a `, `b` — parens and `+` consumed |
| `array[0]` | `array0` |
| `Ctrl^C` | `Ctrl` + literal Ctrl chord |

For a dictation app whose primary output is arbitrary prose, this is a
correctness bug, not a polish item. Separately, the design spawns a
`powershell.exe` process per dictation (~200–500 ms cold), which is a
significant share of the perceived end-to-end latency.

`crates/voxctrl-inject/src/lib.rs:94` has the same shape for the clipboard-paste
path, with a comment that says it out loud: *"PowerShell fallback while we wire
up windows-rs SendInput"*.

#### ❌ B3. The privacy indicator tells Windows users the opposite of the truth

`crates/voxctrl-hotkeys/src/health.rs` contains a direct self-contradiction
about `Backend::WindowsHook`, 126 lines apart:

```rust
// health.rs:78 — WindowsHook is grouped with the backends that read the keyboard
pub fn sees_raw_keys(self) -> bool {
    matches!(self, Self::X11 | Self::Evdev | Self::WindowsHook)
}

// health.rs:202 — ...and simultaneously with the backends that do not
/// Shortcuts are working without VoxCtrl having any access to input devices.
pub fn is_private(&self) -> bool {
    matches!(self.backend(), Backend::Portal | Backend::WindowsHook | Backend::MintDbus)
}
```

Both cannot be true. `WH_KEYBOARD_LL` is a system-wide hook: it sees **every
keystroke on the machine**, exactly like the evdev and X11 backends — whose own
tests in this same file say so (`health.rs:316` *"reading evdev means seeing
every keystroke"*, `health.rs:386` *"raw X11 key events are every keystroke"*),
and both of which correctly report `is_private() == false`.

`is_private()` drives a **user-facing privacy claim**. `docs/development.md:463`
documents the intent plainly:

> `is_private()` is true only for the portal and the Windows hook, the two paths
> where VoxCtrl receives its own shortcuts and no raw keystrokes. The UI states
> this in plain language, and only when true.

The Windows hook is not such a path, and the claim reaches the user's screen.
`src/lib/Settings/HotkeysTab.svelte:658-670` styles the Hotkeys banner green and
prints a **🔒 padlock** whenever `is_private` is true — which, for
`backend === "windows_hook"`, it always is.

Compare the accompanying detail text set in `commands.rs`. The X11 backend, which
sees exactly what the Windows hook sees, discloses it (`commands.rs:974-978`):

> *"…in this mode every keystroke passes through…"*

The Windows hook says only (`commands.rs:971-973`):

> *"Global shortcuts are active."*

So a Windows user is shown a padlock, a green "private" banner, and no
disclosure, on a backend that reads every key they press. For a privacy-first
application this is the most serious defect in the audit — more so than B1 or
B2, which at least fail visibly.

**Fix:** remove `Backend::WindowsHook` from `is_private()`; add a test asserting
`sees_raw_keys() == !is_private()` for every backend so the two can never diverge
again; and give the Windows hook a detail string modelled on the X11 one, saying
what it sees. Then amend `docs/privacy.md` and `docs/development.md:463` to state
what the Windows backend sees and what VoxCtrl does with it (matches it against
bindings, discards it, never logs or transmits it).

This is a **release blocker for any Windows build**, and the one-line
`is_private()` fix should land in Milestone 0, ahead of everything else.

#### ❌ B4. The application crate has never been compiled for Windows

`ci.yml:62` excludes `voxctrl-app` and `voxctrl-inference`. Everything under
`src-tauri/src/` — tray, overlay sidecar spawn, installer, updater, window
management, `stop_key`, `pipeline` — is unverified on Windows. Expect a first
build to surface a batch of ordinary compile errors; they are cheap to fix but
must be found before anything else can be planned around.

`voxctrl-inference` additionally requires the whisper.cpp C++ build under MSVC,
which is where the real build risk sits (§4.4).

### 3.2 Parity gaps — the app runs but a feature is missing

| Gap | Evidence | Impact |
|---|---|---|
| ⚠️ **Piper TTS cannot self-install** | `crates/voxctrl-tts/src/piper.rs:192-224` — `download_piper_binary()` body is `#[cfg(unix)]`; on Windows it returns `Ok(())` having done nothing | Piper is the **default** engine (`voxctrl-config/src/lib.rs:308`). On Windows it silently no-ops and the user must hand-place `piper.exe` per `docs/windows_build.md` |
| ⚠️ **eSpeak-NG absent** | `crates/voxctrl-tts/src/engine.rs:389`, `inflect/phonemes.rs:42` — both probe `espeak-ng` on `PATH` | Two TTS engines (eSpeak, Inflect-Micro) are dead on a stock Windows box. Inflect-Micro is a **default feature** |
| ⚠️ **Notifications likely silent** | `crates/voxctrl-inject/src/lib.rs:109-129` uses `notify-rust` | Unpackaged Windows exes need a Start Menu shortcut carrying a registered AppUserModelID, or toasts are dropped with no error — or attributed to "PowerShell". Tauri's own `tauri-plugin-notification` is *already a dependency* and handles this |
| ❌ **D-Bus targets** 🐧 | `targets.rs:524-545` returns an error on non-Linux | Correct behaviour, but the UI still offers the target type. Needs a Windows counterpart (§4.5) |
| ❌ **`pipe` target (FIFO)** 🐧 | `targets.rs:357-395` opens a path for writing | Windows has no `mkfifo`. Should map to a **named pipe** target |
| ⚠️ **`socket` target loses Unix sockets** | `targets.rs:399` — `#[cfg(unix)]` branch | TCP still works. Win11 *does* support `AF_UNIX`, but Tokio does not expose it; named pipes are the idiomatic replacement |
| ❌ **MCP path is hard-coded POSIX** | `crates/voxctrl-mcp/src/lib.rs:17` — `pub const SOCKET_PATH: &str = "/tmp/voxctrl-mcp.sock"` | The Windows *server* correctly uses `\\.\pipe\voxctrl-mcp` (`lib.rs:162`), but the exported constant, the README, and `docs/architecture.md` still advertise the socket path to MCP clients |
| ❌ **No autostart** | nothing in the tree | A dictation daemon that does not start with the session is not usable day to day. Linux gets this via the `.desktop` file |
| ❌ **No desktop integration** | `src-tauri/src/installer.rs:135` `setup_desktop_integration()` is called only under `#[cfg(target_os = "linux")]` (`lib.rs:286`) | Ties into notifications (AUMID) and autostart |
| ⚠️ **Installer/setup UI is Linux-shaped** | `installer.rs:4-45` detects `pacman`/`apt`/`dnf`/`zypper`; the first-run wizard and `/udev-warning` window surface portal/`wtype`/udev concepts | On Windows these steps are meaningless and must be replaced with Windows-relevant checks (mic privacy, WebView2, model download) |
| ⚠️ **Overlay console window** | `src-tauri/src/overlay_sidecar.rs:50` spawns the sidecar with no `CREATE_NO_WINDOW`, and `src-tauri/src/overlay.rs` has no `windows_subsystem` attribute (unlike `main.rs:2`) | A console window will flash or persist next to the overlay |
| ⚠️ **Overlay click-through unverified** | `overlay.rs:1264-1284` relies on winit `set_cursor_hittest(false)` on Windows | winit maps this to `WS_EX_TRANSPARENT`, which is right, but the window also needs `WS_EX_NOACTIVATE` so it never steals focus mid-dictation, and `WS_EX_LAYERED` for reliable transparency |
| ❌ **Updater assets not published** | `crates/voxctrl-update/src/install.rs:136-140` looks for `-windows-x86_64.exe` | The logic is correct and unit-tested; the release matrix simply never produces that asset (`release.yml:109`). Self-update is dead until the matrix row is re-enabled |
| ⚠️ **`npm run predev` is Linux-only** | `package.json` — `pkill` / `fuser` | Dev-experience papercut for Windows contributors |
| ⚠️ **`exec` target quoting** | `targets.rs:310-316` splits the template on whitespace and spawns `raw_parts[0]` | On Windows there is no `argv`; `CreateProcess` re-joins and each program re-parses. A path with spaces, or an argument with quotes, behaves differently than on Linux |

### 3.3 Explicitly Linux-only — keep, don't port

These should be **hidden**, not stubbed, on Windows:

- XDG GlobalShortcuts portal, KDE `kglobalshortcutsrc` sync
  (`voxctrl-hotkeys/src/portal.rs`)
- X11/XInput2 backend, evdev backend, `/dev/input` device counting
- Linux Mint Cinnamon/MATE `gsettings` shortcut integration
  (`src-tauri/src/mint_shortcuts.rs`, 645 lines — currently compiled on Windows
  as dead code)
- udev warning window (`src/lib/Diagnostics/UdevWarning.svelte`) and the
  `udev-warning` Tauri window (`tauri.conf.json`)
- `host_env.rs` AppImage environment scrubbing
- `pkexec`-based privileged package installation
- AppImage/`.deb` packaging and in-place binary replacement

---

## 4. Strategy: options considered and recommendations

### 4.1 Global hotkeys

The gesture model is the constraint. VoxCtrl supports `hold`, `toggle`,
`double_tap`, and `double_tap_hold` (`voxctrl-routing/src/models.rs:11`). Hold
and double-tap require **key-down and key-up transitions**, plus timing. That
rules out most of the obvious choices.

| Option | Verdict |
|---|---|
| `RegisterHotKey` (Win32) / the `global-hotkey` crate | ❌ Delivers a single `WM_HOTKEY` on press. No release event, no hold, no double-tap. Would reduce Windows to `toggle` only — the same degradation the Cinnamon backend accepts, and unacceptable as the *primary* path |
| Raw Input (`WM_INPUT`, `RIDEV_INPUTSINK`) | ⚠️ Gives down/up for all devices without a hook, and is not subject to the hook timeout. But it does **not** suppress the keystroke, so Super+Space would also reach the focused app. Good as a *secondary* signal |
| `WH_KEYBOARD_LL` low-level hook | ✅ **Recommended.** Down/up with timing, and the callback can swallow the keystroke by returning non-zero. This is what essentially every Windows hotkey daemon uses |
| UI Automation / accessibility APIs | ❌ Wrong layer |

**Recommended: a low-level keyboard hook, written directly against `windows-rs`,
replacing `rdev`.**

Reasons to drop `rdev` (currently pinned at 0.5.3):

- Its author describes it as a pet project; the actively maintained line is the
  RustDesk fork `rdevin`.
- It abstracts away exactly the two things this design needs: the **scan code**
  (the stable, layout-independent identity that maps cleanly to evdev names) and
  **suppression** (returning non-zero from the hook).
- It brings its own key vocabulary, which is the root of bug B1. Going direct
  removes the translation layer rather than fixing it.

Implementation requirements — each of these is a real constraint, not a detail:

1. **Dedicated thread with a message pump.** `SetWindowsHookExW(WH_KEYBOARD_LL,
   ..)` requires the installing thread to run a `GetMessage`/`DispatchMessage`
   loop. The existing `voxctrl-rdev` thread already isolates this; keep the
   shape, replace the body.
2. **The callback must be fast — under ~1 second, and in practice under ~1 ms.**
   Windows silently unhooks a procedure that exceeds `LowLevelHooksTimeout`
   (capped at 1000 ms since Windows 10 1709), *with no notification to the
   application*. The current callback takes two `Mutex` locks and does a
   `crossbeam` `try_recv` inside the hook. Restructure so the hook does nothing
   but push `(scancode, vk, is_down, time)` onto a lock-free queue; all matching,
   gesture timing, and channel sends move to a worker thread.
3. **Detect and self-heal the silent unhook.** Because Windows gives no callback,
   run a watchdog: if no keyboard event has been seen for N seconds while the
   session is interactive, re-install the hook and report it through
   `ListenerHealth`. This is the Windows analogue of the existing evdev
   supervisor in `voxctrl-hotkeys/src/linux.rs`.
4. **Map by scan code, not virtual key.** Scan codes are physical positions and
   map to evdev key names essentially 1:1 — which is precisely the vocabulary
   the rest of the app speaks. Build a static `scancode → "KEY_*"` table. This
   also makes non-US layouts behave the way they do on Linux (position-based, as
   `HotkeysTab.svelte` already assumes via `event.code`).
5. **Accept the two hard limits, and say so in the UI:**
   - Hooks do not receive input destined for a **more-privileged process**
     (UIPI). If the user focuses an elevated window, VoxCtrl's hotkey will not
     fire there unless VoxCtrl is itself elevated. Detect and surface this.
   - Hooks never see the **secure desktop** (UAC prompt, Ctrl+Alt+Del,
     lock screen). Nothing to be done; do not let a gesture get stuck "held"
     across one — the watchdog should force-release, mirroring
     `KeyMatcher::clear()`.
6. **Suppression policy.** Return `1` from the hook only for keys that complete
   a binding the user has bound, never for the whole keyboard.
7. **Health reporting.** Add a `Backend::WindowsHook` health path that reflects
   reality: hooked / unhooked / blocked-by-UIPI, replacing the current
   unconditional "healthy" (`windows.rs:26-30`).

**Fallback ladder** (mirroring the Linux portal → X11 → evdev structure):
`WH_KEYBOARD_LL` → `RegisterHotKey` (toggle-only, advertised as such in the UI,
same way `Backend::MintDbus` already advertises a reduced gesture set).

### 4.2 Text injection

| Option | Verdict |
|---|---|
| PowerShell `SendKeys` (status quo) | ❌ Metacharacter corruption (B2) + per-injection process spawn |
| `SendInput` with `KEYEVENTF_UNICODE` | ✅ **Recommended primary.** Types arbitrary Unicode independent of keyboard layout, no clipboard clobbering, works through RDP/Citrix where clipboard redirection is disabled |
| Clipboard + synthesised Ctrl+V | ✅ **Recommended fallback.** Faster for long text; already the app's Linux last resort. Must save and restore the user's clipboard |
| UI Automation `TextPattern` | ❌ Read-only by design; cannot insert |
| Text Services Framework (TSF) | ⚠️ The *correct* API for this, and what real IMEs use. Substantially more work and only pays off in a minority of apps. Note as a future option, not now |

**Recommended: `SendInput` primary, clipboard-paste fallback, both native.**

Specifics that matter:

- Send each UTF-16 code unit as its own `INPUT` with `KEYEVENTF_UNICODE`;
  **surrogate pairs must be sent as two units in one `SendInput` batch** or
  emoji and CJK extension characters break.
- Batch the whole string into a single `SendInput` call where possible;
  per-character calls interleave with real typing.
- Known quirk: some terminals (notably older Windows Terminal builds) mishandle
  `VK_PACKET`. Keep the clipboard path selectable per-target for those cases —
  the routing model already supports per-target overrides.
- Threshold-switch to clipboard paste above ~2000 characters, where
  synthesising keystrokes becomes visibly slow.
- Restore the previous clipboard contents after a paste-based injection.

Prefer the **`enigo`** crate (actively maintained, cross-platform, handles the
Unicode and surrogate details) unless a direct `windows-rs` implementation
proves necessary for suppression interplay with the hook. Using `enigo` on
*both* platforms is worth evaluating separately — it could eventually retire the
`wtype`/`xdotool` subprocess dependency on Linux too — but that is a follow-up,
not part of this port.

### 4.3 Overlay window

Keep Slint + winit; the sidecar architecture already works cross-platform.
Required Windows work:

- Add `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` to
  `src-tauri/src/overlay.rs`, matching `main.rs:2`, and spawn the sidecar with
  `CREATE_NO_WINDOW` in `overlay_sidecar.rs:50`.
- Set `WS_EX_NOACTIVATE` in addition to winit's `set_cursor_hittest(false)`
  (`WS_EX_TRANSPARENT`) and `with_skip_taskbar(true)` — already present at
  `overlay.rs:1614-1618`. Without `NOACTIVATE`, the overlay can take focus and
  the dictation lands in the wrong window.
- Verify per-monitor DPI v2 awareness and multi-monitor placement; the existing
  positioning logic assumes a single logical coordinate space.
- Replace the X11 unmap/re-map "park offscreen" trick (`overlay.rs:1237-1259`)
  with a plain `ShowWindow(SW_HIDE)` on Windows — the X11 dance exists to work
  around compositor behaviour that has no Windows analogue.

### 4.4 Speech recognition and GPU acceleration

**whisper.cpp / `whisper-rs` (CPU):** builds under MSVC; this is the safe
default and what the Windows release should ship.

**Vulkan:** ⚠️ **Do not ship a Vulkan Windows build in the first release.**
There is an open upstream issue —
[ggml-org/whisper.cpp#3750](https://github.com/ggml-org/whisper.cpp/issues/3750)
— reporting that on **Windows MSVC static builds** (exactly the configuration
`whisper-rs-sys` produces for Rust FFI) the Vulkan backend silently fails to
register in the ggml backend registry, falling back to CPU while reporting "no
GPU found". The report names **whisper-rs 0.16 / whisper-rs-sys 0.15** — the
versions this workspace pins (`crates/voxctrl-inference/Cargo.toml`). The issue
is open and stale-labelled. Silent CPU fallback under a GPU banner is worse
than no GPU option; the app already has an honest `moonshine_gpu_backend()`
reporting path, and shipping this would undermine it.

**CUDA:** works, at the cost of a large runtime. `tauri.conf.json` already
declares `resources/cudart64_12.dll`, `cublas64_12.dll`, `cublasLt64_12.dll` —
note these files are **not in the repo**, so a CUDA build will fail bundling
until they are supplied. Treat CUDA as an opt-in second artifact, not the
default.

**Recommended GPU story for Windows: ONNX Runtime + DirectML for the Moonshine
backend.** DirectML accelerates any DirectX 12 GPU — NVIDIA, AMD, Intel,
Qualcomm — with no vendor SDK at build time and no heavy runtime to bundle. It
is the Windows analogue of what Vulkan is for the Linux AppImage, and it fits
the existing feature structure cleanly:

```toml
# crates/voxctrl-inference/Cargo.toml
moonshine-directml = ["moonshine", "ort/directml"]
```

Caveat to record in the code comment: Microsoft has DirectML in sustained
engineering, with new feature work moving to Windows ML. It is the right choice
today; revisit in 12–18 months.

`ort` already uses `download-binaries` in this workspace, so no system
`onnxruntime.dll` is needed — but the ONNX Runtime build fetched must be the one
carrying the DirectML provider.

### 4.5 Text-to-speech

| Engine | Windows status | Action |
|---|---|---|
| **Piper** (default) | ⚠️ no auto-install | Add a Windows branch to `download_piper_binary()`: fetch the Windows release **zip** (not `.tar.gz`), extract with a zip reader. Requires adding a zip dependency and generalising `extract_piper_archive()` |
| **Pocket-TTS** | ✅ pure Rust + ONNX | Should work as-is once the crate compiles |
| **Breeze-TTS-2** | ✅ candle-based | CPU works; `breeze-cuda` needs nvcc. No Vulkan path exists in candle |
| **Inflect-Micro** | ❌ needs `espeak-ng` for phonemization | Either bundle an `espeak-ng.exe` + `espeak-ng-data` alongside the app (it is GPLv3 — check licence compatibility before bundling), or disable the engine on Windows with a clear message |
| **eSpeak-NG** | ❌ not on `PATH` | Same decision as above |
| **SAPI 5 / WinRT `SpeechSynthesizer`** | — | **Recommended addition**: a `TtsEngine::WindowsSapi` variant. Zero install, every Windows box has voices, gives Windows a guaranteed-working default the way `espeak` does on Linux |

### 4.6 Local IPC and the D-Bus-shaped hole

The MCP **server** already speaks named pipes correctly
(`voxctrl-mcp/src/lib.rs:156-172`) and the MCP **client** target already picks
the right transport per platform (`targets.rs:678-694`). Three fixes:

1. Make `SOCKET_PATH` platform-conditional instead of a hard-coded
   `/tmp/voxctrl-mcp.sock` (`voxctrl-mcp/src/lib.rs:17`), and update the README
   and `docs/architecture.md:196` so MCP clients are told the right address.
2. **Secure the pipe.** The Unix socket is chmod `0600` (`lib.rs:113`) precisely
   so other local users cannot activate the microphone. A named pipe created
   with default security is reachable by other users on the machine. Create it
   with an explicit SECURITY_DESCRIPTOR limited to the current user, and set
   `FILE_FLAG_FIRST_PIPE_INSTANCE` so a squatting process cannot pre-create the
   name. **This is a security requirement, not a nicety.**
3. Replace the `dbus` and `pipe` delivery types on Windows with a single
   **`named_pipe`** delivery type. This covers both use cases (broadcast to a
   listener; write to a FIFO-like channel) with one Windows-native mechanism.
   Add it to `DeliveryType` (`voxctrl-routing/src/models.rs:100-115`), gate it
   in the target editor UI, and keep `dbus`/`pipe` visible only on Linux.

### 4.7 Notifications, autostart, desktop integration

These three are one work item, because on Windows they all hang off the same
artifact: **a Start Menu shortcut carrying a registered AppUserModelID.**

- **Notifications:** stop using `notify-rust` on Windows. `tauri-plugin-notification`
  is already in `src-tauri/Cargo.toml` and already in `package.json`; route
  `show_notification()` (`voxctrl-inject/src/lib.rs:109`) through it on Windows.
  Without a registered AUMID, WinRT toasts are either dropped silently or
  attributed to PowerShell.
- **AUMID + shortcut:** the NSIS installer creates the Start Menu shortcut;
  set the AUMID property on it and match it in the app. This makes toasts work
  *and* gives correct taskbar grouping.
- **Autostart:** add `tauri-plugin-autostart` (registry `Run` key). Expose it in
  Settings → General as "Start VoxCtrl when I sign in", and make the Linux side
  use the same setting via `.desktop` autostart, so the UI is one control on
  both platforms.
- **Windows counterpart to `setup_desktop_integration()`:** register file/URI
  associations if needed, and verify the shortcut exists on launch (repairs a
  user who moved the install).

### 4.8 First-run experience

The wizard is a strong part of VoxCtrl and it is currently Linux-shaped. On
Windows, replace the checks rather than skipping the step:

| Linux step | Windows equivalent |
|---|---|
| Shortcut portal / evdev permissions | Keyboard hook installed? Is a competing hotkey app holding the combo? |
| `wtype`/`xdotool` present | (nothing — `SendInput` is always available) |
| udev warning window | Microphone privacy: **Settings → Privacy & security → Microphone** — apps get *no* mic access silently if this is off |
| Package-manager install of deps | WebView2 present (Win11 always; Win10 may need the bootstrapper) |
| espeak-ng install | TTS engine picker defaulting to SAPI or Pocket-TTS |

Model download and hotkey recording steps carry over unchanged.

### 4.9 Packaging, signing, distribution

- **Bundler: NSIS only.** `release.yml:336-341` already documents that the
  WiX/MSI bundler fails to harvest the `voxctrl-overlay` sidecar. Ship
  `setup.exe`; drop MSI from the advertised artifacts.
- **WebView2:** `tauri.conf.json` already sets
  `webviewInstallMode: embedBootstrapper`. Correct for a Win10 floor; on Win11
  it is a no-op.
- **Code signing:** unsigned installers hit SmartScreen with an "Unknown
  Publisher" wall, which for a microphone-and-keyboard-hook app is fatal to
  adoption. **Recommended: Azure Trusted Signing** (~$10/month, certificate in
  Azure's HSM, signs via API, no private key handling, first-class Tauri
  support). Note that since 2024 **EV certificates no longer bypass SmartScreen
  instantly** — reputation accrues the same way as OV, so signing early matters
  more than certificate tier. Tauri signs the binary before packaging and then
  signs the installer, which is what we want.
- **Architecture:** ship x64 first. Add ARM64 once x64 is stable — Rust, Tauri,
  and WebView2 all support it; whisper.cpp and ONNX Runtime need checking.
- **Self-update:** `voxctrl-update` already classifies a Windows install and
  picks the `-windows-x86_64.exe` asset (`install.rs:79-88`, `136-140`), and
  `spawn_relaunch` already hands off to the installer (`apply.rs:278-292`). The
  logic is written and unit-tested. It needs only the release matrix row
  re-enabled so the asset exists.

---

## 5. Architectural change: make the seams explicit

Rather than adding `#[cfg(target_os = "windows")]` arms next to each of the 104
existing ones, introduce a thin platform layer. This is the difference between a
port that holds and one that drifts.

**Proposed: `crates/voxctrl-platform`**

```
crates/voxctrl-platform/
  src/
    lib.rs          // pub traits + `pub fn current() -> &'static dyn Platform`
    injector.rs     // trait TextInjector { async fn inject(&self, &str) }
    hotkeys.rs      // trait KeySource  { fn start(..) -> Health }
    notifier.rs     // trait Notifier
    ipc.rs          // trait LocalIpc   (unix socket | named pipe)
    autostart.rs    // trait Autostart
    integration.rs  // trait DesktopIntegration
    linux/  windows/
```

Rules that make this worth doing:

1. **No `#[cfg(target_os)]` outside `voxctrl-platform` and the two `-hotkeys`
   backends.** Everything else takes a trait object. `targets.rs` in particular
   should stop knowing about `wtype`, `xdotool`, and PowerShell — it should ask
   the platform's `TextInjector`.
2. **Capability reporting, not silent no-ops.** Add
   `Platform::capabilities() -> Capabilities`, serialise it to the frontend, and
   have the Settings/target-editor UI *hide* what the platform cannot do rather
   than offering a control that fails at runtime. This directly fixes the
   current situation where the UI offers D-Bus targets on Windows.
3. **Move `mint_shortcuts.rs`, `host_env.rs`, and the Linux half of
   `installer.rs` behind `#[cfg(target_os = "linux")] mod` declarations** in
   `src-tauri/src/lib.rs:14-26`, so ~1000 lines of Linux desktop code stop being
   compiled into the Windows binary.

Do this incrementally — extract one seam per milestone, starting with
`TextInjector` (smallest, highest value) — not as a big-bang refactor.

---

## 6. Phased plan

### Milestone 0 — Build truth (2–3 days)

**Goal: know exactly what is broken.**

- **Fix `is_private()` (B3) first.** Drop `Backend::WindowsHook` from
  `health.rs:202`, and add the invariant test
  `sees_raw_keys() == !is_private()` across all backends. One line of code, and
  it removes a false privacy claim before any Windows binary can carry it.
- Remove `--exclude voxctrl-inference --exclude voxctrl-app` from `ci.yml:62`;
  add MSVC + CMake setup to the Windows job.
- Fix every compile error that surfaces in `src-tauri/` and `voxctrl-inference`.
- Add `cargo clippy -D warnings` on Windows.
- Add a `cargo test` job on `windows-latest` for the crates that have tests and
  no display dependency (`voxctrl-config`, `voxctrl-routing`, `voxctrl-update`,
  `voxctrl-text`).

**Exit criteria:** `cargo check --workspace` and `cargo test` are green on
`windows-latest` in CI, with no crates excluded; the privacy invariant test
passes; `docs/development.md` and `docs/privacy.md` no longer claim the Windows
hook is private.

### Milestone 1 — Make it work (1.5 weeks)

**Goal: a developer can dictate into Notepad on Windows 11.**

1. **Rewrite the hotkey backend.** Replace `rdev` with a `windows-rs`
   `WH_KEYBOARD_LL` hook: dedicated message-pump thread, minimal callback,
   lock-free handoff to a worker, scan-code→evdev name table, suppression,
   watchdog re-install, honest `ListenerHealth`. *(Fixes B1.)*
2. **Write the scan-code table with a round-trip test** asserting that every key
   `HotkeysTab.svelte`'s `mapBrowserKeyToEvdev` can produce is reachable from a
   scan code — a unit test that would have caught B1 on day one.
3. **Replace both PowerShell injection paths with `SendInput`** — the type
   target (`targets.rs:142`) and the clipboard-paste helper
   (`voxctrl-inject/src/lib.rs:94`). Extract behind a `TextInjector` trait.
   *(Fixes B2.)*
4. **Test with adversarial payloads:** `50% (a+b) {x} [y] ^C ~z`, emoji,
   CJK, a 5000-character paragraph.

**Exit criteria:** default Super+Space hold-to-dictate produces byte-exact text
in Notepad, Word, Chrome, VS Code, and Windows Terminal.

### Milestone 2 — Parity (2.5 weeks)

- Overlay: `windows_subsystem`, `CREATE_NO_WINDOW`, `WS_EX_NOACTIVATE`,
  DPI/multi-monitor verification.
- Notifications via `tauri-plugin-notification`; AUMID on the Start Menu
  shortcut.
- Autostart via `tauri-plugin-autostart`, unified with the Linux `.desktop`
  autostart under one Settings control.
- Named-pipe delivery type; MCP `SOCKET_PATH` made conditional; **named-pipe
  ACL hardening**; README + `docs/architecture.md` corrected.
- Piper Windows auto-install (zip); `TtsEngine::WindowsSapi`; decide and
  implement the espeak-ng story.
- Windows first-run wizard variant (§4.8).
- Capability-gate the target editor and Settings tabs.
- `exec` target: document and test Windows argument quoting.
- Cross-platform `predev` script.

**Exit criteria:** every row of the §3.2 gap table is closed or explicitly and
visibly declared unavailable in the UI.

### Milestone 3 — Ship (1.5 weeks)

- Re-enable the `windows-cpu` row in `release.yml:109-113`; restore the Windows
  row in the release-body downloads table.
- Azure Trusted Signing wired into the release workflow.
- End-to-end self-update test: install 0.4.0, publish 0.4.1, confirm the app
  detects, downloads, verifies the checksum, and relaunches.
- Rewrite `docs/windows_build.md` (it currently documents the SendKeys
  behaviour as intended) and add a Windows section to `docs/installation.md`.
- Fresh-VM install test on Windows 11 23H2 and 24H2.

**Exit criteria:** a signed `setup.exe` on the GitHub release; a clean Windows 11
VM goes from download to working dictation with no manual steps.

### Milestone 4 — Hardening and polish (2 weeks)

- ONNX Runtime + DirectML feature for Moonshine; a second `windows-directml`
  release artifact.
- ARM64 build.
- UIPI/elevation detection with a clear explanation in the UI.
- Track [whisper.cpp#3750](https://github.com/ggml-org/whisper.cpp/issues/3750);
  add a Vulkan Windows build only once it is genuinely fixed, with a startup
  assertion that the Vulkan backend actually registered.
- Extract the remaining seams into `voxctrl-platform`.
- Windows-native "open shortcut settings" equivalent for
  `open_shortcut_settings()` (`commands.rs:1166`), which currently returns an
  error saying it is Linux-only.

---

## 7. CI and testing strategy

The single most valuable change is structural: **every Windows bug found in this
audit was invisible because Windows code was excluded from CI.** So:

| Lane | Platform | Runs |
|---|---|---|
| `cargo check --workspace` | ubuntu-22.04 + windows-latest | every PR — **no `--exclude`** |
| `cargo clippy -D warnings` | both | every PR |
| `cargo test` (portable crates) | both | every PR |
| `cargo test` (X11-dependent) | ubuntu under `xvfb` | every PR (as today) |
| Frontend `svelte-check` + vitest | ubuntu | every PR |
| Build installer | windows-2022 | every PR to `master`, and releases |
| Smoke test | windows-2022 | launch headless, assert tray + hook install + MCP pipe |

Add these specific regression tests, each of which maps to a bug found above:

- **Key-name round trip** — every evdev name the frontend can emit is produced by
  the Windows scan-code mapper for the corresponding scan code. *(B1)*
- **Injection fidelity** — a table-driven test over metacharacter-heavy strings,
  asserting the injector's output byte-for-byte. *(B2)*
- **Capability coverage** — for each `DeliveryType`, assert that either the
  platform implements it or `Capabilities` reports it unavailable, so a new
  delivery type cannot ship half-ported. *(§3.2, D-Bus/pipe)*
- **Named-pipe ACL** — assert the MCP pipe rejects a connection from another
  user context. *(§4.6)*
- **Privacy invariant** — for every `Backend`, `sees_raw_keys()` and
  `is_private()` are exact opposites. *(B3)*

Manual test matrix for each release: Windows 11 23H2 / 24H2 · x64 · Intel + AMD
+ NVIDIA · 100% and 150% DPI · single and dual monitor · US and non-US keyboard
layout.

---

## 8. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Shipping a false privacy claim (B3) | Certain if unfixed | **Critical** | One-line fix plus an invariant test, in Milestone 0 |
| Antivirus/EDR flags a global keyboard hook | **High** | High | Code-sign early to build reputation; document the behaviour prominently; never log keystroke content; keep the hook's suppression scope minimal and auditable |
| Silent hook removal on timeout | Medium | High | Minimal callback + watchdog re-install + health reporting (§4.1) |
| whisper.cpp MSVC build friction | Medium | Medium | CPU-only first; DirectML for GPU; keep Vulkan off Windows until #3750 resolves |
| Hotkey conflicts with Windows-reserved combos | High | Low | Extend the existing `is_reserved_for_the_desktop()` (`voxctrl-hotkeys/src/trigger.rs`) with a Windows list (Win+L, Ctrl+Alt+Del, Win+G…) |
| SmartScreen suppresses adoption | High | High | Azure Trusted Signing from the first public build |
| Elevated-window dictation fails (UIPI) | Certain | Medium | Detect and explain; document that elevation is required to dictate into elevated apps |
| `voxctrl-platform` refactor destabilises Linux | Medium | High | Extract one seam per milestone; Linux CI stays green throughout; no behaviour changes in the same commit as a move |
| DirectML enters deeper maintenance mode | Low (near term) | Medium | Isolate behind the `ort` EP feature flag; Windows ML is the migration path |

---

## 9. What will *not* reach parity, and why

State these in the docs and in the UI rather than letting users discover them:

1. **Dictation into elevated applications** requires running VoxCtrl elevated
   (UIPI). Linux has no equivalent restriction.
2. **The secure desktop** (UAC prompt, lock screen, Ctrl+Alt+Del) is invisible to
   any user-mode app. Hotkeys cannot fire there.
3. **D-Bus targets** have no Windows counterpart. Named-pipe targets replace them
   for the same use cases.
4. **FIFO (`pipe`) targets** do not exist on Windows; named pipes replace them.
5. **Portal-based hotkey privacy.** On Linux, the XDG portal means the
   compositor owns the grab and VoxCtrl never reads the keyboard — a genuine
   privacy property the app advertises via `is_private()`. A `WH_KEYBOARD_LL`
   hook **does** see every keystroke, so Windows reports `is_private() == false`
   (see B3 — today it wrongly reports `true`).

That last point deserves emphasis. VoxCtrl's privacy story is a headline feature
of the project, and `is_private()` is not a diagnostic — it is a promise shown to
the user. The Windows backend cannot make that promise. What it *can* say, and
what `docs/privacy.md` should be extended to say, is the next-best true thing:
the hook matches each key against the user's bindings, discards it, and never
logs or transmits it.

---

## 10. Summary of recommendations

| Decision | Recommendation |
|---|---|
| Hotkey backend | `windows-rs` `WH_KEYBOARD_LL`, scan-code based; drop `rdev` |
| Hotkey fallback | `RegisterHotKey`, toggle-only, advertised as reduced |
| Text injection | `SendInput` + `KEYEVENTF_UNICODE`; clipboard paste as fallback |
| STT GPU | ONNX Runtime **DirectML**; CUDA opt-in; **no Vulkan on Windows yet** |
| TTS default | Add `WindowsSapi`; fix Piper auto-install |
| IPC | Named pipes, ACL-restricted to the current user |
| Notifications | `tauri-plugin-notification` + AUMID on Start Menu shortcut |
| Packaging | NSIS `setup.exe` only; drop MSI |
| Signing | Azure Trusted Signing, from the first public build |
| Architecture | Extract a `voxctrl-platform` crate, one seam per milestone |
| Privacy | `is_private()` must be **false** for `WindowsHook`; fix before anything else |
| CI | Remove all Windows `--exclude`s in Milestone 0 |

---

## Appendix: sources

- [ggml-org/whisper.cpp#3750 — Vulkan backend silently fails to register on Windows MSVC static builds](https://github.com/ggml-org/whisper.cpp/issues/3750)
- [LowLevelKeyboardProc callback function — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
- [SendInput — windows-rs documentation](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/UI/Input/KeyboardAndMouse/fn.SendInput.html)
- [DirectML Execution Provider — ONNX Runtime](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)
- [Windows Code Signing — Tauri v2](https://v2.tauri.app/distribute/sign/windows/)
- [Windows Code Signing with Azure Trusted Signing — KeyQ](https://www.keyq.cloud/blog/windows-code-signing-with-azure-trusted-signing/)
- [enigo — cross-platform input simulation in Rust](https://github.com/enigo-rs/enigo)
- [rdev — crates.io](https://crates.io/crates/rdev) and the maintained fork [rdevin](https://docs.rs/rdevin/)
- [UI Automation TextPattern Overview — Microsoft Learn](https://learn.microsoft.com/en-us/dotnet/framework/ui-automation/ui-automation-textpattern-overview)
- [Windows native toast notifications require a registered AUMID](https://github.com/Ivy-Interactive/Rustino/issues/11)
