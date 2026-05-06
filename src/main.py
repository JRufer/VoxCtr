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
from tts_engine import TTSEngine
from tts_responder import ResponseListener
from mcp_server import WhisperMCPServer
from routing.loader import load_targets, load_bindings
from routing.router import OutputTargetRouter
from config_validator import validate_all, ConfigValidationError
from gui.tray_icon import WhisperTrayIcon
from gui.settings_window import SettingsWindow
from gui.history_window import HistoryWindow
from gui.overlay_manager import OverlayManager, OverlayProxy, OverlaySubprocessProxy
from gui.setup_dialog import needs_setup, PermissionsSetupDialog

try:
    import atspi_context
    _HAS_ATSPI = True
except ImportError:
    _HAS_ATSPI = False

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
    # Emitted from injector thread → connected to main-thread slots
    text_injected = pyqtSignal(int, str)  # (total_words, text)

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

        # Validate all configs before proceeding; exit cleanly on fatal errors.
        try:
            _startup_targets = load_targets()
            _startup_bindings = load_bindings()
            validate_all(config, _startup_targets, _startup_bindings)
        except ConfigValidationError as e:
            print(str(e), file=sys.stderr)
            sys.exit(1)

        # Show the hotkey-permissions wizard on first run (or if setup is still incomplete).
        # The dialog is non-modal — the user can skip it and still use the app via DBus/tray.
        if needs_setup():
            _setup_dlg = PermissionsSetupDialog()
            _setup_dlg.exec()

        state = AppState()

        audio_queue = queue.Queue()
        text_queue = queue.Queue()
        realtime_text_queue = queue.Queue()

        # Initialize overlay manager.
        # On Wayland, overlays run in a dedicated subprocess with
        # QT_WAYLAND_SHELL_INTEGRATION=layer-shell for native layer-shell support.
        # On X11, overlays run in-process as before.
        overlay_manager = OverlayManager()
        _initial_style  = config.get("overlay_style", "voice_card")
        _active_style   = [_initial_style]

        _on_wayland = bool(os.environ.get("WAYLAND_DISPLAY"))
        if _on_wayland:
            overlay_proxy: OverlaySubprocessProxy | OverlayProxy = OverlaySubprocessProxy(_initial_style)
        else:
            _active_overlay = overlay_manager.load(_initial_style)
            overlay_proxy   = OverlayProxy(_active_overlay)

        def _update_overlay():
            new_style = config.get("overlay_style", "voice_card")
            if new_style != _active_style[0]:
                print(f"[Main] Swapping overlay style: {_active_style[0]} -> {new_style}")
                if _on_wayland:
                    overlay_proxy.swap(new_style)
                else:
                    new_overlay = overlay_manager.load(new_style)
                    if new_overlay:
                        overlay_proxy.swap(new_overlay)
                _active_style[0] = new_style

        # P3.1: Routing system
        router = OutputTargetRouter(load_targets())

        # ── TTS engine + response overlay ─────────────────────────────────
        # On Wayland the TTS overlay lives inside the overlay subprocess.
        # On X11 it is instantiated here in-process.
        tts_engine = TTSEngine(config)

        if _on_wayland:
            def _on_tts_started(text: str, source_label: str = ""):
                if config.get("tts_response_overlay", True):
                    overlay_proxy.show_tts(text, source_label)

            def _on_tts_finished():
                overlay_proxy.hide_tts()
        else:
            from gui.overlays.tts_response import TTSResponseOverlay
            _tts_overlay_widget = TTSResponseOverlay()

            def _on_tts_started(text: str, source_label: str = ""):
                if config.get("tts_response_overlay", True):
                    _tts_overlay_widget.show_response(text, source_label=source_label)

            def _on_tts_finished():
                _tts_overlay_widget.hide_response()

        tts_engine.on_started  = _on_tts_started
        tts_engine.on_finished = _on_tts_finished

        # ── Response pipe listeners (one per target with response_pipe set) ─
        _response_listeners: list = []

        def _start_response_listeners(targets):
            for rl in _response_listeners:
                rl.stop()
            _response_listeners.clear()
            for tgt in targets:
                if tgt.response_pipe:
                    rl = ResponseListener(
                        pipe_path=tgt.response_pipe,
                        tts_speak=tts_engine.speak,
                        label=tgt.label or tgt.id,
                    )
                    rl.start()
                    _response_listeners.append(rl)

        _start_response_listeners(load_targets())

        with ignore_stderr():
            recorder = AudioRecorder(config, audio_queue)

        recorder.visualizer_callbacks.append(overlay_proxy.update_audio)

        inference = InferenceEngine(config, audio_queue, text_queue, realtime_text_queue)


        def on_injection(total_words, text):
            """Called by TextInjector (background thread) — emit signal to cross to main thread."""
            state.text_injected.emit(total_words, text)

        injector = TextInjector(config, text_queue, word_count_callback=on_injection, router=router)

        def on_press(target_id: str = 'default'):
            if recorder.recording:
                return
            print(f"\n[!] Triggered: Recording → target={target_id!r}")
            tgt = router.get_target(target_id)
            state.recording_started.emit(target_id)
            dbus_svc.set_status("recording")
            with ignore_stderr():
                recorder.start_recording()
            inference.set_recording(True, target_id=target_id, target=tgt)

        def on_release(target_id: str = 'default'):
            print("[!] Released: Transcribing...")
            state.recording_stopped.emit()
            dbus_svc.set_status("transcribing")
            recorder.stop_recording()
            inference.set_recording(False)

        def on_toggle():
            """Called by DBus ToggleRecording() from an external tool."""
            from PyQt6.QtCore import QTimer as _QTimer
            if recorder.recording:
                _QTimer.singleShot(0, on_release)
            else:
                _QTimer.singleShot(0, on_press)

        dbus_svc = DBusService(
            on_start=on_press,
            on_stop=on_release,
            on_toggle=on_toggle,
        )

        # ── MCP Server ────────────────────────────────────────────────────
        _mcp_result_queue: queue.Queue = queue.Queue()

        def _mcp_on_record(timeout: float) -> str:
            """Called by MCP tool — triggers a recording and waits for result."""
            import threading as _t
            _mcp_result_queue.queue.clear()
            # Schedule on Qt main thread
            from PyQt6.QtCore import QTimer as _QT
            _QT.singleShot(0, lambda: on_press('mcp'))

            # Auto-stop after timeout seconds (MCP recording has no key-release)
            def _auto_stop():
                time.sleep(timeout)
                from PyQt6.QtCore import QTimer as _QT2
                _QT2.singleShot(0, lambda: on_release('mcp'))

            _t.Thread(target=_auto_stop, daemon=True).start()

            try:
                result = _mcp_result_queue.get(timeout=timeout + 5.0)
            except queue.Empty:
                result = ""
            return result

        def _mcp_on_speak(text: str):
            tts_engine.speak(text)

        def _mcp_get_status() -> dict:
            return {
                "recording": recorder.recording,
                "speaking": tts_engine.is_speaking,
            }

        mcp_server = WhisperMCPServer(
            on_record=_mcp_on_record,
            on_speak=_mcp_on_speak,
            get_status=_mcp_get_status,
        )
        if config.get("mcp_server_enabled", False):
            mcp_server.start()

        listener = InputListener(config, on_press, on_release,
                                 on_tts_stop=tts_engine.stop)

        # UI Toggles
        def show_recording_ui(target_id: str = ""):
            tgt = router.get_target(target_id)
            label = tgt.label if tgt else ""
            overlay_proxy.show_mode(label)

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

        # Connect background-thread injection signal to main-thread UI slots.
        # Qt QueuedConnection ensures these run on the main thread even when
        # text_injected is emitted from the TextInjector background thread.
        def _on_text_injected(total_words, text):
            history_window.add_entry(text)
            lang = inference.last_language
            lang_str = f" [{lang.upper()}]" if lang else ""
            tray.setToolTip(f"Whisper Wayland{lang_str} — {total_words} words this session")
            dbus_svc.set_status("idle")
            dbus_svc.set_word_count(total_words)
            dbus_svc.notify_text(text)
            # Feed MCP result queue if recording was triggered by MCP
            _mcp_result_queue.put(text)

        state.text_injected.connect(_on_text_injected, Qt.ConnectionType.QueuedConnection)

        tray.settings_action.triggered.connect(settings_window.show)
        settings_window.settings_saved.connect(listener.update_hotkey)
        settings_window.settings_saved.connect(listener.update_device)
        settings_window.settings_saved.connect(lambda: _reload_routing(router, _start_response_listeners))
        settings_window.settings_saved.connect(lambda: _apply_mcp_toggle(mcp_server, config))
        settings_window.settings_saved.connect(_update_overlay)

        # Fallback: If tray is not available, show settings so the app isn't "invisible"
        if not tray.isSystemTrayAvailable():
            print("[Main] System tray not available, opening settings directly.")
            settings_window.show()
        else:
            tray.show()

        # Start AT-SPI2 focus tracker (no-op when pyatspi is not installed)
        if _HAS_ATSPI:
            atspi_context.start()

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
            if _HAS_ATSPI:
                atspi_context.stop()
            listener.stop()
            recorder.stop()
            inference.stop()
            injector.stop()
            dbus_svc.stop()
            tts_engine.shutdown()
            mcp_server.stop()
            for rl in _response_listeners:
                rl.stop()
            if isinstance(overlay_proxy, OverlaySubprocessProxy):
                overlay_proxy.stop()
        except:
            pass


def _reload_routing(router: OutputTargetRouter, start_response_listeners=None) -> None:
    """Hot-reload targets after settings save."""
    try:
        targets = load_targets()
        router.update_targets(targets)
        print(f"[Main] Routing targets reloaded ({len(targets)} targets)")
        if start_response_listeners:
            start_response_listeners(targets)
    except Exception as e:
        print(f"[Main] Could not reload routing targets: {e}")


def _apply_mcp_toggle(mcp_server: WhisperMCPServer, config) -> None:
    """Start or stop the MCP server based on the current config."""
    enabled = config.get("mcp_server_enabled", False)
    if enabled:
        mcp_server.start()
    else:
        mcp_server.stop()


if __name__ == "__main__":
    main()
