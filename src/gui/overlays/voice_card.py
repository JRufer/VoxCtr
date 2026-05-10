DISPLAY_NAME = "Voice Card"
DESCRIPTION  = "Scrolling bar waveform in a floating card with a pink/magenta gradient"
VERSION      = "1.0"

import numpy as np
from collections import deque
from PyQt6.QtWidgets import QWidget, QApplication
from PyQt6.QtCore import Qt, QTimer, QMetaObject, QRectF
from PyQt6.QtGui import (
    QPainter, QColor, QBrush, QPen,
    QLinearGradient, QFont, QFontMetrics,
)

from gui.overlays.base import OverlayUIBase


_BAR_W    = 4    # bar width px
_BAR_GAP  = 2    # gap between bars px
_CARD_W   = 480  # card width px
_CARD_H   = 165  # card height px
_WF_PAD   = 16   # horizontal padding inside card
_WF_TOP   = 40   # y offset from card top to waveform area
_WF_BOT   = 14   # gap from waveform bottom to card bottom


class OverlayUI(OverlayUIBase):
    DISPLAY_NAME = DISPLAY_NAME
    DESCRIPTION  = DESCRIPTION

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
        self.setStyleSheet("border: 1px solid transparent;")
        self.setFixedSize(800, 100)

        # How many bars fit in the waveform area
        wf_w = _CARD_W - 2 * _WF_PAD
        self._n_bars = wf_w // (_BAR_W + _BAR_GAP)
        self._amplitudes: deque = deque([0.0] * self._n_bars, maxlen=self._n_bars)

        self._routing_label: str = ""

        self._timer = QTimer(self)
        self._timer.timeout.connect(self.update)
        self._timer.start(30)

        self.hide()

    # ------------------------------------------------------------------ #

    def update_audio(self, data):
        rms = float(np.sqrt(np.mean(data.astype(np.float32) ** 2)))
        self._amplitudes.append(min(1.0, rms / 8192.0))

    def show_mode(self, label: str = "", **kwargs):
        self._routing_label = label
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
        self._amplitudes.extend([0.0] * self._n_bars)   # drain history on stop
        self.hide()

    # ------------------------------------------------------------------ #

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        # Card anchored to bottom-centre, 90 px above screen edge
        card_x = (self.width() - _CARD_W) / 2
        card_y = self.height() - _CARD_H - 90

        # ── Background panel ─────────────────────────────────────────────
        card_rect = QRectF(card_x, card_y, _CARD_W, _CARD_H)
        painter.setBrush(QBrush(QColor(18, 22, 32, 235)))
        painter.setPen(QPen(QColor(55, 65, 95, 180), 1))
        painter.drawRoundedRect(card_rect, 12, 12)

        # ── "Voice Activity" label ───────────────────────────────────────
        painter.setFont(QFont("Segoe UI", 9))
        painter.setPen(QPen(QColor(130, 145, 170, 200)))
        painter.drawText(int(card_x) + 14, int(card_y) + 22, "Voice Activity")

        # ── Routing badge (top-right) ────────────────────────────────────
        badge_text = self._routing_label or "whisper / Wayland"
        badge_font = QFont("Segoe UI", 9, QFont.Weight.Medium)
        painter.setFont(badge_font)
        fm = QFontMetrics(badge_font)
        badge_w = fm.horizontalAdvance(badge_text) + 20
        badge_h = 22
        badge_rect = QRectF(
            card_x + _CARD_W - badge_w - 10,
            card_y + 7,
            badge_w, badge_h,
        )
        painter.setBrush(QBrush(QColor(12, 15, 25, 240)))
        painter.setPen(QPen(QColor(55, 190, 155, 210), 1))
        painter.drawRoundedRect(badge_rect, 5, 5)
        painter.setPen(QPen(QColor(165, 215, 200, 230)))
        painter.drawText(badge_rect, Qt.AlignmentFlag.AlignCenter, badge_text)

        # ── Waveform bars ────────────────────────────────────────────────
        wf_x    = card_x + _WF_PAD
        wf_y    = card_y + _WF_TOP
        wf_w    = _CARD_W - 2 * _WF_PAD
        wf_h    = _CARD_H - _WF_TOP - _WF_BOT
        wf_cy   = wf_y + wf_h / 2

        # Horizontal gradient: old audio (left) is dim purple → recent (right) bright pink
        grad = QLinearGradient(wf_x, 0, wf_x + wf_w, 0)
        grad.setColorAt(0.00, QColor(100, 40,  120, 110))
        grad.setColorAt(0.30, QColor(160, 55,  145, 170))
        grad.setColorAt(0.60, QColor(210, 85,  175, 215))
        grad.setColorAt(0.82, QColor(240, 130, 200, 245))
        grad.setColorAt(1.00, QColor(255, 175, 220, 255))

        painter.setBrush(QBrush(grad))
        painter.setPen(Qt.PenStyle.NoPen)

        max_half_h = (wf_h / 2) * 0.88   # leave a tiny margin at top/bottom

        for i, amp in enumerate(self._amplitudes):
            bx       = wf_x + i * (_BAR_W + _BAR_GAP)
            half_h   = max(3.0, amp * max_half_h)
            painter.drawRoundedRect(
                QRectF(bx, wf_cy - half_h, _BAR_W, half_h * 2),
                2, 2,
            )
