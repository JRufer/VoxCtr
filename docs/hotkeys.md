# Hotkey System

**Crate:** `crates/voxctrl-hotkeys/`

## Overview

VoxCtrl listens for global shortcuts that work regardless of which application has focus.

On Linux it prefers the **XDG desktop portal**, where **VoxCtrl does not read your keyboard**. Your desktop owns the key grab and tells VoxCtrl one thing: that its own shortcut fired. VoxCtrl never sees what you type in your browser, your terminal, your password manager, or anywhere else — not because it chooses not to look, but because it is never given the data.

Where no portal exists, VoxCtrl falls back in order: **X11 raw key events**, then **evdev**, then a **native Cinnamon/MATE shortcut**. Each is described below, and the app always says which one is running.

This is a change from earlier versions, which read `/dev/input/event*` directly. See [Why this changed](#why-this-changed) below.

---

## What VoxCtrl can and cannot see

| | Portal (preferred) | X11 raw events | evdev fallback | Mint native shortcut |
|---|---|---|---|---|
| Can see keystrokes in other apps | **No** | Yes, all of them | Yes, all of them | **No** |
| Needs a udev rule or `input` group | **No** | **No** | Yes | **No** |
| Needs any permission setup | **No** | **No** | Yes, and VoxCtrl will not do it for you | **No** |
| Who chooses the keys | You, through your desktop | You, in VoxCtrl | You, in VoxCtrl | You, in VoxCtrl |
| Works on Wayland | Yes | No | Yes | Yes |
| Works on X11 | Yes | Yes | Yes | Yes |
| Works with no desktop (bare TTY) | No | No | Yes | No |
| Gesture styles it can deliver | All four | All four | All four | `toggle` only |
| Bare-modifier triggers (double-tap Super) | No | Yes | Yes | No |

The Settings → Hotkeys tab and the setup window both state which of these is in use, live. If it says your desktop is handling the shortcuts, VoxCtrl is not reading your keyboard.

### Gesture styles and the backend

A backend that reports individual key **presses and releases** can serve every
gesture: `hold`, `toggle`, `double_tap` and `double_tap_hold`. A backend that
only ever learns that a key went *down* cannot end a hold and cannot tell a tap
from a hold, so it can serve `toggle` and nothing else.

`Backend::gestures()` in `crates/voxctrl-hotkeys/src/health.rs` is the single
definition of that, and the Hotkeys tab offers exactly what it returns — a
gesture style the running backend cannot deliver is not shown at all, rather
than offered and silently doing nothing. A binding saved under a different
backend keeps its gesture and is flagged instead of being rewritten.

---

## Platform Implementations

### Linux — XDG `GlobalShortcuts` portal (default)

VoxCtrl talks to `org.freedesktop.portal.GlobalShortcuts` over D-Bus:

0. It declares its application id — `ai.voxctrl.app` — through
   `org.freedesktop.host.portal.Registry`, via `ashpd::register_host_app`. A
   sandboxed app gets an id from its sandbox; a normal app on the host has none,
   and since xdg-desktop-portal 1.20 it is expected to say who it is. From 1.21
   the GlobalShortcuts portal refuses a session without one, which is the
   `org.freedesktop.portal.Error.NotAllowed: An app id is required` a current
   KDE session reports.

   This is the **first** thing VoxCtrl does, before any portal proxy exists. The
   declaration is allowed once per D-Bus connection and only before the first
   portal call on it, and ashpd shares one connection across every portal it
   opens — so anything that touches a portal first spends that one chance.
   Portals older than 1.20 do not serve this interface, and its absence is not
   an error.

   `ashpd` is built with its `async-io` feature, deliberately not `tokio`.
   Enabling `tokio` selects `zbus`'s Tokio-backed reactor for the *entire*
   zbus 5.x build in the dependency graph — including
   `tauri-plugin-single-instance`'s own use of `zbus::blocking` during plugin
   setup, which runs inside Tauri's ambient Tokio runtime. zbus's Tokio-backed
   blocking facade builds a brand-new multi-thread runtime internally and
   panics with "Cannot start a runtime from within a runtime" the moment that
   happens on a thread already driving one. zbus's default `async-io` reactor
   runs its own background poller thread independent of whatever executor
   polls the outer future, so it mixes safely with Tokio and never touches the
   plugin. Do not add `ashpd`'s `tokio` feature back without checking this.
1. It opens a portal session.
2. It registers its shortcuts, one per distinct key combination, with the keys you configured as a *preferred* trigger.
3. Your desktop decides what to actually bind — it may confirm with you, and it may assign different keys. Whatever it decides wins, and VoxCtrl displays the result.
4. From then on your desktop sends `Activated` when the shortcut is pressed and `Deactivated` when it is released. That is the entire data flow.

**No permissions are required.** There is nothing to install, no group to join, no rule to write, and no logout.

Supported by KDE Plasma (5.27+), GNOME 48+, and Hyprland (via `xdg-desktop-portal-hyprland`). Compositors that do not implement the interface — Sway and most other wlroots compositors as of writing — fall through to the section below.

#### What can be a shortcut

An accelerator is **any number of modifiers plus exactly one regular key**. `Super+Space`, `Ctrl+Alt+D` and `F5` are all fine.

Two shapes are not accelerators, and no desktop can bind them:

- **Modifiers alone** — a lone Super, or Ctrl+Shift with nothing else. This is the one that catches people out, because "double-tap Super" feels like a perfectly ordinary hotkey.
- **Two regular keys** — `A+B`. Use modifiers for everything but the last key.

The key recorder in Settings → Hotkeys **refuses these while you are recording** and says why, rather than saving something that silently never fires. It nudges you as soon as you are holding modifiers with no regular key yet, so you can add one without lifting your fingers, and it shows the accelerator your desktop will receive (`LOGO+space`) once the combination is valid. A refused capture leaves your existing shortcut untouched.

The rule is defined once, in `crates/voxctrl-hotkeys/src/trigger.rs`, and the settings UI validates against it over IPC — so what the recorder accepts and what the portal can register cannot drift apart.

On the X11 and evdev backends and on Windows, VoxCtrl watches the keys itself and a bare modifier genuinely works. There the recorder accepts it and shows a note that it will stop working if shortcut delivery ever moves to the portal, rather than blocking something that works on your machine today.

#### Bare Escape and the exclusive grab

A registered global shortcut is an **exclusive grab**: the compositor routes that key to VoxCtrl and to nothing else. For a combination you chose for VoxCtrl that is the point. For bare Escape it is not — Escape is how every program on your machine says "never mind", and it is the default `tts.stop_key`, so a user who never picked it would lose it desktop-wide: an open menu would stop closing, a dialog would stop cancelling, for as long as VoxCtrl ran.

Re-emitting the key does not fix this, and VoxCtrl does not try. A synthetic press enters the same input pipeline the grab sits on, so the compositor intercepts it again — the key never reaches the focused app, and VoxCtrl's own shortcut fires in a loop instead. Injecting *below* the compositor means uinput, which needs exactly the keyboard access VoxCtrl refuses to arrange (see [Why this changed](#why-this-changed)), and Wayland gives one client no way to deliver a key to another at all.

What VoxCtrl does instead is hold the grab **only while it is speaking**. `src-tauri/src/stop_key.rs` decides:

| Situation | Grab |
| --- | --- |
| No stop key configured | `None` — nothing is registered |
| Stop key has a modifier (`Ctrl+Escape`) | `Always` — nothing else is listening for it |
| Backend watches the keys itself (X11, evdev, Windows) | `Always` — nothing is grabbed, every app still receives Escape |
| Bare Escape, desktop owns the grab (portal, Mint, still starting) | `WhileSpeaking` |

Under `WhileSpeaking` the binding is added to the listener when playback starts and dropped again two seconds after it ends — long enough that the gaps inside one spoken response do not each cost a re-registration, short enough that Escape is yours again almost immediately. The arbiter never re-registers while a dictation gesture is active, because a reload restarts the listener's gesture engine and would drop the recording.

**The transient shortcut lives in its own portal session.** A portal session accepts `BindShortcuts` exactly once, so changing what is registered means a new session — and the first version of this arming did that to the *one* session that also held the dictation shortcuts. Re-registering those ids under a second session and then closing the first left the compositor firing none of them: after the first cancelled playback, the user's own keybinds were dead. So the standing shortcuts and the transient one are now bound on separate sessions. Arming and releasing the stop key creates and closes only its own session, whose id no other session ever registers, and the session holding the dictation shortcuts is rebuilt only when the user actually edits a binding.

Only VoxCtrl's own `__tts_stop__` binding is eligible for that treatment. A user who binds *dictation* to bare Escape shares the group with it, and that group stays standing — a shortcut that worked only while VoxCtrl happened to be speaking would be worse than the grab the recorder warned them about.

Two consequences worth knowing:

- **The first fraction of a second of playback may not be interruptible** on the portal backend, because the grab is established as audio starts. Consecutive utterances stay armed, so this is only ever the first one after a quiet stretch.
- **KDE's shortcut store is left out of it.** The KDE housekeeping — pruning ids VoxCtrl no longer registers, syncing display names into `~/.config/kglobalshortcutsrc` — runs only when the standing set changes, never on an arm or release: nothing the user configured has changed, and rewriting their shortcut store every time VoxCtrl speaks would be pure churn. A transiently-bound id is also exempt from pruning, because KDE keys your "enabled" tick to that id and dropping it would make the next arm register a fresh, disabled shortcut ([bugs.kde.org #483639](https://bugs.kde.org/show_bug.cgi?id=483639)) that never fires.

If any of this goes wrong on a particular compositor, `VOXCTRL_STOP_KEY_ARMING=0` stands the whole mechanism down: a stop key that would need arming is simply never registered (use `Ctrl+Escape` instead), and no session is created or closed while the app runs.

A *dictation* binding on bare Escape is a different matter: those are held for the whole session, so the recorder accepts one and warns you what it costs, rather than silently taking Escape from your desktop.

#### If the portal refuses the session

A refusal is not the same as a missing portal, and the app says which it hit.
"Your desktop has a global-shortcuts portal but refused VoxCtrl's request"
means the interface is there and answered — switching desktops will not help.
The message distinguishes the two app-id cases, because they need different
answers:

- *"VoxCtrl could not declare an application id to this desktop: …"* — the
  declaration itself failed, and the reason is quoted. Usually an
  `xdg-desktop-portal` too old to serve the registry while new enough to demand
  an id.
- *"…declared itself as `ai.voxctrl.app` and the desktop accepted the
  declaration, then still refused the session"* — the handshake worked and the
  portal still said no, which points at a portal bug rather than anything you
  configured.

The exact D-Bus error is shown verbatim under **Portal reported:** in the setup
window either way.

#### KDE registers shortcuts disabled by default

On KDE Plasma, `xdg-desktop-portal-kde` accepts VoxCtrl's `BindShortcuts`
request and lists the shortcuts in System Settings → Shortcuts, but leaves
them **unticked**. Nothing delivers until you open that panel, check the box
next to each VoxCtrl shortcut, and press Apply. This is a confirmed upstream
bug — [bugs.kde.org #483639](https://bugs.kde.org/show_bug.cgi?id=483639) —
not something wrong with your configuration, and it is the most common reason
a freshly-set-up shortcut on KDE does nothing.

There is no D-Bus API that reports whether a registered shortcut is ticked, so
VoxCtrl cannot detect completion of this step or complete it on your behalf —
doing either would require an interface the portal does not expose. What it
does instead: when the backend is the portal and the desktop is KDE, the
Hotkeys tab and the setup window show a standing notice explaining the step,
with an **Open Shortcut Settings** button that launches
`kcmshell6`/`kcmshell5 kcm_keys` (falling back to `systemsettings(6)`)
directly to the right panel. This notice does not block `is_complete` or
`hotkeys_active` — the portal gives no way to tell whether you have already
done it, so treating it as a hard requirement would leave the app reporting
"incomplete" forever for KDE users who already ticked the boxes.

#### Bindings from older versions

A binding saved before this rule existed is not deleted and not silently broken. It is flagged **needs a regular key** in the Hotkeys tab, and VoxCtrl still registers it with the portal without a preferred trigger — which asks your desktop to let you pick the keys in its own settings. Editing the binding and choosing a valid combination is the clean fix.

If your desktop refuses a shortcut for any other reason, the binding is flagged **not bound by your desktop**.

### Linux — X11 raw key events (XInput2)

The backend for X11 desktops that serve no portal and are not getting one —
Cinnamon, MATE, Xfce, and most wlroots compositors' Xwayland-less X11 cousins.

VoxCtrl selects `XI_RawKeyPress` / `XI_RawKeyRelease` on the root window through
XInput2. Raw events are delivered independently of which window has focus, which
is what makes them usable as global shortcuts, and **any X client may ask for
them** — there is no group to join, no udev rule, and nothing for VoxCtrl to
change about the machine. That is what makes this usable where the evdev
fallback can only report that it is locked out.

It carries the same privacy cost as evdev: raw events are every key you press,
not only VoxCtrl's own shortcuts. So it ranks below the portal, and the app says
which one it is running. It also sees presses *and* releases, so every gesture
style works here, including bare-modifier triggers like double-tapping Super
that no accelerator-based backend can express.

X11 keycodes are evdev codes offset by 8, and key names come from the `evdev`
crate rather than a table of VoxCtrl's own — so a binding recorded under one
backend keeps working under the other. Synthetic devices (`XTEST`, `uinput`,
anything named "virtual") are filtered by `sourceid`, so VoxCtrl never reads
back the transcription it types out.

**The XInput version VoxCtrl announces is load-bearing, and 2.1 is the floor.**
The server applies whichever semantics the client asks for, and XI 2.0 has two
that make this backend unusable:

- A raw event under 2.0 goes to the root window *or* to the grabbing client,
  never both. Grabs are everywhere on a desktop — a compositor's own key
  handling, and a **passive grab every time a menu or dropdown opens**. On
  Cinnamon that means a hotkey pressed with any menu on screen is silently
  dropped. XI 2.1 delivers raw events to the root window at all times,
  whatever holds a grab.
- `sourceid` is only set on raw events from 2.1. Without it every event claims
  to come from device 0, the XTEST filter matches nothing, and VoxCtrl reads
  back the text it just injected — retriggering the hotkey inside its own
  output.

A server too old to offer 2.1 (pre-X.Org 1.11, 2011) falls through to evdev
rather than being handed a backend that mishandles its own keystrokes.
`supports_reliable_raw_events` is the single check, with a test pinning 2.0 as
refused.

To disable this path for testing, set `VOXCTRL_DISABLE_X11_HOTKEYS=1`.

### Linux Mint (Cinnamon / MATE) — Native D-Bus Shortcut Integration

Cinnamon and MATE implement no XDG `GlobalShortcuts` portal. On an X11 session
the X11 backend above covers them completely; this route is for the case where
even that is unavailable — a Wayland Cinnamon session, or an X server without
XInput2.

1. VoxCtrl registers a native session D-Bus interface (`ai.voxctrl.Dictation`).
2. Saving bindings mirrors every `toggle` binding into a custom keybinding under
   `org.cinnamon.desktop.keybindings` or
   `org.mate.SettingsDaemon.plugins.media-keys`, each running:
   ```bash
   dbus-send --session --dest=ai.voxctrl.Dictation --type=method_call \
     /ai/voxctrl/Dictation ai.voxctrl.Dictation.ToggleBinding string:'<binding id>'
   ```
   The shortcut names the binding it stands for, so the transcription reaches
   that binding's own targets rather than a global default.
3. Zero special permissions are required, and VoxCtrl reads no keystrokes.

Four things about this path are easy to get wrong, and all four have bitten it:

- **The method name is PascalCase.** `zbus` publishes `toggle_binding` as
  `ToggleBinding`. A command naming the Rust spelling invokes nothing at all:
  registration looks perfectly healthy and the shortcut does nothing.
- **Child keys are written before the slot joins `custom-list`.** The settings
  daemon reacts to the list changing by reading the entry it names; an entry it
  reads first and finds empty stays unbound, and nothing re-notifies it.
- **Slots are numbered `customN` and allocated around the user's own.**
  Cinnamon's Keyboard applet enumerates `customN` and lists nothing else, so a
  shortcut registered under a name of VoxCtrl's own would be invisible and
  unfixable in System Settings — and writing `custom0` blind would overwrite
  whatever the user already had there.
- **`gsettings` needs the host's environment.** Inside the AppImage, linuxdeploy's
  GTK hook exports `GSETTINGS_SCHEMA_DIR` into the AppDir, so an inherited
  environment sends `gsettings` looking for Cinnamon's schemas in the bundle,
  where they are not. It then reports that this desktop has no keybinding
  support, which is indistinguishable from a desktop that genuinely has none.
  `src-tauri/src/host_env.rs` strips the bundle back out for host programs.

Because a custom keybinding fires on key-**press** and reports nothing on
release, this backend advertises `toggle` alone (see
[Gesture styles and the backend](#gesture-styles-and-the-backend)); hold and
double-tap styles are not offered while it is running.

### Linux — evdev fallback

Only used when neither the portal nor the X11 backend is available, **and only if your system already allows this process to read input devices**. VoxCtrl never grants itself that access.

In this mode VoxCtrl reads `/dev/input/event*` directly, which means every keystroke on the system passes through the process. Nothing is logged, stored, or transmitted — key names live briefly in memory and never cross into the UI layer or any network call — but the app is honest about it rather than quiet: the Hotkeys tab and setup window both say so in plain language.

A specific keyboard can be pinned with `audio.evdev_device` (e.g. `"/dev/input/event4"`); otherwise every eligible keyboard is read. Synthetic devices (`uinput`, `XTEST`, anything named "virtual") are always skipped so VoxCtrl cannot react to the keystrokes it injects itself.

To disable the portal path for testing, set `VOXCTRL_DISABLE_PORTAL_HOTKEYS=1`.

### Windows

Uses the Win32 `SetWindowsHookEx` / `WH_KEYBOARD_LL` low-level keyboard hook via `rdev`. No special permissions are required. Like the evdev fallback, this hook sees all keystrokes; that is the only mechanism Windows offers for this kind of shortcut.

---

## Why this changed

Reading `/dev/input/event*` requires a udev rule that tags input devices with `uaccess` (or membership of the `input` group). VoxCtrl used to install one during setup. That rule is not narrow:

```
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"
```

It grants **every process running as you** the ability to read **every keystroke on the system** — a compromised npm postinstall script, any Electron app, any shell one-liner. systemd's own defaults (`/usr/lib/udev/rules.d/70-uaccess.rules`) deliberately grant `uaccess` on input devices to joysticks and nothing else, precisely to prevent this.

A dictation app should not be the reason your machine's security posture changes, and it certainly should not make that change silently during a first-run wizard. So:

- **VoxCtrl never writes that rule, and never runs `usermod -aG input`.** Not at install, not on first launch, not from any button in the UI.
- If your desktop has no shortcuts portal, VoxCtrl **tells you at launch** and explains the trade-off, rather than quietly widening access.
- On a machine that still has the rule from an older version, `install.sh` and `scripts/uninstall.sh` **remove** it, so upgrading narrows access rather than leaving it wide open. To do it by hand: delete `/etc/udev/rules.d/99-voxctrl.rules`, reload udev, and drop yourself from the `input` group with `sudo gpasswd -d $USER input`.

Administrator rights are still requested for one thing: installing host packages such as `wtype` and `xdotool`, which are what type the transcription into your focused window. That step touches no permissions.

---

## Gesture Recognition

Each binding specifies a `gesture` that controls when recording starts and stops. Recognition is shared by every backend, so gestures behave identically on the portal, on evdev, and on Windows.

### `hold`
```
Trigger down ──► (after hold_threshold_ms) START RECORDING
Trigger up   ──► STOP RECORDING
```
Most natural for short dictations. Recording lasts exactly as long as the key is held. `hold_threshold_ms` (default 200ms) is the minimum press duration, which stops an accidental brush from starting a recording.

For a multi-key combo, recording stops only once **every** key is released. Stopping on the first key-up would inject text while a modifier was still physically down, and the compositor would swallow it as a shortcut instead of typing it.

### `toggle`
```
Press once ──► START RECORDING
Press again ──► STOP RECORDING
```
For longer dictations where holding a key would be tiring.

### `double_tap`
```
Tap, tap ──► START RECORDING
Tap, tap ──► STOP RECORDING
```
Two taps within `tap_ms` (default 300ms) of each other. **Recording starts on the second press**, not on its release — the gesture is unambiguous the moment the second press lands, and waiting any longer is latency you would feel.

The one exception: if a `double_tap_hold` binding is configured on the *same keys*, the two gestures are indistinguishable until you either release quickly (a tap) or keep holding (a hold). In that case only, the tap resolves on the release.

### `double_tap_hold`
```
Tap, then press and hold ──► (after hold_threshold_ms) START RECORDING
Release                  ──► STOP RECORDING
```
Recording runs while you keep the key down after a double-tap. `hold_threshold_ms` on the second press is what distinguishes this from a plain `double_tap`.

Releasing the trigger **always** stops the recording, whatever else is going on. A two-minute safety timeout is a last-resort backstop for a release that never arrives at all (a keyboard unplugged mid-gesture, a portal session that dies) — it should never be what ends a normal dictation.

#### Tuning double-tap gestures

- **`tap_ms`** (default 300) — the longest gap between releasing the first tap and pressing the second. Raise it if double-taps get missed; lower it if they fire when you did not mean them to.
- Taps closer together than 15ms are treated as a duplicated event rather than a gesture, and become the first tap of a fresh pair instead of being discarded.
- A first tap held longer than 600ms is not a tap at all, and does not prime the gesture. This is what stops ordinary use of a bound modifier — holding Super for a moment while doing something else — from turning the next press into a phantom double-tap.

---

## Key Names

Keys use **evdev event code names** everywhere in the config (`KEY_LEFTMETA`, `KEY_SPACE`). On Windows they are mapped to Virtual Key codes internally; on the portal they are translated to the XDG shortcuts accelerator syntax (`LOGO+space`, `CTRL+ALT+d`).

### Modifier Keys
| Name | Key |
|---|---|
| `KEY_LEFTCTRL` | Left Ctrl |
| `KEY_RIGHTCTRL` | Right Ctrl |
| `KEY_LEFTSHIFT` | Left Shift |
| `KEY_RIGHTSHIFT` | Right Shift |
| `KEY_LEFTALT` | Left Alt |
| `KEY_RIGHTALT` | Right Alt / AltGr |
| `KEY_LEFTMETA` | Left Super / Windows key |
| `KEY_RIGHTMETA` | Right Super / Windows key |
| `KEY_CAPSLOCK` | Caps Lock |

Left and right variants of a modifier are the same accelerator to the portal — your desktop does not distinguish them.

### Common Keys
| Name | Key |
|---|---|
| `KEY_SPACE` | Space |
| `KEY_ENTER` | Enter |
| `KEY_TAB` | Tab |
| `KEY_ESC` | Escape |
| `KEY_BACKSPACE` | Backspace |
| `KEY_F1`–`KEY_F12` | Function keys |
| `KEY_A`–`KEY_Z` | Letter keys |
| `KEY_0`–`KEY_9` | Number row |

The easiest way to set a binding is the recorder in Settings → Hotkeys: focus the capture box and press the combination. That capture happens inside VoxCtrl's own focused window using ordinary browser key events — it is not a global listener.

---

## Multi-Key Combos

`keys` is an array. All keys must be pressed for the gesture to activate; order does not matter.

```toml
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
```

When two bindings overlap, the longer one wins: holding `Ctrl+Super+Space` does not also fire a `Super+Space` binding. Shadowing is resolved when a key goes *down*, so releasing Ctrl part-way through a gesture cannot start a second recording.

### Staggered Key Release Tolerance (50ms Grace Window)

For `hold` and `double_tap_hold` gestures, VoxCtrl incorporates a 50ms release grace timer. When a multi-key shortcut (e.g. `Super+Space` or `Ctrl+Alt+Space`) is released, fingers rarely lift off every key at the exact microsecond. 

- When the first key in a combination comes up, the combo deactivates and a 50ms grace window starts.
- If the remaining modifier keys come up within 50ms, recording stops immediately upon their release.
- If a modifier key-up event is delayed, swallowed by the OS/compositor, or held down by a resting finger past 50ms, the grace timer automatically fires and stops recording cleanly.
- This ensures the recording session never gets stuck open while avoiding accidental recording cutoffs during rapid key releases.

---

## Hot-Reload

Bindings update at runtime without restarting the listener. When they are saved via the UI or the `save_bindings` IPC command:

1. New bindings are written to `bindings.toml`.
2. They are sent through the `hotkey_reloader` crossbeam channel.
3. The listener stops any gesture still in flight, swaps its binding table, and — on the portal backend — re-registers the shortcuts with your desktop.

---

## GestureEvent

The output of the hotkey system is a stream of `GestureEvent` values on the `gesture_tx` channel:

```rust
pub struct GestureEvent {
    pub binding_id: String,
    pub binding_label: String,
    pub target_id: String,  // comma-joined for multi-target
    pub kind: GestureKind,
}

pub enum GestureKind {
    Start,  // Begin recording
    Stop,   // End recording
}
```

Note what is *not* in it: no key names, no timestamps of individual presses, nothing about what you typed. This is the only hotkey data that reaches the rest of the app, and it is all the UI layer ever sees.

`lib.rs` receives these and coordinates the audio recorder and inference pipeline.

---

## Conflict Handling

Two enabled bindings with the same keys **and** the same gesture are a conflict, and the Hotkeys tab flags them. Disable one.

`double_tap` and `double_tap_hold` on the same keys are **not** a conflict — they are a supported pairing, and the app keeps them straight (a quick double-tap runs one, double-tap-and-hold runs the other). On the portal backend they are registered as a single system shortcut so your desktop is never asked to bind the same keys twice.

```toml
[[binding]]
id = "old_binding"
disabled = true
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
```

---

## Migrating from `chord`

The `chord` gesture (hold base keys, press a subkey to start) has been removed. It could not be expressed as a system shortcut — the portal has no concept of a partially-held combo — and it was the least-used gesture.

Existing `chord` bindings keep working: on load they are converted to `hold` using the keys already in `keys`, and the obsolete `subkey` field is ignored and dropped the next time bindings are saved. Nothing needs to be edited by hand.
