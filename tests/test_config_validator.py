"""Tests for config_validator.py — startup configuration validation."""

import sys
import os
import pytest
from unittest.mock import MagicMock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from config_validator import (
    validate_global_config,
    validate_targets,
    validate_bindings,
    validate_all,
    ConfigValidationError,
)
from routing.models import (
    DeliveryType, GestureType, HotkeyBinding, OutputTarget, TargetProcessingConfig,
)


# ── Helpers ───────────────────────────────────────────────────────────────────

def _make_config(overrides: dict = {}):
    cfg = MagicMock()
    defaults = {
        "model_size": "base",
        "device": "auto",
        "inference_mode": "Balanced",
        "backend_engine": "auto",
        "dictation_mode": "normal",
        "ollama_enabled": False,
        "tts_enabled": False,
        "snippets": {},
        "custom_vocabulary": [],
        "vad_threshold": 0.5,
        "mic_gain": 1.0,
    }
    defaults.update(overrides)
    cfg.config = defaults
    return cfg


def _make_target(id="default", delivery="inject", **kwargs):
    t = MagicMock(spec=OutputTarget)
    t.id = id
    t.delivery = MagicMock()
    t.delivery.value = delivery
    t.command = kwargs.get("command", "")
    t.pipe_path = kwargs.get("pipe_path", "")
    t.socket_host = kwargs.get("socket_host", "")
    t.socket_unix = kwargs.get("socket_unix", "")
    t.file_path = kwargs.get("file_path", "")
    t.dbus_signal = kwargs.get("dbus_signal", "")
    t.response_pipe = kwargs.get("response_pipe", "")
    t.label = kwargs.get("label", "")
    p = MagicMock()
    p.ollama_mode = None
    t.processing = p
    return t


def _make_binding(id="b1", keys=["KEY_RIGHTCTRL"], gesture="hold", target_id="default",
                  tap_ms=300, hold_threshold_ms=200, disabled=False):
    b = MagicMock(spec=HotkeyBinding)
    b.id = id
    b.keys = keys
    b.gesture = MagicMock()
    b.gesture.value = gesture
    b.target_id = target_id
    b.tap_ms = tap_ms
    b.hold_threshold_ms = hold_threshold_ms
    b.disabled = disabled
    return b


# ── validate_global_config ────────────────────────────────────────────────────

class TestValidateGlobalConfig:
    def test_valid_defaults(self):
        errors, warnings = validate_global_config(_make_config())
        assert errors == []
        assert warnings == []

    def test_invalid_model_size(self):
        errors, warnings = validate_global_config(_make_config({"model_size": "giga"}))
        # model_size invalid is a warning (fatal=False)
        assert any("model_size" in w for w in warnings)

    def test_invalid_device_is_fatal(self):
        errors, warnings = validate_global_config(_make_config({"device": "tpu"}))
        assert any("device" in e for e in errors)

    def test_valid_devices(self):
        for dev in ("auto", "cuda", "cpu"):
            errors, _ = validate_global_config(_make_config({"device": dev}))
            assert not any("device" in e for e in errors)

    def test_invalid_backend_is_fatal(self):
        errors, _ = validate_global_config(_make_config({"backend_engine": "magic"}))
        assert any("backend_engine" in e for e in errors)

    def test_ollama_mode_validated_when_enabled(self):
        _, warnings = validate_global_config(_make_config({
            "ollama_enabled": True,
            "ollama_mode": "INVALID",
        }))
        assert any("ollama_mode" in w for w in warnings)

    def test_ollama_mode_not_checked_when_disabled(self):
        errors, warnings = validate_global_config(_make_config({
            "ollama_enabled": False,
            "ollama_mode": "INVALID",
        }))
        assert not any("ollama_mode" in w for w in warnings)

    def test_tts_engine_validated_when_enabled(self):
        errors, _ = validate_global_config(_make_config({
            "tts_enabled": True,
            "tts_engine": "bad-engine",
        }))
        assert any("tts_engine" in e for e in errors)

    def test_vad_threshold_out_of_range(self):
        _, warnings = validate_global_config(_make_config({"vad_threshold": 1.5}))
        assert any("vad_threshold" in w for w in warnings)

    def test_mic_gain_out_of_range(self):
        _, warnings = validate_global_config(_make_config({"mic_gain": 0.1}))
        assert any("mic_gain" in w for w in warnings)

    def test_snippets_non_dict_is_error(self):
        errors, _ = validate_global_config(_make_config({"snippets": "oops"}))
        assert any("snippets" in e for e in errors)


# ── validate_targets ──────────────────────────────────────────────────────────

