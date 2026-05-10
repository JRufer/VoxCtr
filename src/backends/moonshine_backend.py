from __future__ import annotations
import os
import time
import numpy as np

from .protocol import TranscriptionResult, BackendCapabilities

# moonshine_voice.moonshine_api.ModelArch integer values
_ARCH_MAP: dict[str, int] = {
    "tiny":             0,
    "base":             1,
    "tiny-streaming":   2,
    "base-streaming":   3,
    "small-streaming":  4,
    "medium-streaming": 5,
}

MODEL_SIZES = list(_ARCH_MAP.keys())

SUPPORTED_LANGUAGES: list[str] = ["en", "es", "ar", "ja", "ko", "vi", "uk", "zh"]

LANGUAGE_NAMES: dict[str, str] = {
    "en": "English",
    "es": "Spanish",
    "ar": "Arabic",
    "ja": "Japanese",
    "ko": "Korean",
    "vi": "Vietnamese",
    "uk": "Ukrainian",
    "zh": "Mandarin Chinese",
}

_CACHE_DIR = os.path.join(os.path.expanduser("~"), ".cache", "moonshine_voice")


def _model_cache_dir(model_size: str, language: str) -> str:
    arch_int = _ARCH_MAP.get(model_size, 1)
    return os.path.join(
        _CACHE_DIR,
        "download.moonshine.ai", "model", language, str(arch_int),
    )


def is_model_downloaded(model_size: str, language: str = "en") -> bool:
    path = _model_cache_dir(model_size, language)
    if not os.path.isdir(path):
        return False
    return any(f.endswith((".ort", ".bin")) for f in os.listdir(path))


def parse_model_size_language(encoded: str) -> tuple[str, str]:
    """Parse 'base-en' or 'tiny-es' into ('base', 'en') / ('tiny', 'es').

    Falls back to ('base', 'en') for plain strings like 'base'.
    """
    # Try splitting at the last '-' and checking if suffix is a language code
    if "-" in encoded:
        parts = encoded.rsplit("-", 1)
        candidate_size, candidate_lang = parts
        if candidate_lang in SUPPORTED_LANGUAGES and candidate_size in _ARCH_MAP:
            return candidate_size, candidate_lang
        # Handle compound sizes like 'tiny-streaming-en'
        for size in sorted(_ARCH_MAP, key=len, reverse=True):
            if encoded.startswith(size + "-"):
                remainder = encoded[len(size) + 1:]
                if remainder in SUPPORTED_LANGUAGES:
                    return size, remainder
    if encoded in _ARCH_MAP:
        return encoded, "en"
    return "base", "en"


def encode_model_size_language(model_size: str, language: str) -> str:
    return f"{model_size}-{language}"


class MoonshineBackend:
    name = "moonshine"

    def __init__(self) -> None:
        self._transcriber = None
        self._model_size: str = "base"
        self._language: str = "en"
        # Streaming state
        self._partial_text: str = ""
        self._stream_samples: int = 0

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
            gpu_vendor_support=["cpu"],
        )

    @staticmethod
    def list_downloaded_models(language: str = "en") -> list[str]:
        """Return model size names that are locally cached for the given language."""
        return [size for size in _ARCH_MAP if is_model_downloaded(size, language)]

    @staticmethod
    def download_model(model_size: str, language: str = "en") -> str:
        """Download a moonshine model and return the local cache path."""
        from moonshine_voice.download import get_model_for_language
        from moonshine_voice.moonshine_api import ModelArch

        arch_int = _ARCH_MAP.get(model_size, 1)
        arch = ModelArch(arch_int)
        model_path, _ = get_model_for_language(language, arch)
        return model_path

    def load_model(self, model_size: str, device: str, compute_type: str) -> None:
        """Load a Moonshine model.

        model_size is either a plain size ('base', 'tiny') which defaults to English,
        or an encoded 'size-language' string ('base-en', 'tiny-es').
        device and compute_type are accepted for interface compatibility but not used —
        Moonshine runs on CPU via a bundled ONNX Runtime native library.
        """
        from moonshine_voice import Transcriber
        from moonshine_voice.download import get_model_for_language
        from moonshine_voice.moonshine_api import ModelArch

        size, language = parse_model_size_language(str(model_size))
        self._model_size = size
        self._language = language

        arch_int = _ARCH_MAP.get(size, 1)
        arch = ModelArch(arch_int)
        model_path, model_arch = get_model_for_language(language, arch)
        self._transcriber = Transcriber(model_path, model_arch)

    def unload_model(self) -> None:
        if self._transcriber is not None:
            try:
                self._transcriber.close()
            except Exception:
                pass
        self._transcriber = None

    # ── Streaming interface (StreamingTranscriptionBackend) ───────────────

    def start_stream(self) -> None:
        """Begin a new streaming transcription session."""
        if self._transcriber is None:
            raise RuntimeError("Model not loaded — call load_model() first")
        self._transcriber.start()
        self._partial_text = ""
        self._stream_samples = 0

    def feed_audio(self, chunk: bytes) -> str | None:
        """Feed one raw int16 PCM chunk (16 kHz, as produced by AudioRecorder).

        Returns the updated partial transcript text when it changes, else None.
        """
        if self._transcriber is None:
            return None
        audio = np.frombuffer(chunk, dtype=np.int16).astype(np.float32) / 32768.0
        self._stream_samples += len(audio)
        self._transcriber.add_audio(audio.tolist(), sample_rate=16000)
        transcript = self._transcriber.update_transcription()
        text = " ".join(
            line.text for line in transcript.lines if getattr(line, "text", "")
        ).strip()
        if text != self._partial_text:
            self._partial_text = text
            return text
        return None

    def end_stream(self) -> TranscriptionResult:
        """Signal end of audio, wait for the final transcript, and return it."""
        if self._transcriber is None:
            raise RuntimeError("Model not loaded — call load_model() first")
        t0 = time.monotonic_ns()
        self._transcriber.stop()
        transcript = self._transcriber.update_transcription()
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)
        text = " ".join(
            line.text for line in transcript.lines if getattr(line, "text", "")
        ).strip()
        duration_ms = int(self._stream_samples / 16)  # 16 000 Hz → ms
        self._partial_text = ""
        self._stream_samples = 0
        return TranscriptionResult(
            text=text,
            language=self._language,
            language_probability=1.0,
            duration_ms=duration_ms,
            inference_ms=inference_ms,
        )

    # ── Batch interface (TranscriptionBackend) ────────────────────────────

    def transcribe(
        self,
        audio: np.ndarray,
        language: str | None = None,
        word_timestamps: bool = False,
        initial_prompt: str | None = None,
    ) -> TranscriptionResult:
        if self._transcriber is None:
            raise RuntimeError("Model not loaded — call load_model() first")

        audio_f32 = audio.astype(np.float32) if audio.dtype != np.float32 else audio

        t0 = time.monotonic_ns()
        transcript = self._transcriber.transcribe_without_streaming(
            audio_f32.tolist(), sample_rate=16000
        )
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)

        text = " ".join(line.text for line in transcript.lines if getattr(line, "text", "")).strip()

        return TranscriptionResult(
            text=text,
            language=self._language,
            language_probability=1.0,
            duration_ms=int(len(audio) / 16),
            inference_ms=inference_ms,
        )
