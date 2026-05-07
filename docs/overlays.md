# Overlay UI Guide

VoxCtl displays a visual overlay while the microphone is active. The overlay system is fully swappable: you can choose from the built-in styles or create your own by dropping a single Python file into a special folder.

---

## Built-in Styles

Three styles are included out of the box.

### Voice Card *(default)*

A floating card inspired by terminal-style audio monitors.

- Scrolling bar history: new bars arrive from the right, old bars slide left
- Symmetric bars (mirrored top and bottom from a centre line)
- Horizontal gradient: dim purple on the left (oldest audio) through magenta to bright pink-white on the right (most recent audio)
- "Voice Activity" label and a routing indicator badge displayed in the card header

### Waveform

An OpenGL oscilloscope that renders the raw audio signal as a min/max envelope. Fast and lightweight.

- Dark rounded box anchored to the bottom-centre of the screen
- Updates in real time with each audio chunk

### Pulse Circle

A soft glowing circle that expands and contracts with the RMS amplitude of your voice.

- Smooth 30 fps animation with exponential decay
- Blue glow rings fade out during silence

---

## Selecting an Overlay

1. Open **⚙ Settings** from the system tray icon.
2. Go to the **✨ Dictation** tab.
3. In the **Overlay Appearance** section, pick a style from the dropdown.
4. Click **Save Settings**.

The overlay switches instantly — no restart is required. Custom overlays you add to the overlays folder appear in the same dropdown automatically after the next save.

---

## Creating a Custom Overlay

### Where to put the file

Place a `.py` file in:

```
~/.config/voxctl/overlays/
```

Click **"Open Overlays Folder"** in Settings to open this directory in your file manager. On first open, a `_template.py` starter file is written there for you.

> Files whose names begin with `_` (e.g. `_template.py`) are ignored by the loader — use this convention for notes, drafts, or helper modules.

### Required structure

Your file must contain:

| Item | Type | Required | Description |
|------|------|----------|-------------|
| `DISPLAY_NAME` | `str` | Yes | Name shown in the Settings dropdown |
| `DESCRIPTION` | `str` | No | One-line description shown below the dropdown |
| `VERSION` | `str` | No | Version string (informational only) |
| `class OverlayUI(QWidget)` | class | Yes | The overlay widget itself |

`OverlayUI` must implement these three methods:

```python
def update_audio(self, data: numpy.ndarray) -> None: ...
def show_mode(self, label: str = "") -> None: ...
def hide_mode(self) -> None: ...
```

### The three methods in detail

#### `update_audio(data)`

Called from the audio thread approximately every 20–60 ms while recording is active.

| Parameter | Type | Details |
|-----------|------|---------|
| `data` | `numpy.ndarray` | `dtype=float32`, typically 1024 samples per call, amplitude range roughly ±32 768 (signed 16-bit scale) |

Because this is called from a background thread, **do not call Qt drawing methods directly here**. Store the data and schedule a repaint using:

```python
QMetaObject.invokeMethod(self, "update", Qt.ConnectionType.QueuedConnection)
```

#### `show_mode(label="")`

Called on the Qt main thread when the user starts recording. `label` is the human-readable name of the active output target (from `targets.toml`) and is used by built-in overlays to display the routing indicator badge. You can use it or ignore it. Your overlay should cover the full screen so it is visible over any application. The standard implementation:

```python
def show_mode(self, label: str = ""):
    self._routing_label = label   # store for use in paintEvent
    screen = QApplication.primaryScreen()
    if screen:
        g = screen.geometry()
        self.setGeometry(g)
        self.setFixedSize(g.width(), g.height())
        self.move(g.x(), g.y())
    self.show()
    self.raise_()
```

#### `hide_mode()`

Called on the Qt main thread when recording stops.

```python
def hide_mode(self):
    self.hide()
```

### Window flags for a transparent, click-through overlay

All built-in overlays use these flags. Copy them into your `__init__`:

```python
self.setWindowFlags(
    Qt.WindowType.ToolTip |
    Qt.WindowType.FramelessWindowHint |
    Qt.WindowType.WindowStaysOnTopHint |
    Qt.WindowType.X11BypassWindowManagerHint |
    Qt.WindowType.WindowTransparentForInput   # click-through
)
self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
```

---

## Full Template

This is the same file written to `_template.py` when you first open the overlays folder. Copy it, rename it (without a leading `_`), and customise freely.

