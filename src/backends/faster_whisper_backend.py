from __future__ import annotations
import time
import numpy as np
from .protocol import TranscriptionResult, WordTimestamp, BackendCapabilities


class FasterWhisperBackend:
    name = "faster-whisper"

    def __init__(self) -> None:
        self._model = None
        self._model_size: str | None = None
        self._device: str | None = None
        self._compute_type: str | None = None

    @property
    def is_available(self) -> bool:
        try:
            import faster_whisper  # noqa: F401
            return True
        except ImportError:
            return False

    @property
    def capabilities(self) -> BackendCapabilities:
        return BackendCapabilities(
            word_timestamps=True,
            language_detection=True,
            initial_prompt=True,
            streaming=False,
            gpu_vendor_support=["nvidia", "cpu"],
        )

    def load_model(self, model_size: str, device: str, compute_type: str) -> None:
        from faster_whisper import WhisperModel
        import os

        self._model_size = model_size
        self._device = device
        self._compute_type = compute_type

        cache_dir = os.path.join(os.path.expanduser("~"), ".cache", "whisper-wayland")
        self._model = WhisperModel(
            model_size,
            device=device,
            compute_type=compute_type,
            download_root=cache_dir,
        )

    def unload_model(self) -> None:
        self._model = None

    def transcribe(
        self,
        audio: np.ndarray,
        language: str | None = None,
        word_timestamps: bool = False,
        initial_prompt: str | None = None,
    ) -> TranscriptionResult:
        if self._model is None:
            raise RuntimeError("Model not loaded — call load_model() first")

        t0 = time.monotonic_ns()
        segments, info = self._model.transcribe(
            audio,
            language=language,
            word_timestamps=word_timestamps,
            initial_prompt=initial_prompt,
            beam_size=5,
        )
        # Materialise the generator so timing is accurate
        segments = list(segments)
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)

        text = " ".join(s.text.strip() for s in segments)
        wts = _extract_words(segments) if word_timestamps else None

        return TranscriptionResult(
            text=text,
            language=info.language,
            language_probability=info.language_probability,
            duration_ms=int(len(audio) / 16),
            inference_ms=inference_ms,
            word_timestamps=wts,
        )

    def transcribe_with_vad(
        self,
        audio: np.ndarray,
        vad_parameters: dict | None = None,
        initial_prompt: str | None = None,
        language: str | None = None,
    ) -> TranscriptionResult:
        """Extended method that exposes VAD filtering — used by InferenceEngine."""
        if self._model is None:
            raise RuntimeError("Model not loaded — call load_model() first")

        t0 = time.monotonic_ns()
        vad_params = vad_parameters or {}
        segments, info = self._model.transcribe(
            audio,
            beam_size=1,
            condition_on_previous_text=False,
            vad_filter=True,
            vad_parameters=vad_params,
            initial_prompt=initial_prompt,
            language=language,
        )
        segments = list(segments)
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)

        text = " ".join(s.text.strip() for s in segments)

        return TranscriptionResult(
            text=text,
            language=info.language,
            language_probability=info.language_probability,
            duration_ms=int(len(audio) / 16),
            inference_ms=inference_ms,
            word_timestamps=None,
        )


def _extract_words(segments) -> list[WordTimestamp]:
    words: list[WordTimestamp] = []
    for seg in segments:
        if seg.words:
            for w in seg.words:
                words.append(
                    WordTimestamp(
                        word=w.word,
                        start_ms=int(w.start * 1000),
                        end_ms=int(w.end * 1000),
                        probability=w.probability,
                    )
                )
    return words
