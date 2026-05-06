"""
P2.1 — Local LLM post-processing via Ollama

Design contract:
  - If Ollama is not installed / not running → process() returns text unchanged, no exception.
  - If Ollama is available but disabled in config → pass-through immediately.
  - All network calls are synchronous but capped by a short timeout so they
    never stall the injection pipeline for more than a couple of seconds.
  - Zero third-party dependencies: uses only stdlib urllib.
  - Per-target overrides: process_with_target() accepts an effective-processing
    dict from the InferenceEngine and uses target-specific model/mode/prompt.
"""

import json
import urllib.request
import urllib.error
from typing import Optional

# ── Ollama API constants ──────────────────────────────────────────────────
_BASE_URL = "http://localhost:11434"
_GENERATE_URL = f"{_BASE_URL}/api/generate"
_TAGS_URL = f"{_BASE_URL}/api/tags"
_PROBE_TIMEOUT = 2.0   # seconds — availability check
_GENERATE_TIMEOUT = 8.0  # seconds — generation call

# ── Mode prompts ──────────────────────────────────────────────────────────
_PROMPTS = {
    "clean": (
        "Fix any grammar or punctuation errors in the following text. "
        "Output ONLY the corrected text with no explanation, no quotes, "
        "no preamble:\n\n{text}"
    ),
    "formal": (
        "Rewrite the following text in a professional, formal tone. "
        "Output ONLY the rewritten text with no explanation:\n\n{text}"
    ),
    "casual": (
        "Rewrite the following text in a friendly, casual conversational tone. "
        "Output ONLY the rewritten text:\n\n{text}"
    ),
    "bullet": (
        "Convert the following text into a concise bullet-point list. "
        "Use • as the bullet character. Output ONLY the bullet points:\n\n{text}"
    ),
    "concise": (
        "Make the following text more concise without losing meaning. "
        "Output ONLY the shortened text:\n\n{text}"
    ),
}

# Human-readable labels for the settings UI
MODE_LABELS = {
    "off":     "Off (Whisper output only)",
    "clean":   "Fix grammar & punctuation",
    "formal":  "Rewrite — formal tone",
    "casual":  "Rewrite — casual tone",
    "bullet":  "Convert to bullet points",
    "concise": "Make concise",
}


class LLMPostprocessor:
    """
    Wraps Ollama for optional post-processing of Whisper transcriptions.

    Thread-safety: process() is called from the InferenceEngine thread.
    Availability state is read-only after __init__ so no locking needed.
    """

    def __init__(self, config):
        self.config = config
        self._available: Optional[bool] = None   # None = not yet probed
        self._available_models: list[str] = []

    # ── Public API ────────────────────────────────────────────────────────

    def probe(self) -> bool:
        """
        Check whether Ollama is reachable. Safe to call from any thread.
        Caches the result so subsequent calls are instant.
        Returns True if Ollama responded, False otherwise.
        """
        if self._available is not None:
            return self._available
        try:
            req = urllib.request.Request(_TAGS_URL, method="GET")
            with urllib.request.urlopen(req, timeout=_PROBE_TIMEOUT) as resp:
                body = json.loads(resp.read().decode())
                self._available_models = [
                    m.get("name", "") for m in body.get("models", [])
                ]
            self._available = True
        except Exception:
            self._available = False
        return self._available

    def refresh_probe(self) -> bool:
        """Force a fresh availability check (e.g. after user clicks 'Retry')."""
        self._available = None
        return self.probe()

    @property
    def available(self) -> bool:
        """True if Ollama was reachable on last probe."""
        return bool(self._available)

    @property
    def available_models(self) -> list[str]:
        return list(self._available_models)

    def process(self, text: str) -> str:
        """Apply LLM post-processing using global config settings.

        Guaranteed to return a string. If anything goes wrong the original
        text is returned.
        """
        if not text.strip():
            return text
        if not self.config.get("ollama_enabled", False):
            return text

        mode = self.config.get("ollama_mode", "clean")
        model = self.config.get("ollama_model", "llama3.2:1b")
        return self._process_internal(text, model=model, mode=mode, custom_prompt=None)

    def process_with_target(self, text: str, effective: dict) -> str:
        """Apply LLM post-processing using a resolved effective-processing dict.

        This is the preferred entry point from InferenceEngine — it uses
        per-target overrides for model, mode, and custom prompt.

        Args:
            text: Transcribed and locally post-processed text.
            effective: Dict from InferenceEngine._build_effective_processing(),
                       containing ollama_enabled, ollama_model, ollama_mode,
                       ollama_prompt.
        """
        if not text.strip():
            return text
        if not effective.get("ollama_enabled", False):
            return text

        model  = effective.get("ollama_model", self.config.get("ollama_model", "llama3.2:1b"))
        mode   = effective.get("ollama_mode",  self.config.get("ollama_mode", "clean"))
        custom = effective.get("ollama_prompt")  # None → use mode's built-in prompt

        return self._process_internal(text, model=model, mode=mode, custom_prompt=custom)

    # ── Internal ──────────────────────────────────────────────────────────

    def _process_internal(self, text: str, model: str, mode: str,
                          custom_prompt: Optional[str]) -> str:
        """Core processing logic shared by process() and process_with_target()."""
        if mode == "off" and not custom_prompt:
            return text

        # Build the prompt
        if custom_prompt:
            # Custom prompt: treat as a template; {text} is replaced if present,
            # otherwise the transcribed text is appended after a blank line.
            if "{text}" in custom_prompt:
                prompt = custom_prompt.replace("{text}", text)
            else:
                prompt = f"{custom_prompt}\n\n{text}"
        else:
            prompt_template = _PROMPTS.get(mode)
            if not prompt_template:
                return text
            prompt = prompt_template.format(text=text)

        # Lazy probe
        if self._available is None:
            self.probe()
        if not self._available:
            return text

        result = self._call_ollama(prompt, model=model, mode=mode)
        return result if result else text

    def _call_ollama(self, prompt: str, model: str, mode: str) -> str:
        """POST to Ollama /api/generate with stream=False.

        Returns the model response string, or empty string on any error
        (caller falls back to original text).
        """
        payload = json.dumps({
            "model": model,
            "prompt": prompt,
            "stream": False,
            "options": {
                "temperature": 0.2,   # low for deterministic cleanup
                "num_predict": 512,
            },
        }).encode("utf-8")

        req = urllib.request.Request(
            _GENERATE_URL,
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=_GENERATE_TIMEOUT) as resp:
                body = json.loads(resp.read().decode())
                result = body.get("response", "").strip()
                if result:
                    print(f"[LLM] {mode} ({model}) → {result[:60]}…")
                    return result
        except urllib.error.URLError:
            self._available = False
            print("[LLM] Ollama became unreachable — disabling for this session.")
        except Exception as e:
            print(f"[LLM] Unexpected error: {e}")
        return ""
