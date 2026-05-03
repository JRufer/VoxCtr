import sys
import threading
import queue
import os
import contextlib
from PyQt6.QtWidgets import QApplication
from PyQt6.QtCore import pyqtSignal, QObject, QTimer
from config import Config
from input_listener import InputListener
from audio_recorder import AudioRecorder
from inference_engine import InferenceEngine
from text_injector import TextInjector
from dbus_service import DBusService
from gui.tray_icon import WhisperTrayIcon
from gui.settings_window import SettingsWindow
from gui.waveform_overlay import WaveformOverlay
from gui.history_window import HistoryWindow

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
    recording_started = pyqtSignal()
    recording_stopped = pyqtSignal()
    text_emitted = pyqtSignal(str)
    text_updated = pyqtSignal(str)

def main():
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)

    config = Config()
    state = AppState()
    
    audio_queue = queue.Queue()
    text_queue = queue.Queue()
    realtime_text_queue = queue.Queue()

    # Initialize components
    waveform = WaveformOverlay()
    
    with ignore_stderr():
        recorder = AudioRecorder(config, audio_queue)
    
    recorder.visualizer_callback = waveform.update_audio

    inference = InferenceEngine(config, audio_queue, text_queue, realtime_text_queue)

    def on_injection(total_words, text):
        """Called by TextInjector after each successful injection."""
        # P0.6: tray tooltip
        lang = inference.last_language
        lang_str = f" [{lang.upper()}]" if lang else ""
        tray.setToolTip(f"Whisper Wayland{lang_str} \u2014 {total_words} words this session")
        # P1.2: history panel (thread-safe via QTimer)
        history_window._pending_text = text
        from PyQt6.QtCore import QTimer as _QTimer
        _QTimer.singleShot(0, lambda: history_window.add_entry(history_window._pending_text))
        # P2.3: DBus signal + word count
        dbus_svc.set_word_count(total_words)
        dbus_svc.notify_text(text)

    injector = TextInjector(config, text_queue, word_count_callback=on_injection)

    def on_press():
        print("\n[!] Triggered: Recording...")
        state.recording_started.emit()
        dbus_svc.set_status("recording")
        with ignore_stderr():
            recorder.start_recording()
        inference.set_recording(True)

    def on_release():
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
    # (injection happens on injector thread; status update is thread-safe via dbus)
    _orig_on_injection = on_injection
    def _on_injection_with_idle(total_words, text):
        _orig_on_injection(total_words, text)
        dbus_svc.set_status("idle")
    injector.word_count_callback = _on_injection_with_idle

    listener = InputListener(config, on_press, on_release)

    # Note: real-time text feature is now disabled in the UI based on user request.
    # We keep the inference running but don't show the overlay.

    # UI Toggles
    def show_recording_ui():
        # Only show the waveform as the primary indicator
        waveform.show_mode()

    def hide_recording_ui():
        waveform.hide_mode()

    from PyQt6.QtCore import Qt
    state.recording_started.connect(show_recording_ui, Qt.ConnectionType.QueuedConnection)
    state.recording_stopped.connect(hide_recording_ui, Qt.ConnectionType.QueuedConnection)

    # UI
    history_window = HistoryWindow(config)
    settings_window = SettingsWindow(config, inference)
    tray = WhisperTrayIcon(state, history_window)
    tray.settings_action.triggered.connect(settings_window.show)
    settings_window.settings_saved.connect(listener.update_hotkey)
    settings_window.settings_saved.connect(listener.update_device)
    tray.show()

    # Start threads
    recorder.start()
    inference.start()
    injector.start()
    listener.start()
    dbus_svc.start()  # P2.3: no-op if dbus-python not installed

    print("Whisper-Wayland is running...")

    try:
        sys.exit(app.exec())
    finally:
        print("Shutting down...")
        listener.stop()
        recorder.stop()
        inference.stop()
        injector.stop()
        dbus_svc.stop()  # P2.3

if __name__ == "__main__":
    main()
