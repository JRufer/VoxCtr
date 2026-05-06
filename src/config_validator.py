"""
Startup configuration validator for Whisper-Wayland.

Validates config.json, targets.toml, and bindings.toml on startup.
Logs problems clearly and raises ConfigValidationError if any fatal
issue is found so the application can exit cleanly rather than crashing
at an unpredictable point.

Usage (in main.py after loading config and routing files):

    from config_validator import validate_all, ConfigValidationError
    try:
        validate_all(config, targets, bindings)
    except ConfigValidationError as e:
        print(e)
        sys.exit(1)
"""

import os
import sys
from pathlib import Path
from typing import Optional


class ConfigValidationError(Exception):
    """Raised when the configuration contains a fatal error."""


# ── Allowed value sets ────────────────────────────────────────────────────────

_VALID_MODEL_SIZES = {
    "tiny", "tiny.en", "base", "base.en", "small", "small.en",
    "medium", "medium.en", "large-v1", "large-v2", "large-v3",
}
_VALID_DEVICES = {"auto", "cuda", "cpu"}
_VALID_COMPUTE_TYPES = {"default", "float16", "float32", "int8", "int8_float16", "int8_float32"}
_VALID_INFERENCE_MODES = {"Balanced", "Aggressive"}
_VALID_BACKENDS = {"auto", "faster-whisper", "whisper-cpp"}
_VALID_DICTATION_MODES = {"normal", "code"}
_VALID_OVERLAY_STYLES_BUILTIN = {"waveform", "pulse", "voice_card"}
_VALID_TTS_ENGINES = {"piper", "espeak"}
_VALID_OLLAMA_MODES = {"off", "clean", "formal", "casual", "bullet", "concise"}
_VALID_DELIVERY_TYPES = {"inject", "clipboard", "exec", "pipe", "socket", "file", "dbus"}
_VALID_GESTURE_TYPES = {"hold", "toggle", "double_tap", "chord"}
_VALID_PP_LEGACY = {"default", "none", "strip_fillers", "ollama_only", "snippets_only"}


# ── Helpers ───────────────────────────────────────────────────────────────────

def _check(condition: bool, msg: str, fatal: bool, errors: list, warnings: list) -> None:
    if not condition:
        if fatal:
            errors.append(msg)
        else:
            warnings.append(msg)


def _is_str(val, name: str, errors: list) -> bool:
    if not isinstance(val, str):
        errors.append(f"  {name}: expected string, got {type(val).__name__!r}")
        return False
    return True


def _is_bool(val, name: str, errors: list) -> bool:
    if not isinstance(val, bool):
        errors.append(f"  {name}: expected bool, got {type(val).__name__!r}")
        return False
    return True


def _is_int_or_none(val, name: str, errors: list) -> bool:
    if val is not None and not isinstance(val, int):
        errors.append(f"  {name}: expected int or null, got {type(val).__name__!r}")
        return False
    return True


# ── Validators ────────────────────────────────────────────────────────────────

