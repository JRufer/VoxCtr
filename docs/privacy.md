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
| Does it phone home, check for updates, or send telemetry? | **No.** |
| Does it need root? | **No.** Administrator rights are requested once, optionally, to install packages. |
| Can I verify all this? | Yes — see [Verifying it yourself](#verifying-it-yourself). |

---

## Keystrokes

VoxCtrl needs to know when you press your dictation shortcut. There are two ways an app can learn that on Linux, and they are not equivalent.

### What VoxCtrl does: ask the desktop

VoxCtrl registers its shortcuts with `org.freedesktop.portal.GlobalShortcuts`. Your desktop compositor grabs the keys and sends VoxCtrl a D-Bus signal — `Activated` or `Deactivated`, carrying a shortcut ID — when one fires.

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

### The fallback, and being honest about it

If your desktop provides no portal **and** your system already lets this process read input devices, VoxCtrl will use that access rather than refusing to work. In that mode every keystroke does pass through the process.

VoxCtrl does not hide this. The Hotkeys tab and the setup window both say so in plain language, and `is_private` is false throughout the status API. What it does *not* do in that mode:

- log key names (the reader has no logging on the key path)
- store them (key names are transient `String`s and a small set of held keys)
- send them anywhere (they never cross into the UI layer, and no network path can reach them)
- read mice, touchpads or tablets (only devices that look like keyboards are opened)
- read its own injected keystrokes (synthetic devices — `uinput`, `XTEST`, anything named "virtual" — are always skipped)

If you would rather it never happened at all, a desktop that implements the portal avoids it entirely. See [Hotkeys](hotkeys.md#linux--evdev-fallback).

### Windows

Windows offers no portal equivalent; a low-level keyboard hook (`WH_KEYBOARD_LL`) is the only mechanism for application-defined global shortcuts. That hook sees all keystrokes. The same handling applies: nothing logged, nothing stored, nothing transmitted.

---

## Audio

- The microphone is opened when a gesture starts recording and closed when it stops. It is not held open in between.
- Audio is transcribed on your machine by `whisper.cpp` (or Moonshine). No audio is uploaded anywhere.
- Transcription history is stored locally under `~/.local/share/voxctrl/`, and can be cleared from the History panel.

The one exception is one you configure: `http`, `webhook`, `chat` and `mcp` targets send **transcribed text** to wherever you point them, and LLM post-processing sends text to the endpoint you configure. Those are opt-in, per-target, and visible in `targets.toml`. Audio itself is never sent by any target.

---

## Network

VoxCtrl makes no network requests on its own behalf. It has no telemetry, no analytics, no update check, and no crash reporting.

It reaches the network only when you ask it to:

| Trigger | Destination |
|---|---|
| Downloading a speech model | HuggingFace / the model host, on demand |
| Downloading a TTS voice | HuggingFace / the Piper voice host, on demand |
| LLM post-processing | The OpenAI-compatible endpoint you configured |
| `http` / `webhook` / `chat` / `mcp` targets | The destination you configured |

Once the app and its models are on disk, VoxCtrl runs fully air-gapped.

---

## What the installer touches

The optional setup step (`--install`, or the button in the setup window) does exactly three things:

1. Installs host packages via your package manager — WebKitGTK, OpenSSL, PortAudio, `wtype`, `xdotool`, clipboard helpers. These are what type transcriptions into your focused window.
2. Writes `~/.local/share/applications/voxctrl.desktop` and an icon.
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

**Read the code.** The hotkey crate is about 1,500 lines and self-contained:

- `crates/voxctrl-hotkeys/src/portal.rs` — the portal backend, the whole data path
- `crates/voxctrl-hotkeys/src/gestures.rs` — gesture recognition, which never sees a key name
- `crates/voxctrl-hotkeys/src/linux.rs` — the evdev fallback and why it is a fallback
- `src-tauri/src/installer.rs` — everything the installer does, in one file

---

## Reporting a problem

If you find something here that is not true, that is a bug and we want to know. Open an issue at <https://github.com/JRufer/VoxCtrl/issues>. For anything you would rather not disclose publicly, say so in the issue and we will arrange a private channel.
