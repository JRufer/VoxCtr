import sys
import threading
import queue
import os
import contextlib
import signal
import time
from datetime import datetime
from PyQt6.QtWidgets import QApplication
from PyQt6.QtCore import pyqtSignal, QObject, QTimer
from config import Config
from input_listener import InputListener
from audio_recorder import AudioRecorder
from inference_engine import InferenceEngine
from text_injector import TextInjector
from dbus_service import DBusService
from routing.loader import load_targets
from routing.router import OutputTargetRouter
from gui.tray_icon import WhisperTrayIcon
from gui.settings_window import SettingsWindow
from gui.history_window import HistoryWindow
from gui.overlay_manager import OverlayManager, OverlayProxy
from gui.setup_dialog import needs_setup, PermissionsSetupDialog

@contextlib.contextmanager
def ignore_stderr():
    devnull = os.open(os.devnull, os.O_WRONLY)
    old_stderr = os.dup(sys.stderr.fileno())
    os.dup2(devnull, sys.stderr.fileno())
    try:
        yield
    finally:
        os.dup2(old_stderr, sys.stderr.fileno())
        os.close(devnull)
        os.close(old_stderr)

class AppState(QObject):
    recording_started = pyqtSignal(str)   # emits target_id
    recording_stopped = pyqtSignal()
    text_emitted = pyqtSignal(str)
    text_updated = pyqtSignal(str)

def ensure_single_instance():
    lock_file = os.path.join(os.path.expanduser("~"), ".local", "share", "whisper-wayland", "app.pid")
    if os.path.exists(lock_file):
        try:
            with open(lock_file, "r") as f:
                old_pid = int(f.read().strip())

            # P0.7: Don't kill ourselves!
            if old_pid == os.getpid():
                return

            # Check if process is still alive
            os.kill(old_pid, 0)

            # If we reach here, it's alive. Kill it.
            print(f"[Main] Terminating existing instance (PID {old_pid})...")
            os.kill(old_pid, signal.SIGTERM)

            # Wait up to 2 seconds for it to exit
            for _ in range(20):
                time.sleep(0.1)
                try:
                    os.kill(old_pid, 0)
                except ProcessLookupError:
                    break
            else:
                # Still alive? Force kill.
                os.kill(old_pid, signal.SIGKILL)
        except (ValueError, ProcessLookupError, PermissionError):
            # PID file was stale or we can't see/kill it
            pass

    # Save current PID
    try:
        os.makedirs(os.path.dirname(lock_file), exist_ok=True)
        with open(lock_file, "w") as f:
            f.write(str(os.getpid()))
    except Exception as e:
        print(f"[Main] Warning: Could not write PID file: {e}")

