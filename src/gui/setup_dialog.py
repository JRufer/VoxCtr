"""
First-run permissions setup dialog for Whisper-Wayland.

Global hotkeys require read access to /dev/input/event* which is gated behind
the 'input' group on most Linux systems. This module detects missing permissions
and provides a one-click fix via pkexec (polkit) — no terminal or manual script
running needed.
"""

import glob
import grp
import os
import pwd
import shutil
import subprocess
from pathlib import Path

from PyQt6.QtCore import Qt, QThread, pyqtSignal
from PyQt6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel, QPushButton, QVBoxLayout,
)

_SCRIPTS_DIR = Path(__file__).parent.parent.parent / 'scripts'
_UDEV_RULE_PATH = Path('/etc/udev/rules.d/99-whisper-wayland.rules')


# ── Permission detection ──────────────────────────────────────────────────────

def can_access_input_devices() -> bool:
    """Return True if the process can open at least one /dev/input/event* device."""
    candidates = sorted(glob.glob('/dev/input/event*'))[:8]
    if not candidates:
        return True  # No devices present (VM / unusual system) — don't block startup
    for path in candidates:
        try:
            with open(path, 'rb'):
                return True
        except PermissionError:
            return False
        except OSError:
            continue
    return True


def _username() -> str:
    try:
        return pwd.getpwuid(os.getuid()).pw_name
    except Exception:
        return os.environ.get('USER', '')


def user_in_input_group() -> bool:
    """Return True if the user is listed in /etc/group for 'input' (reflects usermod, pre-relogin)."""
    try:
        info = grp.getgrnam('input')
        name = _username()
        if pwd.getpwnam(name).pw_gid == info.gr_gid:
            return True
        return name in info.gr_mem
    except Exception:
        return False


def udev_rule_exists() -> bool:
    return _UDEV_RULE_PATH.exists()


def needs_setup() -> bool:
    """Return True if the permissions setup dialog should be shown at startup."""
    return not can_access_input_devices()


# ── Worker thread (runs setup script via pkexec in background) ───────────────

class _SetupWorker(QThread):
    finished = pyqtSignal(bool, str)  # (success, error_message)

    def run(self):
        script = _SCRIPTS_DIR / 'setup-permissions.sh'
        if not script.exists():
            self.finished.emit(False, f"Setup script not found:\n{script}")
            return

        pkexec = shutil.which('pkexec')
        if not pkexec:
            self.finished.emit(
                False,
                "pkexec not found. Run this in a terminal instead:\n\n"
                f"  sudo bash {script}\n\nThen log out and back in.",
            )
            return

        try:
            result = subprocess.run(
                [pkexec, 'bash', str(script)],
                capture_output=True,
                text=True,
                timeout=60,
            )
            if result.returncode == 0:
                self.finished.emit(True, "")
            else:
                stderr = result.stderr.strip() or f"Exit code {result.returncode}"
                self.finished.emit(False, stderr)
        except subprocess.TimeoutExpired:
            self.finished.emit(False, "Operation timed out.")
        except Exception as e:
            self.finished.emit(False, str(e))


# ── Dialog ────────────────────────────────────────────────────────────────────

_QSS = """
QDialog {
    background: #0f1117;
}
QLabel {
    color: #e2e8f0;
    font-size: 13px;
    background: transparent;
}
QFrame#card {
    background: #131720;
    border: 1px solid #1e2433;
    border-radius: 8px;
}
QFrame#divider {
    background: #1e2433;
    border: none;
    max-height: 1px;
}
QPushButton#primary {
    background: #4a9eff;
    color: #ffffff;
    border: none;
    border-radius: 6px;
    padding: 10px 22px;
    font-size: 13px;
    font-weight: 600;
    min-width: 190px;
}
QPushButton#primary:hover   { background: #5aaaff; }
QPushButton#primary:pressed  { background: #3a8ef0; }
QPushButton#primary:disabled { background: #2a3448; color: #4a5568; }
QPushButton#secondary {
    background: transparent;
    color: #8892a4;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 10px 22px;
    font-size: 13px;
}
QPushButton#secondary:hover { color: #e2e8f0; border-color: #4a9eff; }
"""


