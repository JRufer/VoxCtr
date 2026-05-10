from __future__ import annotations
from typing import Protocol, runtime_checkable
from dataclasses import dataclass, field
import numpy as np


@dataclass
class WordTimestamp:
    word: str
    start_ms: int
    end_ms: int
    probability: float


@dataclass
class TranscriptionResult:
    text: str
    language: str
    language_probability: float
    duration_ms: int
    inference_ms: int
    word_timestamps: list[WordTimestamp] | None = field(default=None)


@dataclass
class BackendCapabilities:
    word_timestamps: bool
    language_detection: bool
    initial_prompt: bool
    streaming: bool
    gpu_vendor_support: list[str]


@runtime_checkable
class TranscriptionBackend(Protocol):
    @property
    def name(self) -> str: ...

    @property
    def is_available(self) -> bool: ...

    @property
    def capabilities(self) -> BackendCapabilities: ...

    def load_model(self, model_size: str, device: str, compute_type: str) -> None: ...

    def unload_model(self) -> None: ...

    def transcribe(
        self,
        audio: np.ndarray,
        language: str | None = None,
        word_timestamps: bool = False,
        initial_prompt: str | None = None,
    ) -> TranscriptionResult: ...


@runtime_checkable
class StreamingTranscriptionBackend(Protocol):
    """Extended protocol for backends that support real-time chunk-by-chunk transcription.

    A backend that sets capabilities.streaming = True should also implement this
    protocol.  The InferenceEngine checks isinstance(backend, StreamingTranscriptionBackend)
    and routes audio through the streaming path when True.

    Lifecycle:
        start_stream()              — called once at recording start
        feed_audio(chunk) → str?   — called for every raw PCM chunk (int16 bytes, 16 kHz)
                                      returns updated partial text when it changes, else None
        end_stream() → result      — called once at recording stop; blocks until final
                                      transcript is ready; returns the authoritative result
    """

    def start_stream(self) -> None: ...

    def feed_audio(self, chunk: bytes) -> str | None: ...

    def end_stream(self) -> TranscriptionResult: ...

