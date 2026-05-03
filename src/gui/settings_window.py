import os
import pyaudio
import evdev
import shutil
import grp
from evdev import ecodes
from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QComboBox,
    QPushButton, QMessageBox, QSlider, QCheckBox, QTextEdit,
    QTabWidget, QGroupBox, QSizePolicy, QFrame, QScrollArea,
    QLineEdit, QProgressBar, QTableWidget, QHeaderView, QTableWidgetItem
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
QTabWidget::pane {
    border: 1px solid #1e2433;
    border-radius: 8px;
    background: #0f1117;
}
QTabBar::tab {
    background: #1a1f2e;
    color: #8892a4;
    padding: 10px 20px;
    margin-right: 2px;
    border-top-left-radius: 8px;
    border-top-right-radius: 8px;
    font-weight: 500;
}
QTabBar::tab:selected {
    background: #0f1117;
    color: #4a9eff;
    border-bottom: 2px solid #4a9eff;
}
QTabBar::tab:hover:!selected {
    background: #1e2433;
    color: #c8d3e0;
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
QCheckBox::indicator:checked:after { content: ""; }
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

    def __init__(self, config, inference_engine=None, audio_recorder=None):
        super().__init__()
        self.config = config
        self.inference_engine = inference_engine
        self.audio_recorder = audio_recorder
        self.recorded_keys = set()
        self.recorded_toggle_keys = set()
        self.active_recording_mode = None

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
            status = QLabel(
                f"  Running: {config.get('model_size')} · "
                f"{inference_engine.actual_device.upper()} · "
                f"{inference_engine.actual_compute_type}"
            )
        else:
            status = QLabel("  Engine not connected")
        status.setObjectName("hint")
        root.addWidget(status)

        # ── Tabs ──────────────────────────────────────────────────────────
        tabs = QTabWidget()
        tabs.addTab(self._tab_general(), "🎙  General")
        tabs.addTab(self._tab_audio(), "🔊  Audio")
        tabs.addTab(self._tab_hotkeys(), "⌨  Hotkeys")
        tabs.addTab(self._tab_dictation(), "✨  Dictation")
        tabs.addTab(self._tab_snippets(), "📎  Snippets")
        tabs.addTab(self._tab_ai(), "🤖  AI")
        root.addWidget(tabs)

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
        lay.addWidget(toggle_box)

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
        self.overlay_checkbox = QCheckBox("Show real-time waveform overlay while recording")
        self.overlay_checkbox.setChecked(self.config.get("show_overlay", True))
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
        for cb in (self.quiet_mode_checkbox, self.overlay_checkbox,
                   self.notification_checkbox, self.code_mode_checkbox):
            recording_box.layout().addWidget(cb)
        recording_box.layout().addWidget(_hint(
            "Code mode example: say \"get underscore user dot name\" → get_user.name"
        ))
        lay.addWidget(recording_box)

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

    def populate_audio_devices(self):
        p = pyaudio.PyAudio()
        current_index = self.config.get("input_device_index")
        for i in range(p.get_device_count()):
            info = p.get_device_info_by_index(i)
            if info.get("maxInputChannels", 0) > 0:
                name = info.get("name", f"Device {i}")
                self.device_combo.addItem(f"{name}", i)
                if current_index == i:
                    self.device_combo.setCurrentIndex(self.device_combo.count() - 1)
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
        btn = self.record_btn_hold if mode == "hold" else self.record_btn_toggle

        if btn.isChecked():
            other_btn = self.record_btn_toggle if mode == "hold" else self.record_btn_hold
            if other_btn.isChecked():
                other_btn.setChecked(False)

            self.active_recording_mode = mode
            label = self.hotkey_label if mode == "hold" else self.toggle_hotkey_label
            label.setText("Press keys…")
            if mode == "hold":
                self.recorded_keys = set()
            else:
                self.recorded_toggle_keys = set()
            btn.setText("Done")
            self.grabKeyboard()
        else:
            self.stop_recording()

    def stop_recording(self):
        self.active_recording_mode = None
        for btn in (self.record_btn_hold, self.record_btn_toggle):
            btn.setChecked(False)
            btn.setText("Record")
        self.releaseKeyboard()
        if not self.recorded_keys:
            self.hotkey_label.setText(self._fmt_keys(self.config.get("hotkey", [])))
        if not self.recorded_toggle_keys:
            self.toggle_hotkey_label.setText(self._fmt_keys(self.config.get("toggle_hotkey", [])))

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
            else:
                self.recorded_toggle_keys.add(key_name)
                self.toggle_hotkey_label.setText(self._fmt_keys(sorted(self.recorded_toggle_keys)))

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
        restart_keys = ["model_size", "device", "inference_mode", "input_device_index"]
        old_vals = {k: self.config.get(k) for k in restart_keys}

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

        # Hotkeys
        if self.recorded_keys:
            self.config.set("hotkey", sorted(self.recorded_keys))
        if self.recorded_toggle_keys:
            self.config.set("toggle_hotkey", sorted(self.recorded_toggle_keys))
        # Dictation
        self.config.set("remove_fillers", self.filler_checkbox.isChecked())
        self.config.set("spoken_punctuation", self.spoken_punct_checkbox.isChecked())
        self.config.set("auto_format_lists", self.auto_list_checkbox.isChecked())
        self.config.set("quiet_mode", self.quiet_mode_checkbox.isChecked())
        self.config.set("show_overlay", self.overlay_checkbox.isChecked())
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