class PermissionsSetupDialog(QDialog):
    """
    Shown at startup when /dev/input/event* is not accessible.

    Detects whether the user needs full setup or just a re-login, then offers
    a one-click fix via pkexec (polkit auth dialog — no terminal needed).
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Whisper-Wayland — Hotkey Setup")
        self.setStyleSheet(_QSS)
        self.setMinimumWidth(520)
        self.setModal(False)
        self._worker = None
        self._already_configured = user_in_input_group() and udev_rule_exists()
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setSpacing(0)
        layout.setContentsMargins(28, 26, 28, 26)

        # Title
        title = QLabel("Global Hotkey Setup")
        title.setStyleSheet(
            "font-size: 18px; font-weight: 700; color: #f0f4f8; margin-bottom: 4px;"
        )
        layout.addWidget(title)

        if self._already_configured:
            sub_text = "Setup is complete — just log out and back in to activate"
        else:
            sub_text = "One-time permission setup needed for hold-to-talk and global hotkeys"
        sub = QLabel(sub_text)
        sub.setStyleSheet("color: #8892a4; font-size: 12px; margin-bottom: 20px;")
        layout.addWidget(sub)

        # Explanation card
        card = QFrame()
        card.setObjectName("card")
        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(16, 14, 16, 14)
        card_layout.setSpacing(0)

        if self._already_configured:
            body_text = (
                "Your user account has already been added to the <code>input</code> group "
                "and the udev rule is installed, but the change won't be visible to the "
                "current login session until you log out and back in.<br><br>"
                "After re-logging in, hold-to-talk and all other hotkeys will work "
                "automatically — no further setup needed."
            )
        else:
            body_text = (
                "To capture keyboard shortcuts globally (while other windows are focused), "
                "Whisper-Wayland needs read access to <code>/dev/input/event*</code>.<br><br>"
                "This is gated behind the <code>input</code> system group. "
                "Click <b>Set Up Permissions</b> and you will be prompted for your "
                "administrator password — this only needs to happen once."
            )

        body = QLabel(body_text)
        body.setWordWrap(True)
        body.setTextFormat(Qt.TextFormat.RichText)
        body.setStyleSheet("color: #c8d3e0; line-height: 1.6;")
        card_layout.addWidget(body)
        layout.addWidget(card)
        layout.addSpacing(18)

        # Status indicators
        self._status_label = QLabel()
        self._status_label.setTextFormat(Qt.TextFormat.RichText)
        self._status_label.setWordWrap(True)
        self._refresh_status()
        layout.addWidget(self._status_label)
        layout.addSpacing(22)

        # Divider
        div = QFrame()
        div.setObjectName("divider")
        div.setFixedHeight(1)
        layout.addWidget(div)
        layout.addSpacing(18)

        # Buttons
        btn_row = QHBoxLayout()
        btn_row.setSpacing(12)

        self._skip_btn = QPushButton("Skip — Use App Without Hotkeys")
        self._skip_btn.setObjectName("secondary")
        self._skip_btn.clicked.connect(self.accept)
        btn_row.addWidget(self._skip_btn)

        btn_row.addStretch()

        if self._already_configured:
            self._setup_btn = QPushButton("Close")
            self._setup_btn.setObjectName("primary")
            self._setup_btn.clicked.connect(self.accept)
        else:
            self._setup_btn = QPushButton("Set Up Permissions")
            self._setup_btn.setObjectName("primary")
            self._setup_btn.clicked.connect(self._start_setup)

        btn_row.addWidget(self._setup_btn)
        layout.addLayout(btn_row)

    def _refresh_status(self):
        in_group = user_in_input_group()
        udev = udev_rule_exists()

        def row(ok, text):
            icon, color = ("✓", "#4ade80") if ok else ("✗", "#f87171")
            return f'<span style="color:{color};">{icon}</span>&nbsp;&nbsp;{text}'

        lines = [
            row(in_group, "User added to <code>input</code> group"),
            row(udev, "udev rule installed"),
        ]
        self._status_label.setText(
            '<span style="color:#8892a4; font-size:12px;">Current status:</span><br>'
            + "<br>".join(lines)
        )

    def _start_setup(self):
        self._setup_btn.setEnabled(False)
        self._setup_btn.setText("Running setup…")
        self._skip_btn.setEnabled(False)
        self._worker = _SetupWorker()
        self._worker.finished.connect(self._on_finished)
        self._worker.start()

    def _on_finished(self, success: bool, error: str):
        self._setup_btn.setEnabled(True)
        self._skip_btn.setEnabled(True)
        self._refresh_status()

        if success:
            self._setup_btn.setText("Close")
            self._setup_btn.clicked.disconnect()
            self._setup_btn.clicked.connect(self.accept)
            self._skip_btn.setVisible(False)
            self._status_label.setText(
                '<span style="color:#4ade80; font-size:14px; font-weight:600;">'
                "✓ Permissions configured!"
                "</span><br><br>"
                '<span style="color:#c8d3e0;">'
                "Please <b>log out and log back in</b> for the group change to take effect.<br>"
                "After re-logging in, hold-to-talk and all other hotkeys will work "
                "automatically — no setup needed next time."
                "</span>"
            )
        else:
            self._setup_btn.setText("Retry Setup")
            self._status_label.setText(
                '<span style="color:#f87171;">Setup failed:</span><br>'
                f'<span style="color:#8892a4;">{error.replace(chr(10), "<br>")}</span>'
            )
