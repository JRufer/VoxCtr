DISPLAY_NAME = "Waveform"
DESCRIPTION  = "Classic oscilloscope-style audio waveform (OpenGL)"
VERSION      = "1.0"

import numpy as np
from PyQt6.QtWidgets import QWidget, QVBoxLayout
from PyQt6.QtOpenGLWidgets import QOpenGLWidget
from PyQt6.QtCore import Qt, QMetaObject, QRect
from PyQt6.QtGui import QColor, QSurfaceFormat, QPainter, QBrush, QPen
from OpenGL.GL import *
from OpenGL.GL import shaders

from gui.overlays.base import OverlayUIBase


class _WaveformGLWidget(QOpenGLWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.audio_data = np.zeros(1024, dtype=np.float32)
        self.program = None

        fmt = QSurfaceFormat()
        fmt.setRenderableType(QSurfaceFormat.RenderableType.OpenGL)
        fmt.setProfile(QSurfaceFormat.OpenGLContextProfile.CoreProfile)
        fmt.setVersion(3, 3)
        self.setFormat(fmt)

    def update_audio(self, data):
        self.audio_data = data
        QMetaObject.invokeMethod(self, "update", Qt.ConnectionType.QueuedConnection)

    def initializeGL(self):
        glClearColor(0.0, 0.0, 0.0, 0.0)

        VERTEX_SHADER = """
        #version 330 core
        layout(location = 0) in float amplitude;
        uniform int totalSamples;
        void main() {
            int x_index = gl_VertexID / 2;
            int num_lines = totalSamples / 2;
            float x = (float(x_index) / float(max(1, num_lines - 1))) * 2.0 - 1.0;
            float y = (amplitude / 8192.0) * 0.9;
            gl_Position = vec4(x, y, 0.0, 1.0);
        }
        """

        FRAGMENT_SHADER = """
        #version 330 core
        out vec4 FragColor;
        void main() {
            FragColor = vec4(0.8, 0.8, 0.8, 1.0);
        }
        """

        try:
            vs = shaders.compileShader(VERTEX_SHADER, GL_VERTEX_SHADER)
            fs = shaders.compileShader(FRAGMENT_SHADER, GL_FRAGMENT_SHADER)
            self.program = shaders.compileProgram(vs, fs)
            self._vao = glGenVertexArrays(1)
            self._vbo = glGenBuffers(1)
        except Exception:
            pass

    def paintGL(self):
        if self.program is None:
            return
        try:
            glClear(GL_COLOR_BUFFER_BIT)
            glUseProgram(self.program)

            width = self.width()
            if width > 0 and len(self.audio_data) > 0:
                samples_per_pixel = max(1, len(self.audio_data) // width)
                valid_len = (len(self.audio_data) // samples_per_pixel) * samples_per_pixel
                if valid_len > 0:
                    reshaped = self.audio_data[:valid_len].reshape(-1, samples_per_pixel)
                    mins = np.min(reshaped, axis=1)[:width]
                    maxs = np.max(reshaped, axis=1)[:width]
                    actual_width = len(mins)

                    plot_data = np.empty(actual_width * 2, dtype=np.float32)
                    plot_data[0::2] = mins
                    plot_data[1::2] = maxs

                    loc = glGetUniformLocation(self.program, "totalSamples")
                    glUniform1i(loc, actual_width * 2)

                    glBindVertexArray(self._vao)
                    glBindBuffer(GL_ARRAY_BUFFER, self._vbo)
                    glBufferData(GL_ARRAY_BUFFER, plot_data.nbytes, plot_data, GL_STREAM_DRAW)
                    glEnableVertexAttribArray(0)
                    glVertexAttribPointer(0, 1, GL_FLOAT, GL_FALSE, 0, None)
                    glDrawArrays(GL_LINES, 0, actual_width * 2)
                    glDisableVertexAttribArray(0)
                    glBindVertexArray(0)
                    glBindBuffer(GL_ARRAY_BUFFER, 0)
        except Exception:
            pass

    def resizeGL(self, w, h):
        glViewport(0, 0, w, h)


class OverlayUI(OverlayUIBase):
    DISPLAY_NAME = DISPLAY_NAME
    DESCRIPTION  = DESCRIPTION

    def __init__(self):
        super().__init__()
        self.setFixedSize(800, 100)
        self.setWindowFlags(
            Qt.WindowType.ToolTip |
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.X11BypassWindowManagerHint |
            Qt.WindowType.WindowTransparentForInput
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setStyleSheet("border: 1px solid transparent;")

        self.gl_widget = _WaveformGLWidget()
        self.gl_widget.setFixedSize(70, 70)
        self.gl_widget.setAttribute(Qt.WidgetAttribute.WA_AlwaysStackOnTop)

        layout = QVBoxLayout()
        layout.setContentsMargins(0, 0, 0, 100)
        layout.setAlignment(Qt.AlignmentFlag.AlignBottom | Qt.AlignmentFlag.AlignHCenter)
        layout.addWidget(self.gl_widget)
        self.setLayout(layout)

        self.hide()

    def update_audio(self, data):
        try:
            if self.gl_widget:
                self.gl_widget.update_audio(data)
        except (RuntimeError, AttributeError):
            pass

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        box_width, box_height = 70, 70
        x = (self.width() - box_width) // 2
        y = self.height() - box_height - 100
        box_rect = QRect(x, y, box_width, box_height).adjusted(1, 1, -1, -1)
        painter.setBrush(QBrush(QColor(0, 0, 0, 230)))
        painter.setPen(QPen(QColor(68, 68, 68), 1))
        painter.drawRoundedRect(box_rect, 15, 15)

    def show_mode(self):
        from PyQt6.QtWidgets import QApplication
        screen = QApplication.primaryScreen()
        if screen:
            geom = screen.geometry()
            self.setGeometry(geom)
            self.setFixedSize(geom.width(), geom.height())
            self.move(geom.x(), geom.y())
            self.show()
            self.raise_()
        else:
            self.show()

    def hide_mode(self):
        self.hide()