def validate_global_config(config) -> tuple[list, list]:
    """Validate config.json settings.

    Returns (errors, warnings).  Errors are fatal; warnings are advisory.
    """
    errors: list = []
    warnings: list = []
    cfg = config.config  # raw dict

    # model_size
    ms = cfg.get("model_size", "base")
    _check(_is_str(ms, "model_size", errors) and ms in _VALID_MODEL_SIZES,
           f"  model_size: {ms!r} is not a valid Whisper model size. "
           f"Valid: {sorted(_VALID_MODEL_SIZES)}", False, errors, warnings)

    # device
    device = cfg.get("device", "auto")
    _check(_is_str(device, "device", errors) and device in _VALID_DEVICES,
           f"  device: {device!r} is not valid. Valid: {sorted(_VALID_DEVICES)}",
           True, errors, warnings)

    # inference_mode
    mode = cfg.get("inference_mode", "Balanced")
    _check(isinstance(mode, str) and mode in _VALID_INFERENCE_MODES,
           f"  inference_mode: {mode!r} is not valid. Valid: {sorted(_VALID_INFERENCE_MODES)}",
           False, errors, warnings)

    # backend_engine
    backend = cfg.get("backend_engine", "auto")
    _check(isinstance(backend, str) and backend in _VALID_BACKENDS,
           f"  backend_engine: {backend!r} is not valid. Valid: {sorted(_VALID_BACKENDS)}",
           True, errors, warnings)

    # dictation_mode
    dm = cfg.get("dictation_mode", "normal")
    _check(isinstance(dm, str) and dm in _VALID_DICTATION_MODES,
           f"  dictation_mode: {dm!r} is not valid. Valid: {sorted(_VALID_DICTATION_MODES)}",
           False, errors, warnings)

    # Ollama
    if cfg.get("ollama_enabled", False):
        om = cfg.get("ollama_mode", "clean")
        _check(isinstance(om, str) and om in _VALID_OLLAMA_MODES,
               f"  ollama_mode: {om!r} is not valid. Valid: {sorted(_VALID_OLLAMA_MODES)}",
               False, errors, warnings)

    # TTS
    if cfg.get("tts_enabled", False):
        tts_eng = cfg.get("tts_engine", "piper")
        _check(isinstance(tts_eng, str) and tts_eng in _VALID_TTS_ENGINES,
               f"  tts_engine: {tts_eng!r} is not valid. Valid: {sorted(_VALID_TTS_ENGINES)}",
               True, errors, warnings)

    # snippets: must be dict of str→str
    snippets = cfg.get("snippets", {})
    if not isinstance(snippets, dict):
        errors.append(f"  snippets: expected dict, got {type(snippets).__name__!r}")
    else:
        for k, v in snippets.items():
            if not isinstance(k, str) or not isinstance(v, str):
                warnings.append(f"  snippets: key {k!r} or value {v!r} is not a string")

    # custom_vocabulary: must be list of str
    vocab = cfg.get("custom_vocabulary", [])
    if not isinstance(vocab, list):
        warnings.append(f"  custom_vocabulary: expected list, got {type(vocab).__name__!r}")

    # numeric ranges
    vad_threshold = cfg.get("vad_threshold", 0.5)
    if not isinstance(vad_threshold, (int, float)) or not (0.0 <= vad_threshold <= 1.0):
        warnings.append(f"  vad_threshold: {vad_threshold!r} should be a float between 0 and 1")

    gain = cfg.get("mic_gain", 1.0)
    if isinstance(gain, (int, float)) and not (0.5 <= gain <= 10.0):
        warnings.append(f"  mic_gain: {gain!r} is outside the expected range [0.5, 10.0]")

    return errors, warnings


def validate_targets(targets: list) -> tuple[list, list]:
    """Validate output targets from targets.toml."""
    errors: list = []
    warnings: list = []
    ids_seen: set = set()

    if not targets:
        warnings.append("  No targets defined. A 'default' inject target is assumed.")
        return errors, warnings

    for i, t in enumerate(targets):
        prefix = f"  target[{i}] id={t.id!r}"

        # Unique IDs
        if t.id in ids_seen:
            errors.append(f"{prefix}: duplicate target id {t.id!r}")
        ids_seen.add(t.id)

        if not t.id.strip():
            errors.append(f"{prefix}: id must not be empty")

        # Delivery-specific required fields
        if t.delivery.value not in _VALID_DELIVERY_TYPES:
            errors.append(f"{prefix}: unknown delivery type {t.delivery.value!r}")
        elif t.delivery.value == "exec" and not t.command:
            errors.append(f"{prefix}: 'exec' delivery requires a command string")
        elif t.delivery.value == "pipe" and not t.pipe_path:
            errors.append(f"{prefix}: 'pipe' delivery requires pipe_path")
        elif t.delivery.value == "socket" and not (t.socket_host or t.socket_unix):
            errors.append(f"{prefix}: 'socket' delivery requires socket_host or socket_unix")
        elif t.delivery.value == "file" and not t.file_path:
            errors.append(f"{prefix}: 'file' delivery requires file_path")
        elif t.delivery.value == "dbus" and not t.dbus_signal:
            errors.append(f"{prefix}: 'dbus' delivery requires dbus_signal")

        # Processing config validation
        p = t.processing
        if p.ollama_mode is not None and p.ollama_mode not in _VALID_OLLAMA_MODES:
            warnings.append(f"{prefix}: processing.ollama_mode {p.ollama_mode!r} is not a known mode. "
                            f"Valid: {sorted(_VALID_OLLAMA_MODES)}")

        # TTS loopback: warn if response_pipe set but file doesn't exist/is not a FIFO
        if t.response_pipe:
            rp = Path(os.path.expanduser(t.response_pipe))
            if rp.exists() and not rp.is_fifo():
                warnings.append(f"{prefix}: response_pipe {t.response_pipe!r} exists but is not a FIFO")

    return errors, warnings


