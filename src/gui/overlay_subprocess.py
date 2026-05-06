"""
overlay_subprocess.py — Runs all overlay widgets in a dedicated process.

By running overlays here we can set QT_WAYLAND_SHELL_INTEGRATION=layer-shell
*before* QApplication is created, which activates Qt's native wlr-layer-shell
protocol support.  The main process must NOT set this env var so its settings
window and tray icon remain ordinary xdg_toplevel windows.

On compositors that do not support zwlr_layer_shell_v1 (e.g. GNOME Wayland),
Qt falls back silently to xdg_toplevel — the overlay still appears, just
without compositor-guaranteed z-ordering above fullscreen windows.

IPC (parent → child):
    Commands sent over a multiprocessing.Pipe (one-directional, parent writes):
        ("show",   label: str)           — show recording overlay
        ("hide",)                        — hide recording overlay
        ("swap",   overlay_id: str)      — hot-swap to a different recording overlay
        ("audio",  seq: int)             — new audio frame available in shared memory
        ("tts_show", text, source_label) — show TTS response overlay
        ("tts_hide",)                    — hide TTS response overlay
        ("quit",)                        — clean exit

    Audio data is written into a shared-memory block (float32 numpy array of
    _AUDIO_SAMPLES elements) identified by name; the ("audio", seq) command is
    only a lightweight notification that new data is ready.
"""

import os
import sys

# MUST be set before QApplication is created.
os.environ["QT_WAYLAND_SHELL_INTEGRATION"] = "layer-shell"
# Ensure we use the Wayland backend even if DISPLAY is also set.
if "WAYLAND_DISPLAY" in os.environ and "QT_QPA_PLATFORM" not in os.environ:
    os.environ["QT_QPA_PLATFORM"] = "wayland"

import multiprocessing.connection
import multiprocessing.shared_memory
import numpy as np
from pathlib import Path

# Ensure the src/ directory is on sys.path so overlay modules can import
# app-internal packages.  The parent sets PYTHONPATH but this handles the
# case where the subprocess is invoked directly (e.g. during testing).
_src_dir = str(Path(__file__).parent.parent)
if _src_dir not in sys.path:
    sys.path.insert(0, _src_dir)

from PyQt6.QtWidgets import QApplication
from PyQt6.QtCore import QTimer

_AUDIO_SAMPLES = 1024


def _run(overlay_id: str, cmd_fd: int, shm_name: str, shm_size: int) -> None:
    app = QApplication(sys.argv[:1])

    # ── Shared memory (audio data written by main process) ────────────────
    shm = multiprocessing.shared_memory.SharedMemory(name=shm_name)
    shm_buf = np.frombuffer(shm.buf, dtype=np.float32)

    # ── Command pipe (read-only child end) ────────────────────────────────
    conn = multiprocessing.connection.Connection(cmd_fd, writable=False)

    # ── Load overlays ─────────────────────────────────────────────────────
    from gui.overlay_manager import OverlayManager, OverlayProxy
    from gui.overlays.tts_response import TTSResponseOverlay

    manager     = OverlayManager()
    rec_overlay = manager.load(overlay_id) or manager.load("voice_card")
    rec_proxy   = OverlayProxy(rec_overlay)
    tts_overlay = TTSResponseOverlay()

    # Track whether we are currently in show_mode (for swap continuity)
    _showing    = [False]
    _show_label = [""]

    # ── Pipe poll loop ────────────────────────────────────────────────────
    def _poll() -> None:
        while conn.poll():
            try:
                msg = conn.recv()
            except EOFError:
                app.quit()
                return

            cmd = msg[0] if msg else None

            if cmd == "show":
                _showing[0]    = True
                _show_label[0] = msg[1] if len(msg) > 1 else ""
                rec_proxy.show_mode(_show_label[0])

            elif cmd == "hide":
                _showing[0] = False
                rec_proxy.hide_mode()

            elif cmd == "audio":
                # Copy slice to avoid aliasing during paint
                data = shm_buf[:_AUDIO_SAMPLES].copy()
                rec_proxy.update_audio(data)

            elif cmd == "swap":
                new_id  = msg[1]
                new_ov  = manager.load(new_id)
                if new_ov:
                    rec_proxy.swap(new_ov)
                    if _showing[0]:
                        rec_proxy.show_mode(_show_label[0])

            elif cmd == "tts_show":
                text         = msg[1] if len(msg) > 1 else ""
                source_label = msg[2] if len(msg) > 2 else ""
                tts_overlay.show_response(text, source_label)

            elif cmd == "tts_hide":
                tts_overlay.hide_response()

            elif cmd == "quit":
                rec_proxy.hide_mode()
                tts_overlay.hide_response()
                app.quit()
                return

    timer = QTimer()
    timer.timeout.connect(_poll)
    timer.start(16)   # ~60 Hz — well above the ~15 Hz audio callback rate

    app.exec()

    shm.close()


if __name__ == "__main__":
    if len(sys.argv) < 5:
        print(
            "usage: overlay_subprocess.py <overlay_id> <cmd_fd> <shm_name> <shm_size>",
            file=sys.stderr,
        )
        sys.exit(1)

    _run(
        overlay_id=sys.argv[1],
        cmd_fd=int(sys.argv[2]),
        shm_name=sys.argv[3],
        shm_size=int(sys.argv[4]),
    )
