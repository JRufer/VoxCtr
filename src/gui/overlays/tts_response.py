DISPLAY_NAME = "TTS Response"
DESCRIPTION  = "Shown while the AI is speaking a response via text-to-speech"
VERSION      = "1.0"

import math
from collections import deque
from PyQt6.QtWidgets import QWidget, QApplication
from PyQt6.QtCore import Qt, QTimer, QRectF, QMetaObject
from PyQt6.QtGui import (
    QPainter, QColor, QBrush, QPen,
    QLinearGradient, QFont, QFontMetrics,
)


_CARD_W  = 520
_CARD_H  = 130
_PAD     = 16
_DOT_R   = 5      # speaking-dot radius
_N_DOTS  = 4


class TTSResponseOverlay(QWidget):
    """
    Frameless overlay shown at the bottom-centre of the screen while TTS is
    playing.  Displays the text being spoken and a pulsing wave animation in
    a teal/cyan colour scheme distinct from the pink recording overlay.
    """

    def __init__(self):
        super().__init__()
        self.setWindowFlags(
            Qt.WindowType.ToolTip |
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.X11BypassWindowManagerHint |
            Qt.WindowType.WindowTransparentForInput,
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)

        self._text: str = ""
        self._source_label: str = ""   # routing target label (e.g. "Hermes Agent")
        self._phase: float = 0.0   # drives the wave animation
        self._visible: bool = False

        self._timer = QTimer(self)
        self._timer.timeout.connect(self._tick)
        self._timer.start(30)
        self.hide()

    # ── Public interface ──────────────────────────────────────────────────────

    def show_response(self, text: str = "", source_label: str = ""):
        """Call from any thread — schedules show on the Qt main thread.

        Args:
            text: The text being spoken by the TTS engine.
            source_label: Human-readable name of the routing target (e.g. "Hermes Agent").
        """
        self._text = text
        self._source_label = source_label
        QMetaObject.invokeMethod(self, "_do_show", Qt.ConnectionType.QueuedConnection)

    def hide_response(self):
        """Call from any thread."""
        QMetaObject.invokeMethod(self, "_do_hide", Qt.ConnectionType.QueuedConnection)

    # Slots called on main thread
    def _do_show(self):
        screen = QApplication.primaryScreen()
        if screen:
            g = screen.geometry()
            self.setGeometry(g)
            self.setFixedSize(g.width(), g.height())
            self.move(g.x(), g.y())
        self._visible = True
        self.show()
        self.raise_()

    def _do_hide(self):
        self._visible = False
        self.hide()

    def update_text(self, text: str):
        self._text = text
        self.update()

    # ── Animation ─────────────────────────────────────────────────────────────

    def _tick(self):
        if self._visible:
            self._phase = (self._phase + 0.08) % (2 * math.pi)
            self.update()

    # ── Paint ─────────────────────────────────────────────────────────────────

    def paintEvent(self, event):
        if not self._visible:
            return
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        card_x = (self.width() - _CARD_W) / 2
        card_y = self.height() - _CARD_H - 90

        # ── Card background ───────────────────────────────────────────────
        card_rect = QRectF(card_x, card_y, _CARD_W, _CARD_H)
        painter.setBrush(QBrush(QColor(10, 28, 38, 240)))
        painter.setPen(QPen(QColor(30, 140, 160, 160), 1))
        painter.drawRoundedRect(card_rect, 12, 12)

        # ── "AI Speaking" label (top-left) with optional source name ─────
        painter.setFont(QFont("Segoe UI", 9))
        painter.setPen(QPen(QColor(80, 200, 210, 200)))
        speaking_label = f"AI Speaking — {self._source_label}" if self._source_label else "AI Speaking"
        painter.drawText(int(card_x) + _PAD, int(card_y) + 22, speaking_label)

        # ── Animated wave dots (top-right badge) ─────────────────────────
        badge_cx = card_x + _CARD_W - _PAD - (_N_DOTS * 16)
        badge_cy = card_y + 14
        for i in range(_N_DOTS):
            offset = math.sin(self._phase + i * 0.9) * 4.5
            cx = badge_cx + i * 16
            cy = badge_cy + offset
            alpha = int(160 + 80 * math.sin(self._phase + i * 0.9))
            painter.setBrush(QBrush(QColor(30, 200, 210, alpha)))
            painter.setPen(Qt.PenStyle.NoPen)
            painter.drawEllipse(
                QRectF(cx - _DOT_R, cy - _DOT_R, _DOT_R * 2, _DOT_R * 2)
            )

        # ── Spoken text ───────────────────────────────────────────────────
        if self._text:
            text_rect = QRectF(
                card_x + _PAD,
                card_y + 36,
                _CARD_W - _PAD * 2,
                _CARD_H - 36 - _PAD,
            )
            font = QFont("Segoe UI", 11)
            painter.setFont(font)
            painter.setPen(QPen(QColor(210, 240, 245, 230)))
            # Truncate with ellipsis if too long
            fm = QFontMetrics(font)
            display = fm.elidedText(
                self._text,
                Qt.TextElideMode.ElideRight,
                int(text_rect.width()),
            )
            painter.drawText(
                text_rect,
                Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignTop
                | Qt.TextFlag.TextWordWrap,
                display,
            )

        # ── Bottom gradient bar ───────────────────────────────────────────
        bar_rect = QRectF(card_x + _PAD, card_y + _CARD_H - 8, _CARD_W - _PAD * 2, 4)
        grad = QLinearGradient(bar_rect.left(), 0, bar_rect.right(), 0)
        grad.setColorAt(0.0, QColor(0, 180, 200, 80))
        grad.setColorAt(0.5, QColor(30, 220, 240, 220))
        grad.setColorAt(1.0, QColor(0, 180, 200, 80))
        painter.setBrush(QBrush(grad))
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawRoundedRect(bar_rect, 2, 2)

        painter.end()


# Alias so OverlayManager can load it as a built-in if desired
OverlayUI = TTSResponseOverlay
