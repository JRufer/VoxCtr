from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class GestureType(str, Enum):
    HOLD       = "hold"
    TOGGLE     = "toggle"
    DOUBLE_TAP = "double_tap"
    CHORD      = "chord"


@dataclass
class HotkeyBinding:
    id: str
    keys: list
    gesture: GestureType
    target_id: str
    tap_ms: int = 250
    hold_threshold_ms: int = 200
    label: str = ""
    disabled: bool = False


class DeliveryType(str, Enum):
    INJECT    = "inject"
    CLIPBOARD = "clipboard"
    EXEC      = "exec"
    PIPE      = "pipe"
    SOCKET    = "socket"
    FILE      = "file"
    DBUS      = "dbus"


# ── Optional features that have global on/off master switches ─────────────────
# If the global switch is off, the feature is disabled regardless of target config.
GLOBALLY_GATED_FEATURES = {
    "ollama":            "ollama_enabled",
    "noise_suppression": "noise_suppression",
    "tts":               "tts_enabled",
}

# Human-readable names for globally-gated features (used in UI warnings)
FEATURE_LABELS = {
    "ollama":            "Ollama LLM",
    "noise_suppression": "Noise Suppression",
    "tts":               "TTS / Voice Out",
}


@dataclass
class TargetProcessingConfig:
    """Per-target overrides for preprocessing and postprocessing steps.

    None means "inherit the global config setting".
    Explicitly setting True/False overrides the global default for that feature.

    For globally-gated optional features (Ollama, noise suppression):
    the global master switch is still respected as an absolute OFF.
    A target requesting Ollama while the global toggle is off will be visually
    flagged in the UI, but Ollama will not run.
    """

    # ── Preprocessing ──────────────────────────────────────────────────────
    noise_suppression: Optional[bool] = None   # None = use global toggle
    quiet_mode: Optional[bool] = None          # None = use global toggle
    atspi_context: Optional[bool] = None       # feed surrounding text to Whisper

    # ── Postprocessing ─────────────────────────────────────────────────────
    remove_fillers: Optional[bool] = None      # remove uh/um/hmm
    spoken_punctuation: Optional[bool] = None  # "period" → "."
    auto_format_lists: Optional[bool] = None   # format numbered lists
    apply_snippets: Optional[bool] = None      # expand snippet triggers
    code_mode: Optional[bool] = None           # force code dictation mode

    # ── Ollama / LLM ──────────────────────────────────────────────────────
    ollama_enabled: Optional[bool] = None      # None = use global toggle
    ollama_model: Optional[str] = None         # None = use global model
    ollama_mode: Optional[str] = None          # None = use global mode
    ollama_prompt: Optional[str] = None        # custom prompt (overrides mode)

    # ── Helpers ────────────────────────────────────────────────────────────

    def to_dict(self) -> dict:
        """Serialize to dict, omitting None values (no override = not stored)."""
        keys = [
            "noise_suppression", "quiet_mode", "atspi_context",
            "remove_fillers", "spoken_punctuation", "auto_format_lists",
            "apply_snippets", "code_mode",
            "ollama_enabled", "ollama_model", "ollama_mode", "ollama_prompt",
        ]
        return {k: getattr(self, k) for k in keys if getattr(self, k) is not None}

    @classmethod
    def from_dict(cls, d: dict) -> "TargetProcessingConfig":
        return cls(
            noise_suppression=d.get("noise_suppression"),
            quiet_mode=d.get("quiet_mode"),
            atspi_context=d.get("atspi_context"),
            remove_fillers=d.get("remove_fillers"),
            spoken_punctuation=d.get("spoken_punctuation"),
            auto_format_lists=d.get("auto_format_lists"),
            apply_snippets=d.get("apply_snippets"),
            code_mode=d.get("code_mode"),
            ollama_enabled=d.get("ollama_enabled"),
            ollama_model=d.get("ollama_model"),
            ollama_mode=d.get("ollama_mode"),
            ollama_prompt=d.get("ollama_prompt"),
        )

    def has_any(self) -> bool:
        """True if at least one override is set."""
        return bool(self.to_dict())

    def get_feature_warnings(self, global_config) -> list:
        """Return list of (feature_key, label) tuples for features that this
        target enables but are globally disabled — used for UI warning badges."""
        warnings = []
        # Ollama: target requests it but global switch is off
        effective_ollama = (
            self.ollama_enabled
            if self.ollama_enabled is not None
            else global_config.get("ollama_enabled", False)
        )
        if effective_ollama and not global_config.get("ollama_enabled", False):
            warnings.append(("ollama", FEATURE_LABELS["ollama"]))

        # Noise suppression
        effective_ns = (
            self.noise_suppression
            if self.noise_suppression is not None
            else global_config.get("noise_suppression", False)
        )
        if effective_ns and not global_config.get("noise_suppression", False):
            warnings.append(("noise_suppression", FEATURE_LABELS["noise_suppression"]))

        # TTS loopback (checked by caller via response_pipe on OutputTarget)
        return warnings


@dataclass
class OutputTarget:
    id: str
    label: str
    delivery: DeliveryType
    command: Optional[str] = None
    pipe_path: Optional[str] = None
    socket_host: Optional[str] = None
    socket_port: Optional[int] = None
    socket_unix: Optional[str] = None
    file_path: Optional[str] = None
    file_prefix: str = ""
    file_timestamp: bool = True
    dbus_signal: Optional[str] = None
    # Legacy postprocessing mode string (used if processing.has_any() is False)
    post_processing: str = "default"
    send_on_release: bool = True
    append_newline: bool = True
    initial_prompt: Optional[str] = None
    # Per-target processing pipeline configuration (overrides global defaults)
    processing: TargetProcessingConfig = field(default_factory=TargetProcessingConfig)
    # TTS response loopback
    response_pipe: Optional[str] = None   # FIFO path the AI writes responses to
    tts_engine: str = "piper"             # per-target TTS engine override
    tts_voice: Optional[str] = None       # per-target voice (None = use global)


@dataclass
class DeliveryResult:
    success: bool
    error: Optional[str] = None
    delivered_text: Optional[str] = None


@dataclass
class TestResult:
    reachable: bool
    detail: str
