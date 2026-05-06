"""
Base class for all recording overlay UIs.

To create a custom overlay, place a .py file in:
    ~/.config/whisper-wayland/overlays/

Your file must:
  1. Set DISPLAY_NAME = "Your Overlay Name"  (module-level string)
  2. Optionally set DESCRIPTION = "..."  and  VERSION = "1.0"
  3. Define a class named  OverlayUI  that inherits from QWidget
  4. Implement the three methods below

Example skeleton:

    DISPLAY_NAME = "My Overlay"
    DESCRIPTION  = "What it looks like"
    VERSION      = "1.0"

    import numpy as np
    from PyQt6.QtWidgets import QWidget, QApplication
    from PyQt6.QtCore import Qt, QMetaObject

    class OverlayUI(QWidget):
        def __init__(self):
            super().__init__()
            self.setWindowFlags(
                Qt.WindowType.FramelessWindowHint |
                Qt.WindowType.WindowStaysOnTopHint |
                Qt.WindowType.WindowTransparentForInput
            )
            self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
            self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
            self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
            self.hide()

        def update_audio(self, data):
            # data: numpy float32 array, ~1024 samples, amplitude range +-32768
            QMetaObject.invokeMethod(self, "update", Qt.ConnectionType.QueuedConnection)

        def show_mode(self):
            screen = QApplication.primaryScreen()
            if screen:
                g = screen.geometry()
                self.setGeometry(g)
                self.setFixedSize(g.width(), g.height())
                self.move(g.x(), g.y())
            self.show()
            self.raise_()

        def hide_mode(self):
            self.hide()
"""

from PyQt6.QtWidgets import QWidget


class OverlayUIBase(QWidget):
    """Inherit from this class (optional but recommended) for type safety."""

    DISPLAY_NAME = "Unnamed Overlay"
    DESCRIPTION  = ""
    VERSION      = "1.0"

    def update_audio(self, data):
        """Called from the audio thread with float32 PCM samples (~1024 per call, ±32768 range)."""
        raise NotImplementedError

    def show_mode(self, label: str = ""):
        """Expand to full screen and become visible.  label is the routing target name."""
        raise NotImplementedError

    def hide_mode(self):
        """Become invisible."""
        raise NotImplementedError
