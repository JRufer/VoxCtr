from __future__ import annotations
import os
import time
import numpy as np

from .protocol import TranscriptionResult, BackendCapabilities


# English model sizes and their moonshine-voice model_arch identifiers
_MODEL_ARCH_MAP: dict[str, str] = {
    "tiny":   "moonshine/tiny",
    "small":  "moonshine/small",
    "medium": "moonshine/medium",
}

# Non-English multilingual model (single architecture, 58M params)
# Covers: Arabic, Japanese, Korean, Mandarin, Spanish, Ukrainian, Vietnamese
_MULTILINGUAL_ARCH = "moonshine/base-multilingual"
MULTILINGUAL_LANGS = {"ar", "ja", "ko", "zh", "es", "uk", "vi"}
SUPPORTED_LANGUAGES = ["en", "es", "zh", "ja", "ko", "vi", "uk", "ar"]

_DEFAULT_CACHE = os.path.join(os.path.expanduser("~"), ".cache", "voxctl", "moonshine")


class MoonshineBackend:
    name = "moonshine"

    def __init__(self) -> None:
        self._model_size: str | None = None
        self._model_arch: str | None = None
        self._language: str = "en"
        self._cache_dir: str = _DEFAULT_CACHE

    @property
    def is_available(self) -> bool:
        try:
            import moonshine_voice  # noqa: F401
            return True
        except ImportError:
            return False

    @property
    def capabilities(self) -> BackendCapabilities:
        return BackendCapabilities(
            word_timestamps=False,
            language_detection=False,
            initial_prompt=False,
            streaming=True,
            gpu_vendor_support=["nvidia", "amd", "intel", "cpu"],
        )

    def load_model(self, model_size: str, device: str, compute_type: str) -> None:
        if isinstance(model_size, list):
            model_size = str(model_size[0]) if model_size else "medium"
        model_size = str(model_size)
        if model_size not in _MODEL_ARCH_MAP:
            model_size = "medium"

        os.makedirs(self._cache_dir, exist_ok=True)
        self._model_size = model_size
        self._model_arch = self._resolve_arch(model_size, self._language)

        # Force model download + first-inference warmup so the first real call
        # isn't penalised by model loading time.
        from moonshine_voice import Transcriber
        silence = np.zeros(1600, dtype=np.float32)  # 100 ms of silence
        tr = Transcriber(model_path=self._cache_dir, model_arch=self._model_arch)
        tr.start()
        tr.add_audio(silence, 16000)
        tr.stop()

    def unload_model(self) -> None:
        self._model_arch = None
        self._model_size = None

    def configure_language(self, language: str) -> None:
        """Switch the active language after model load."""
        self._language = language or "en"
        if self._model_size:
            self._model_arch = self._resolve_arch(self._model_size, self._language)

    def transcribe(
        self,
        audio: np.ndarray,
        language: str | None = None,
        word_timestamps: bool = False,
        initial_prompt: str | None = None,
    ) -> TranscriptionResult:
        if self._model_arch is None:
            raise RuntimeError("Model not loaded — call load_model() first")
        return self._run(audio, language)

    def transcribe_with_vad(
        self,
        audio: np.ndarray,
        vad_parameters: dict | None = None,
        initial_prompt: str | None = None,
        language: str | None = None,
    ) -> TranscriptionResult:
        """Moonshine includes built-in VAD; delegates straight to the normal path."""
        if self._model_arch is None:
            raise RuntimeError("Model not loaded — call load_model() first")
        return self._run(audio, language)

    # ── Internal ──────────────────────────────────────────────────────────────

    def _resolve_arch(self, model_size: str, language: str) -> str:
        if language and language != "en" and language in MULTILINGUAL_LANGS:
            return _MULTILINGUAL_ARCH
        return _MODEL_ARCH_MAP.get(model_size, "moonshine/medium")

    def _run(self, audio: np.ndarray, language: str | None = None) -> TranscriptionResult:
        from moonshine_voice import Transcriber, TranscriptEventListener

        lang = language or self._language or "en"
        arch = self._resolve_arch(self._model_size or "medium", lang)

        lines: list[str] = []

        class _Listener(TranscriptEventListener):
            def on_line_completed(self, event) -> None:  # type: ignore[override]
                # event.line.text holds the finalised segment text
                text = getattr(getattr(event, "line", None), "text", None)
                if text:
                    lines.append(text)

        t0 = time.monotonic_ns()

        # A new Transcriber per call keeps state clean between recordings.
        # Model weights are cached in the C++ runtime after the first load_model()
        # warmup, so object creation here is lightweight.
        tr = Transcriber(model_path=self._cache_dir, model_arch=arch)
        tr.add_listener(_Listener())
        tr.start()
        tr.add_audio(audio, 16000)
        tr.stop()  # blocks until all buffered audio is processed and callbacks fired

        inference_ms = int((time.monotonic_ns() - t0) / 1e6)
        text = " ".join(line.strip() for line in lines if line.strip())

        return TranscriptionResult(
            text=text,
            language=lang,
            language_probability=1.0,
            duration_ms=int(len(audio) / 16),
            inference_ms=inference_ms,
            word_timestamps=None,
        )
