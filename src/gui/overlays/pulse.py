DISPLAY_NAME = "Pulse Circle"
DESCRIPTION  = "A glowing circle that pulses with audio amplitude"
VERSION      = "1.0"

import numpy as np
from PyQt6.QtWidgets import QWidget, QApplication
from PyQt6.QtCore import Qt, QTimer, QMetaObject
from PyQt6.QtGui import QPainter, QColor, QBrush, QPen

from gui.overlays.base import OverlayUIBase


class OverlayUI(OverlayUIBase):
    DISPLAY_NAME = DISPLAY_NAME
    DESCRIPTION  = DESCRIPTION

    def __init__(self):
        super().__init__()
        self._amplitude = 0.0
        self._smooth_amp = 0.0

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
        self.setStyleSheet("border: 1px solid transparent;")
        self.setFixedSize(800, 100)

        self._timer = QTimer(self)
        self._timer.timeout.connect(self._animate)
        self._timer.start(30)

        self.hide()

    def _animate(self):
        self._smooth_amp = self._smooth_amp * 0.7 + self._amplitude * 0.3
        self.update()

    def update_audio(self, data):
        rms = float(np.sqrt(np.mean(data.astype(np.float32) ** 2)))
        self._amplitude = min(1.0, rms / 8192.0)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        cx = self.width() // 2
        cy = self.height() - 65

        base_r  = 25
        pulse_r = int(base_r + self._smooth_amp * 20)

        # Outer glow rings
        for i in range(3, 0, -1):
            alpha = int(50 * (1.0 - i / 4.0) * (0.3 + self._smooth_amp * 0.7))
            r = pulse_r + i * 7
            painter.setBrush(QBrush(QColor(74, 158, 255, alpha)))
            painter.setPen(Qt.PenStyle.NoPen)
            painter.drawEllipse(cx - r, cy - r, r * 2, r * 2)

        # Main circle body
        painter.setBrush(QBrush(QColor(0, 0, 0, 210)))
        painter.setPen(QPen(QColor(74, 158, 255, 255), 2))
        painter.drawEllipse(cx - pulse_r, cy - pulse_r, pulse_r * 2, pulse_r * 2)

        # Inner accent dot
        inner_r = max(4, int(pulse_r * 0.28))
        painter.setBrush(QBrush(QColor(74, 158, 255, 180)))
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawEllipse(cx - inner_r, cy - inner_r, inner_r * 2, inner_r * 2)

    def show_mode(self):
        screen = QApplication.primaryScreen()
        if screen:
            geom = screen.geometry()
            self.setGeometry(geom)
            self.setFixedSize(geom.width(), geom.height())
            self.move(geom.x(), geom.y())
            self.show()
            self.raise_()
        else:
            self.show()

    def hide_mode(self):
        self.hide()
