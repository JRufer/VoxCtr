# Testing VoxCtrl on Windows

Thanks for helping test this. VoxCtrl is a voice dictation app: you hold a
shortcut, speak, and what you said is typed into whatever window you were using.
Everything runs on your own machine — no audio or text leaves your computer.

**Windows support is brand new.** The app has run on Linux for a while; this is
the first Windows release, so the parts that touch Windows directly — the
keyboard shortcut, typing the text, the on-screen overlay — are the parts most
likely to misbehave. That is what would be most useful to hear about.

This should take about fifteen minutes.

---

## 1. Install

1. Download the installer — about 18 MB:
   **[VoxCtrl_0.5.0_x64-setup-windows-x86_64.exe](https://github.com/JRufer/VoxCtrl/releases/download/v0.5.0/VoxCtrl_0.5.0_x64-setup-windows-x86_64.exe)**
   (from the [v0.5.0 release page](https://github.com/JRufer/VoxCtrl/releases/tag/v0.5.0),
   if you'd rather see everything).
2. Run it. Windows will show a blue **"Windows protected your PC"** box, because
   the installer is not yet signed with a certificate. Click **More info**, then
   **Run anyway**.
3. Follow the installer, then launch VoxCtrl.

Windows 10 (version 21H2 or newer) or Windows 11. Nothing else to install first.

## 2. First run

A setup window walks you through choosing a speech model, picking a shortcut,
and doing a test dictation. The model download is a few hundred megabytes, so
give it a minute.

**If dictation produces nothing at all, check the microphone first.** Windows
denies microphone access silently, with no prompt and no error. Open
**Settings → Privacy & security → Microphone** and make sure "Let desktop apps
access your microphone" is on. This catches most people once.

VoxCtrl keeps running in the system tray after you close its window. Right-click
the tray icon for Settings, or to quit.

---

## 3. What would be most useful to try

The default shortcut is **hold Windows key + Space**, speak, then let go.

**a. Does dictation work at all?** Open Notepad, hold the shortcut, say a
sentence, release. The text should appear where your cursor is.

**b. Try punctuation and symbols.** This is the single most valuable test — the
old Windows code mangled exactly this, and the fix is new. Dictate something
like:

> "fifty percent of users, open paren a plus b close paren, and array bracket
> zero"

Anything with `% ( ) + [ ] { } ^ ~` in the result is worth checking character by
character. If any of those come out missing, doubled, or turned into something
else, that's a real bug — please report it with the exact text you got.

**c. Try it in a few different apps.** Notepad, a web browser, VS Code, Windows
Terminal, Word. Some apps accept synthetic typing differently.

**d. Watch the overlay.** A small floating panel should appear while you speak
and fade out after. Things to notice:
- Does a **black console window** flash up or sit next to it? (It shouldn't.)
- Does the overlay **steal focus**, so your text ends up in the wrong place?
- On a multi-monitor or scaled display, does it appear in a sensible spot?

**e. Try the other gesture styles.** In Settings → Hotkeys you can set a
shortcut to toggle on/off, or double-tap, instead of hold. All four styles
should work.

**f. Long dictation.** Say a paragraph or two without stopping. Above roughly
2000 characters the app switches to pasting instead of typing; it should hand
your clipboard back afterwards.

---

## 4. If something goes wrong — this is all I need

VoxCtrl writes a small log file. **Sending me that file after reproducing the
problem is the single most useful thing you can do.**

To get a clean, short log:

1. **Quit VoxCtrl** (right-click the tray icon → Quit).
2. Press **Windows key + R**, paste this, and press Enter:
   ```
   %LOCALAPPDATA%\voxctrl
   ```
   An Explorer window opens.
3. Delete **`startup_errors.log`** if it's there.
4. **Start VoxCtrl and make the problem happen again.**
5. Quit VoxCtrl, then send me the new **`startup_errors.log`** from that folder.

That gives a log containing only the run where things went wrong, which is far
easier to read than months of history.

**What's in the log:** what VoxCtrl was doing, which shortcut backend it's using,
which speech engine loaded, and any warnings or errors.

**What is deliberately *not* in it:** anything you dictated. The app filters out
transcription text before writing, so the log is safe to send as-is.

Along with the log, it helps to say:

- **What you did**, and **what you expected instead**.
- **Which app** you were dictating into.
- **Your Windows version and GPU** — press Windows key + R, type `winver` for
  the Windows version.

### Please don't send your settings folder

`%APPDATA%\voxctrl` holds `config.json` and `targets.toml`. Those can contain
**API keys and access tokens** if you've set any up. I don't need them, and you
shouldn't share them. The log file above is enough.

---

## 5. Known limitations — not bugs, no need to report

These are all expected in this release:

- **Elevated apps.** If a program is running as administrator, Windows blocks
  VoxCtrl from both receiving shortcuts and typing into it. Task Manager and
  most installers behave this way. Nothing can be done unless VoxCtrl is also
  run as administrator.
- **The UAC prompt and the lock screen.** Windows hides all keyboard input from
  apps there, so shortcuts won't fire.
- **SmartScreen's "unknown publisher" warning.** Code signing isn't set up yet.
- **Speech runs on the CPU.** GPU acceleration for Windows is in progress. On a
  modern machine the smaller models are still comfortably fast.
- **Piper text-to-speech needs manual setup.** If you want to try the app
  speaking back to you, pick **Pocket-TTS** in Settings → Text-to-Speech
  instead — it works out of the box. Piper is the default but can't install
  itself on Windows yet, so it will tell you so rather than failing quietly.
- **Right-hand modifier keys.** A shortcut recorded with left Ctrl won't fire
  from right Ctrl. This matches how it behaves on Linux.

---

## 6. Anything else

Rough edges, confusing wording, anything that made you hesitate — all of it is
worth mentioning. A first Windows release is exactly when that feedback is
cheapest to act on.

Thank you.