def validate_bindings(bindings: list, target_ids: set) -> tuple[list, list]:
    """Validate hotkey bindings from bindings.toml."""
    errors: list = []
    warnings: list = []
    ids_seen: set = set()

    for i, b in enumerate(bindings):
        prefix = f"  binding[{i}] id={b.id!r}"

        if b.id in ids_seen:
            errors.append(f"{prefix}: duplicate binding id {b.id!r}")
        ids_seen.add(b.id)

        if not b.keys:
            errors.append(f"{prefix}: keys list must not be empty")

        if b.gesture.value not in _VALID_GESTURE_TYPES:
            errors.append(f"{prefix}: unknown gesture {b.gesture.value!r}")

        if b.target_id not in target_ids:
            errors.append(f"{prefix}: references unknown target {b.target_id!r}. "
                          f"Available: {sorted(target_ids)}")

        if b.tap_ms < 50 or b.tap_ms > 2000:
            warnings.append(f"{prefix}: tap_ms={b.tap_ms} is outside recommended range [50, 2000]")

        if b.hold_threshold_ms < 10 or b.hold_threshold_ms > 1000:
            warnings.append(f"{prefix}: hold_threshold_ms={b.hold_threshold_ms} "
                            "is outside recommended range [10, 1000]")

    return errors, warnings


def validate_all(config, targets: list, bindings: list,
                 fatal_on_error: bool = True) -> bool:
    """Run all validators. Prints a summary of issues.

    Returns True if all checks pass, False if there are warnings only.
    Raises ConfigValidationError if there are fatal errors and fatal_on_error=True.
    """
    all_errors: list = []
    all_warnings: list = []

    # Global config
    e, w = validate_global_config(config)
    if e:
        all_errors.append("[config.json — ERRORS]")
        all_errors.extend(e)
    if w:
        all_warnings.append("[config.json — warnings]")
        all_warnings.extend(w)

    # Targets
    e, w = validate_targets(targets)
    if e:
        all_errors.append("[targets.toml — ERRORS]")
        all_errors.extend(e)
    if w:
        all_warnings.append("[targets.toml — warnings]")
        all_warnings.extend(w)

    # Bindings
    target_ids = {t.id for t in targets}
    e, w = validate_bindings(bindings, target_ids)
    if e:
        all_errors.append("[bindings.toml — ERRORS]")
        all_errors.extend(e)
    if w:
        all_warnings.append("[bindings.toml — warnings]")
        all_warnings.extend(w)

    if all_warnings:
        print("\n[Config] Validation warnings:")
        for msg in all_warnings:
            print(f"  {msg}")

    if all_errors:
        msg = (
            "\n[Config] FATAL configuration errors — fix these before starting:\n"
            + "\n".join(all_errors)
        )
        if fatal_on_error:
            raise ConfigValidationError(msg)
        else:
            print(msg)
            return False

    if not all_warnings:
        print("[Config] All configuration checks passed.")

    return True
