import os
import pyaudio
import evdev
import shutil
import grp
from evdev import ecodes
from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QComboBox,
    QPushButton, QMessageBox, QSlider, QCheckBox, QTextEdit,
    QStackedWidget, QTabWidget, QGroupBox, QSizePolicy, QFrame, QScrollArea,
    QLineEdit, QProgressBar, QTableWidget, QHeaderView, QTableWidgetItem,
    QDialog, QDialogButtonBox, QListWidget, QListWidgetItem, QSplitter,
    QSpinBox, QRadioButton, QButtonGroup, QFormLayout,
)
from PyQt6.QtCore import pyqtSignal, Qt, QTimer
from PyQt6.QtGui import QIcon, QFont, QPainter, QLinearGradient, QColor

# ── Palette ────────────────────────────────────────────────────────────────
QSS = """
QWidget {
    background-color: #0f1117;
    color: #e2e8f0;
    font-family: 'Segoe UI', 'Inter', 'Ubuntu', sans-serif;
    font-size: 13px;
}
QPushButton#nav_btn {
    background: transparent;
    border: none;
    border-radius: 6px;
    padding: 9px 14px;
    color: #8892a4;
    font-weight: 500;
    text-align: left;
}
QPushButton#nav_btn:hover {
    background: #1a1f2e;
    color: #c8d3e0;
    border: none;
}
QPushButton#nav_btn:checked {
    background: #1a1f2e;
    color: #4a9eff;
    border-left: 2px solid #4a9eff;
    border-radius: 0px 6px 6px 0px;
    padding-left: 12px;
}
QFrame#nav_panel {
    background: #0a0d14;
    border-right: 1px solid #1e2433;
}
QGroupBox {
    border: 1px solid #1e2433;
    border-radius: 8px;
    margin-top: 14px;
    padding: 12px 10px 10px 10px;
    font-weight: 600;
    color: #8892a4;
    font-size: 11px;
    letter-spacing: 0.8px;
    text-transform: uppercase;
}
QGroupBox::title {
    subcontrol-origin: margin;
    subcontrol-position: top left;
    padding: 0 6px;
    left: 10px;
}
QComboBox {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 7px 10px;
    color: #e2e8f0;
    min-height: 20px;
}
QComboBox:focus { border-color: #4a9eff; }
QComboBox::drop-down { border: none; width: 24px; }
QComboBox::down-arrow { image: none; width: 0; }
QComboBox QAbstractItemView {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    selection-background-color: #4a9eff;
    color: #e2e8f0;
}
QCheckBox { spacing: 8px; color: #c8d3e0; }
QCheckBox::indicator {
    width: 18px; height: 18px;
    border: 1px solid #2a3448;
    border-radius: 4px;
    background: #1a1f2e;
}
QCheckBox::indicator:checked {
    background: #4a9eff;
    border-color: #4a9eff;
    image: none;
}
QSlider::groove:horizontal {
    height: 4px;
    background: #2a3448;
    border-radius: 2px;
}
QSlider::handle:horizontal {
    background: #4a9eff;
    width: 16px; height: 16px;
    margin: -6px 0;
    border-radius: 8px;
}
QSlider::sub-page:horizontal { background: #4a9eff; border-radius: 2px; }
QTextEdit {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 6px;
    color: #e2e8f0;
}
QTextEdit:focus { border-color: #4a9eff; }
QPushButton {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 8px 16px;
    color: #c8d3e0;
    font-weight: 500;
}
QPushButton:hover { background: #242b3d; border-color: #4a9eff; color: #e2e8f0; }
QPushButton:pressed { background: #4a9eff; color: #fff; }
QPushButton#btn_save {
    background: #4a9eff;
    border: none;
    color: #fff;
    font-weight: 600;
    padding: 10px 28px;
    font-size: 14px;
}
QPushButton#btn_save:hover { background: #6aafff; }
QPushButton#btn_cancel { color: #8892a4; }
QLabel#hint {
    color: #4a5568;
    font-size: 11px;
    padding-left: 2px;
}
QLabel#section_head {
    color: #e2e8f0;
    font-weight: 600;
    font-size: 13px;
}
QFrame#hotkey_pill {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 6px 10px;
}
QPushButton#btn_record {
    background: transparent;
    border: 1px solid #4a9eff;
    color: #4a9eff;
    border-radius: 6px;
    padding: 6px 14px;
}
QPushButton#btn_record:checked {
    background: #4a9eff22;
    color: #6aafff;
}
QScrollArea { border: none; }
"""


def _hint(text):
    lbl = QLabel(text)
    lbl.setObjectName("hint")
    lbl.setWordWrap(True)
    return lbl


def _hint_warn(text):
    """Like _hint but styled in amber to draw attention to a warning."""
    lbl = QLabel(text)
    lbl.setWordWrap(True)
    lbl.setStyleSheet("color: #f59e0b; font-size: 11px;")
    return lbl


def _section(title):
    box = QGroupBox(title)
    box.setLayout(QVBoxLayout())
    box.layout().setSpacing(8)
    box.layout().setContentsMargins(12, 18, 12, 12)
    return box


MODE_LABELS = {
    "clean": "✨  Fix grammar (Clean)",
    "formal": "👔  Formal rewriting",
    "casual": "🍕  Casual rewriting",
    "bullet": "📝  Bullet points",
    "concise": "✂️  Concise mode"
}


class VUMeter(QWidget):
    def __init__(self):
        super().__init__()
        self.setMinimumHeight(12)
        self.setMaximumHeight(12)
        self._level = 0.0  # 0.0 to 1.0
        self._peak = 0.0
        self._peak_decay = 0.005
        
        self.timer = QTimer(self)
        self.timer.timeout.connect(self._decay_peak)
        self.timer.start(30)

    def set_level(self, level):
        """level should be 0.0 to 1.0 (RMS or Peak)"""
        self._level = min(1.0, max(0.0, level))
        if self._level > self._peak:
            self._peak = self._level
        self.update()

    def _decay_peak(self):
        if self._peak > 0:
            self._peak = max(0, self._peak - self._peak_decay)
            self.update()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        
        # Background
        bg_rect = self.rect()
        painter.fillRect(bg_rect, QColor("#1e293b"))
        
        # Level bar
        w = int(bg_rect.width() * self._level)
        if w > 0:
            grad = QLinearGradient(0, 0, bg_rect.width(), 0)
            grad.setColorAt(0.0, QColor("#4ade80")) # Green
            grad.setColorAt(0.7, QColor("#facc15")) # Yellow
            grad.setColorAt(1.0, QColor("#f87171")) # Red
            
            painter.fillRect(0, 0, w, bg_rect.height(), grad)
            
        # Peak indicator
        px = int(bg_rect.width() * self._peak)
        if px > 0:
            painter.setPen(QColor("#ffffff"))
            painter.drawLine(px, 0, px, bg_rect.height())


