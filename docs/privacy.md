# Privacy & Security

This page answers one question: **what can VoxCtrl actually see, and what does installing it change about your machine?**

Everything below describes what the code does, with pointers to where it does it, so you can check rather than take our word for it.

---

## The short version

| Question | Answer |
|---|---|
| Can VoxCtrl read what I type in other applications? | **No.** Your desktop delivers shortcuts; VoxCtrl is never given keystrokes. |
| Does installing it grant any process new access to my keyboard? | **No.** No udev rule, no `input` group, nothing. |
| Does my audio leave the machine? | **No**, unless you configure a target that sends it somewhere. |
| Does it send telemetry or analytics? | **No.** Nothing about you or your machine is ever transmitted. |
| Does it phone home at all? | **Once, for updates.** On launch it asks GitHub what the latest release is — no identifiers, nothing about you — and one setting turns it off. [Details](#network). |
| Does it need root? | **No.** Administrator rights are requested once, optionally, to install packages. |
| Can I verify all this? | Yes — see [Verifying it yourself](#verifying-it-yourself). |

---

## Keystrokes

VoxCtrl needs to know when you press your dictation shortcut. There are two ways an app can learn that on Linux, and they are not equivalent.

### What VoxCtrl does: ask the desktop

VoxCtrl identifies itself to the portal as `ai.voxctrl.app` and registers its shortcuts with `org.freedesktop.portal.GlobalShortcuts`. Your desktop compositor grabs the keys and sends VoxCtrl a D-Bus signal — `Activated` or `Deactivated`, carrying a shortcut ID — when one fires.

That signal is the entire input. VoxCtrl does not receive, and cannot request, anything about keys it did not register. There is no filtering step to trust and no policy to get wrong: the data never arrives.

The internal event type says the same thing (`crates/voxctrl-hotkeys/src/gestures.rs`):

```rust
pub struct GestureEvent {
    pub binding_id: String,
    pub binding_label: String,
    pub target_id: String,
    pub kind: GestureKind,   // Start | Stop
}
```

No key names. No timing of individual presses. Nothing about what you typed. This is the only hotkey data that reaches the rest of the app, and the only thing the UI layer ever sees.

On KDE Plasma, the portal accepts these shortcuts but leaves them disabled in System Settings until you tick a box and press Apply — an upstream KDE bug, not a VoxCtrl privacy question. See [KDE registers shortcuts disabled by default](hotkeys.md#kde-registers-shortcuts-disabled-by-default).

### What VoxCtrl deliberately stopped doing: reading `/dev/input`

Earlier versions read `/dev/input/event*` directly, which requires a udev rule tagging input devices with `uaccess`:

```
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"
```

VoxCtrl's installer wrote that rule. It is not narrow. It grants **every process running as you** the ability to read **every keystroke on the system** — your sudo password, your browser, your password manager — for as long as the rule exists. Not just VoxCtrl: a compromised npm postinstall script, any Electron app, any shell one-liner.

systemd's own defaults (`/usr/lib/udev/rules.d/70-uaccess.rules`) grant `uaccess` on input devices to joysticks and nothing else, precisely to prevent this. VoxCtrl was overriding a deliberate security decision, on the user's behalf, during a first-run wizard.

**It no longer does.** VoxCtrl never writes that rule, never runs `usermod -aG input`, and offers no button that does either. If your desktop has no shortcuts portal, VoxCtrl says so at launch and explains the trade-off rather than quietly widening access. Two tests fail if this regresses:

- `the_privileged_script_never_touches_input_permissions` (`src-tauri/src/installer.rs`)
- `never offers to grant keyboard access` (`tests/svelte/SetupWindow.test.ts`)

### The fallbacks, and being honest about them

If your desktop provides no portal, VoxCtrl reads keys itself: X11 raw key
events (XInput2) where there is an X server, and `/dev/input/event*` only where
your system already lets this process do so. In either mode every keystroke does
pass through the process.

The X11 backend needs no permission at all — any X client may ask the server for
raw key events — so it is what a stock Cinnamon, MATE or Xfce desktop uses. That
makes it *easier* to reach than the evdev fallback, not more private: it sees
exactly as much. It is chosen over evdev because it asks the user for nothing,
and it is ranked below the portal for precisely this reason.

VoxCtrl does not hide this. The Hotkeys tab and the setup window both say so in plain language, and `is_private` is false throughout the status API. What it does *not* do in that mode:

- log key names (the reader has no logging on the key path)
- store them (key names are transient `String`s and a small set of held keys)
- send them anywhere (they never cross into the UI layer, and no network path can reach them)
- read mice, touchpads or tablets (only devices that look like keyboards are opened; on X11 only key events are selected)
- read its own injected keystrokes (synthetic devices — `uinput`, `XTEST`, anything named "virtual" — are always skipped, by name on evdev and by `sourceid` on X11)

If you would rather it never happened at all, a desktop that implements the portal avoids it entirely — as does the native Cinnamon/MATE shortcut route, where the desktop holds the grab and VoxCtrl reads nothing. See [Hotkeys](hotkeys.md#linux--x11-raw-key-events-xinput2).

### Windows

Windows offers no portal equivalent; a low-level keyboard hook (`WH_KEYBOARD_LL`) is the only mechanism for application-defined global shortcuts. That hook sees all keystrokes. The same handling applies: nothing logged, nothing stored, nothing transmitted.

---

## Audio

- The microphone is opened when a gesture starts recording and closed when it stops. It is not held open in between.
- Audio is transcribed on your machine by `whisper.cpp` (or Moonshine). No audio is uploaded anywhere.

The one exception is one you configure: `http`, `webhook`, `chat` and `mcp` targets send **transcribed text** to wherever you point them, and LLM post-processing sends text to the endpoint you configure. Those are opt-in, per-target, and visible in `targets.toml`. Audio itself is never sent by any target.

---

## Network

VoxCtrl has no telemetry, no analytics, and no crash reporting. Nothing is ever
sent about what you dictate, what you type, what you have installed, or who you
are.

It makes exactly one request you did not personally trigger: **the update
check**. Everything else on the network happens because you asked for it:

| Trigger | Destination | Sends |
|---|---|---|
| Update check, ~10 s after launch | `api.github.com/repos/JRufer/VoxCtrl/releases/latest` | A `User-Agent` of `VoxCtrl/<version>`. Nothing else. |
| Installing an offered update | `github.com` release download | Nothing beyond the request for the file |
| Downloading a speech model | HuggingFace / the model host, on demand | Nothing beyond the request for the file |
| Downloading a TTS voice | HuggingFace / the Piper voice host, on demand | Nothing beyond the request for the file |
| LLM post-processing | The OpenAI-compatible endpoint you configured | The transcribed text |
| `http` / `webhook` / `chat` / `mcp` targets | The destination you configured | The transcribed text |

### The update check, in full

It is a plain unauthenticated `GET` for the public release listing — the same
URL anyone can open in a browser. There is no request body, no cookie, no
account, no install ID, and no way for it to carry one: GitHub is told which
version of VoxCtrl is asking (because the API requires a `User-Agent`) and
nothing more. What comes back is the release's tag, notes and file list, which
is what the update window shows you. Nothing is downloaded or installed unless
you press **Update and restart**, and a downloaded update is checked against the
SHA-256 checksum GitHub publishes for it before it replaces anything.

**Turning it off:** Settings → General → untick "Check for a new version on
launch" (or `"updates": { "auto_check": false }` in `config.json`). VoxCtrl then
makes no request at all unless you press "Check now". The update window offers
the same switch, so declining an update and stopping the checks is one click.

Once the app and its models are on disk, VoxCtrl runs fully air-gapped —
including with update checking left on, which fails quietly and changes nothing
when there is no network.

**The code:** `crates/voxctrl-update/` is the whole of it — about 400 lines,
with no dependency on the rest of the app. `release.rs` builds the one request,
`apply.rs` downloads and verifies, `src-tauri/src/updater.rs` decides when to
ask and what to show.

---

## What the installer touches

The optional setup step (`--install`, or the button in the setup window) does exactly three things:

1. Installs host packages via your package manager — WebKitGTK, OpenSSL, PortAudio, `wtype`, `xdotool`, clipboard helpers. These are what type transcriptions into your focused window.
2. Writes `~/.local/share/applications/ai.voxctrl.app.desktop` and an icon (the app does this itself on every launch too, without privileges — the installer is not needed for it).
3. **Removes** the udev rule older VoxCtrl versions installed, if it finds one.

It does not create system users, services, or permissions. `scripts/uninstall.sh` reverses everything, including leftovers from older versions.

---

## Verifying it yourself

**Confirm no input devices are open.** With VoxCtrl running on a desktop with portal support:

```bash
ls -l /proc/$(pgrep -f voxctrl | head -1)/fd | grep /dev/input
```

No output means VoxCtrl has no input device open — it is not reading your keyboard.

**Confirm which backend is live.** Settings → Hotkeys states it at the top of the page, and the setup window's first step says the same. A 🔒 means the portal path.

**Confirm no udev rule was installed:**

```bash
ls /etc/udev/rules.d/ | grep -i voxctrl    # expect no output
groups | grep input                        # expect no match, unless you added it yourself
```

**Watch the D-Bus traffic.** Everything VoxCtrl learns about your keyboard travels over this, so you can see the whole of it:

```bash
dbus-monitor "interface='org.freedesktop.portal.GlobalShortcuts'"
```

Press your shortcut. You will see `Activated` and `Deactivated` with a shortcut ID. Type anything else, anywhere else: nothing appears.

**Confirm no network traffic.** Run VoxCtrl with the network namespace cut off, and dictation still works once models are downloaded:

```bash
sudo unshare -n sudo -u "$USER" ./VoxCtrl-x86_64.AppImage
```

**See the update check for yourself.** With checking enabled, watch what leaves
the machine in the first minute after launch:

```bash
sudo tcpdump -n -i any 'host api.github.com'   # one TLS connection, then nothing
```

Untick "Check for a new version on launch" and the same command stays silent.

**Read the code.** The hotkey crate is about 1,500 lines and self-contained:

- `crates/voxctrl-hotkeys/src/portal.rs` — the portal backend, the whole data path
- `crates/voxctrl-hotkeys/src/trigger.rs` — what VoxCtrl is allowed to ask a desktop to bind
- `crates/voxctrl-hotkeys/src/gestures.rs` — gesture recognition, which never sees a key name
- `crates/voxctrl-hotkeys/src/linux.rs` — the backend order and why each is where it is
- `crates/voxctrl-hotkeys/src/x11.rs` — the X11 backend, what it sees and what it filters
- `src-tauri/src/installer.rs` — everything the installer does, in one file

---

## Reporting a problem

If you find something here that is not true, that is a bug and we want to know. Open an issue at <https://github.com/JRufer/VoxCtrl/issues>. For anything you would rather not disclose publicly, say so in the issue and we will arrange a private channel.