class TestValidateTargets:
    def test_empty_targets_warns(self):
        _, warnings = validate_targets([])
        assert any("No targets" in w for w in warnings)

    def test_valid_inject_target(self):
        errors, warnings = validate_targets([_make_target()])
        assert errors == []

    def test_duplicate_target_ids(self):
        t1 = _make_target(id="dup")
        t2 = _make_target(id="dup")
        errors, _ = validate_targets([t1, t2])
        assert any("duplicate" in e for e in errors)

    def test_empty_id_is_error(self):
        t = _make_target(id="   ")
        errors, _ = validate_targets([t])
        # strip() makes it empty → error
        assert any("must not be empty" in e for e in errors)

    def test_exec_requires_command(self):
        t = _make_target(id="e1", delivery="exec", command="")
        errors, _ = validate_targets([t])
        assert any("exec" in e and "command" in e for e in errors)

    def test_exec_valid_with_command(self):
        t = _make_target(id="e1", delivery="exec", command="notify-send hello")
        errors, _ = validate_targets([t])
        assert not any("exec" in e for e in errors)

    def test_pipe_requires_pipe_path(self):
        t = _make_target(id="p1", delivery="pipe", pipe_path="")
        errors, _ = validate_targets([t])
        assert any("pipe_path" in e for e in errors)

    def test_file_requires_file_path(self):
        t = _make_target(id="f1", delivery="file", file_path="")
        errors, _ = validate_targets([t])
        assert any("file_path" in e for e in errors)

    def test_dbus_requires_signal(self):
        t = _make_target(id="d1", delivery="dbus", dbus_signal="")
        errors, _ = validate_targets([t])
        assert any("dbus_signal" in e for e in errors)

    def test_socket_requires_host_or_unix(self):
        t = _make_target(id="s1", delivery="socket", socket_host="", socket_unix="")
        errors, _ = validate_targets([t])
        assert any("socket" in e for e in errors)

    def test_socket_valid_with_host(self):
        t = _make_target(id="s1", delivery="socket", socket_host="localhost:9999")
        errors, _ = validate_targets([t])
        assert not any("socket" in e for e in errors)

    def test_unknown_delivery_type(self):
        t = _make_target(id="x1", delivery="teleport")
        errors, _ = validate_targets([t])
        assert any("unknown delivery" in e for e in errors)

    def test_invalid_ollama_mode_in_processing_warns(self):
        t = _make_target(id="o1")
        t.processing.ollama_mode = "BADMODE"
        _, warnings = validate_targets([t])
        assert any("ollama_mode" in w for w in warnings)


# ── validate_bindings ─────────────────────────────────────────────────────────

class TestValidateBindings:
    def test_valid_binding(self):
        b = _make_binding()
        errors, warnings = validate_bindings([b], {"default"})
        assert errors == []

    def test_unknown_target_id(self):
        b = _make_binding(target_id="nonexistent")
        errors, _ = validate_bindings([b], {"default"})
        assert any("unknown target" in e for e in errors)

    def test_duplicate_binding_ids(self):
        b1 = _make_binding(id="dup")
        b2 = _make_binding(id="dup")
        errors, _ = validate_bindings([b1, b2], {"default"})
        assert any("duplicate" in e for e in errors)

    def test_empty_keys_is_error(self):
        b = _make_binding(keys=[])
        errors, _ = validate_bindings([b], {"default"})
        assert any("keys" in e for e in errors)

    def test_invalid_gesture_is_error(self):
        b = _make_binding(gesture="wave")
        errors, _ = validate_bindings([b], {"default"})
        assert any("gesture" in e for e in errors)

    def test_tap_ms_out_of_range_warns(self):
        b = _make_binding(tap_ms=5)
        _, warnings = validate_bindings([b], {"default"})
        assert any("tap_ms" in w for w in warnings)

    def test_hold_threshold_out_of_range_warns(self):
        b = _make_binding(hold_threshold_ms=5000)
        _, warnings = validate_bindings([b], {"default"})
        assert any("hold_threshold_ms" in w for w in warnings)

    def test_all_gesture_types_valid(self):
        target_ids = {"default"}
        for gesture in ("hold", "toggle", "double_tap", "chord"):
            b = _make_binding(id=f"b-{gesture}", gesture=gesture)
            errors, _ = validate_bindings([b], target_ids)
            assert not any("gesture" in e for e in errors), f"gesture {gesture!r} was rejected"


# ── validate_all ──────────────────────────────────────────────────────────────

class TestValidateAll:
    def test_all_valid_returns_true(self):
        cfg = _make_config()
        t = _make_target()
        b = _make_binding()
        result = validate_all(cfg, [t], [b])
        assert result is True

    def test_fatal_error_raises(self):
        cfg = _make_config({"device": "bad"})
        t = _make_target()
        b = _make_binding()
        with pytest.raises(ConfigValidationError):
            validate_all(cfg, [t], [b])

    def test_fatal_error_returns_false_when_not_fatal(self):
        cfg = _make_config({"device": "bad"})
        t = _make_target()
        b = _make_binding()
        result = validate_all(cfg, [t], [b], fatal_on_error=False)
        assert result is False

    def test_empty_targets_does_not_raise(self):
        cfg = _make_config()
        result = validate_all(cfg, [], [], fatal_on_error=True)
        assert result is True
