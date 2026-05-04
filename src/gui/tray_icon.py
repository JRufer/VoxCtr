from PyQt6.QtWidgets import QSystemTrayIcon, QMenu, QApplication
from PyQt6.QtGui import QIcon, QAction, QColor, QPixmap
from PyQt6.QtCore import QObject
import os

class WhisperTrayIcon(QSystemTrayIcon):
    def __init__(self, app_state, history_window=None):
        super().__init__()
        self.app_state = app_state
        self.history_window = history_window
        self._active_target_label = ""

        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        assets_dir = os.path.join(base_dir, "assets")

        self.icon_on = QIcon(os.path.join(assets_dir, "record_on.png"))
        self.icon_off = QIcon(os.path.join(assets_dir, "record_off.png"))

        self.set_idle_icon()
        self.setToolTip("Whisper Wayland")

        self.menu = QMenu()

        self.settings_action = QAction("⚙  Settings")
        self.menu.addAction(self.settings_action)

        if history_window is not None:
            self.history_action = QAction("📋  History")
            self.history_action.triggered.connect(history_window.show)
            self.menu.addAction(self.history_action)

        self.menu.addSeparator()
        self.quit_action = QAction("Quit")
        self.quit_action.triggered.connect(QApplication.instance().quit)
        self.menu.addAction(self.quit_action)

        self.setContextMenu(self.menu)

        self.app_state.recording_started.connect(self._on_recording_started)
        self.app_state.recording_stopped.connect(self.set_idle_icon)

    def _on_recording_started(self, target_id: str = 'default'):
        self._active_target_label = target_id
        self.set_recording_icon()
        tip = self.toolTip()
        self.setToolTip(f"{tip} • Recording → {target_id}")

    def set_idle_icon(self):
        self._active_target_label = ""
        if hasattr(self, 'icon_off') and not self.icon_off.isNull():
            self.setIcon(self.icon_off)
        else:
            pixmap = QPixmap(64, 64)
            pixmap.fill(QColor("grey"))
            self.setIcon(QIcon(pixmap))

    def set_recording_icon(self):
        if hasattr(self, 'icon_on') and not self.icon_on.isNull():
            self.setIcon(self.icon_on)
        else:
            pixmap = QPixmap(64, 64)
            pixmap.fill(QColor("red"))
            self.setIcon(QIcon(pixmap))
