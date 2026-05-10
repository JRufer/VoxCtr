"""Tests for AudioRecorder monitoring state machine.

Exercises start_monitoring / stop_monitoring / get_rms_level without
starting real threads or opening real audio hardware. pyaudio and numpy
are stubbed so the module can be imported in a headless test environment.
"""
import os
import sys
import types
import threading
import unittest.mock as mock

import pytest

# ── Stub heavy native deps before importing src ────────────────────────────────

def _stub(name, **attrs):
    m = types.ModuleType(name)
    for k, v in attrs.items():
        setattr(m, k, v)
    sys.modules[name] = m
    return m

# pyaudio stub (used at module level in audio_recorder.py)
_stub("pyaudio", paInt16=8, PyAudio=mock.MagicMock)

# numpy stub – only the functions used by AudioRecorder.run() matter;
# the methods under test don't call numpy directly.
# Save and restore the real numpy so other test modules aren't affected.
_real_numpy = sys.modules.get("numpy")
_np = _stub("numpy")
_np.frombuffer = mock.MagicMock(return_value=mock.MagicMock())
_np.sqrt = mock.MagicMock(return_value=0.1)
_np.mean = mock.MagicMock(return_value=0.01)

# noisereduce is optional; absence is handled gracefully.

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from audio_recorder import AudioRecorder

# Restore real numpy in sys.modules so other test files get the real module.
if _real_numpy is not None:
    sys.modules["numpy"] = _real_numpy
else:
    sys.modules.pop("numpy", None)


# ── Helpers ───────────────────────────────────────────────────────────────────

def _recorder(device_index=0):
    """Return a fresh AudioRecorder with a mock config and no real PyAudio."""
    cfg = mock.MagicMock()
    cfg.get.side_effect = lambda key, default=None: {
        "input_device_index": device_index,
        "mic_gain": 1.0,
        "quiet_mode": False,
        "noise_suppression": False,
    }.get(key, default)
    return AudioRecorder(cfg, mock.MagicMock())


# ── get_rms_level ──────────────────────────────────────────────────────────────

class TestGetRmsLevel:
    def test_returns_zero_before_any_audio(self):
        r = _recorder()
        assert r.get_rms_level() == 0.0

    def test_returns_float(self):
        r = _recorder()
        assert isinstance(r.get_rms_level(), float)


# ── start_monitoring ──────────────────────────────────────────────────────────

class TestStartMonitoring:
    def test_sets_monitoring_flag(self):
        r = _recorder()
        r.start_monitoring()
        assert r.monitoring is True

    def test_opens_stream_when_none_exists(self):
        r = _recorder()
        r._open_stream = mock.MagicMock()
        r.start_monitoring()
        r._open_stream.assert_called_once()

    def test_does_not_reopen_existing_stream(self):
        r = _recorder()
        r.stream = mock.MagicMock()  # pretend stream already open
        r._open_stream = mock.MagicMock()
        r.start_monitoring()
        r._open_stream.assert_not_called()

    def test_clears_pending_close_flag(self):
        r = _recorder()
        r._pending_close = True
        r.start_monitoring()
        assert r._pending_close is False


# ── stop_monitoring ───────────────────────────────────────────────────────────

class TestStopMonitoring:
    def test_clears_monitoring_flag(self):
        r = _recorder()
        r.monitoring = True
        r.stop_monitoring()
        assert r.monitoring is False

    def test_schedules_stream_close_when_not_recording(self):
        r = _recorder()
        r.monitoring = True
        r.recording = False
        r.stop_monitoring()
        assert r._pending_close is True

    def test_does_not_schedule_close_while_recording(self):
        r = _recorder()
        r.monitoring = True
        r.recording = True
        r._pending_close = False
        r.stop_monitoring()
        # Stream must stay open while actively recording.
        assert r._pending_close is False


# ── start_recording / stop_recording interaction ──────────────────────────────

class TestRecordingAndMonitoringInteraction:
    def test_stop_recording_schedules_close_when_not_monitoring(self):
        r = _recorder()
        r.recording = True
        r.monitoring = False
        r.stream = mock.MagicMock()
        r.stop_recording()
        assert r._pending_close is True

    def test_stop_recording_does_not_schedule_close_while_monitoring(self):
        r = _recorder()
        r.recording = True
        r.monitoring = True
        r._pending_close = False
        r.stream = mock.MagicMock()
        r.stop_recording()
        assert r._pending_close is False

    def test_start_recording_opens_stream_when_none(self):
        r = _recorder()
        r._open_stream = mock.MagicMock()
        r.start_recording()
        r._open_stream.assert_called_once()


# ── _close_stream ─────────────────────────────────────────────────────────────

class TestCloseStream:
    def test_closes_and_nones_stream(self):
        r = _recorder()
        stream = mock.MagicMock()
        r.stream = stream
        r._close_stream()
        stream.stop_stream.assert_called_once()
        stream.close.assert_called_once()
        assert r.stream is None

    def test_safe_when_stream_already_none(self):
        r = _recorder()
        r.stream = None
        r._close_stream()  # should not raise

    def test_tolerates_close_exception(self):
        r = _recorder()
        stream = mock.MagicMock()
        stream.stop_stream.side_effect = OSError("already closed")
        r.stream = stream
        r._close_stream()  # should not raise
        assert r.stream is None
