DISPLAY_NAME = "Blue Wave"
DESCRIPTION  = "Full-width teal waveform with model info and routing target on a dark card"
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


_BAR_W  = 3    # bar width px
_BAR_GAP = 1   # gap between bars px
_CARD_W = 760  # card width px
_CARD_H = 130  # card height px
_PAD    = 16   # horizontal/vertical padding


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

        n_bars = (_CARD_W - 2 * _PAD) // (_BAR_W + _BAR_GAP)
        self._n_bars = n_bars
        self._amplitudes: deque = deque([0.0] * n_bars, maxlen=n_bars)

        self._routing_label: str = ""
        self._model_info: dict = {}

        self._timer = QTimer(self)
        self._timer.timeout.connect(self.update)
        self._timer.start(30)

        self.hide()

    # ------------------------------------------------------------------ #

    def update_audio(self, data):
        rms = float(np.sqrt(np.mean(data.astype(np.float32) ** 2)))
        self._amplitudes.append(min(1.0, rms / 8192.0))

    def show_mode(self, label: str = "", model_info: dict = None):
        self._routing_label = label
        self._model_info = model_info or {}
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
        self._amplitudes.extend([0.0] * self._n_bars)
        self.hide()

    # ------------------------------------------------------------------ #

    def _left_text(self) -> str:
        """Build the info string shown on the left: '● MIC  ·  whisper.large-v3  ·  cuda fp16'"""
        parts = ["● MIC"]
        mi = self._model_info

        model_size = mi.get("model_size", "")
        if model_size:
            parts.append(f"whisper.{model_size}")

        device = mi.get("device", "")
        compute = mi.get("compute_type", "")
        device_str = "  ".join(
            p for p in [
                device.lower() if device.lower() not in ("unknown", "auto", "") else "",
                compute.lower() if compute.lower() not in ("unknown", "default", "") else "",
            ] if p
        )
        if device_str:
            parts.append(device_str)

        return "  ·  ".join(parts)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        card_x = (self.width() - _CARD_W) / 2
        card_y = self.height() - _CARD_H - 90

        # ── Card background ───────────────────────────────────────────────
        card_rect = QRectF(card_x, card_y, _CARD_W, _CARD_H)
        painter.setBrush(QBrush(QColor(8, 14, 22, 248)))
        painter.setPen(QPen(QColor(20, 100, 130, 110), 1))
        painter.drawRoundedRect(card_rect, 12, 12)

        # ── Waveform bars (full card height) ─────────────────────────────
        wf_x  = card_x + _PAD
        wf_w  = _CARD_W - 2 * _PAD
        wf_cy = card_y + _CARD_H / 2
        max_half = (_CARD_H / 2 - 4) * 0.96

        grad = QLinearGradient(wf_x, 0, wf_x + wf_w, 0)
        grad.setColorAt(0.00, QColor(0,  140, 170, 110))
        grad.setColorAt(0.18, QColor(0,  170, 200, 175))
        grad.setColorAt(0.38, QColor(0,  200, 220, 225))
        grad.setColorAt(0.50, QColor(30, 220, 240, 255))
        grad.setColorAt(0.62, QColor(0,  200, 220, 225))
        grad.setColorAt(0.82, QColor(0,  170, 200, 175))
        grad.setColorAt(1.00, QColor(0,  140, 170, 110))

        painter.setBrush(QBrush(grad))
        painter.setPen(Qt.PenStyle.NoPen)

        for i, amp in enumerate(self._amplitudes):
            bx     = wf_x + i * (_BAR_W + _BAR_GAP)
            half_h = max(2.0, amp * max_half)
            painter.drawRoundedRect(
                QRectF(bx, wf_cy - half_h, _BAR_W, half_h * 2),
                1, 1,
            )

        # ── Left info text ────────────────────────────────────────────────
        info_font = QFont("Segoe UI", 9)
        painter.setFont(info_font)
        painter.setPen(QPen(QColor(70, 190, 210, 215)))
        painter.drawText(int(card_x) + _PAD, int(card_y) + 22, self._left_text())

        # ── Right routing badge ("→ Target Name") ─────────────────────────
        if self._routing_label:
            badge_text = f"→  {self._routing_label}"
            badge_font = QFont("Segoe UI", 9, QFont.Weight.Medium)
            painter.setFont(badge_font)
            fm = QFontMetrics(badge_font)
            badge_w = fm.horizontalAdvance(badge_text) + 26
            badge_h = 26
            badge_rect = QRectF(
                card_x + _CARD_W - badge_w - _PAD,
                card_y + 8,
                badge_w,
                badge_h,
            )
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.setPen(QPen(QColor(40, 170, 195, 210), 1.2))
            painter.drawRoundedRect(badge_rect, 13, 13)
            painter.setPen(QPen(QColor(80, 210, 228, 235)))
            painter.drawText(badge_rect, Qt.AlignmentFlag.AlignCenter, badge_text)