class SettingsWindow(QWidget):
    settings_saved = pyqtSignal()
    _hw_probe_finished = pyqtSignal(str, str)       # (hw_text, backend_text)
    _tts_progress_sig = pyqtSignal(int, int)        # (bytes_done, bytes_total)
    _tts_download_ok_sig = pyqtSignal()
    _tts_download_err_sig = pyqtSignal(str)
    _tts_test_done_sig = pyqtSignal(str)        # status message (empty = success)
    _cpp_download_ok_sig = pyqtSignal(str)          # filename
    _cpp_download_err_sig = pyqtSignal(str)

    def __init__(self, config, inference_engine=None, audio_recorder=None, overlay_manager=None):
        super().__init__()
        self.config = config
        self.inference_engine = inference_engine
        self.audio_recorder = audio_recorder
        self.overlay_manager = overlay_manager
        self._hw_probe_finished.connect(self._on_hw_probe_finished)
        self.recorded_keys = set()
        self.recorded_toggle_keys = set()
        self.recorded_dt_keys = set()
        self.active_recording_mode = None
        self.router = None
        self._tts_stop_recording = False
        self._recorded_tts_stop_keys: set = set()
        
        from routing.loader import load_bindings
        try:
            self._routing_bindings = load_bindings()
        except Exception:
            self._routing_bindings = []

        self.setWindowTitle("Whisper Wayland — Settings")
        self.setMinimumWidth(520)
        self.setMinimumHeight(580)
        self.setStyleSheet(QSS)

        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        icon_path = os.path.join(base_dir, "assets", "app_icon.png")
        if os.path.exists(icon_path):
            self.setWindowIcon(QIcon(icon_path))

        root = QVBoxLayout(self)
        root.setContentsMargins(16, 16, 16, 16)
        root.setSpacing(12)

        # ── Header ────────────────────────────────────────────────────────
        header = QLabel("⚙  Settings")
        header.setFont(QFont("Segoe UI", 16, QFont.Weight.Bold))
        header.setStyleSheet("color: #e2e8f0; padding-bottom: 4px;")
        root.addWidget(header)

        if inference_engine:
            backend_label = getattr(inference_engine, "active_backend_name", inference_engine.actual_device)
            status = QLabel(
                f"  {backend_label} · {config.get('model_size')} · "
                f"{inference_engine.actual_device.upper()} · "
                f"{inference_engine.actual_compute_type}"
            )
        else:
            status = QLabel("  Engine not connected")
        status.setObjectName("hint")
        root.addWidget(status)

        # ── Sidebar nav + content stack ───────────────────────────────────
        body = QHBoxLayout()
        body.setSpacing(0)
        body.setContentsMargins(0, 0, 0, 0)

        # Left sidebar
        nav_panel = QFrame()
        nav_panel.setObjectName("nav_panel")
        nav_panel.setFixedWidth(130)
        nav_layout = QVBoxLayout(nav_panel)
        nav_layout.setContentsMargins(0, 8, 0, 8)
        nav_layout.setSpacing(2)

        # Content stack (right side)
        self._stack = QStackedWidget()

        _nav_items = [
            ("🎙", "General",   self._tab_general),
            ("⚡", "Engine",    self._tab_engine),
            ("🔊", "Audio",     self._tab_audio),
            ("⌨", "Hotkeys",   self._tab_hotkeys),
            ("✨", "Dictation", self._tab_dictation),
            ("🎨", "Appearance", self._tab_appearance),
            ("📎", "Snippets",  self._tab_snippets),
            ("🤖", "AI",        self._tab_ai),
            ("🔀", "Routing",   self._tab_routing),
            ("🔈", "Voice Out", self._tab_voice_output),
        ]

        self._nav_buttons = []
        for i, (icon, label, builder) in enumerate(_nav_items):
            self._stack.addWidget(builder())
            btn = QPushButton(f"{icon}  {label}")
            btn.setObjectName("nav_btn")
            btn.setCheckable(True)
            btn.setFlat(True)
            btn.clicked.connect(lambda _, idx=i: self._switch_nav(idx))
            nav_layout.addWidget(btn)
            self._nav_buttons.append(btn)

        nav_layout.addStretch()
        self._nav_buttons[0].setChecked(True)

        body.addWidget(nav_panel)
        body.addWidget(self._stack, 1)
        root.addLayout(body)

        # ── Footer buttons ────────────────────────────────────────────────
        footer = QHBoxLayout()
        footer.setSpacing(10)
        self.check_btn = QPushButton("🔍  System Check")
        self.check_btn.clicked.connect(self.run_system_check)
        footer.addWidget(self.check_btn)
        footer.addStretch()

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setObjectName("btn_cancel")
        cancel_btn.clicked.connect(self.close)

        save_btn = QPushButton("Save Settings")
        save_btn.setObjectName("btn_save")
        save_btn.clicked.connect(self.save_settings)

        footer.addWidget(cancel_btn)
        footer.addWidget(save_btn)
        root.addLayout(footer)

        self.monitor_timer = QTimer(self)
        self.monitor_timer.timeout.connect(self._monitor_audio)
        self.monitor_timer.start(50)

    def showEvent(self, event):
        """Start monitoring mic levels when window opens."""
        super().showEvent(event)
        if self.audio_recorder:
            self.audio_recorder.start_monitoring()
        self.monitor_timer.start(50)

    def closeEvent(self, event):
        """Stop monitoring when window closes."""
        if self.audio_recorder:
            self.audio_recorder.stop_monitoring()
        self.monitor_timer.stop()
        super().closeEvent(event)

    def _monitor_audio(self):
        """Update VU meter with real-time RMS levels."""
        if self.audio_recorder:
            level = self.audio_recorder.get_rms_level()
            if hasattr(self, 'vu_meter'):
                # Peak scaling for visibility
                scaled = min(1.0, level * 5.0) 
                self.vu_meter.set_level(scaled)

    def _switch_nav(self, idx: int):
        self._stack.setCurrentIndex(idx)
        for i, btn in enumerate(self._nav_buttons):
            btn.setChecked(i == idx)

    # ── Tab: General ──────────────────────────────────────────────────────
    def _tab_general(self):
        w = self._scrollable()
        lay = w.layout()

        # Model
        model_box = _section("Whisper Model")
        model_box.layout().addWidget(_hint(
            "Larger models are more accurate but slower. 'base' works well for most users."
        ))
        self.model_combo = QComboBox()
        self.model_combo.addItems(["tiny", "base", "small", "medium", "large-v3"])
        self.model_combo.setCurrentText(self.config.get("model_size", "base"))
        model_box.layout().addWidget(self.model_combo)
        lay.addWidget(model_box)

        # Device
        device_box = _section("Processing Device")
        device_box.layout().addWidget(_hint(
            "Use 'auto' to let the app choose. Select 'cuda' if you have an NVIDIA GPU for faster transcription."
        ))
        h = QHBoxLayout()
        h.setSpacing(8)
        self.device_type_combo = QComboBox()
        self.device_type_combo.addItems(["auto", "cuda", "cpu"])
        self.device_type_combo.setCurrentText(self.config.get("device", "auto"))
        self.inference_mode_combo = QComboBox()
        self.inference_mode_combo.addItems(["Balanced", "Aggressive"])
        self.inference_mode_combo.setCurrentText(self.config.get("inference_mode", "Balanced"))
        h.addWidget(QLabel("Hardware:"))
        h.addWidget(self.device_type_combo, 1)
        h.addWidget(QLabel("Real-time:"))
        h.addWidget(self.inference_mode_combo, 1)
        device_box.layout().addLayout(h)
        device_box.layout().addWidget(_hint(
            "'Aggressive' real-time mode transcribes more often while you speak (uses more CPU)."
        ))
        lay.addWidget(device_box)

        lay.addStretch()
        return w

    # ── Tab: Engine ───────────────────────────────────────────────────────
    def _tab_engine(self):
        import threading
        w = self._scrollable()
        lay = w.layout()

        # ── Engine selector ────────────────────────────────────────────────
        engine_box = _section("Transcription Backend")
        engine_box.layout().addWidget(_hint(
            "Auto selects the best backend for your GPU.\n"
            "faster-whisper: best for NVIDIA CUDA.\n"
            "whisper-cpp: enables AMD/Intel GPU via Vulkan."
        ))
        self.engine_combo = QComboBox()
        self.engine_combo.addItems(["auto", "faster-whisper", "whisper-cpp"])
        self.engine_combo.setCurrentText(self.config.get("backend_engine", "auto"))
        self.engine_combo.currentTextChanged.connect(self._on_engine_changed)
        engine_box.layout().addWidget(self.engine_combo)
        lay.addWidget(engine_box)

        # ── Detected hardware panel ────────────────────────────────────────
        hw_box = _section("Detected Hardware")
        hw_row = QHBoxLayout()
        self._hw_label = QLabel("⏳  Probing…")
        self._hw_label.setWordWrap(True)
        self._hw_label.setStyleSheet("font-size: 12px; background: transparent; border: none;")
        hw_row.addWidget(self._hw_label, 1)
        refresh_btn = QPushButton("Refresh")
        refresh_btn.clicked.connect(self._refresh_hardware)
        hw_row.addWidget(refresh_btn)
        hw_box.layout().addLayout(hw_row)

        self._active_backend_label = QLabel("")
        self._active_backend_label.setObjectName("hint")
        hw_box.layout().addWidget(self._active_backend_label)
        lay.addWidget(hw_box)

        # ── whisper.cpp settings ───────────────────────────────────────────
        self._cpp_box = _section("whisper.cpp Settings")
        self._cpp_box.layout().addWidget(_hint(
            "These settings apply when the whisper-cpp backend is active."
        ))

        # Binary path
        bin_row = QHBoxLayout()
        bin_row.addWidget(QLabel("Binary:"))
        self.cpp_binary_edit = QLineEdit(self.config.get("whisper_cpp_binary", "whisper-cli"))
        self.cpp_binary_edit.setPlaceholderText("whisper-cli  (must be on $PATH or full path)")
        bin_row.addWidget(self.cpp_binary_edit, 1)
        self._cpp_box.layout().addLayout(bin_row)

        # Model size for whisper.cpp
        cpp_model_row = QHBoxLayout()
        cpp_model_row.addWidget(QLabel("Model:"))
        self.cpp_model_combo = QComboBox()
        from backends.whisper_cpp_backend import GGUF_MAP
        self.cpp_model_combo.addItems(list(GGUF_MAP.keys()))
        self.cpp_model_combo.setCurrentText(
            self.config.get("whisper_cpp_model_size", "large-v3")
        )
        cpp_model_row.addWidget(self.cpp_model_combo, 1)
        self._cpp_box.layout().addLayout(cpp_model_row)

        # Device
        cpp_dev_row = QHBoxLayout()
        cpp_dev_row.addWidget(QLabel("Device:"))
        self.cpp_device_combo = QComboBox()
        self.cpp_device_combo.addItems(["auto", "vulkan", "cuda", "cpu"])
        self.cpp_device_combo.setCurrentText(
            self.config.get("whisper_cpp_device", "auto")
        )
        cpp_dev_row.addWidget(self.cpp_device_combo, 1)
        self._cpp_box.layout().addLayout(cpp_dev_row)

        # Thread count
        threads_row = QHBoxLayout()
        threads_row.addWidget(QLabel("CPU threads:"))
        from PyQt6.QtWidgets import QSpinBox
        self.cpp_threads_spin = QSpinBox()
        self.cpp_threads_spin.setRange(0, 32)
        self.cpp_threads_spin.setSpecialValueText("Auto")
        self.cpp_threads_spin.setValue(self.config.get("whisper_cpp_threads", 0))
        self.cpp_threads_spin.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:4px 8px; color:#e2e8f0;"
        )
        threads_row.addWidget(self.cpp_threads_spin)
        threads_row.addStretch()
        self._cpp_box.layout().addLayout(threads_row)

        # Model download status
        self._cpp_model_status = QLabel("")
        self._cpp_model_status.setObjectName("hint")
        self._cpp_model_status.setWordWrap(True)
        self._cpp_box.layout().addWidget(self._cpp_model_status)

        # Download button
        self._cpp_download_btn = QPushButton("Download Selected Model")
        self._cpp_download_btn.clicked.connect(self._download_cpp_model)
        self._cpp_box.layout().addWidget(self._cpp_download_btn)

        lay.addWidget(self._cpp_box)

        # ── pywhispercpp binding status ────────────────────────────────────
        binding_box = _section("pywhispercpp Binding (Optional)")
        try:
            import pywhispercpp  # noqa: F401
            binding_label = QLabel("✅  pywhispercpp installed — in-process mode active (lower latency)")
            binding_label.setStyleSheet("color: #4ade80; background: transparent; border: none; font-size: 12px;")
        except ImportError:
            binding_label = QLabel(
                "ℹ️  pywhispercpp not installed — subprocess mode will be used.\n"
                "Install for lower latency: <b>./venv/bin/pip install pywhispercpp</b>"
            )
            binding_label.setStyleSheet("color: #8892a4; background: transparent; border: none; font-size: 12px;")
        binding_label.setWordWrap(True)
        binding_box.layout().addWidget(binding_label)
        lay.addWidget(binding_box)

        lay.addStretch()

        # Populate hardware info asynchronously
        threading.Thread(target=self._probe_hardware_async, daemon=True).start()
        # Show/hide cpp settings depending on current selection
        self._on_engine_changed(self.engine_combo.currentText())

        return w

    def _on_engine_changed(self, engine: str):
        show_cpp = engine in ("whisper-cpp", "auto")
        if hasattr(self, "_cpp_box"):
            self._cpp_box.setVisible(show_cpp)

    def _refresh_hardware(self):
        if hasattr(self, "_hw_label"):
            self._hw_label.setText("⏳  Probing…")
        threading.Thread(target=self._probe_hardware_async, daemon=True, name="hw-probe").start()

    def _probe_hardware_async(self):
        try:
            from backends.selector import probe_gpu, _cuda_available, _vulkan_available
            gpu = probe_gpu()
            cuda = _cuda_available()
            vulkan = _vulkan_available()

            if gpu:
                vram_str = f", {gpu.vram_mb} MB VRAM" if gpu.vram_mb else ""
                api_str = gpu.api.upper()
                hw_text = (
                    f"GPU: {gpu.vendor.upper()}{vram_str}  |  "
                    f"API: {api_str}  |  "
                    f"CUDA: {'yes' if cuda else 'no'}  |  "
                    f"Vulkan: {'yes' if vulkan else 'no'}"
                )
            else:
                hw_text = f"No GPU detected  |  CUDA: {'yes' if cuda else 'no'}  |  Vulkan: {'yes' if vulkan else 'no'}"

            backend_text = ""
            if self.inference_engine:
                backend_text = (
                    f"Active backend: {self.inference_engine.active_backend_name}  |  "
                    f"Device: {self.inference_engine.actual_device}  |  "
                    f"Compute: {self.inference_engine.actual_compute_type}"
                )
            self._hw_probe_finished.emit(hw_text, backend_text)
        except Exception as e:
            print(f"[Settings] HW Probe error: {e}")
            self._hw_probe_finished.emit(f"Probe failed: {e}", "")

    def _on_hw_probe_finished(self, hw_text, backend_text):
        if hasattr(self, "_hw_label"):
            self._hw_label.setText(hw_text)
        if hasattr(self, "_active_backend_label"):
            self._active_backend_label.setText(backend_text)
        self._update_cpp_model_status()

    def _update_cpp_model_status(self):
        if not hasattr(self, "cpp_model_combo"):
            return
        from backends.whisper_cpp_backend import WhisperCppBackend
        import os

        model_dir = self.config.get("whisper_cpp_model_dir", "") or os.path.join(
            os.path.expanduser("~"), ".local", "share", "whisper-wayland", "models"
        )
        cpp = WhisperCppBackend(model_dir=model_dir)
        downloaded = cpp.list_downloaded_models()
        selected = self.cpp_model_combo.currentText()

        if selected in downloaded:
            self._cpp_model_status.setText(f"✅  {selected} is downloaded and ready.")
            self._cpp_model_status.setStyleSheet("color: #4ade80; background: transparent; border: none;")
        else:
            self._cpp_model_status.setText(
                f"⚠  {selected} not found in {model_dir}.\n"
                f"Click 'Download Selected Model' to fetch it."
            )
            self._cpp_model_status.setStyleSheet("color: #facc15; background: transparent; border: none;")

    def _download_cpp_model(self):
        import threading
        from backends.whisper_cpp_backend import WhisperCppBackend, GGUF_BASE_URL
        import os
        import urllib.request

        model_size = self.cpp_model_combo.currentText()
        model_dir = self.config.get("whisper_cpp_model_dir", "") or os.path.join(
            os.path.expanduser("~"), ".local", "share", "whisper-wayland", "models"
        )
        cpp = WhisperCppBackend(model_dir=model_dir)
        url = cpp.get_model_url(model_size)

        from backends.whisper_cpp_backend import GGUF_MAP
        candidates = GGUF_MAP.get(model_size, [f"ggml-{model_size}.bin"])
        filename = candidates[0] if isinstance(candidates, list) else candidates
        dest = os.path.join(model_dir, filename)

        if not hasattr(self, "_cpp_download_btn"):
            return

        self._cpp_download_btn.setEnabled(False)
        self._cpp_model_status.setText(f"⏳  Downloading {filename}…")
        self._cpp_model_status.setStyleSheet("color: #e2e8f0; background: transparent; border: none;")

        try:
            self._cpp_download_ok_sig.disconnect()
            self._cpp_download_err_sig.disconnect()
        except (RuntimeError, TypeError):
            pass

        self._cpp_download_ok_sig.connect(
            lambda fn: (
                self._cpp_model_status.setText(f"✅  Downloaded {fn} successfully."),
                self._cpp_model_status.setStyleSheet("color: #4ade80; background: transparent; border: none;"),
                self._cpp_download_btn.setEnabled(True),
            )
        )
        self._cpp_download_err_sig.connect(
            lambda msg: (
                self._cpp_model_status.setText(f"❌  Download failed: {msg}"),
                self._cpp_model_status.setStyleSheet("color: #f87171; background: transparent; border: none;"),
                self._cpp_download_btn.setEnabled(True),
            )
        )

        def do_download():
            try:
                os.makedirs(model_dir, exist_ok=True)
                urllib.request.urlretrieve(url, dest)
                self._cpp_download_ok_sig.emit(filename)
            except Exception as e:
                self._cpp_download_err_sig.emit(str(e))

        threading.Thread(target=do_download, daemon=True).start()

    # ── Tab: Audio ────────────────────────────────────────────────────────
    def _tab_audio(self):
        w = self._scrollable()
        lay = w.layout()

        mic_box = _section("Microphone")
        mic_box.layout().addWidget(_hint("Select the microphone you want to use for dictation."))
        self.device_combo = QComboBox()
        self.populate_audio_devices()
        mic_box.layout().addWidget(self.device_combo)
        
        self.vu_meter = VUMeter()
        mic_box.layout().addWidget(self.vu_meter)
        lay.addWidget(mic_box)

        gain_box = _section("Microphone Boost")
        gain_box.layout().addWidget(_hint(
            "Increase if Whisper can't hear you clearly. 1.0× = no boost."
        ))
        gain_row = QHBoxLayout()
        self.gain_slider = QSlider(Qt.Orientation.Horizontal)
        self.gain_slider.setRange(5, 50)
        current_gain = self.config.get("mic_gain", 1.0)
        self.gain_slider.setValue(int(current_gain * 10))
        self.gain_label = QLabel(f"{current_gain:.1f}×")
        self.gain_label.setMinimumWidth(38)
        self.gain_slider.valueChanged.connect(
            lambda v: self.gain_label.setText(f"{v / 10.0:.1f}×")
        )
        gain_row.addWidget(self.gain_slider)
        gain_row.addWidget(self.gain_label)
        gain_box.layout().addLayout(gain_row)
        lay.addWidget(gain_box)

        kbd_box = _section("Keyboard Device (Advanced)")
        kbd_box.layout().addWidget(_hint(
            "The evdev keyboard device used to detect your hotkeys. Usually auto-detected correctly."
        ))
        self.kbd_combo = QComboBox()
        self.populate_keyboard_devices()
        kbd_box.layout().addWidget(self.kbd_combo)
        lay.addWidget(kbd_box)

        # P2.2: Noise Suppression
        from audio_recorder import _HAS_NOISEREDUCE
        nr_box = _section("Noise Suppression")
        if _HAS_NOISEREDUCE:
            nr_status = "✅  noisereduce installed"
            nr_style = "color: #4ade80;"
        else:
            nr_status = "❌  noisereduce not installed  —  pip install noisereduce"
            nr_style = "color: #f87171;"
        nr_status_label = QLabel(nr_status)
        nr_status_label.setStyleSheet(f"{nr_style} background: transparent; border: none; font-size: 12px;")
        nr_box.layout().addWidget(nr_status_label)

        self.noise_suppression_cb = QCheckBox(
            "Enable noise suppression  (reduces background hiss and room noise)"
        )
        self.noise_suppression_cb.setChecked(
            self.config.get("noise_suppression", False) and _HAS_NOISEREDUCE
        )
        self.noise_suppression_cb.setEnabled(_HAS_NOISEREDUCE)
        nr_box.layout().addWidget(self.noise_suppression_cb)
        nr_box.layout().addWidget(_hint(
            "Runs a stationary noise profile filter on each audio chunk before transcription.\n"
            "Improves accuracy in noisy environments. Adds ~10–20ms of processing latency."
        ))
        lay.addWidget(nr_box)

        lay.addStretch()
        return w


    # ── Tab: Hotkeys ──────────────────────────────────────────────────────
    def _tab_hotkeys(self):
        """Full binding manager — create, edit, disable and assign all hotkey bindings."""
        tab = QWidget()
        lay = QVBoxLayout(tab)
        lay.setContentsMargins(12, 12, 12, 12)
        lay.setSpacing(10)

        lay.addWidget(_hint(
            "Create hotkey bindings and assign them to output targets.  "
            "Each binding can use a different gesture and be independently enabled or disabled.  "
            "Bindings are saved to ~/.config/whisper-wayland/bindings.toml."
        ))

        # ── Gesture guide ─────────────────────────────────────────────────
        guide = QGroupBox("Gesture Types")
        guide_lay = QFormLayout(guide)
        guide_lay.setSpacing(4)
        for name, desc in [
            ("hold",       "Hold keys → record; release → transcribe"),
            ("toggle",     "Tap once to start, tap again to stop"),
            ("double_tap", "Double-tap keys to start recording"),
            ("chord",      "Press an additional key while base keys are held"),
        ]:
            lbl = QLabel(name)
            lbl.setStyleSheet("color:#4a9eff; font-weight:600;")
            guide_lay.addRow(lbl, QLabel(desc))
        lay.addWidget(guide)

        # ── Binding list ──────────────────────────────────────────────────
        head = QLabel("Hotkey Bindings")
        head.setObjectName("section_head")
        lay.addWidget(head)

        self._binding_list = QListWidget()
        self._binding_list.setAlternatingRowColors(True)
        lay.addWidget(self._binding_list, 1)

        # ── Buttons ───────────────────────────────────────────────────────
        btn_row = QHBoxLayout()
        for label, slot in [
            ("Add",     self._hotkey_add_binding),
            ("Edit",    self._hotkey_edit_binding),
            ("Delete",  self._hotkey_delete_binding),
            ("Toggle Disabled", self._hotkey_toggle_disabled),
        ]:
            btn = QPushButton(label)
            btn.setFixedHeight(30)
            btn.clicked.connect(slot)
            btn_row.addWidget(btn)
        lay.addLayout(btn_row)

        # Load bindings (targets must be loaded first for the editor combo)
        try:
            from routing.loader import load_targets, load_bindings
            if not hasattr(self, '_routing_targets') or not self._routing_targets:
                self._routing_targets = load_targets()
            if not hasattr(self, '_routing_bindings') or not self._routing_bindings:
                self._routing_bindings = load_bindings()
        except Exception:
            pass

        self._routing_render_bindings()
        return tab

    # ── Hotkey binding CRUD (used from Hotkeys tab) ───────────────────────────

    def _hotkey_add_binding(self):
        if not getattr(self, '_routing_targets', []):
            QMessageBox.warning(self, "No Targets",
                                "Add at least one output target in the Routing tab first.")
            return
        dlg = _BindingEditorDialog(targets=self._routing_targets, parent=self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            self._routing_bindings.append(dlg.result_binding)
            self._routing_save_bindings()
            self._routing_render_bindings()
            self.settings_saved.emit()

    def _hotkey_edit_binding(self):
        b = self._routing_selected_binding()
        if b is None:
            return
        dlg = _BindingEditorDialog(binding=b, targets=self._routing_targets, parent=self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            idx = next((i for i, x in enumerate(self._routing_bindings) if x.id == b.id), None)
            if idx is not None:
                self._routing_bindings[idx] = dlg.result_binding
            self._routing_save_bindings()
            self._routing_render_bindings()
            self.settings_saved.emit()

    def _hotkey_delete_binding(self):
        b = self._routing_selected_binding()
        if b is None:
            return
        if QMessageBox.question(
            self, "Delete Binding",
            f"Delete binding '{b.label or b.id}'?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        ) == QMessageBox.StandardButton.Yes:
            self._routing_bindings = [x for x in self._routing_bindings if x.id != b.id]
            self._routing_save_bindings()
            self._routing_render_bindings()
            self.settings_saved.emit()

    def _hotkey_toggle_disabled(self):
        """Toggle the disabled flag on the selected binding without opening the editor."""
        b = self._routing_selected_binding()
        if b is None:
            return
        idx = next((i for i, x in enumerate(self._routing_bindings) if x.id == b.id), None)
        if idx is not None:
            self._routing_bindings[idx].disabled = not self._routing_bindings[idx].disabled
            self._routing_save_bindings()
            self._routing_render_bindings()
            self.settings_saved.emit()

    # ── Tab: Dictation ────────────────────────────────────────────────────
    def _tab_dictation(self):
        w = self._scrollable()
        lay = w.layout()

        cleanup_box = _section("Text Cleanup")
        self.filler_checkbox = QCheckBox("Remove filler words  (um, uh, hmm, er, ah)")
        self.filler_checkbox.setChecked(self.config.get("remove_fillers", True))
        self.spoken_punct_checkbox = QCheckBox(
            'Spoken punctuation  — say "period", "comma", "new line", etc.'
        )
        self.spoken_punct_checkbox.setChecked(self.config.get("spoken_punctuation", True))
        self.auto_list_checkbox = QCheckBox(
            'Auto-format lists  — "1. Apples 2. Bananas" becomes a real list'
        )
        self.auto_list_checkbox.setChecked(self.config.get("auto_format_lists", True))
        for cb in (self.filler_checkbox, self.spoken_punct_checkbox, self.auto_list_checkbox):
            cleanup_box.layout().addWidget(cb)
        lay.addWidget(cleanup_box)

        recording_box = _section("Recording Modes")
        self.quiet_mode_checkbox = QCheckBox(
            "Quiet / whisper mode  — lower VAD sensitivity + boost for soft speech"
        )
        self.quiet_mode_checkbox.setChecked(self.config.get("quiet_mode", False))
        self.notification_checkbox = QCheckBox(
            "Desktop notification after each transcription  (requires notify-send)"
        )
        self.notification_checkbox.setChecked(self.config.get("show_notification", False))

        self.code_mode_checkbox = QCheckBox(
            "💻  Code / developer mode  — disables normal formatting; speaks underscores, dots, parens"
        )
        self.code_mode_checkbox.setChecked(
            self.config.get("dictation_mode", "normal") == "code"
        )
        for cb in (self.quiet_mode_checkbox,
                   self.notification_checkbox, self.code_mode_checkbox):
            recording_box.layout().addWidget(cb)
        recording_box.layout().addWidget(_hint(
            "Code mode example: say \"get underscore user dot name\" → get_user.name"
        ))
        lay.addWidget(recording_box)

        lay.addStretch()
        return w

    # ── Tab: Appearance ───────────────────────────────────────────────────
    def _tab_appearance(self):
        w = self._scrollable()
        lay = w.layout()

        overlay_box = _section("Recording Overlay")
        self.overlay_checkbox = QCheckBox("Show real-time visualizer while recording")
        self.overlay_checkbox.setChecked(self.config.get("show_overlay", True))
        overlay_box.layout().addWidget(self.overlay_checkbox)

        overlay_box.layout().addWidget(_hint(
            "Choose the visual style shown while recording. "
            "Drop custom .py files in the overlays folder to add your own."
        ))
        overlay_row = QHBoxLayout()
        self.overlay_style_combo = QComboBox()
        self._populate_overlay_styles()
        overlay_row.addWidget(self.overlay_style_combo, 1)
        open_dir_btn = QPushButton("Open Overlays Folder")
        open_dir_btn.clicked.connect(self._open_overlays_folder)
        overlay_row.addWidget(open_dir_btn)
        overlay_box.layout().addLayout(overlay_row)

        self._overlay_desc_label = QLabel("")
        self._overlay_desc_label.setObjectName("hint")
        self._overlay_desc_label.setWordWrap(True)
        overlay_box.layout().addWidget(self._overlay_desc_label)

        self.overlay_style_combo.currentIndexChanged.connect(self._on_overlay_selected)
        self._on_overlay_selected()
        lay.addWidget(overlay_box)

        vocab_box = _section("Custom Vocabulary")
        vocab_box.layout().addWidget(_hint(
            "Add words or phrases Whisper should recognise, comma-separated. "
            "Use for names, jargon, product names, etc."
        ))
        self.vocab_edit = QTextEdit()
        self.vocab_edit.setPlaceholderText("e.g. PyQt6, Wayland, kubernetes, Josh Rufer")
        self.vocab_edit.setFixedHeight(68)
        vocab_list = self.config.get("custom_vocabulary", [])
        self.vocab_edit.setPlainText(", ".join(vocab_list))
        vocab_box.layout().addWidget(self.vocab_edit)
        lay.addWidget(vocab_box)

        lay.addStretch()
        return w

    # ── Tab: Snippets ─────────────────────────────────────────────────────
    def _tab_snippets(self):
        w = self._scrollable()
        lay = w.layout()

        info_box = _section("Voice Shortcuts")
        info_box.layout().addWidget(_hint(
            "Define trigger phrases that expand to full text when spoken.\n"
            "Example: say \"my email\" → inserts your full email address.\n"
            "Triggers are case-insensitive and matched after transcription."
        ))
        lay.addWidget(info_box)

        # Snippet rows container
        self._snippet_rows = []
        self._snippets_container = QWidget()
        self._snippets_layout = QVBoxLayout(self._snippets_container)
        self._snippets_layout.setContentsMargins(0, 0, 0, 0)
        self._snippets_layout.setSpacing(6)

        # Header row
        header = QHBoxLayout()
        header.addWidget(QLabel("Trigger phrase"), 1)
        header.addWidget(QLabel("Expands to"), 2)
        header.addWidget(QLabel(""), 0)  # spacer for delete button column
        self._snippets_layout.addLayout(header)

        # Pre-populate from config
        saved = self.config.get("snippets", {})
        for trigger, expansion in saved.items():
            self._add_snippet_row(trigger, expansion)

        lay.addWidget(self._snippets_container)

        add_btn = QPushButton("＋  Add Snippet")
        add_btn.clicked.connect(lambda: self._add_snippet_row("", ""))
        lay.addWidget(add_btn)

        lay.addStretch()
        return w

    def _add_snippet_row(self, trigger="", expansion=""):
        row_widget = QWidget()
        row_layout = QHBoxLayout(row_widget)
        row_layout.setContentsMargins(0, 0, 0, 0)
        row_layout.setSpacing(8)

        trigger_edit = QLineEdit(trigger)
        trigger_edit.setPlaceholderText("e.g. my email")
        trigger_edit.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:6px 8px; color:#e2e8f0;"
        )
        expansion_edit = QLineEdit(expansion)
        expansion_edit.setPlaceholderText("e.g. john.doe@example.com")
        expansion_edit.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:6px 8px; color:#e2e8f0;"
        )

        del_btn = QPushButton("✕")
        del_btn.setFixedWidth(30)
        del_btn.setStyleSheet(
            "QPushButton { background:transparent; border:none; color:#4a5568; font-size:14px; }"
            "QPushButton:hover { color:#f87171; }"
        )

        row_layout.addWidget(trigger_edit, 1)
        row_layout.addWidget(expansion_edit, 2)
        row_layout.addWidget(del_btn)

        row_data = {"widget": row_widget, "trigger": trigger_edit, "expansion": expansion_edit}
        self._snippet_rows.append(row_data)
        self._snippets_layout.addWidget(row_widget)

        def remove():
            self._snippet_rows.remove(row_data)
            self._snippets_layout.removeWidget(row_widget)
            row_widget.deleteLater()
        del_btn.clicked.connect(remove)

    # ── Tab: AI (Ollama) ──────────────────────────────────────────────────
    def _tab_ai(self):
        from llm_postprocessor import LLMPostprocessor, MODE_LABELS
        w = self._scrollable()
        lay = w.layout()

        # ── Status card ───────────────────────────────────────────────────
        status_box = _section("Ollama Status")

        status_row = QHBoxLayout()
        self._ollama_status_label = QLabel("⏳  Checking…")
        self._ollama_status_label.setStyleSheet("font-size: 13px; background: transparent; border: none;")
        status_row.addWidget(self._ollama_status_label, 1)

        recheck_btn = QPushButton("Re-check")
        recheck_btn.clicked.connect(self._recheck_ollama)
        status_row.addWidget(recheck_btn)
        status_box.layout().addLayout(status_row)

        self._ollama_models_label = QLabel("")
        self._ollama_models_label.setObjectName("hint")
        self._ollama_models_label.setWordWrap(True)
        status_box.layout().addWidget(self._ollama_models_label)

        status_box.layout().addWidget(_hint(
            "Ollama provides free, fully offline AI post-processing. "
            "Install from https://ollama.com — no account needed.\n"
            "Recommended model: ollama pull llama3.2:1b  (fast, ~800 MB)"
        ))
        lay.addWidget(status_box)

        # ── Enable toggle ─────────────────────────────────────────────────
        enable_box = _section("Enable AI Post-Processing")
        self.ollama_enabled_cb = QCheckBox(
            "Send transcriptions through Ollama after Whisper finishes"
        )
        self.ollama_enabled_cb.setChecked(self.config.get("ollama_enabled", False))
        self.ollama_enabled_cb.toggled.connect(self._on_ollama_toggle)
        enable_box.layout().addWidget(self.ollama_enabled_cb)
        enable_box.layout().addWidget(_hint(
            "When enabled, the Whisper transcription is sent to Ollama for a "
            "second pass before being typed into your app. If Ollama is "
            "unavailable, the raw Whisper output is used — no errors."
        ))
        lay.addWidget(enable_box)

        # ── Model & Mode ──────────────────────────────────────────────────
        model_box = _section("Model & Mode")

        model_row = QHBoxLayout()
        model_row.addWidget(QLabel("Model:"))
        self.ollama_model_combo = QComboBox()
        self.ollama_model_combo.setEditable(True)
        self.ollama_model_combo.addItem(self.config.get("ollama_model", "llama3.2:1b"))
        model_row.addWidget(self.ollama_model_combo, 1)
        model_box.layout().addLayout(model_row)
        model_box.layout().addWidget(_hint(
            "Type any model name or select from the list after clicking Re-check."
        ))

        mode_row = QHBoxLayout()
        mode_row.addWidget(QLabel("Mode:"))
        self.ollama_mode_combo = QComboBox()
        current_mode = self.config.get("ollama_mode", "clean")
        for key, label in MODE_LABELS.items():
            if key == "off":
                continue  # "off" is handled by the master toggle
            self.ollama_mode_combo.addItem(label, key)
            if key == current_mode:
                self.ollama_mode_combo.setCurrentIndex(self.ollama_mode_combo.count() - 1)
        mode_row.addWidget(self.ollama_mode_combo, 1)
        model_box.layout().addLayout(mode_row)
        model_box.layout().addWidget(_hint(
            "Fix grammar — minimal cleanup, great for professional writing\n"
            "Formal / Casual — rewrite in a different tone\n"
            "Bullet points — convert spoken thoughts to a list\n"
            "Concise — shorten without losing meaning"
        ))
        lay.addWidget(model_box)

        # Enable/disable controls based on current toggle state
        self._on_ollama_toggle(self.ollama_enabled_cb.isChecked())
        self._update_ollama_status()

        lay.addStretch()
        return w

    def _on_ollama_toggle(self, enabled: bool):
        """Grey out model/mode controls when Ollama is disabled."""
        if hasattr(self, "ollama_model_combo"):
            self.ollama_model_combo.setEnabled(enabled)
        if hasattr(self, "ollama_mode_combo"):
            self.ollama_mode_combo.setEnabled(enabled)

    def _update_ollama_status(self):
        """Refresh the status label from the engine's LLM probe result."""
        if not hasattr(self, "_ollama_status_label"):
            return
        if self.inference_engine and hasattr(self.inference_engine, "llm"):
            llm = self.inference_engine.llm
            if llm._available is None:
                self._ollama_status_label.setText("⏳  Not yet checked")
                self._ollama_models_label.setText("")
            elif llm.available:
                models = llm.available_models
                self._ollama_status_label.setText(
                    "✅  Ollama is running"
                )
                self._ollama_status_label.setStyleSheet(
                    "color: #4ade80; font-size: 13px; background: transparent; border: none;"
                )
                if models:
                    # Populate model combo with detected models
                    current = self.ollama_model_combo.currentText()
                    self.ollama_model_combo.clear()
                    for m in models:
                        self.ollama_model_combo.addItem(m, m)
                    # Restore user's selection if still present
                    idx = self.ollama_model_combo.findText(current)
                    if idx >= 0:
                        self.ollama_model_combo.setCurrentIndex(idx)
                    self._ollama_models_label.setText(
                        f"Installed models: {', '.join(models)}"
                    )
                else:
                    self._ollama_models_label.setText(
                        "No models installed. Run: ollama pull llama3.2:1b"
                    )
            else:
                self._ollama_status_label.setText("❌  Ollama not found")
                self._ollama_status_label.setStyleSheet(
                    "color: #f87171; font-size: 13px; background: transparent; border: none;"
                )
                self._ollama_models_label.setText(
                    "Install Ollama from https://ollama.com to enable AI post-processing."
                )
        else:
            self._ollama_status_label.setText("⚠️  Engine not connected")
            self._ollama_models_label.setText("")

    def _recheck_ollama(self):
        """Force a fresh Ollama probe and update the status label."""
        import threading
        self._ollama_status_label.setText("⏳  Checking…")
        self._ollama_status_label.setStyleSheet(
            "color: #e2e8f0; font-size: 13px; background: transparent; border: none;"
        )

        def probe_and_update():
            if self.inference_engine and hasattr(self.inference_engine, "llm"):
                self.inference_engine.llm.refresh_probe()
            from PyQt6.QtCore import QTimer
            QTimer.singleShot(0, self._update_ollama_status)

        threading.Thread(target=probe_and_update, daemon=True).start()

    # ── Helpers ───────────────────────────────────────────────────────────

    def _populate_overlay_styles(self):
        self.overlay_style_combo.clear()
        current = self.config.get("overlay_style", "voice_card")
        if self.overlay_manager:
            overlays = self.overlay_manager.available()
        else:
            overlays = {"voice_card": {"display_name": "Voice Card", "description": "", "builtin": True}}
        for oid, info in overlays.items():
            label = info["display_name"]
            if not info.get("builtin", True):
                label += "  (custom)"
            self.overlay_style_combo.addItem(label, oid)
            if oid == current:
                self.overlay_style_combo.setCurrentIndex(self.overlay_style_combo.count() - 1)

    def _on_overlay_selected(self):
        if not hasattr(self, "_overlay_desc_label"):
            return
        oid = self.overlay_style_combo.currentData()
        desc = ""
        if self.overlay_manager and oid:
            info = self.overlay_manager.available().get(oid, {})
            desc = info.get("description", "")
        self._overlay_desc_label.setText(desc)

    def _open_overlays_folder(self):
        import subprocess
        if self.overlay_manager:
            folder = self.overlay_manager.ensure_user_dir()
        else:
            from pathlib import Path
            folder = Path.home() / ".config" / "whisper-wayland" / "overlays"
            folder.mkdir(parents=True, exist_ok=True)
        try:
            subprocess.Popen(["xdg-open", str(folder)])
        except Exception:
            from PyQt6.QtWidgets import QMessageBox
            QMessageBox.information(self, "Overlays Folder", f"Overlays folder:\n{folder}")

    def _scrollable(self):
        """Returns a QWidget with a scroll area wrapping a vbox layout."""
        container = QWidget()
        inner_lay = QVBoxLayout(container)
        inner_lay.setContentsMargins(8, 8, 8, 8)
        inner_lay.setSpacing(10)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(container)

        # We return a wrapper that holds the scroll area
        wrapper = QWidget()
        wl = QVBoxLayout(wrapper)
        wl.setContentsMargins(0, 0, 0, 0)
        wl.addWidget(scroll)
        # Expose inner layout directly for convenience
        wrapper.layout = lambda: inner_lay
        return wrapper

    def _fmt_keys(self, keys):
        """Turn ['KEY_LEFTMETA', 'KEY_SPACE'] → 'Super + Space'."""
        nice = {
            "KEY_LEFTMETA": "Super", "KEY_RIGHTMETA": "Super",
            "KEY_LEFTCTRL": "Ctrl", "KEY_RIGHTCTRL": "Ctrl",
            "KEY_LEFTALT": "Alt", "KEY_RIGHTALT": "Alt",
            "KEY_LEFTSHIFT": "Shift", "KEY_RIGHTSHIFT": "Shift",
            "KEY_SPACE": "Space", "KEY_ENTER": "Enter",
        }
        parts = []
        for k in keys:
            parts.append(nice.get(k, k.replace("KEY_", "").title()))
        return "  +  ".join(parts) if parts else "(not set)"

    def _current_hotkeys(self):
        """Return (hold_keys, toggle_keys, dt_keys) as sets, reflecting pending recorded changes."""
        hold = set(self.recorded_keys) if self.recorded_keys else set(self.config.get("hotkey", []))
        toggle = set(self.recorded_toggle_keys) if self.recorded_toggle_keys else set(self.config.get("toggle_hotkey", []))
        dt = set(self.recorded_dt_keys) if self.recorded_dt_keys else set(self.config.get("double_tap_hotkey", ["KEY_LEFTALT"]))
        return hold, toggle, dt

    def _check_hotkey_conflicts(self):
        """Recompute and display conflict warnings on the Hotkeys tab."""
        if not hasattr(self, "hold_conflict_label"):
            return

        MODIFIERS = {
            "KEY_LEFTMETA", "KEY_RIGHTMETA",
            "KEY_LEFTCTRL", "KEY_RIGHTCTRL",
            "KEY_LEFTALT", "KEY_RIGHTALT",
            "KEY_LEFTSHIFT", "KEY_RIGHTSHIFT",
        }

        hold_keys, toggle_keys, dt_keys = self._current_hotkeys()
        hold_w, toggle_w, dt_w = [], [], []

        # Exact duplicates
        if hold_keys and toggle_keys and hold_keys == toggle_keys:
            hold_w.append("Identical to Toggle keys — both gestures will fire at once")
            toggle_w.append("Identical to Hold keys — both gestures will fire at once")
        if hold_keys and dt_keys and hold_keys == dt_keys:
            hold_w.append("Identical to Double-tap keys — will conflict")
            dt_w.append("Identical to Hold keys — will conflict")
        if toggle_keys and dt_keys and toggle_keys == dt_keys:
            toggle_w.append("Identical to Double-tap keys — will conflict")
            dt_w.append("Identical to Toggle keys — will conflict")

        # Subset/superset within HOLD vs TOGGLE (both are level- or edge-triggered on same keys)
        if hold_keys and toggle_keys:
            if hold_keys < toggle_keys:
                hold_w.append("Hold keys are a subset of Toggle — Hold fires whenever Toggle would")
            elif toggle_keys < hold_keys:
                toggle_w.append("Toggle keys are a subset of Hold — Toggle fires whenever Hold would")

        # Double-tap key overlaps with hold or toggle combo
        if dt_keys and hold_keys:
            shared = dt_keys & hold_keys
            if shared:
                nice = self._fmt_keys(sorted(shared))
                dt_w.append(f"{nice} also appears in Hold — double-tap may mis-fire while holding")
        if dt_keys and toggle_keys:
            shared = dt_keys & toggle_keys
            if shared:
                nice = self._fmt_keys(sorted(shared))
                dt_w.append(f"{nice} also appears in Toggle — double-tap may mis-fire while toggling")

        # Single bare (non-modifier) key as Hold or Toggle intercepts every keypress of that key
        if hold_keys and len(hold_keys) == 1 and not (hold_keys & MODIFIERS):
            nice = self._fmt_keys(sorted(hold_keys))
            hold_w.append(f"Single bare key ({nice}) — every {nice} keypress will start recording")
        if toggle_keys and len(toggle_keys) == 1 and not (toggle_keys & MODIFIERS):
            nice = self._fmt_keys(sorted(toggle_keys))
            toggle_w.append(f"Single bare key ({nice}) — every {nice} keypress will toggle recording")

        self._set_conflict_label(self.hold_conflict_label, hold_w)
        self._set_conflict_label(self.toggle_conflict_label, toggle_w)
        self._set_conflict_label(self.dt_conflict_label, dt_w)

    def _set_conflict_label(self, label, warnings):
        if warnings:
            label.setText("\n".join(f"⚠  {w}" for w in warnings))
            label.setVisible(True)
        else:
            label.setVisible(False)

    def populate_audio_devices(self):
        # Reuse the recorder's PyAudio instance to avoid calling Pa_Terminate()
        # while the recorder's instance is still alive. PortAudio does not use
        # reference counting for Pa_Initialize/Pa_Terminate, so a stray terminate()
        # invalidates all existing PyAudio objects and causes a SIGSEGV on next use.
        if self.audio_recorder:
            p = self.audio_recorder.p
            do_terminate = False
        else:
            p = pyaudio.PyAudio()
            do_terminate = True
        current_index = self.config.get("input_device_index")
        for i in range(p.get_device_count()):
            info = p.get_device_info_by_index(i)
            if info.get("maxInputChannels", 0) > 0:
                name = info.get("name", f"Device {i}")
                self.device_combo.addItem(f"{name}", i)
                if current_index == i:
                    self.device_combo.setCurrentIndex(self.device_combo.count() - 1)
        if do_terminate:
            p.terminate()

    def populate_keyboard_devices(self):
        current_path = self.config.get("evdev_device")
        devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
        devices.sort(
            key=lambda d: (
                ecodes.EV_KEY in d.capabilities()
                and ecodes.KEY_A in d.capabilities().get(ecodes.EV_KEY, [])
            ),
            reverse=True,
        )
        for dev in devices:
            if ecodes.EV_KEY in dev.capabilities():
                self.kbd_combo.addItem(f"{dev.name}  ({dev.path})", dev.path)
                if current_path == dev.path:
                    self.kbd_combo.setCurrentIndex(self.kbd_combo.count() - 1)

    # ── Hotkey recording ──────────────────────────────────────────────────
    def toggle_recording(self, mode):
        if mode == "hold":
            btn = self.record_btn_hold
        elif mode == "toggle":
            btn = self.record_btn_toggle
        else:
            btn = self.record_btn_dt

        if btn.isChecked():
            # Uncheck other record buttons
            for other in (self.record_btn_hold, self.record_btn_toggle, self.record_btn_dt):
                if other != btn:
                    other.setChecked(False)

            self.active_recording_mode = mode
            if mode == "hold":
                label = self.hotkey_label
                self.recorded_keys = set()
            elif mode == "toggle":
                label = self.toggle_hotkey_label
                self.recorded_toggle_keys = set()
            else:
                label = self.dt_hotkey_label
                self.recorded_dt_keys = set()

            label.setText("Press keys…")
            btn.setText("Done")
            self.grabKeyboard()
        else:
            self.stop_recording()

    def stop_recording(self):
        self.active_recording_mode = None
        for btn in (self.record_btn_hold, self.record_btn_toggle, self.record_btn_dt):
            btn.setChecked(False)
            btn.setText("Record")
        self.releaseKeyboard()
        if not self.recorded_keys:
            self.hotkey_label.setText(self._fmt_keys(self.config.get("hotkey", [])))
        if not self.recorded_toggle_keys:
            self.toggle_hotkey_label.setText(self._fmt_keys(self.config.get("toggle_hotkey", [])))
        if not self.recorded_dt_keys:
            self.dt_hotkey_label.setText(self._fmt_keys(self.config.get("double_tap_hotkey", ["KEY_LEFTALT"])))
        self._check_hotkey_conflicts()

    # ── System check ──────────────────────────────────────────────────────
    def run_system_check(self):
        import shutil, grp
        from PyQt6.QtWidgets import QDialog, QVBoxLayout, QScrollArea, QWidget, QLabel, QPushButton, QHBoxLayout
        from PyQt6.QtCore import Qt

        sections: list[tuple[str, list[tuple[str, str, str]]]] = []
        # Each entry: (icon, label, detail/fix hint)

        # ── Permissions & devices ──────────────────────────────────────────
        perm_rows: list[tuple[str, str, str]] = []
        if os.path.exists("/dev/uinput"):
            if os.access("/dev/uinput", os.W_OK):
                perm_rows.append(("✅", "/dev/uinput", "Writable — key injection available"))
            else:
                perm_rows.append(("❌", "/dev/uinput",
                    "Permission denied\n    Fix: sudo usermod -aG input,uinput $USER  (then log out/in)"))
        else:
            perm_rows.append(("❌", "/dev/uinput",
                "Device not found — uinput kernel module may not be loaded\n    Fix: sudo modprobe uinput"))

        try:
            groups = [grp.getgrgid(g).gr_name for g in os.getgroups()]
            if "input" in groups:
                perm_rows.append(("✅", "'input' group", "User is a member"))
            else:
                perm_rows.append(("❌", "'input' group",
                    "User is NOT in 'input' group — evdev hotkeys will not work\n    Fix: sudo usermod -aG input $USER  (then log out/in)"))
        except Exception:
            perm_rows.append(("⚠️", "'input' group", "Could not verify group membership"))

        xdg_cfg = os.path.expanduser("~/.config/whisper-wayland")
        if os.path.isdir(xdg_cfg):
            perm_rows.append(("✅", "Config directory", f"{xdg_cfg}"))
        else:
            perm_rows.append(("⚠️", "Config directory",
                f"{xdg_cfg} does not exist yet — will be created on first save"))

        sections.append(("Permissions & Devices", perm_rows))

        # ── Required Python packages ───────────────────────────────────────
        req_rows: list[tuple[str, str, str]] = []

        def _pkg(import_name, display, install_hint):
            try:
                __import__(import_name)
                req_rows.append(("✅", display, "Installed"))
            except ImportError:
                req_rows.append(("❌", display, f"Missing — {install_hint}"))

        _pkg("PyQt6",         "PyQt6",          "pip install PyQt6")
        _pkg("sounddevice",   "sounddevice",     "pip install sounddevice")
        _pkg("numpy",         "numpy",           "pip install numpy")
        _pkg("scipy",         "scipy",           "pip install scipy")

        sections.append(("Required Python Packages", req_rows))

        # ── ASR backends ──────────────────────────────────────────────────
        asr_rows: list[tuple[str, str, str]] = []

        def _try_import(name):
            try:
                __import__(name)
                return True
            except ImportError:
                return False

        if _try_import("faster_whisper"):
            cuda_note = ""
            try:
                import torch
                if torch.cuda.is_available():
                    cuda_note = " + CUDA GPU available"
                else:
                    cuda_note = " (CPU only — no CUDA)"
            except ImportError:
                cuda_note = " (torch not installed — GPU status unknown)"
            asr_rows.append(("✅", "faster-whisper", f"Installed{cuda_note}"))
        else:
            asr_rows.append(("⚠️", "faster-whisper",
                "Not installed — pip install faster-whisper\n    Required for faster-whisper backend"))

        whisper_cpp_bin = self.config.get("whisper_cpp_binary", "whisper-cli") if hasattr(self, 'config') else "whisper-cli"
        if shutil.which(whisper_cpp_bin) or shutil.which("whisper-cli") or shutil.which("main"):
            found = shutil.which(whisper_cpp_bin) or shutil.which("whisper-cli") or shutil.which("main")
            asr_rows.append(("✅", "whisper.cpp", f"Binary found: {found}"))
        else:
            asr_rows.append(("⚠️", "whisper.cpp",
                f"Binary '{whisper_cpp_bin}' not found in PATH\n"
                "    Required for whisper-cpp backend\n"
                "    Build from: https://github.com/ggerganov/whisper.cpp"))

        if self.inference_engine and getattr(self.inference_engine, 'actual_device', None) == "cuda":
            asr_rows.append(("✅", "GPU / CUDA", "Active — using GPU acceleration"))
        else:
            try:
                import torch
                if torch.cuda.is_available():
                    asr_rows.append(("ℹ️", "GPU / CUDA",
                        "Available but not currently active — set device=cuda in config"))
                else:
                    asr_rows.append(("ℹ️", "GPU / CUDA",
                        "Not available — running on CPU (install CUDA + torch with CUDA support)"))
            except ImportError:
                asr_rows.append(("ℹ️", "GPU / CUDA",
                    "torch not installed — GPU detection unavailable\n    pip install torch"))

        sections.append(("ASR Backends", asr_rows))

        # ── Clipboard & text injection ─────────────────────────────────────
        clip_rows: list[tuple[str, str, str]] = []

        if shutil.which("wl-copy"):
            clip_rows.append(("✅", "wl-copy (wl-clipboard)", "Installed — clipboard delivery available"))
        else:
            clip_rows.append(("❌", "wl-copy (wl-clipboard)",
                "Not found — clipboard targets will not work\n"
                "    Install: sudo pacman -S wl-clipboard  or  sudo apt install wl-clipboard"))

        if shutil.which("wl-paste"):
            clip_rows.append(("✅", "wl-paste (wl-clipboard)", "Installed"))
        else:
            clip_rows.append(("⚠️", "wl-paste (wl-clipboard)",
                "Not found (part of wl-clipboard package)"))

        if shutil.which("xdotool"):
            clip_rows.append(("✅", "xdotool", "Installed — X11 injection fallback available"))
        else:
            clip_rows.append(("ℹ️", "xdotool",
                "Not found — optional X11 text injection fallback\n"
                "    Install: sudo pacman -S xdotool  or  sudo apt install xdotool"))

        if shutil.which("ydotool"):
            clip_rows.append(("✅", "ydotool", "Installed — Wayland uinput injection available"))
        else:
            clip_rows.append(("ℹ️", "ydotool",
                "Not found — optional Wayland text injection tool\n"
                "    Install: sudo pacman -S ydotool  or  build from source"))

        sections.append(("Clipboard & Text Injection", clip_rows))

        # ── Optional Python packages ───────────────────────────────────────
        opt_rows: list[tuple[str, str, str]] = []

        try:
            from audio_recorder import _HAS_NOISEREDUCE
            if _HAS_NOISEREDUCE:
                opt_rows.append(("✅", "noisereduce", "Installed — noise suppression available"))
            else:
                opt_rows.append(("ℹ️", "noisereduce",
                    "Not installed — noise suppression unavailable\n    pip install noisereduce"))
        except ImportError:
            opt_rows.append(("ℹ️", "noisereduce",
                "Not installed\n    pip install noisereduce"))

        try:
            import dbus
            opt_rows.append(("✅", "dbus-python", "Installed — DBus control interface active"))
        except ImportError:
            opt_rows.append(("ℹ️", "dbus-python",
                "Not installed — DBus interface unavailable\n"
                "    pip install dbus-python  or  sudo pacman -S python-dbus"))

        try:
            import pyatspi
            opt_rows.append(("✅", "pyatspi", "Installed — AT-SPI2 context tracking active"))
        except ImportError:
            opt_rows.append(("ℹ️", "pyatspi",
                "Not installed — focused-window context for transcription unavailable\n"
                "    pip install pyatspi  or  sudo pacman -S python-pyatspi"))

        try:
            import mcp
            opt_rows.append(("✅", "mcp", "Installed — MCP server available"))
        except ImportError:
            opt_rows.append(("ℹ️", "mcp",
                "Not installed — MCP server feature unavailable\n    pip install mcp"))

        try:
            import websockets
            opt_rows.append(("✅", "websockets", "Installed — WebSocket targets available"))
        except ImportError:
            opt_rows.append(("ℹ️", "websockets",
                "Not installed — WebSocket delivery unavailable\n    pip install websockets"))

        sections.append(("Optional Python Packages", opt_rows))

        # ── TTS engines ───────────────────────────────────────────────────
        tts_rows: list[tuple[str, str, str]] = []

        if shutil.which("piper") or shutil.which("piper-tts"):
            found = shutil.which("piper") or shutil.which("piper-tts")
            tts_rows.append(("✅", "piper (TTS)", f"Binary found: {found}"))
        else:
            tts_rows.append(("ℹ️", "piper (TTS)",
                "Not found — Piper TTS engine unavailable\n"
                "    Install: https://github.com/rhasspy/piper/releases\n"
                "    Or: pip install piper-tts"))

        if shutil.which("espeak") or shutil.which("espeak-ng"):
            found = shutil.which("espeak-ng") or shutil.which("espeak")
            tts_rows.append(("✅", "espeak-ng (TTS)", f"Binary found: {found}"))
        else:
            tts_rows.append(("ℹ️", "espeak-ng (TTS)",
                "Not found — espeak TTS engine unavailable\n"
                "    Install: sudo pacman -S espeak-ng  or  sudo apt install espeak-ng"))

        sections.append(("Text-to-Speech Engines", tts_rows))

        # ── LLM / Ollama ──────────────────────────────────────────────────
        llm_rows: list[tuple[str, str, str]] = []

        if shutil.which("ollama"):
            llm_rows.append(("✅", "ollama", "Binary found — LLM postprocessing available"))
            # Try pinging ollama service
            try:
                import urllib.request
                urllib.request.urlopen("http://localhost:11434/api/tags", timeout=1)
                llm_rows.append(("✅", "Ollama service", "Running on localhost:11434"))
            except Exception:
                llm_rows.append(("⚠️", "Ollama service",
                    "Not responding on localhost:11434\n    Start with: ollama serve"))
        else:
            llm_rows.append(("ℹ️", "ollama",
                "Not found — LLM postprocessing (ollama_enabled) unavailable\n"
                "    Install: https://ollama.ai  or  curl -fsSL https://ollama.ai/install.sh | sh"))

        sections.append(("LLM / Ollama", llm_rows))

        # ── Build the dialog ──────────────────────────────────────────────
        dlg = QDialog(self)
        dlg.setWindowTitle("System Check — Dependencies & Status")
        dlg.resize(640, 560)
        dlg_layout = QVBoxLayout(dlg)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        container = QWidget()
        c_layout = QVBoxLayout(container)
        c_layout.setSpacing(6)

        has_errors = False
        for section_title, rows in sections:
            sec_label = QLabel(f"<b>{section_title}</b>")
            sec_label.setStyleSheet("font-size: 12px; margin-top: 10px;")
            c_layout.addWidget(sec_label)

            for icon, name, detail in rows:
                if icon == "❌":
                    has_errors = True
                row_widget = QWidget()
                row_layout = QHBoxLayout(row_widget)
                row_layout.setContentsMargins(8, 2, 4, 2)
                row_layout.setSpacing(8)

                icon_lbl = QLabel(icon)
                icon_lbl.setFixedWidth(22)
                name_lbl = QLabel(f"<b>{name}</b>")
                name_lbl.setFixedWidth(200)
                detail_lbl = QLabel(detail)
                detail_lbl.setWordWrap(True)
                detail_lbl.setStyleSheet(
                    "color: #cc0000;" if icon == "❌" else
                    "color: #aa6600;" if icon == "⚠️" else
                    "color: #555555;" if icon == "ℹ️" else ""
                )

                row_layout.addWidget(icon_lbl)
                row_layout.addWidget(name_lbl)
                row_layout.addWidget(detail_lbl, 1)
                c_layout.addWidget(row_widget)

        c_layout.addStretch()
        scroll.setWidget(container)
        dlg_layout.addWidget(scroll)

        btn_box = QHBoxLayout()
        btn_box.addStretch()
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(dlg.accept)
        btn_box.addWidget(close_btn)
        dlg_layout.addLayout(btn_box)

        if not has_errors:
            status_lbl = QLabel("✅  No critical issues found.")
        else:
            status_lbl = QLabel("❌  Critical issues detected — see items marked with ❌ above.")
            status_lbl.setStyleSheet("color: #cc0000; font-weight: bold;")
        dlg_layout.insertWidget(0, status_lbl)

        dlg.exec()

    # ── Save ──────────────────────────────────────────────────────────────
    def save_settings(self):
        # Track if restart-required settings changed
        restart_keys = [
            "model_size", "device", "inference_mode", "input_device_index",
            "backend_engine", "whisper_cpp_model_size", "whisper_cpp_device",
        ]
        old_vals = {k: self.config.get(k) for k in restart_keys}

        # Engine
        if hasattr(self, "engine_combo"):
            self.config.set("backend_engine", self.engine_combo.currentText())
        if hasattr(self, "cpp_binary_edit"):
            self.config.set("whisper_cpp_binary", self.cpp_binary_edit.text().strip() or "whisper-cli")
        if hasattr(self, "cpp_model_combo"):
            self.config.set("whisper_cpp_model_size", self.cpp_model_combo.currentText())
        if hasattr(self, "cpp_device_combo"):
            self.config.set("whisper_cpp_device", self.cpp_device_combo.currentText())
        if hasattr(self, "cpp_threads_spin"):
            self.config.set("whisper_cpp_threads", self.cpp_threads_spin.value())

        # General
        self.config.set("model_size", self.model_combo.currentText())
        self.config.set("inference_mode", self.inference_mode_combo.currentText())
        new_device = self.device_type_combo.currentText()
        self.config.set("device", new_device)
        self.config.set("compute_type",
            "int8" if new_device == "cpu" else
            "float16" if new_device == "cuda" else "default"
        )
        # Audio
        self.config.set("input_device_index", self.device_combo.currentData())
        self.config.set("evdev_device", self.kbd_combo.currentData())
        self.config.set("mic_gain", self.gain_slider.value() / 10.0)
        if hasattr(self, "noise_suppression_cb"):
            self.config.set("noise_suppression", self.noise_suppression_cb.isChecked())

        # Hotkeys: bindings are now managed exclusively through bindings.toml
        # via the Hotkeys tab CRUD UI.  Keep legacy config.json keys in sync
        # by reading the current bindings list for backward compat.
        _hold_b   = next((b for b in self._routing_bindings if b.gesture.value == 'hold'    and not b.disabled), None)
        _toggle_b = next((b for b in self._routing_bindings if b.gesture.value == 'toggle'  and not b.disabled), None)
        _dt_b     = next((b for b in self._routing_bindings if b.gesture.value == 'double_tap' and not b.disabled), None)
        if _hold_b:
            self.config.set("hotkey", _hold_b.keys)
        if _toggle_b:
            self.config.set("toggle_hotkey", _toggle_b.keys)
        if _dt_b:
            self.config.set("double_tap_hotkey", _dt_b.keys)

        # Dictation
        self.config.set("remove_fillers", self.filler_checkbox.isChecked())
        self.config.set("spoken_punctuation", self.spoken_punct_checkbox.isChecked())
        self.config.set("auto_format_lists", self.auto_list_checkbox.isChecked())
        self.config.set("quiet_mode", self.quiet_mode_checkbox.isChecked())
        self.config.set("show_overlay", self.overlay_checkbox.isChecked())
        if hasattr(self, "overlay_style_combo"):
            selected_style = self.overlay_style_combo.currentData()
            if selected_style:
                self.config.set("overlay_style", selected_style)
        self.config.set("show_notification", self.notification_checkbox.isChecked())
        self.config.set(
            "dictation_mode",
            "code" if self.code_mode_checkbox.isChecked() else "normal"
        )
        raw = self.vocab_edit.toPlainText()
        self.config.set("custom_vocabulary", [w.strip() for w in raw.split(",") if w.strip()])
        # Snippets
        snippets = {}
        for row in self._snippet_rows:
            trigger = row["trigger"].text().strip().lower()
            expansion = row["expansion"].text().strip()
            if trigger and expansion:
                snippets[trigger] = expansion
        self.config.set("snippets", snippets)
        # AI / Ollama
        if hasattr(self, "ollama_enabled_cb"):
            ollama_on = self.ollama_enabled_cb.isChecked()
            self.config.set("ollama_enabled", ollama_on)
            self.config.set("ollama_model", self.ollama_model_combo.currentText().strip())
            mode_data = self.ollama_mode_combo.currentData()
            self.config.set("ollama_mode", mode_data or "clean")
            # Apply live to running engine without restart
            if self.inference_engine and hasattr(self.inference_engine, "llm"):
                self.inference_engine.llm.config = self.config

        # Voice Output / TTS
        if hasattr(self, "tts_enabled_cb"):
            self.config.set("tts_enabled", self.tts_enabled_cb.isChecked())
        if hasattr(self, "tts_engine_combo"):
            self.config.set("tts_engine", self.tts_engine_combo.currentData() or "piper")
        if hasattr(self, "tts_voice_combo"):
            self.config.set("tts_voice", self.tts_voice_combo.currentData() or "en-us-lessac-medium")
        if hasattr(self, "tts_stop_key_label"):
            stop_keys = getattr(self, "_recorded_tts_stop_keys", None)
            if stop_keys:
                self.config.set("tts_stop_key", sorted(stop_keys))
        if hasattr(self, "tts_overlay_cb"):
            self.config.set("tts_response_overlay", self.tts_overlay_cb.isChecked())
        # MCP Server
        if hasattr(self, "mcp_enabled_cb"):
            self.config.set("mcp_server_enabled", self.mcp_enabled_cb.isChecked())

        # Check if anything changed that needs a restart
        needs_restart = False
        for k in restart_keys:
            if self.config.get(k) != old_vals[k]:
                needs_restart = True
                break

        self.config.save()
        self.settings_saved.emit()

        if needs_restart:
            QMessageBox.information(
                self, "Settings Saved",
                "Changes saved.\n\nA restart is required to apply your new model or device settings."
            )

        self.close()

    # ── Tab: Voice Output (TTS + MCP) ─────────────────────────────────────────

    def _tab_voice_output(self):
        import threading as _threading
        from tts_engine import (
            VOICE_CATALOG, available_tts_engine,
            is_voice_downloaded, download_voice, SAMPLE_TEXT,
        )

        w = self._scrollable()
        lay = w.layout()

        # ── TTS Enable ────────────────────────────────────────────────────────
        tts_box = _section("Text-to-Speech")
        self.tts_enabled_cb = QCheckBox(
            "Enable TTS responses — AI agents can speak replies aloud"
        )
        self.tts_enabled_cb.setChecked(self.config.get("tts_enabled", False))
        self.tts_enabled_cb.toggled.connect(self._on_tts_toggle)
        tts_box.layout().addWidget(self.tts_enabled_cb)

        engine_row = QHBoxLayout()
        engine_row.addWidget(QLabel("Engine:"))
        self.tts_engine_combo = QComboBox()
        detected = available_tts_engine()
        self.tts_engine_combo.addItem("Piper (neural, recommended)", "piper")
        self.tts_engine_combo.addItem("espeak-ng (lightweight fallback)", "espeak")
        cur_engine = self.config.get("tts_engine", "piper")
        self.tts_engine_combo.setCurrentIndex(0 if cur_engine == "piper" else 1)
        engine_row.addWidget(self.tts_engine_combo, 1)
        if detected == "none":
            engine_status = QLabel("⚠  Neither piper nor espeak-ng found on PATH")
            engine_status.setStyleSheet("color:#facc15; background:transparent; border:none; font-size:11px;")
        elif detected == "piper":
            engine_status = QLabel("✅  piper found")
            engine_status.setStyleSheet("color:#4ade80; background:transparent; border:none; font-size:11px;")
        else:
            engine_status = QLabel("ℹ️  piper not found — espeak-ng available")
            engine_status.setStyleSheet("color:#8892a4; background:transparent; border:none; font-size:11px;")
        tts_box.layout().addLayout(engine_row)
        tts_box.layout().addWidget(engine_status)
        lay.addWidget(tts_box)

        # ── Voice Picker ──────────────────────────────────────────────────────
        voice_box = _section("Voice")
        voice_box.layout().addWidget(_hint(
            "Select a voice for TTS output. Neural voices (Piper) require a one-time "
            "model download (~40–130 MB each)."
        ))

        voice_row = QHBoxLayout()
        self.tts_voice_combo = QComboBox()
        cur_voice = self.config.get("tts_voice", "en-us-lessac-medium")
        for vid, info in VOICE_CATALOG.items():
            downloaded = is_voice_downloaded(vid)
            label = info["display"] + ("  ✅" if downloaded else "  ⬇")
            self.tts_voice_combo.addItem(label, vid)
            if vid == cur_voice:
                self.tts_voice_combo.setCurrentIndex(self.tts_voice_combo.count() - 1)
        self.tts_voice_combo.currentIndexChanged.connect(self._on_tts_voice_selected)
        voice_row.addWidget(self.tts_voice_combo, 1)

        self._tts_test_btn = QPushButton("▶  Test Voice")
        self._tts_test_btn.setFixedWidth(110)
        self._tts_test_btn.clicked.connect(self._tts_test_voice)
        voice_row.addWidget(self._tts_test_btn)
        voice_box.layout().addLayout(voice_row)

        # Model download status + progress
        self._tts_model_status = QLabel("")
        self._tts_model_status.setObjectName("hint")
        self._tts_model_status.setWordWrap(True)
        voice_box.layout().addWidget(self._tts_model_status)

        self._tts_progress = QProgressBar()
        self._tts_progress.setRange(0, 100)
        self._tts_progress.setValue(0)
        self._tts_progress.setVisible(False)
        self._tts_progress.setStyleSheet(
            "QProgressBar { background:#1a1f2e; border:1px solid #2a3448; border-radius:4px; height:8px; text-align:center; }"
            "QProgressBar::chunk { background:#4a9eff; border-radius:4px; }"
        )
        voice_box.layout().addWidget(self._tts_progress)

        self._tts_download_btn = QPushButton("⬇  Download Selected Voice")
        self._tts_download_btn.clicked.connect(self._tts_download_voice)
        voice_box.layout().addWidget(self._tts_download_btn)
        lay.addWidget(voice_box)

        # Populate status for initially selected voice
        self._on_tts_voice_selected()

        # ── Stop Hotkey ───────────────────────────────────────────────────────
        stop_box = _section("Stop TTS Playback")
        stop_box.layout().addWidget(_hint(
            "Press this key at any time to immediately stop TTS playback. "
            "Works globally even when another window is focused."
        ))
        stop_row = QHBoxLayout()
        cur_stop = self.config.get("tts_stop_key", ["KEY_ESCAPE"])
        self.tts_stop_key_label = QLabel(self._fmt_keys(cur_stop))
        self.tts_stop_key_label.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:7px 12px; color:#4a9eff; font-weight:600;"
        )
        self._recorded_tts_stop_keys: set = set()
        self._tts_stop_recording = False

        self._tts_stop_record_btn = QPushButton("Record")
        self._tts_stop_record_btn.setObjectName("btn_record")
        self._tts_stop_record_btn.setCheckable(True)
        self._tts_stop_record_btn.clicked.connect(self._toggle_tts_stop_recording)
        stop_row.addWidget(self.tts_stop_key_label, 1)
        stop_row.addWidget(self._tts_stop_record_btn)
        stop_box.layout().addLayout(stop_row)
        lay.addWidget(stop_box)

        # ── Response Overlay ──────────────────────────────────────────────────
        overlay_box = _section("Response Overlay")
        self.tts_overlay_cb = QCheckBox(
            "Show overlay while AI is speaking a TTS response"
        )
        self.tts_overlay_cb.setChecked(self.config.get("tts_response_overlay", True))
        overlay_box.layout().addWidget(self.tts_overlay_cb)
        overlay_box.layout().addWidget(_hint(
            "A teal card (distinct from the pink recording overlay) will appear at the "
            "bottom of the screen showing the text being spoken."
        ))
        lay.addWidget(overlay_box)

        # ── MCP Server ────────────────────────────────────────────────────────
        mcp_box = _section("MCP Server (AI Tool Integration)")
        mcp_box.layout().addWidget(_hint(
            "Exposes Whisper-Wayland as an MCP tool so AI clients (Claude Desktop, "
            "custom agents) can trigger voice recording and TTS directly."
        ))

        self.mcp_enabled_cb = QCheckBox("Enable MCP server")
        self.mcp_enabled_cb.setChecked(self.config.get("mcp_server_enabled", False))
        self.mcp_enabled_cb.toggled.connect(self._on_mcp_toggle)
        mcp_box.layout().addWidget(self.mcp_enabled_cb)

        from mcp_server import SOCKET_PATH as _MCP_SOCK
        sock_label = QLabel(f"Socket:  {_MCP_SOCK}")
        sock_label.setStyleSheet(
            "font-family: monospace; color:#4a9eff; background:transparent; border:none; font-size:11px;"
        )
        mcp_box.layout().addWidget(sock_label)

        self._mcp_status_label = QLabel("")
        self._mcp_status_label.setObjectName("hint")
        self._mcp_status_label.setWordWrap(True)
        mcp_box.layout().addWidget(self._mcp_status_label)

        mcp_btn_row = QHBoxLayout()
        self._mcp_claude_btn = QPushButton("Register in Claude Desktop")
        self._mcp_claude_btn.clicked.connect(self._mcp_register_claude_desktop)
        mcp_btn_row.addWidget(self._mcp_claude_btn)
        mcp_btn_row.addStretch()
        mcp_box.layout().addLayout(mcp_btn_row)

        mcp_box.layout().addWidget(_hint(
            "Claude Desktop connection requires socat:  sudo pacman -S socat  (or apt install socat)\n"
            "Tools exposed:  transcribe_voice · speak_text · get_status"
        ))
        lay.addWidget(mcp_box)

        self._on_tts_toggle(self.tts_enabled_cb.isChecked())
        self._on_mcp_toggle(self.mcp_enabled_cb.isChecked())
        lay.addStretch()
        return w

    # ── Voice Output helpers ──────────────────────────────────────────────────

    def _on_tts_toggle(self, enabled: bool):
        for attr in ("tts_engine_combo", "tts_voice_combo", "_tts_test_btn",
                     "_tts_download_btn", "tts_overlay_cb"):
            if hasattr(self, attr):
                getattr(self, attr).setEnabled(enabled)

    def _on_mcp_toggle(self, enabled: bool):
        if hasattr(self, "_mcp_claude_btn"):
            self._mcp_claude_btn.setEnabled(enabled)
        if hasattr(self, "_mcp_status_label"):
            if enabled:
                self._mcp_status_label.setText(
                    "Server will start on next app launch (or restart now)."
                )
            else:
                self._mcp_status_label.setText("")

    def _on_tts_voice_selected(self):
        if not hasattr(self, "tts_voice_combo"):
            return
        from tts_engine import VOICE_CATALOG, is_voice_downloaded
        vid = self.tts_voice_combo.currentData()
        if not vid:
            return
        downloaded = is_voice_downloaded(vid)
        if downloaded:
            self._tts_model_status.setText(f"✅  Voice downloaded and ready.")
            self._tts_model_status.setStyleSheet("color:#4ade80; background:transparent; border:none;")
            self._tts_download_btn.setEnabled(False)
            self._tts_test_btn.setEnabled(True)
        else:
            self._tts_model_status.setText(
                f"⬇  Not yet downloaded. Click 'Download Selected Voice' to install."
            )
            self._tts_model_status.setStyleSheet("color:#facc15; background:transparent; border:none;")
            self._tts_download_btn.setEnabled(True)
            self._tts_test_btn.setEnabled(False)

    def _tts_download_voice(self):
        import threading as _threading
        from tts_engine import download_voice

        vid = self.tts_voice_combo.currentData()
        if not vid:
            return

        self._tts_download_btn.setEnabled(False)
        self._tts_model_status.setText("⏳  Downloading…")
        self._tts_model_status.setStyleSheet("color:#e2e8f0; background:transparent; border:none;")
        self._tts_progress.setValue(0)
        self._tts_progress.setVisible(True)

        # Connect signals (disconnect first to avoid duplicates from repeat clicks)
        try:
            self._tts_progress_sig.disconnect()
            self._tts_download_ok_sig.disconnect()
            self._tts_download_err_sig.disconnect()
        except (RuntimeError, TypeError):
            pass  # not yet connected

        def _on_progress(done, total):
            if total > 0:
                self._tts_progress.setRange(0, 100)
                self._tts_progress.setValue(int(done * 100 / total))
            else:
                self._tts_progress.setRange(0, 0)  # indeterminate

        def _on_ok():
            self._tts_model_status.setText("✅  Download complete.")
            self._tts_model_status.setStyleSheet("color:#4ade80; background:transparent; border:none;")
            self._tts_progress.setRange(0, 100)
            self._tts_progress.setValue(100)
            self._tts_progress.setVisible(False)
            self._tts_test_btn.setEnabled(True)
            self._tts_download_btn.setEnabled(False)
            self._refresh_tts_voice_labels()

        def _on_err(msg):
            self._tts_model_status.setText(f"❌  Download failed: {msg}")
            self._tts_model_status.setStyleSheet("color:#f87171; background:transparent; border:none;")
            self._tts_progress.setRange(0, 100)
            self._tts_progress.setVisible(False)
            self._tts_download_btn.setEnabled(True)

        self._tts_progress_sig.connect(_on_progress)
        self._tts_download_ok_sig.connect(_on_ok)
        self._tts_download_err_sig.connect(_on_err)

        def _do():
            try:
                download_voice(vid, progress_cb=lambda d, t: self._tts_progress_sig.emit(d, t))
                self._tts_download_ok_sig.emit()
            except Exception as e:
                self._tts_download_err_sig.emit(str(e))

        _threading.Thread(target=_do, daemon=True).start()

    def _refresh_tts_voice_labels(self):
        from tts_engine import VOICE_CATALOG, is_voice_downloaded
        cur = self.tts_voice_combo.currentData()
        self.tts_voice_combo.blockSignals(True)
        self.tts_voice_combo.clear()
        for vid, info in VOICE_CATALOG.items():
            downloaded = is_voice_downloaded(vid)
            label = info["display"] + ("  ✅" if downloaded else "  ⬇")
            self.tts_voice_combo.addItem(label, vid)
            if vid == cur:
                self.tts_voice_combo.setCurrentIndex(self.tts_voice_combo.count() - 1)
        self.tts_voice_combo.blockSignals(False)

    def _tts_test_voice(self):
        import threading as _threading
        from tts_engine import TTSEngine, SAMPLE_TEXT, is_voice_downloaded

        vid = self.tts_voice_combo.currentData()
        if not vid or not is_voice_downloaded(vid):
            return

        self._tts_test_btn.setEnabled(False)
        self._tts_test_btn.setText("▶  Playing…")

        # Temporarily patch config for the test
        orig_voice = self.config.get("tts_voice")
        orig_engine = self.config.get("tts_engine")
        self.config.set("tts_voice", vid)
        self.config.set("tts_engine", self.tts_engine_combo.currentData() or "piper")
        self.config.set("tts_enabled", True)

        engine = TTSEngine(self.config)

        try:
            self._tts_test_done_sig.disconnect()
        except (RuntimeError, TypeError):
            pass

        def _on_test_done(err_msg):
            self._tts_test_btn.setEnabled(True)
            self._tts_test_btn.setText("▶  Test Voice")
            self.config.set("tts_voice", orig_voice)
            self.config.set("tts_engine", orig_engine)
            if err_msg:
                self._tts_model_status.setText(f"❌  {err_msg.splitlines()[0]}")
                self._tts_model_status.setStyleSheet("color:#f87171; background:transparent; border:none;")

        self._tts_test_done_sig.connect(_on_test_done)

        def _run():
            err = ""
            try:
                engine.speak_test(vid)
            except Exception as e:
                err = str(e)
                print(f"[TTS test] {e}")
            finally:
                engine.shutdown()
                self._tts_test_done_sig.emit(err)

        _threading.Thread(target=_run, daemon=True).start()

    def _toggle_tts_stop_recording(self):
        if self._tts_stop_record_btn.isChecked():
            self._tts_stop_recording = True
            self._recorded_tts_stop_keys = set()
            self.tts_stop_key_label.setText("Press key…")
            self._tts_stop_record_btn.setText("Done")
            self.grabKeyboard()
        else:
            self._tts_stop_recording = False
            self._tts_stop_record_btn.setText("Record")
            self.releaseKeyboard()
            if not self._recorded_tts_stop_keys:
                cur = self.config.get("tts_stop_key", ["KEY_ESCAPE"])
                self.tts_stop_key_label.setText(self._fmt_keys(cur))

    def keyPressEvent(self, event):
        # TTS stop key recording intercepts key events when active
        if self._tts_stop_recording:
            key = event.key()
            mapping = {
                Qt.Key.Key_Meta: "KEY_LEFTMETA",
                Qt.Key.Key_Alt: "KEY_LEFTALT",
                Qt.Key.Key_Control: "KEY_LEFTCTRL",
                Qt.Key.Key_Shift: "KEY_LEFTSHIFT",
                Qt.Key.Key_Space: "KEY_SPACE",
                Qt.Key.Key_Escape: "KEY_ESCAPE",
                Qt.Key.Key_Enter: "KEY_ENTER",
                Qt.Key.Key_Return: "KEY_ENTER",
            }
            if Qt.Key.Key_A <= key <= Qt.Key.Key_Z:
                key_name = f"KEY_{chr(key)}"
            else:
                key_name = mapping.get(key)
                if not key_name:
                    key_name = f"KEY_{event.text().upper()}" if event.text() else None
            if key_name:
                self._recorded_tts_stop_keys.add(key_name)
                self.tts_stop_key_label.setText(self._fmt_keys(sorted(self._recorded_tts_stop_keys)))
            return
        if not self.active_recording_mode:
            return super().keyPressEvent(event)
        key = event.key()
        mapping = {
            Qt.Key.Key_Meta: "KEY_LEFTMETA",
            Qt.Key.Key_Alt: "KEY_LEFTALT",
            Qt.Key.Key_Control: "KEY_LEFTCTRL",
            Qt.Key.Key_Shift: "KEY_LEFTSHIFT",
            Qt.Key.Key_Space: "KEY_SPACE",
            Qt.Key.Key_Enter: "KEY_ENTER",
            Qt.Key.Key_Return: "KEY_ENTER",
        }
        if Qt.Key.Key_A <= key <= Qt.Key.Key_Z:
            key_name = f"KEY_{chr(key)}"
        else:
            key_name = mapping.get(key)
            if not key_name:
                key_name = f"KEY_{event.text().upper()}" if event.text() else None
        if key_name:
            if self.active_recording_mode == "hold":
                self.recorded_keys.add(key_name)
                self.hotkey_label.setText(self._fmt_keys(sorted(self.recorded_keys)))
            elif self.active_recording_mode == "toggle":
                self.recorded_toggle_keys.add(key_name)
                self.toggle_hotkey_label.setText(self._fmt_keys(sorted(self.recorded_toggle_keys)))
            elif self.active_recording_mode == "double_tap":
                self.recorded_dt_keys.add(key_name)
                self.dt_hotkey_label.setText(self._fmt_keys(sorted(self.recorded_dt_keys)))
            self._check_hotkey_conflicts()

    def _mcp_register_claude_desktop(self):
        try:
            from mcp_server import WhisperMCPServer
            path = WhisperMCPServer.write_claude_desktop_config()
            QMessageBox.information(
                self, "Claude Desktop Registered",
                f"MCP entry written to:\n{path}\n\n"
                "Restart Claude Desktop to activate.\n\n"
                "Requires:  socat  (install via your package manager)."
            )
        except Exception as e:
            QMessageBox.warning(self, "Registration Failed", str(e))

    # ── Routing tab ───────────────────────────────────────────────────────────

    def _tab_routing(self):
        from routing.loader import load_targets, load_bindings, save_targets, save_bindings
        from routing.models import (
            OutputTarget, HotkeyBinding, DeliveryType, GestureType,
        )

        tab = QWidget()
        lay = QVBoxLayout(tab)
        lay.setContentsMargins(12, 12, 12, 12)
        lay.setSpacing(10)

        lay.addWidget(_hint(
            "Define output targets that receive transcribed text.  "
            "Each target controls where text goes and how it is processed.  "
            "Hotkey bindings are managed in the Hotkeys tab.  "
            "Changes are saved to ~/.config/whisper-wayland/targets.toml."
        ))

        # ── Targets panel (full width) ─────────────────────────────────────
        tp_head = QLabel("Output Targets")
        tp_head.setObjectName("section_head")
        lay.addWidget(tp_head)

        self._target_list = QListWidget()
        self._target_list.setAlternatingRowColors(True)
        lay.addWidget(self._target_list, 1)

        tp_btns = QHBoxLayout()
        for label, slot in [
            ("Add",    lambda: self._routing_add_target()),
            ("Edit",   lambda: self._routing_edit_target()),
            ("Delete", lambda: self._routing_delete_target()),
            ("Test",   lambda: self._routing_test_target()),
        ]:
            btn = QPushButton(label)
            btn.setFixedHeight(30)
            tp_btns.addWidget(btn)
            btn.clicked.connect(slot)
        lay.addLayout(tp_btns)

        # Reload from disk when tab is shown
        self._routing_refresh()
        return tab

    def _routing_refresh(self):
        from routing.loader import load_targets, load_bindings
        try:
            self._routing_targets = load_targets()
        except Exception:
            self._routing_targets = []
        try:
            if not hasattr(self, '_routing_bindings') or not self._routing_bindings:
                self._routing_bindings = load_bindings()
        except Exception:
            self._routing_bindings = []
        self._routing_render_targets()
        # Bindings are rendered in the Hotkeys tab; refresh if the widget exists
        if hasattr(self, '_binding_list') and self._binding_list is not None:
            self._routing_render_bindings()

    def _routing_render_targets(self):
        self._target_list.clear()
        for t in self._routing_targets:
            label = f"{t.label}  [{t.delivery.value}]  id={t.id!r}"
            # Compute feature warnings: features the target wants but are globally off
            warnings = t.processing.get_feature_warnings(self.config.config)
            # Also warn if target has a response_pipe but TTS is globally disabled
            if t.response_pipe and not self.config.get("tts_enabled", False):
                from routing.models import FEATURE_LABELS
                warnings.append(("tts", FEATURE_LABELS.get("tts", "TTS / Voice Out")))
            if warnings:
                badge = "  ⚠ " + ", ".join(lbl for _, lbl in warnings) + " disabled"
                label += badge
            item = QListWidgetItem(label)
            item.setData(Qt.ItemDataRole.UserRole, t)
            if warnings:
                item.setForeground(QColor('#f59e0b'))  # amber warning colour
            self._target_list.addItem(item)

    def _routing_render_bindings(self):
        self._binding_list.clear()
        target_map = {t.id: t.label for t in self._routing_targets}
        for b in self._routing_bindings:
            keys_str = '+'.join(k.replace('KEY_', '') for k in b.keys)
            tgt_label = target_map.get(b.target_id, b.target_id)
            txt = f"{b.label or b.id}  [{b.gesture.value}]  {keys_str} → {tgt_label}"
            if b.disabled:
                txt = f"[disabled] {txt}"
            item = QListWidgetItem(txt)
            item.setData(Qt.ItemDataRole.UserRole, b)
            if b.disabled:
                item.setForeground(QColor('#4a5568'))
            self._binding_list.addItem(item)

    def _routing_selected_target(self):
        item = self._target_list.currentItem()
        if item:
            return item.data(Qt.ItemDataRole.UserRole)
        return None

    def _routing_selected_binding(self):
        item = self._binding_list.currentItem()
        if item:
            return item.data(Qt.ItemDataRole.UserRole)
        return None

    def _routing_save_targets(self):
        from routing.loader import save_targets
        try:
            save_targets(self._routing_targets)
        except Exception as e:
            QMessageBox.warning(self, "Save Error", f"Could not save targets.toml:\n{e}")

    def _routing_save_bindings(self):
        from routing.loader import save_bindings
        try:
            save_bindings(self._routing_bindings)
        except Exception as e:
            QMessageBox.warning(self, "Save Error", f"Could not save bindings.toml:\n{e}")

    def _routing_add_target(self):
        dlg = _TargetEditorDialog(config=self.config, parent=self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            self._routing_targets.append(dlg.result_target)
            self._routing_save_targets()
            self._routing_render_targets()
            if self.router:
                self.router.update_targets(self._routing_targets)

    def _routing_edit_target(self):
        tgt = self._routing_selected_target()
        if tgt is None:
            return
        dlg = _TargetEditorDialog(target=tgt, config=self.config, parent=self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            idx = next((i for i, t in enumerate(self._routing_targets) if t.id == tgt.id), None)
            if idx is not None:
                self._routing_targets[idx] = dlg.result_target
            self._routing_save_targets()
            self._routing_render_targets()
            if self.router:
                self.router.update_targets(self._routing_targets)

    def _routing_delete_target(self):
        tgt = self._routing_selected_target()
        if tgt is None:
            return
        used_by = [b.id for b in self._routing_bindings if b.target_id == tgt.id]
        if used_by:
            QMessageBox.warning(
                self, "Cannot Delete",
                f"Target '{tgt.id}' is referenced by bindings: {', '.join(used_by)}.\n"
                "Remove those bindings first."
            )
            return
        if QMessageBox.question(
            self, "Delete Target",
            f"Delete target '{tgt.label}' ({tgt.id})?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        ) == QMessageBox.StandardButton.Yes:
            self._routing_targets = [t for t in self._routing_targets if t.id != tgt.id]
            self._routing_save_targets()
            self._routing_render_targets()
            if self.router:
                self.router.update_targets(self._routing_targets)

    def _routing_test_target(self):
        tgt = self._routing_selected_target()
        if tgt is None:
            return
        if self.router:
            result = self.router.test_target(tgt.id)
            icon = "✅" if result.reachable else "❌"
            QMessageBox.information(self, "Target Test", f"{icon}  {result.detail}")
        else:
            QMessageBox.information(self, "Target Test", "Router not available.")

    # Binding CRUD is in _hotkey_add/edit/delete_binding (Hotkeys tab).
    # These aliases kept so any external caller/test can still reference them.


# ── Helpers for 3-state (None / True / False) combos ─────────────────────────

def _make_tristate_combo(value=None) -> QComboBox:
    """Return a QComboBox with Default / Enabled / Disabled options.

    value: None → 'Default (global)', True → 'Enabled', False → 'Disabled'
    """
    cb = QComboBox()
    cb.addItem("Default (global)", None)
    cb.addItem("Enabled", True)
    cb.addItem("Disabled", False)
    if value is True:
        cb.setCurrentIndex(1)
    elif value is False:
        cb.setCurrentIndex(2)
    else:
        cb.setCurrentIndex(0)
    return cb


# ── Target Editor Dialog ──────────────────────────────────────────────────────

class _TargetEditorDialog(QDialog):
    def __init__(self, target=None, config=None, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Edit Target" if target else "Add Target")
        self.setMinimumWidth(520)
        self.setMinimumHeight(600)
        self.setStyleSheet(QSS)
        self.result_target = None
        self._config = config  # global Config object for showing default values
        self._build_ui(target)

    def _build_ui(self, tgt):
        from routing.models import DeliveryType
        p = tgt.processing if tgt else None  # TargetProcessingConfig | None

        outer = QVBoxLayout(self)
        outer.setSpacing(8)
        outer.setContentsMargins(12, 12, 12, 12)

        # ── Scrollable content ─────────────────────────────────────────────
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        content = QWidget()
        lay = QVBoxLayout(content)
        lay.setSpacing(10)
        lay.setContentsMargins(0, 0, 8, 0)

        # ── Identity ───────────────────────────────────────────────────────
        id_form = QFormLayout()
        id_form.setSpacing(6)
        self._id_edit = QLineEdit(tgt.id if tgt else "")
        self._label_edit = QLineEdit(tgt.label if tgt else "")
        self._delivery_combo = QComboBox()
        for dt in DeliveryType:
            self._delivery_combo.addItem(dt.value, dt)
        if tgt:
            idx = self._delivery_combo.findData(tgt.delivery)
            if idx >= 0:
                self._delivery_combo.setCurrentIndex(idx)
        self._append_newline_cb = QCheckBox("Append newline after text")
        self._append_newline_cb.setChecked(tgt.append_newline if tgt else True)

        id_form.addRow("ID:", self._id_edit)
        id_form.addRow("Label:", self._label_edit)
        id_form.addRow("Delivery:", self._delivery_combo)
        id_form.addRow("", self._append_newline_cb)
        lay.addLayout(id_form)

        # ── Delivery-specific fields ───────────────────────────────────────
        self._delivery_group = QGroupBox("Delivery Settings")
        df_lay = QFormLayout(self._delivery_group)
        df_lay.setSpacing(6)
        self._df_lay = df_lay

        self._command_edit = QLineEdit(tgt.command or "" if tgt else "")
        self._command_edit.setPlaceholderText("e.g. xdg-open {TEXT}")
        self._pipe_path_edit = QLineEdit(tgt.pipe_path or "" if tgt else "")
        self._pipe_path_edit.setPlaceholderText("/tmp/myfifo")
        self._socket_host_edit = QLineEdit(tgt.socket_host or "localhost" if tgt else "localhost")
        self._socket_port_spin = QSpinBox()
        self._socket_port_spin.setRange(1, 65535)
        self._socket_port_spin.setValue(tgt.socket_port or 9000 if tgt else 9000)
        self._socket_unix_edit = QLineEdit(tgt.socket_unix or "" if tgt else "")
        self._socket_unix_edit.setPlaceholderText("/tmp/app.sock")
        self._file_path_edit = QLineEdit(tgt.file_path or "" if tgt else "")
        self._file_path_edit.setPlaceholderText("~/notes.md")
        self._file_prefix_edit = QLineEdit(tgt.file_prefix or "" if tgt else "")
        self._file_prefix_edit.setPlaceholderText("- ")
        self._file_ts_cb = QCheckBox("Include timestamp")
        self._file_ts_cb.setChecked(tgt.file_timestamp if tgt else True)
        self._dbus_signal_edit = QLineEdit(tgt.dbus_signal or "" if tgt else "")
        self._dbus_signal_edit.setPlaceholderText("com.example.App.TextReady")

        df_lay.addRow("Command {TEXT}:", self._command_edit)
        df_lay.addRow("Pipe path:", self._pipe_path_edit)
        df_lay.addRow("Socket host:", self._socket_host_edit)
        df_lay.addRow("Socket port:", self._socket_port_spin)
        df_lay.addRow("Unix socket:", self._socket_unix_edit)
        df_lay.addRow("File path:", self._file_path_edit)
        df_lay.addRow("File prefix:", self._file_prefix_edit)
        df_lay.addRow("", self._file_ts_cb)
        df_lay.addRow("DBus signal:", self._dbus_signal_edit)
        lay.addWidget(self._delivery_group)

        # ── Preprocessing ──────────────────────────────────────────────────
        pre_group = QGroupBox("Preprocessing  (None = use global setting)")
        pre_form = QFormLayout(pre_group)
        pre_form.setSpacing(6)

        self._noise_suppression_combo = _make_tristate_combo(p.noise_suppression if p else None)
        self._quiet_mode_combo = _make_tristate_combo(p.quiet_mode if p else None)
        self._atspi_context_combo = _make_tristate_combo(p.atspi_context if p else None)
        self._initial_prompt_edit = QLineEdit(tgt.initial_prompt or "" if tgt else "")
        self._initial_prompt_edit.setPlaceholderText("Whisper context (leave blank = auto)")

        pre_form.addRow("Noise suppression:", self._noise_suppression_combo)
        pre_form.addRow("Quiet mode:", self._quiet_mode_combo)
        pre_form.addRow("AT-SPI2 context:", self._atspi_context_combo)
        pre_form.addRow("Initial prompt:", self._initial_prompt_edit)

        # Global-off warnings for preprocessing
        if self._config and not self._config.get("noise_suppression", False):
            pre_form.addRow("", _hint_warn("Noise suppression is globally disabled — "
                                           "enabling here will have no effect until turned on globally."))
        lay.addWidget(pre_group)

        # ── Postprocessing ─────────────────────────────────────────────────
        post_group = QGroupBox("Postprocessing  (None = use global setting)")
        post_form = QFormLayout(post_group)
        post_form.setSpacing(6)

        self._remove_fillers_combo = _make_tristate_combo(p.remove_fillers if p else None)
        self._spoken_punct_combo = _make_tristate_combo(p.spoken_punctuation if p else None)
        self._auto_lists_combo = _make_tristate_combo(p.auto_format_lists if p else None)
        self._apply_snippets_combo = _make_tristate_combo(p.apply_snippets if p else None)
        self._code_mode_combo = _make_tristate_combo(p.code_mode if p else None)

        post_form.addRow("Remove fillers (uh/um):", self._remove_fillers_combo)
        post_form.addRow("Spoken punctuation:", self._spoken_punct_combo)
        post_form.addRow("Auto-format lists:", self._auto_lists_combo)
        post_form.addRow("Apply snippets:", self._apply_snippets_combo)
        post_form.addRow("Code dictation mode:", self._code_mode_combo)
        lay.addWidget(post_group)

        # ── Ollama / LLM ───────────────────────────────────────────────────
        ollama_group = QGroupBox("Ollama / LLM Post-Processing")
        ollama_form = QFormLayout(ollama_group)
        ollama_form.setSpacing(6)

        self._ollama_enabled_combo = _make_tristate_combo(p.ollama_enabled if p else None)

        self._ollama_model_edit = QLineEdit(p.ollama_model or "" if p else "")
        self._ollama_model_edit.setPlaceholderText(
            self._config.get("ollama_model", "llama3.2:1b") if self._config else "llama3.2:1b"
        )

        self._ollama_mode_combo = QComboBox()
        self._ollama_mode_combo.addItem("Default (global)", "")
        from llm_postprocessor import MODE_LABELS
        for mode_key, mode_label in MODE_LABELS.items():
            if mode_key != "off":
                self._ollama_mode_combo.addItem(mode_label, mode_key)
        target_mode = p.ollama_mode if p else None
        if target_mode:
            idx = self._ollama_mode_combo.findData(target_mode)
            if idx >= 0:
                self._ollama_mode_combo.setCurrentIndex(idx)

        self._ollama_prompt_edit = QTextEdit()
        self._ollama_prompt_edit.setMaximumHeight(80)
        self._ollama_prompt_edit.setPlaceholderText(
            "Custom system prompt (optional). Use {text} as placeholder for the transcription.\n"
            "Leave blank to use the selected mode's built-in prompt."
        )
        if p and p.ollama_prompt:
            self._ollama_prompt_edit.setPlainText(p.ollama_prompt)

        ollama_form.addRow("Ollama enabled:", self._ollama_enabled_combo)
        ollama_form.addRow("Model override:", self._ollama_model_edit)
        ollama_form.addRow("Mode:", self._ollama_mode_combo)
        ollama_form.addRow("Custom prompt:", self._ollama_prompt_edit)

        if self._config and not self._config.get("ollama_enabled", False):
            ollama_form.addRow("", _hint_warn(
                "Ollama is globally disabled (AI tab). Enabling it here will have no "
                "effect until the global toggle is turned on."
            ))
        lay.addWidget(ollama_group)

        scroll.setWidget(content)
        outer.addWidget(scroll, 1)

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.accepted.connect(self._accept)
        btns.rejected.connect(self.reject)
        outer.addWidget(btns)

        self._delivery_combo.currentIndexChanged.connect(self._update_delivery_fields)
        self._update_delivery_fields()
        self._label_edit.textChanged.connect(self._auto_id)

    def _auto_id(self, text):
        if not self._id_edit.text() or self._id_edit.text() == self._last_auto_id():
            safe = text.lower().replace(' ', '_')
            import re
            safe = re.sub(r'[^a-z0-9_]', '', safe)
            self._id_edit.setText(safe)

    def _last_auto_id(self):
        import re
        text = self._label_edit.text().lower().replace(' ', '_')
        return re.sub(r'[^a-z0-9_]', '', text)

    def _update_delivery_fields(self):
        from routing.models import DeliveryType
        dt = self._delivery_combo.currentData()
        show_command = dt == DeliveryType.EXEC
        show_pipe = dt == DeliveryType.PIPE
        show_socket = dt in (DeliveryType.SOCKET,)
        show_file = dt == DeliveryType.FILE
        show_dbus = dt == DeliveryType.DBUS

        self._command_edit.setVisible(show_command)
        lbl = self._df_lay.labelForField(self._command_edit)
        if lbl:
            lbl.setVisible(show_command)
        self._pipe_path_edit.setVisible(show_pipe)
        lbl = self._df_lay.labelForField(self._pipe_path_edit)
        if lbl:
            lbl.setVisible(show_pipe)
        self._socket_host_edit.setVisible(show_socket)
        lbl = self._df_lay.labelForField(self._socket_host_edit)
        if lbl:
            lbl.setVisible(show_socket)
        self._socket_port_spin.setVisible(show_socket)
        lbl = self._df_lay.labelForField(self._socket_port_spin)
        if lbl:
            lbl.setVisible(show_socket)
        self._socket_unix_edit.setVisible(show_socket)
        lbl = self._df_lay.labelForField(self._socket_unix_edit)
        if lbl:
            lbl.setVisible(show_socket)
        self._file_path_edit.setVisible(show_file)
        lbl = self._df_lay.labelForField(self._file_path_edit)
        if lbl:
            lbl.setVisible(show_file)
        self._file_prefix_edit.setVisible(show_file)
        lbl = self._df_lay.labelForField(self._file_prefix_edit)
        if lbl:
            lbl.setVisible(show_file)
        self._file_ts_cb.setVisible(show_file)
        self._dbus_signal_edit.setVisible(show_dbus)
        lbl = self._df_lay.labelForField(self._dbus_signal_edit)
        if lbl:
            lbl.setVisible(show_dbus)

    def _accept(self):
        from routing.models import OutputTarget, TargetProcessingConfig
        target_id = self._id_edit.text().strip()
        if not target_id:
            QMessageBox.warning(self, "Validation", "ID is required.")
            return
        label = self._label_edit.text().strip() or target_id
        delivery = self._delivery_combo.currentData()

        # Build TargetProcessingConfig from UI
        processing = TargetProcessingConfig(
            # Preprocessing
            noise_suppression=self._noise_suppression_combo.currentData(),
            quiet_mode=self._quiet_mode_combo.currentData(),
            atspi_context=self._atspi_context_combo.currentData(),
            # Postprocessing
            remove_fillers=self._remove_fillers_combo.currentData(),
            spoken_punctuation=self._spoken_punct_combo.currentData(),
            auto_format_lists=self._auto_lists_combo.currentData(),
            apply_snippets=self._apply_snippets_combo.currentData(),
            code_mode=self._code_mode_combo.currentData(),
            # Ollama
            ollama_enabled=self._ollama_enabled_combo.currentData(),
            ollama_model=self._ollama_model_edit.text().strip() or None,
            ollama_mode=self._ollama_mode_combo.currentData() or None,
            ollama_prompt=self._ollama_prompt_edit.toPlainText().strip() or None,
        )

        self.result_target = OutputTarget(
            id=target_id,
            label=label,
            delivery=delivery,
            command=self._command_edit.text().strip() or None,
            pipe_path=self._pipe_path_edit.text().strip() or None,
            socket_host=self._socket_host_edit.text().strip() or None,
            socket_port=self._socket_port_spin.value(),
            socket_unix=self._socket_unix_edit.text().strip() or None,
            file_path=self._file_path_edit.text().strip() or None,
            file_prefix=self._file_prefix_edit.text(),
            file_timestamp=self._file_ts_cb.isChecked(),
            dbus_signal=self._dbus_signal_edit.text().strip() or None,
            processing=processing,
            append_newline=self._append_newline_cb.isChecked(),
            initial_prompt=self._initial_prompt_edit.text().strip() or None,
        )
        self.accept()


# ── Binding Editor Dialog ─────────────────────────────────────────────────────

class _BindingEditorDialog(QDialog):
    def __init__(self, binding=None, targets=None, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Edit Binding" if binding else "Add Binding")
        self.setMinimumWidth(420)
        self.setStyleSheet(QSS)
        self.result_binding = None
        self._targets = targets or []
        self._build_ui(binding)

    def _build_ui(self, b):
        from routing.models import GestureType
        lay = QVBoxLayout(self)
        form = QFormLayout()
        form.setSpacing(8)

        self._id_edit = QLineEdit(b.id if b else "")
        self._label_edit = QLineEdit(b.label if b else "")
        self._label_edit.textChanged.connect(self._auto_id)

        # Keys: comma-separated evdev names
        self._keys_edit = QLineEdit(', '.join(b.keys) if b else "")
        self._keys_edit.setPlaceholderText("e.g. KEY_LEFTCTRL, KEY_SPACE")

        self._gesture_combo = QComboBox()
        for g in GestureType:
            self._gesture_combo.addItem(g.value, g)
        if b:
            idx = self._gesture_combo.findData(b.gesture)
            if idx >= 0:
                self._gesture_combo.setCurrentIndex(idx)

        self._target_combo = QComboBox()
        for t in self._targets:
            self._target_combo.addItem(f"{t.label}  [{t.delivery.value}]", t.id)
        if b:
            idx = self._target_combo.findData(b.target_id)
            if idx >= 0:
                self._target_combo.setCurrentIndex(idx)

        self._tap_ms_spin = QSpinBox()
        self._tap_ms_spin.setRange(100, 600)
        self._tap_ms_spin.setValue(b.tap_ms if b else 250)
        self._tap_ms_spin.setSuffix(" ms")

        self._hold_ms_spin = QSpinBox()
        self._hold_ms_spin.setRange(50, 500)
        self._hold_ms_spin.setValue(b.hold_threshold_ms if b else 200)
        self._hold_ms_spin.setSuffix(" ms")

        self._disabled_cb = QCheckBox("Disabled")
        self._disabled_cb.setChecked(b.disabled if b else False)

        form.addRow("ID:", self._id_edit)
        form.addRow("Label:", self._label_edit)
        form.addRow("Keys:", self._keys_edit)
        form.addRow("Gesture:", self._gesture_combo)
        form.addRow("Target:", self._target_combo)
        form.addRow("Double-tap window:", self._tap_ms_spin)
        form.addRow("Hold threshold:", self._hold_ms_spin)
        form.addRow("", self._disabled_cb)
        lay.addLayout(form)

        lay.addWidget(_hint("Keys: comma-separated evdev names, e.g. KEY_LEFTCTRL or KEY_LEFTMETA, KEY_SPACE"))

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.accepted.connect(self._accept)
        btns.rejected.connect(self.reject)
        lay.addWidget(btns)

    def _auto_id(self, text):
        import re
        safe = re.sub(r'[^a-z0-9_]', '', text.lower().replace(' ', '_'))
        if not self._id_edit.text() or self._id_edit.text() == self._last_safe:
            self._id_edit.setText(safe)
        self._last_safe = safe

    _last_safe = ""

    def _accept(self):
        from routing.models import HotkeyBinding
        binding_id = self._id_edit.text().strip()
        if not binding_id:
            QMessageBox.warning(self, "Validation", "ID is required.")
            return
        keys_raw = self._keys_edit.text().strip()
        if not keys_raw:
            QMessageBox.warning(self, "Validation", "At least one key is required.")
            return
        keys = [k.strip() for k in keys_raw.split(',') if k.strip()]
        target_id = self._target_combo.currentData()
        if not target_id:
            QMessageBox.warning(self, "Validation", "Select a target.")
            return
        self.result_binding = HotkeyBinding(
            id=binding_id,
            label=self._label_edit.text().strip(),
            keys=keys,
            gesture=self._gesture_combo.currentData(),
            target_id=target_id,
            tap_ms=self._tap_ms_spin.value(),
            hold_threshold_ms=self._hold_ms_spin.value(),
            disabled=self._disabled_cb.isChecked(),
        )
        self.accept()