def main():
    try:
        print(f"[{datetime.now().strftime('%H:%M:%S')}] Whisper-Wayland starting up...")
        ensure_single_instance()
        app = QApplication(sys.argv)
        app.setQuitOnLastWindowClosed(False)

        config = Config()

        # Show the hotkey-permissions wizard on first run (or if setup is still incomplete).
        # The dialog is non-modal — the user can skip it and still use the app via DBus/tray.
        if needs_setup():
            _setup_dlg = PermissionsSetupDialog()
            _setup_dlg.exec()

        state = AppState()

        audio_queue = queue.Queue()
        text_queue = queue.Queue()
        realtime_text_queue = queue.Queue()

        # Initialize overlay manager and load the user-selected overlay
        overlay_manager = OverlayManager()
        _initial_style  = config.get("overlay_style", "waveform")
        _active_overlay = overlay_manager.load(_initial_style)
        overlay_proxy   = OverlayProxy(_active_overlay)
        _active_style   = [_initial_style]   # mutable cell for the closure below

        # P3.1: Routing system
        router = OutputTargetRouter(load_targets())

        with ignore_stderr():
            recorder = AudioRecorder(config, audio_queue)

        recorder.visualizer_callbacks.append(overlay_proxy.update_audio)

        inference = InferenceEngine(config, audio_queue, text_queue, realtime_text_queue)

        def on_injection(total_words, text):
            """Called by TextInjector after each successful injection."""
            # P0.6: tray tooltip
            lang = inference.last_language
            lang_str = f" [{lang.upper()}]" if lang else ""
            tray.setToolTip(f"Whisper Wayland{lang_str} — {total_words} words this session")
            # P1.2: history panel (thread-safe via QTimer)
            history_window._pending_text = text
            from PyQt6.QtCore import QTimer as _QTimer
            _QTimer.singleShot(0, lambda: history_window.add_entry(history_window._pending_text))
            # P2.3: DBus signal + word count
            dbus_svc.set_word_count(total_words)
            dbus_svc.notify_text(text)

        injector = TextInjector(config, text_queue, word_count_callback=on_injection, router=router)

        def _get_target_info(target_id: str) -> tuple:
            """Return (post_processing, initial_prompt) for a target_id."""
            tgt = router.get_target(target_id)
            if tgt is None:
                return 'default', None
            return tgt.post_processing, tgt.initial_prompt

        def on_press(target_id: str = 'default'):
            if recorder.recording:
                # Already recording; ignore redundant triggers from overlapping hotkeys
                return
            print(f"\n[!] Triggered: Recording → target={target_id!r}")
            post_processing, initial_prompt = _get_target_info(target_id)
            state.recording_started.emit(target_id)
            dbus_svc.set_status("recording")
            with ignore_stderr():
                recorder.start_recording()
            inference.set_recording(True, target_id=target_id,
                                    post_processing=post_processing,
                                    initial_prompt_override=initial_prompt)

        def on_release(target_id: str = 'default'):
            print("[!] Released: Transcribing...")
            state.recording_stopped.emit()
            dbus_svc.set_status("transcribing")
            recorder.stop_recording()
            inference.set_recording(False)

        def on_toggle():
            """P2.3: called by DBus ToggleRecording() from an external tool."""
            from PyQt6.QtCore import QTimer as _QTimer
            if recorder.recording:
                _QTimer.singleShot(0, on_release)
            else:
                _QTimer.singleShot(0, on_press)

        # P2.3: DBus service (no-op stub if dbus-python not installed)
        dbus_svc = DBusService(
            on_start=on_press,
            on_stop=on_release,
            on_toggle=on_toggle,
        )

        # Restore idle status after injection completes
        _orig_on_injection = on_injection
        def _on_injection_with_idle(total_words, text):
            _orig_on_injection(total_words, text)
            dbus_svc.set_status("idle")
        injector.word_count_callback = _on_injection_with_idle

        listener = InputListener(config, on_press, on_release)

        # UI Toggles
        def show_recording_ui():
            overlay_proxy.show_mode()

        def hide_recording_ui():
            overlay_proxy.hide_mode()

        from PyQt6.QtCore import Qt
        state.recording_started.connect(show_recording_ui, Qt.ConnectionType.QueuedConnection)
        state.recording_stopped.connect(hide_recording_ui, Qt.ConnectionType.QueuedConnection)

        # UI
        history_window = HistoryWindow(config)
        settings_window = SettingsWindow(config, inference, recorder,
                                         overlay_manager=overlay_manager)
        settings_window.router = router
        tray = WhisperTrayIcon(state, history_window)
        tray.settings_action.triggered.connect(settings_window.show)
        settings_window.settings_saved.connect(listener.update_hotkey)
        settings_window.settings_saved.connect(listener.update_device)
        settings_window.settings_saved.connect(lambda: _reload_routing(router))

        # Fallback: If tray is not available, show settings so the app isn't "invisible"
        if not tray.isSystemTrayAvailable():
            print("[Main] System tray not available, opening settings directly.")
            settings_window.show()
        else:
            tray.show()

        # Start threads
        recorder.start()
        inference.start()
        injector.start()
        listener.start()
        dbus_svc.start()

        sys.exit(app.exec())
    except Exception as e:
        import traceback
        with open("crash_report.txt", "w") as f:
            f.write(f"CRASH AT {datetime.now()}:\n")
            f.write(str(e) + "\n")
            f.write(traceback.format_exc())
        print(f"FATAL ERROR: {e}")
        sys.exit(1)
    finally:
        try:
            listener.stop()
            recorder.stop()
            inference.stop()
            injector.stop()
            dbus_svc.stop()
        except:
            pass


def _reload_routing(router: OutputTargetRouter) -> None:
    """Hot-reload targets after settings save."""
    try:
        targets = load_targets()
        router.update_targets(targets)
        print(f"[Main] Routing targets reloaded ({len(targets)} targets)")
    except Exception as e:
        print(f"[Main] Could not reload routing targets: {e}")


if __name__ == "__main__":
    main()