```python
# Save as  ~/.config/voxctl/overlays/my_overlay.py
# Then select "My Custom Overlay" in Settings → Dictation → Overlay Appearance.

DISPLAY_NAME = "My Custom Overlay"
DESCRIPTION  = "Describe what your overlay looks like"
VERSION      = "1.0"

import numpy as np
from PyQt6.QtWidgets import QWidget, QApplication
from PyQt6.QtCore import Qt, QMetaObject, QTimer
from PyQt6.QtGui import QPainter, QColor, QBrush


class OverlayUI(QWidget):

    def __init__(self):
        super().__init__()
        self.setWindowFlags(
            Qt.WindowType.ToolTip |
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.X11BypassWindowManagerHint |
            Qt.WindowType.WindowTransparentForInput
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setFixedSize(800, 100)
        self._amp = 0.0
        self.hide()

        # Drive smooth animations at 30 fps
        self._timer = QTimer(self)
        self._timer.timeout.connect(self.update)
        self._timer.start(30)

    # ------------------------------------------------------------------ #
    #  Required interface                                                  #
    # ------------------------------------------------------------------ #

    def update_audio(self, data):
        """
        Called from the audio thread ~every 20-60 ms.
        data: numpy.ndarray float32, ~1024 samples, amplitude range ±32768.
        Store data here; schedule repaints with invokeMethod — never draw directly.
        """
        rms = float(np.sqrt(np.mean(data.astype(float) ** 2)))
        self._amp = min(1.0, rms / 8192.0)
        QMetaObject.invokeMethod(self, "update", Qt.ConnectionType.QueuedConnection)

    def show_mode(self, label: str = ""):
        """Called when recording starts. label is the active target name (routing badge)."""
        self._routing_label = label
        screen = QApplication.primaryScreen()
        if screen:
            g = screen.geometry()
            self.setGeometry(g)
            self.setFixedSize(g.width(), g.height())
            self.move(g.x(), g.y())
        self.show()
        self.raise_()

    def hide_mode(self):
        """Called when recording stops."""
        self.hide()

    # ------------------------------------------------------------------ #
    #  Custom drawing — replace this with your own design                 #
    # ------------------------------------------------------------------ #

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        # Example: a simple circle that grows with amplitude
        cx = self.width() // 2
        cy = self.height() - 65
        r  = int(20 + self._amp * 25)

        painter.setBrush(QBrush(QColor(74, 158, 255, 180)))
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawEllipse(cx - r, cy - r, r * 2, r * 2)
```

---

## Tips

**Positioning your widget**

The widget is expanded to full-screen size in `show_mode(label)`, but your visual element only needs to occupy a small part of it. Draw everything relative to `self.width()` and `self.height()` so it adapts to any screen resolution:

```python
cx = self.width() // 2           # horizontal centre
cy = self.height() - 80          # near the bottom
```

**Smooth animations**

Start a `QTimer` in `__init__` that calls `self.update()` at your desired frame rate. 30 ms (≈ 33 fps) is a good default that stays light on CPU:

```python
self._timer = QTimer(self)
self._timer.timeout.connect(self.update)
self._timer.start(30)
```

**Exponential smoothing**

Raw RMS values jump around. A simple one-liner gives smooth motion:

```python
self._smooth = self._smooth * 0.75 + new_value * 0.25
```

**Scrolling history**

Use `collections.deque` with a `maxlen` to keep a fixed-length rolling buffer of amplitude values. Appending to the right automatically evicts the oldest entry from the left — no manual shifting required:

```python
from collections import deque
self._history = deque([0.0] * 80, maxlen=80)

# In update_audio:
self._history.append(new_amp)
```

**Importing app modules**

The app's `src/` directory is temporarily added to `sys.path` when your overlay is loaded, so you can import from the app if needed:

```python
from gui.overlays.base import OverlayUIBase   # optional base class
```

Inheriting from `OverlayUIBase` is optional — plain `QWidget` works just as well. The base class is there for editors that want autocomplete on the interface methods.

---

## How the Loader Works

At startup (and again after each Settings save), `OverlayManager` scans:

1. `src/gui/overlays/` — built-in overlays, always available
2. `~/.config/voxctl/overlays/` — your custom overlays

For each `.py` file (excluding `_`-prefixed files), it imports the module and looks for:
- `DISPLAY_NAME` string
- `OverlayUI` class

If both are present the overlay is registered and appears in the Settings dropdown. Load errors are printed to the terminal but do not crash the app — the previously-active overlay continues running.

The active overlay is wrapped in an `OverlayProxy`. All wiring in the main app (audio callbacks, recording signals) points at the proxy, so swapping the underlying widget at runtime requires no rewiring.
