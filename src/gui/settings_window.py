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
    _hw_probe_finished = pyqtSignal(str, str)  # (hw_text, backend_text)

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
        filename = GGUF_MAP.get(model_size, f"ggml-{model_size}.bin")
        dest = os.path.join(model_dir, filename)

        if not hasattr(self, "_cpp_download_btn"):
            return

        self._cpp_download_btn.setEnabled(False)
        self._cpp_model_status.setText(f"⏳  Downloading {filename}…")
        self._cpp_model_status.setStyleSheet("color: #e2e8f0; background: transparent; border: none;")

        def do_download():
            from PyQt6.QtCore import QTimer
            try:
                os.makedirs(model_dir, exist_ok=True)
                urllib.request.urlretrieve(url, dest)
                def done():
                    self._cpp_model_status.setText(f"✅  Downloaded {filename} successfully.")
                    self._cpp_model_status.setStyleSheet("color: #4ade80; background: transparent; border: none;")
                    self._cpp_download_btn.setEnabled(True)
                QTimer.singleShot(0, done)
            except Exception as e:
                def err():
                    self._cpp_model_status.setText(f"❌  Download failed: {e}")
                    self._cpp_model_status.setStyleSheet("color: #f87171; background: transparent; border: none;")
                    self._cpp_download_btn.setEnabled(True)
                QTimer.singleShot(0, err)

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
        w = self._scrollable()
        lay = w.layout()

        _conflict_style = (
            "color:#f5a34f; font-size:12px; padding:3px 2px 0 2px;"
        )

        hold_box = _section("Hold-to-Talk")
        hold_box.layout().addWidget(_hint(
            "Hold this key combination to record. Release to transcribe and type."
        ))
        hold_row = QHBoxLayout()
        self.hotkey_label = QLabel(self._fmt_keys(self.config.get("hotkey", [])))
        self.hotkey_label.setObjectName("hotkey_pill")
        self.hotkey_label.setFrameShape(QFrame.Shape.StyledPanel)
        self.hotkey_label.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:7px 12px; color:#4a9eff; font-weight:600;"
        )
        self.record_btn_hold = QPushButton("Record")
        self.record_btn_hold.setObjectName("btn_record")
        self.record_btn_hold.setCheckable(True)
        self.record_btn_hold.clicked.connect(lambda: self.toggle_recording("hold"))
        hold_row.addWidget(self.hotkey_label, 1)
        hold_row.addWidget(self.record_btn_hold)
        hold_box.layout().addLayout(hold_row)
        self.hold_conflict_label = QLabel()
        self.hold_conflict_label.setStyleSheet(_conflict_style)
        self.hold_conflict_label.setWordWrap(True)
        self.hold_conflict_label.setVisible(False)
        hold_box.layout().addWidget(self.hold_conflict_label)
        lay.addWidget(hold_box)

        toggle_box = _section("Toggle-to-Talk")
        toggle_box.layout().addWidget(_hint(
            "Tap once to start recording, tap again to stop. Great for long dictation."
        ))
        toggle_row = QHBoxLayout()
        self.toggle_hotkey_label = QLabel(self._fmt_keys(self.config.get("toggle_hotkey", [])))
        self.toggle_hotkey_label.setFrameShape(QFrame.Shape.StyledPanel)
        self.toggle_hotkey_label.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:7px 12px; color:#4a9eff; font-weight:600;"
        )
        self.record_btn_toggle = QPushButton("Record")
        self.record_btn_toggle.setObjectName("btn_record")
        self.record_btn_toggle.setCheckable(True)
        self.record_btn_toggle.clicked.connect(lambda: self.toggle_recording("toggle"))
        toggle_row.addWidget(self.toggle_hotkey_label, 1)
        toggle_row.addWidget(self.record_btn_toggle)
        toggle_box.layout().addLayout(toggle_row)
        self.toggle_conflict_label = QLabel()
        self.toggle_conflict_label.setStyleSheet(_conflict_style)
        self.toggle_conflict_label.setWordWrap(True)
        self.toggle_conflict_label.setVisible(False)
        toggle_box.layout().addWidget(self.toggle_conflict_label)
        lay.addWidget(toggle_box)

        dt_box = _section("Double-Tap to Talk")
        dt_box.layout().addWidget(_hint(
            "Quickly tap the hotkey twice to start recording. Great for hands-free use."
        ))
        dt_row = QHBoxLayout()
        self.dt_hotkey_label = QLabel(self._fmt_keys(self.config.get("double_tap_hotkey", ["KEY_LEFTALT"])))
        self.dt_hotkey_label.setFrameShape(QFrame.Shape.StyledPanel)
        self.dt_hotkey_label.setStyleSheet(
            "background:#1a1f2e; border:1px solid #2a3448; border-radius:6px;"
            "padding:7px 12px; color:#4a9eff; font-weight:600;"
        )
        self.record_btn_dt = QPushButton("Record")
        self.record_btn_dt.setObjectName("btn_record")
        self.record_btn_dt.setCheckable(True)
        self.record_btn_dt.clicked.connect(lambda: self.toggle_recording("double_tap"))
        dt_row.addWidget(self.dt_hotkey_label, 1)
        dt_row.addWidget(self.record_btn_dt)
        dt_box.layout().addLayout(dt_row)
        self.dt_conflict_label = QLabel()
        self.dt_conflict_label.setStyleSheet(_conflict_style)
        self.dt_conflict_label.setWordWrap(True)
        self.dt_conflict_label.setVisible(False)
        dt_box.layout().addWidget(self.dt_conflict_label)
        lay.addWidget(dt_box)

        self._check_hotkey_conflicts()
        lay.addStretch()
        return w

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
        current = self.config.get("overlay_style", "waveform")
        if self.overlay_manager:
            overlays = self.overlay_manager.available()
        else:
            overlays = {"waveform": {"display_name": "Waveform", "description": "", "builtin": True}}
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

    def keyPressEvent(self, event):
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

    # ── System check ──────────────────────────────────────────────────────
    def run_system_check(self):
        import shutil, grp
        report = []
        ok = True

        if os.path.exists("/dev/uinput"):
            if os.access("/dev/uinput", os.W_OK):
                report.append("✅  /dev/uinput: Writable")
            else:
                report.append("❌  /dev/uinput: Permission denied"); ok = False
        else:
            report.append("❌  /dev/uinput: Not found"); ok = False

        try:
            groups = [grp.getgrgid(g).gr_name for g in os.getgroups()]
            if "input" in groups:
                report.append("✅  User is in 'input' group")
            else:
                report.append("❌  User NOT in 'input' group"); ok = False
        except Exception:
            report.append("⚠️  Could not verify group membership")

        if shutil.which("wl-copy"):
            report.append("✅  wl-clipboard: installed")
        else:
            report.append("❌  wl-clipboard: not found"); ok = False

        if self.inference_engine and self.inference_engine.actual_device == "cuda":
            report.append("✅  GPU (CUDA): active")

        from audio_recorder import _HAS_NOISEREDUCE
        if _HAS_NOISEREDUCE:
            report.append("✅  noisereduce: installed (noise suppression available)")
        else:
            report.append("ℹ️  noisereduce: not installed  (pip install noisereduce)")

        try:
            import dbus
            report.append("✅  dbus-python: installed (DBus control interface active)")
        except ImportError:
            report.append("ℹ️  dbus-python: not installed  (pip install dbus-python)")


        msg = "\n".join(report)
        if ok:
            QMessageBox.information(self, "System Check", f"Everything looks good!\n\n{msg}")
        else:
            fix = "\n\nTo fix permissions:\n  sudo usermod -aG input,uinput $USER\n(Log out and back in after running this)"
            QMessageBox.warning(self, "System Check", f"Issues found:\n\n{msg}{fix}")

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

        # Hotkeys (Sync to both config.json and bindings.toml)
        from routing.loader import save_bindings
        bindings_changed = False
        
        if self.recorded_keys:
            self.config.set("hotkey", sorted(self.recorded_keys))
            for b in self._routing_bindings:
                if b.id == 'default_hold':
                    b.keys = sorted(self.recorded_keys)
                    bindings_changed = True
                    
        if self.recorded_toggle_keys:
            self.config.set("toggle_hotkey", sorted(self.recorded_toggle_keys))
            for b in self._routing_bindings:
                if b.id == 'default_toggle':
                    b.keys = sorted(self.recorded_toggle_keys)
                    bindings_changed = True
                    
        if self.recorded_dt_keys:
            self.config.set("double_tap_hotkey", sorted(self.recorded_dt_keys))
            dt_b = next((b for b in self._routing_bindings if b.id == 'default_dt'), None)
            if dt_b:
                dt_b.keys = sorted(self.recorded_dt_keys)
            else:
                from routing.models import HotkeyBinding, GestureType
                self._routing_bindings.append(HotkeyBinding(
                    id='default_dt', label='Dictate (Double-Tap)',
                    keys=sorted(self.recorded_dt_keys),
                    gesture=GestureType.DOUBLE_TAP, target_id='default',
                ))
            bindings_changed = True

        if bindings_changed:
            save_bindings(self._routing_bindings)
            # Re-render routing tab if it has been instantiated
            if hasattr(self, '_routing_render_bindings'):
                self._routing_render_bindings()

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
            "Define output targets and map hotkey gestures to them. "
            "Changes are saved to ~/.config/whisper-wayland/targets.toml and bindings.toml."
        ))

        splitter = QSplitter(Qt.Orientation.Horizontal)

        # ── Targets panel ──────────────────────────────────────────────────
        targets_panel = QWidget()
        tp_lay = QVBoxLayout(targets_panel)
        tp_lay.setContentsMargins(0, 0, 0, 0)
        tp_lay.setSpacing(6)
        tp_head = QLabel("Output Targets")
        tp_head.setObjectName("section_head")
        tp_lay.addWidget(tp_head)
        self._target_list = QListWidget()
        self._target_list.setAlternatingRowColors(True)
        tp_lay.addWidget(self._target_list)
        tp_btns = QHBoxLayout()
        for label, slot in [
            ("Add", lambda: self._routing_add_target()),
            ("Edit", lambda: self._routing_edit_target()),
            ("Delete", lambda: self._routing_delete_target()),
            ("Test", lambda: self._routing_test_target()),
        ]:
            btn = QPushButton(label)
            btn.setFixedHeight(30)
            tp_btns.addWidget(btn)
            btn.clicked.connect(slot)
        tp_lay.addLayout(tp_btns)

        # ── Bindings panel ─────────────────────────────────────────────────
        bindings_panel = QWidget()
        bp_lay = QVBoxLayout(bindings_panel)
        bp_lay.setContentsMargins(0, 0, 0, 0)
        bp_lay.setSpacing(6)
        bp_head = QLabel("Hotkey Bindings")
        bp_head.setObjectName("section_head")
        bp_lay.addWidget(bp_head)
        self._binding_list = QListWidget()
        self._binding_list.setAlternatingRowColors(True)
        bp_lay.addWidget(self._binding_list)
        bp_btns = QHBoxLayout()
        for label, slot in [
            ("Add", lambda: self._routing_add_binding()),
            ("Edit", lambda: self._routing_edit_binding()),
            ("Delete", lambda: self._routing_delete_binding()),
        ]:
            btn = QPushButton(label)
            btn.setFixedHeight(30)
            bp_btns.addWidget(btn)
            btn.clicked.connect(slot)
        bp_lay.addLayout(bp_btns)

        splitter.addWidget(targets_panel)
        splitter.addWidget(bindings_panel)
        splitter.setSizes([260, 320])
        lay.addWidget(splitter, 1)

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
            self._routing_bindings = load_bindings()
        except Exception:
            self._routing_bindings = []
        self._routing_render_targets()
        self._routing_render_bindings()

    def _routing_render_targets(self):
        self._target_list.clear()
        for t in self._routing_targets:
            item = QListWidgetItem(f"{t.label}  [{t.delivery.value}]  id={t.id!r}")
            item.setData(Qt.ItemDataRole.UserRole, t)
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
        dlg = _TargetEditorDialog(parent=self)
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
        dlg = _TargetEditorDialog(target=tgt, parent=self)
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

    def _routing_add_binding(self):
        if not self._routing_targets:
            QMessageBox.warning(self, "No Targets", "Add at least one output target first.")
            return
        dlg = _BindingEditorDialog(targets=self._routing_targets, parent=self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            self._routing_bindings.append(dlg.result_binding)
            self._routing_save_bindings()
            self._routing_render_bindings()
            self.settings_saved.emit()

    def _routing_edit_binding(self):
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

    def _routing_delete_binding(self):
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


# ── Target Editor Dialog ──────────────────────────────────────────────────────

class _TargetEditorDialog(QDialog):
    def __init__(self, target=None, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Edit Target" if target else "Add Target")
        self.setMinimumWidth(420)
        self.setStyleSheet(QSS)
        self.result_target = None
        self._build_ui(target)

    def _build_ui(self, tgt):
        from routing.models import DeliveryType
        lay = QVBoxLayout(self)
        form = QFormLayout()
        form.setSpacing(8)

        self._id_edit = QLineEdit(tgt.id if tgt else "")
        self._label_edit = QLineEdit(tgt.label if tgt else "")

        self._delivery_combo = QComboBox()
        for dt in DeliveryType:
            self._delivery_combo.addItem(dt.value, dt)
        if tgt:
            idx = self._delivery_combo.findData(tgt.delivery)
            if idx >= 0:
                self._delivery_combo.setCurrentIndex(idx)

        self._pp_combo = QComboBox()
        for pp in ("default", "none", "strip_fillers", "ollama_only", "snippets_only"):
            self._pp_combo.addItem(pp, pp)
        if tgt:
            idx = self._pp_combo.findData(tgt.post_processing)
            if idx >= 0:
                self._pp_combo.setCurrentIndex(idx)

        self._append_newline_cb = QCheckBox("Append newline")
        self._append_newline_cb.setChecked(tgt.append_newline if tgt else True)

        # Delivery-specific fields
        self._command_edit = QLineEdit(tgt.command or "" if tgt else "")
        self._pipe_path_edit = QLineEdit(tgt.pipe_path or "" if tgt else "")
        self._socket_host_edit = QLineEdit(tgt.socket_host or "localhost" if tgt else "localhost")
        self._socket_port_spin = QSpinBox()
        self._socket_port_spin.setRange(1, 65535)
        self._socket_port_spin.setValue(tgt.socket_port or 9000 if tgt else 9000)
        self._socket_unix_edit = QLineEdit(tgt.socket_unix or "" if tgt else "")
        self._file_path_edit = QLineEdit(tgt.file_path or "" if tgt else "")
        self._file_prefix_edit = QLineEdit(tgt.file_prefix or "" if tgt else "")
        self._file_ts_cb = QCheckBox("Include timestamp")
        self._file_ts_cb.setChecked(tgt.file_timestamp if tgt else True)
        self._dbus_signal_edit = QLineEdit(tgt.dbus_signal or "" if tgt else "")
        self._initial_prompt_edit = QLineEdit(tgt.initial_prompt or "" if tgt else "")

        form.addRow("ID:", self._id_edit)
        form.addRow("Label:", self._label_edit)
        form.addRow("Delivery:", self._delivery_combo)
        form.addRow("Post-processing:", self._pp_combo)
        form.addRow("", self._append_newline_cb)

        # Container for delivery-specific fields
        self._delivery_fields = QWidget()
        self._df_lay = QFormLayout(self._delivery_fields)
        self._df_lay.setSpacing(6)
        self._df_lay.addRow("Command {TEXT}:", self._command_edit)
        self._df_lay.addRow("Pipe path:", self._pipe_path_edit)
        self._df_lay.addRow("Socket host:", self._socket_host_edit)
        self._df_lay.addRow("Socket port:", self._socket_port_spin)
        self._df_lay.addRow("Unix socket:", self._socket_unix_edit)
        self._df_lay.addRow("File path:", self._file_path_edit)
        self._df_lay.addRow("File prefix:", self._file_prefix_edit)
        self._df_lay.addRow("", self._file_ts_cb)
        self._df_lay.addRow("DBus signal:", self._dbus_signal_edit)
        self._df_lay.addRow("Initial prompt:", self._initial_prompt_edit)

        form.addRow(self._delivery_fields)
        lay.addLayout(form)

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.accepted.connect(self._accept)
        btns.rejected.connect(self.reject)
        lay.addWidget(btns)

        self._delivery_combo.currentIndexChanged.connect(self._update_delivery_fields)
        self._update_delivery_fields()

        # Auto-generate ID from label
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
        self._df_lay.labelForField(self._command_edit) and \
            self._df_lay.labelForField(self._command_edit).setVisible(show_command)
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
        self.adjustSize()

    def _accept(self):
        from routing.models import OutputTarget
        target_id = self._id_edit.text().strip()
        if not target_id:
            QMessageBox.warning(self, "Validation", "ID is required.")
            return
        label = self._label_edit.text().strip() or target_id
        delivery = self._delivery_combo.currentData()
        pp = self._pp_combo.currentData()

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
            post_processing=pp,
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
