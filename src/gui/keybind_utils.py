"""Pure-logic helpers for key-name formatting used by _KeyCaptureWidget."""
try:
    from evdev import ecodes as _ecodes
except ImportError:
    _ecodes = None


NICE_KEY_NAMES = {
    "KEY_LEFTMETA": "Super (L)", "KEY_RIGHTMETA": "Super (R)",
    "KEY_LEFTCTRL": "Ctrl (L)", "KEY_RIGHTCTRL": "Ctrl (R)",
    "KEY_LEFTALT": "Alt (L)", "KEY_RIGHTALT": "Alt (R)",
    "KEY_LEFTSHIFT": "Shift (L)", "KEY_RIGHTSHIFT": "Shift (R)",
    "KEY_SPACE": "Space", "KEY_ENTER": "Enter", "KEY_ESC": "Esc",
    "KEY_TAB": "Tab", "KEY_BACKSPACE": "Backspace", "KEY_DELETE": "Delete",
    "KEY_INSERT": "Insert", "KEY_HOME": "Home", "KEY_END": "End",
    "KEY_PAGEUP": "PgUp", "KEY_PAGEDOWN": "PgDn",
    "KEY_UP": "↑", "KEY_DOWN": "↓", "KEY_LEFT": "←", "KEY_RIGHT": "→",
}

_MODIFIER_ORDER = [
    "KEY_LEFTMETA", "KEY_RIGHTMETA",
    "KEY_LEFTCTRL", "KEY_RIGHTCTRL",
    "KEY_LEFTALT", "KEY_RIGHTALT",
    "KEY_LEFTSHIFT", "KEY_RIGHTSHIFT",
]


def evdev_name_from_scan(scan_code):
    """Map an X11 keycode (nativeScanCode) to an evdev key name string."""
    if _ecodes is None:
        return None
    try:
        evdev_code = scan_code - 8  # X11 keycode → evdev scancode
        name = _ecodes.KEY.get(evdev_code)
        if isinstance(name, list):
            name = name[0]
        return name
    except Exception:
        return None


def fmt_evdev_keys(keys):
    """Format a list of evdev key names into a human-readable string."""
    parts = [NICE_KEY_NAMES.get(k, k.replace("KEY_", "").title()) for k in keys]
    return "  +  ".join(parts) if parts else ""


def sorted_evdev_keys(keys):
    """Sort keys so modifiers come first (Meta > Ctrl > Alt > Shift), then others."""
    mods = [k for k in _MODIFIER_ORDER if k in keys]
    rest = [k for k in keys if k not in _MODIFIER_ORDER]
    return mods + rest
