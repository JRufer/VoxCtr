"""AT-SPI2 accessibility integration for context-aware dictation.

Provides three capabilities:
  - Focus tracking: remembers the last focused accessible widget via an event listener.
  - Context reading: returns the app name, widget role, and text preceding the cursor.
  - Text injection: inserts text directly via the AT-SPI2 Text interface, bypassing
    keypress simulation and avoiding modifier-key conflicts.

All public symbols degrade gracefully when pyatspi is not installed.
"""

import logging
import threading
from typing import NamedTuple, Optional

logger = logging.getLogger(__name__)

try:
    import pyatspi  # python-atspi on Arch; python3-pyatspi on Debian/Ubuntu
    _AVAILABLE = True
except ImportError:
    _AVAILABLE = False
    logger.debug("pyatspi not installed; AT-SPI2 integration disabled")

# Roles that are typically code/terminal contexts → suggest code dictation mode
_CODE_ROLES = frozenset({
    "terminal",
    "text",          # generic editable text (often IDEs)
})

# Roles of widgets that implement the AT-SPI2 Text interface
_TEXT_ROLES = frozenset({
    "entry",
    "text",
    "terminal",
    "paragraph",
    "document frame",
    "document web",
    "document email content",
    "spin button",
    "combo box",
    "editable combo box",
})


class FocusContext(NamedTuple):
    """Snapshot of the focused accessible widget at a point in time."""
    app_name: str
    role_name: str
    surrounding_text: str  # text before the caret, up to max_chars
    cursor_offset: int
    is_code_context: bool  # True when terminal/IDE-like role detected


# ── internal state ────────────────────────────────────────────────────────────

_focused: object = None   # pyatspi.Accessible (or None)
_lock = threading.Lock()
_running = False


# ── lifecycle ─────────────────────────────────────────────────────────────────

def is_available() -> bool:
    """Return True when pyatspi is importable."""
    return _AVAILABLE


def start() -> None:
    """Start the AT-SPI2 event listener in a background daemon thread.

    Safe to call multiple times; subsequent calls are no-ops.
    """
    global _running
    if not _AVAILABLE or _running:
        return
    try:
        pyatspi.Registry.registerEventListener(
            _on_focus_changed, "object:state-changed:focused"
        )
        t = threading.Thread(target=_run_loop, daemon=True, name="atspi-loop")
        t.start()
        _running = True
        logger.info("AT-SPI2 focus tracker started")
    except Exception as exc:
        logger.warning("AT-SPI2 start failed: %s", exc)


def stop() -> None:
    """Deregister the focus listener and stop the AT-SPI2 event loop."""
    global _running
    if not _AVAILABLE or not _running:
        return
    try:
        pyatspi.Registry.deregisterEventListener(
            _on_focus_changed, "object:state-changed:focused"
        )
        pyatspi.Registry.stop()
    except Exception:
        pass
    _running = False
    logger.debug("AT-SPI2 focus tracker stopped")


def _run_loop() -> None:
    """Entry point for the daemon thread; runs the AT-SPI2 GLib event loop."""
    try:
        pyatspi.Registry.start()
    except Exception as exc:
        logger.debug("AT-SPI2 event loop exited: %s", exc)


def _on_focus_changed(event) -> None:
    """Called by the AT-SPI2 event loop when a widget gains or loses focus."""
    global _focused
    if event.detail1:  # detail1 == 1 → gained focus
        with _lock:
            _focused = event.source


# ── public API ────────────────────────────────────────────────────────────────

def get_focused_context(max_chars: int = 500) -> Optional[FocusContext]:
    """Return a FocusContext snapshot for the currently focused widget.

    Returns None when AT-SPI2 is unavailable or no widget is tracked yet.
    The ``surrounding_text`` field contains up to *max_chars* characters
    immediately before the caret — useful as a Whisper initial_prompt.
    """
    if not _AVAILABLE:
        return None
    with _lock:
        focused = _focused
    if focused is None:
        return None
    try:
        app_name = _safe_app_name(focused)
        role_name = _safe_role(focused)
        surrounding_text, cursor_offset = _safe_text(focused, max_chars)
        is_code = role_name in _CODE_ROLES
        return FocusContext(app_name, role_name, surrounding_text, cursor_offset, is_code)
    except Exception as exc:
        logger.debug("get_focused_context error: %s", exc)
        return None


def inject_text(text: str) -> bool:
    """Insert *text* at the caret position via the AT-SPI2 Text interface.

    Returns True on success, False when the focused widget does not expose
    the Text interface or when AT-SPI2 is unavailable. Falls back silently
    so callers can chain to wtype / xdotool.
    """
    if not _AVAILABLE or not text:
        return False
    with _lock:
        focused = _focused
    if focused is None:
        return False
    try:
        text_iface = focused.queryText()
        offset = text_iface.caretOffset
        ok = bool(text_iface.insertText(offset, text, len(text)))
        if ok:
            logger.debug("AT-SPI2 injected %d chars at offset %d", len(text), offset)
        return ok
    except Exception as exc:
        logger.debug("AT-SPI2 inject_text failed: %s", exc)
        return False


# ── helpers ───────────────────────────────────────────────────────────────────

def _safe_app_name(accessible) -> str:
    try:
        app = accessible.getApplication()
        return app.name if app else ""
    except Exception:
        return ""


def _safe_role(accessible) -> str:
    try:
        return accessible.getRoleName()
    except Exception:
        return ""


def _safe_text(accessible, max_chars: int) -> tuple[str, int]:
    """Return (surrounding_text, cursor_offset) or ("", 0) on failure."""
    try:
        iface = accessible.queryText()
        offset = iface.caretOffset
        start = max(0, offset - max_chars)
        text = iface.getText(start, offset)
        return text, offset
    except Exception:
        return "", 0
