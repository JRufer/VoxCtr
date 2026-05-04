"""
Discovers, loads, and manages swappable recording overlay UIs.

Built-in overlays live in  src/gui/overlays/*.py
User overlays live in      ~/.config/whisper-wayland/overlays/*.py

Each overlay file must expose:
    DISPLAY_NAME : str        – human-readable name shown in Settings
    DESCRIPTION  : str        – one-line description (optional)
    OverlayUI    : class      – QWidget subclass with update_audio / show_mode / hide_mode
"""

import os
import sys
import importlib.util
from pathlib import Path

_BUILTIN_DIR = Path(__file__).parent / "overlays"
_USER_DIR    = Path.home() / ".config" / "whisper-wayland" / "overlays"

# Names of built-in overlay modules, in the order they appear in Settings.
_BUILTIN_NAMES = ["waveform", "pulse", "voice_card"]

# Template written to the user overlay directory on first open so users have a
# ready-made starting point.
_TEMPLATE_CONTENT = '''\
# Custom overlay template for Whisper-Wayland
# Place this file (or a copy) in:  ~/.config/whisper-wayland/overlays/
#
# Required:
#   DISPLAY_NAME   – name shown in the Settings overlay picker
#   class OverlayUI(QWidget)  with the three methods below
#
# The app injects audio data via update_audio() ~every 20 ms while recording.

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

        # Optional: drive smooth animations
        self._timer = QTimer(self)
        self._timer.timeout.connect(self.update)
        self._timer.start(30)

    # ------------------------------------------------------------------ #
    #  Required interface                                                  #
    # ------------------------------------------------------------------ #

    def update_audio(self, data):
        """Called from audio thread.
        data: numpy.ndarray float32, ~1024 samples, amplitude range +-32768
        """
        rms = float(np.sqrt(np.mean(data.astype(float) ** 2)))
        self._amp = min(1.0, rms / 8192.0)
        # Thread-safe repaint request:
        QMetaObject.invokeMethod(self, "update", Qt.ConnectionType.QueuedConnection)

    def show_mode(self):
        """Called when recording starts.  Expand to full screen."""
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
    #  Custom drawing                                                      #
    # ------------------------------------------------------------------ #

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        cx = self.width() // 2
        cy = self.height() - 65
        r  = int(20 + self._amp * 25)

        painter.setBrush(QBrush(QColor(74, 158, 255, 180)))
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawEllipse(cx - r, cy - r, r * 2, r * 2)
'''


class OverlayProxy:
    """
    Thin proxy that forwards overlay calls to the currently-active overlay widget.
    Keeping a stable proxy object means the visualizer callback list and the
    show/hide closures in main.py never need to be rewired after a swap.
    """

    def __init__(self, initial_overlay):
        self._overlay = initial_overlay

    def swap(self, new_overlay):
        if self._overlay is not None:
            try:
                self._overlay.hide_mode()
            except Exception:
                pass
        self._overlay = new_overlay

    def update_audio(self, data):
        if self._overlay is not None:
            try:
                self._overlay.update_audio(data)
            except Exception:
                pass

    def show_mode(self):
        if self._overlay is not None:
            try:
                self._overlay.show_mode()
            except Exception:
                pass

    def hide_mode(self):
        if self._overlay is not None:
            try:
                self._overlay.hide_mode()
            except Exception:
                pass


class OverlayManager:
    """Discovers overlay modules and instantiates them on demand."""

    def __init__(self):
        self._registry: dict = {}   # overlay_id -> {display_name, description, builtin, module}
        self._discover()

    # ------------------------------------------------------------------ #
    #  Discovery                                                           #
    # ------------------------------------------------------------------ #

    def _discover(self):
        self._registry = {}

        # Built-ins first (deterministic order)
        for name in _BUILTIN_NAMES:
            path = _BUILTIN_DIR / f"{name}.py"
            if path.exists():
                self._register(name, path, builtin=True)

        # User overlays (alphabetical, skip names that shadow built-ins)
        if _USER_DIR.exists():
            for path in sorted(_USER_DIR.glob("*.py")):
                name = path.stem
                if name.startswith("_"):
                    continue   # underscore prefix = private / template
                if name not in self._registry:
                    self._register(name, path, builtin=False)

    def _register(self, overlay_id: str, path: Path, builtin: bool):
        # Temporarily extend sys.path so user overlays can import app modules.
        src_dir = str(Path(__file__).parent.parent)
        injected = src_dir not in sys.path
        if injected:
            sys.path.insert(0, src_dir)

        try:
            spec   = importlib.util.spec_from_file_location(f"_overlay_{overlay_id}", path)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
        except Exception as exc:
            print(f"[OverlayManager] Failed to load {path}: {exc}")
            return
        finally:
            if injected and src_dir in sys.path:
                sys.path.remove(src_dir)

        if not hasattr(module, "OverlayUI"):
            print(f"[OverlayManager] {path.name}: no OverlayUI class — skipped")
            return

        self._registry[overlay_id] = {
            "display_name": getattr(module, "DISPLAY_NAME", overlay_id.title()),
            "description":  getattr(module, "DESCRIPTION",  ""),
            "builtin":      builtin,
            "module":       module,
        }

    # ------------------------------------------------------------------ #
    #  Public API                                                          #
    # ------------------------------------------------------------------ #

    def available(self) -> dict:
        """Return {overlay_id: {display_name, description, builtin}} for all discovered overlays."""
        return {
            k: {field: v[field] for field in ("display_name", "description", "builtin")}
            for k, v in self._registry.items()
        }

    def load(self, overlay_id: str):
        """Instantiate and return an overlay widget.  Falls back to 'waveform' on error."""
        if overlay_id not in self._registry:
            print(f"[OverlayManager] Unknown overlay '{overlay_id}', falling back to 'waveform'")
            overlay_id = "waveform"

        if overlay_id not in self._registry:
            return None

        try:
            return self._registry[overlay_id]["module"].OverlayUI()
        except Exception as exc:
            print(f"[OverlayManager] Failed to instantiate '{overlay_id}': {exc}")
            return None

    def refresh(self):
        """Re-scan overlay directories (picks up newly added user overlays)."""
        self._discover()

    def ensure_user_dir(self) -> Path:
        """Create the user overlays folder and write a template file if absent."""
        _USER_DIR.mkdir(parents=True, exist_ok=True)
        template = _USER_DIR / "_template.py"
        if not template.exists():
            template.write_text(_TEMPLATE_CONTENT)
        return _USER_DIR
