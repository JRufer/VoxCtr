from __future__ import annotations
import io
import json
import shutil
import subprocess
import time
import wave
import os
import numpy as np

from .protocol import TranscriptionResult, WordTimestamp, BackendCapabilities

# Friendly model-size name → GGUF filename mapping
GGUF_MAP: dict[str, str] = {
    "tiny":     "ggml-tiny-q5_1.bin",
    "tiny.en":  "ggml-tiny.en-q5_1.bin",
    "base":     "ggml-base-q5_1.bin",
    "small":    "ggml-small-q5_k.bin",
    "medium":   "ggml-medium-q5_k.bin",
    "large-v2": "ggml-large-v2-q5_k.bin",
    "large-v3": "ggml-large-v3-q5_k_m.bin",
}

GGUF_BASE_URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/"

DEFAULT_MODEL_DIR = os.path.join(
    os.path.expanduser("~"), ".local", "share", "whisper-wayland", "models"
)


class WhisperCppBackend:
    name = "whisper-cpp"

    def __init__(
        self,
        binary_path: str = "whisper-cli",
        model_dir: str = DEFAULT_MODEL_DIR,
    ) -> None:
        self._binary = binary_path
        self._model_dir = model_dir
        self._model_path: str | None = None
        self._device_flags: list[str] = []
        self._threads: int = max(1, (os.cpu_count() or 4) // 2)

        # Prefer pywhispercpp in-process bindings when available
        try:
            from pywhispercpp.model import Model as _CppModel  # type: ignore
            self._CppModel = _CppModel
            self._use_bindings = True
        except ImportError:
            self._CppModel = None
            self._use_bindings = False

    @property
    def is_available(self) -> bool:
        if self._use_bindings:
            return True
        return shutil.which(self._binary) is not None

    @property
    def capabilities(self) -> BackendCapabilities:
        return BackendCapabilities(
            word_timestamps=True,
            language_detection=True,
            initial_prompt=True,
            streaming=False,
            gpu_vendor_support=["nvidia", "amd", "intel", "cpu"],
        )

    def load_model(self, model_size: str, device: str, compute_type: str) -> None:
        self._model_path = self._resolve_model_path(model_size)
        self._device_flags = _device_to_flags(device)

        if self._use_bindings:
            self._cpp_model = self._CppModel(
                self._model_path,
                n_threads=self._threads,
            )
        else:
            self._cpp_model = None

    def unload_model(self) -> None:
        self._cpp_model = None
        self._model_path = None

    def transcribe(
        self,
        audio: np.ndarray,
        language: str | None = None,
        word_timestamps: bool = False,
        initial_prompt: str | None = None,
    ) -> TranscriptionResult:
        if self._model_path is None:
            raise RuntimeError("Model not loaded — call load_model() first")

        if self._use_bindings and self._cpp_model is not None:
            return self._transcribe_bindings(audio, language, word_timestamps, initial_prompt)
        return self._transcribe_subprocess(audio, language, word_timestamps, initial_prompt)

    # ── Subprocess path ────────────────────────────────────────────────────

    def _transcribe_subprocess(
        self,
        audio: np.ndarray,
        language: str | None,
        word_timestamps: bool,
        initial_prompt: str | None,
    ) -> TranscriptionResult:
        wav_bytes = _numpy_to_wav(audio)

        cmd = [
            self._binary,
            "--model", self._model_path,
            "--output-json",
            "--threads", str(self._threads),
        ]

        if not word_timestamps:
            cmd += ["--no-timestamps"]

        if language:
            cmd += ["--language", language]
        if initial_prompt:
            cmd += ["--prompt", initial_prompt]
        if self._device_flags:
            cmd += self._device_flags

        # whisper-cli reads WAV from stdin when --file - is given
        cmd += ["--file", "-"]

        t0 = time.monotonic_ns()
        try:
            proc = subprocess.run(
                cmd,
                input=wav_bytes,
                capture_output=True,
                timeout=60,
            )
        except subprocess.TimeoutExpired:
            raise RuntimeError("whisper-cli timed out after 60 seconds")
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)

        if proc.returncode != 0:
            stderr = proc.stderr.decode(errors="replace")
            raise RuntimeError(f"whisper-cli exited {proc.returncode}: {stderr[:500]}")

        stdout = proc.stdout.decode(errors="replace").strip()
        if not stdout:
            # whisper-cli sometimes writes JSON to a sidecar file instead of stdout
            return TranscriptionResult(
                text="",
                language=language or "en",
                language_probability=1.0,
                duration_ms=int(len(audio) / 16),
                inference_ms=inference_ms,
            )

        try:
            data = json.loads(stdout)
        except json.JSONDecodeError:
            # Fall back: treat stdout as plain text
            return TranscriptionResult(
                text=stdout,
                language=language or "en",
                language_probability=1.0,
                duration_ms=int(len(audio) / 16),
                inference_ms=inference_ms,
            )

        return _parse_cpp_json(data, audio, inference_ms, word_timestamps)

    # ── pywhispercpp bindings path ─────────────────────────────────────────

    def _transcribe_bindings(
        self,
        audio: np.ndarray,
        language: str | None,
        word_timestamps: bool,
        initial_prompt: str | None,
    ) -> TranscriptionResult:
        params: dict = {}
        if language:
            params["language"] = language
        if initial_prompt:
            params["initial_prompt"] = initial_prompt

        t0 = time.monotonic_ns()
        segments = self._cpp_model.transcribe(audio.tolist(), **params)
        inference_ms = int((time.monotonic_ns() - t0) / 1e6)

        text = " ".join(s.text.strip() for s in segments)
        detected_lang = getattr(self._cpp_model, "language", language or "en")

        wts: list[WordTimestamp] | None = None
        if word_timestamps:
            wts = []
            for seg in segments:
                wts.append(
                    WordTimestamp(
                        word=seg.text.strip(),
                        start_ms=int(seg.t0 * 10),
                        end_ms=int(seg.t1 * 10),
                        probability=1.0,
                    )
                )

        return TranscriptionResult(
            text=text,
            language=detected_lang,
            language_probability=1.0,
            duration_ms=int(len(audio) / 16),
            inference_ms=inference_ms,
            word_timestamps=wts,
        )

    # ── Model resolution ───────────────────────────────────────────────────

    def _resolve_model_path(self, model_size: str) -> str:
        # If model_size is already an absolute path, use it directly
        if os.path.isabs(model_size) and os.path.isfile(model_size):
            return model_size

        # Check if it looks like a GGUF filename (has .bin extension)
        if model_size.endswith(".bin"):
            candidate = os.path.join(self._model_dir, model_size)
            if os.path.isfile(candidate):
                return candidate
            raise FileNotFoundError(
                f"GGUF model file not found: {candidate}\n"
                f"Download it to {self._model_dir}/"
            )

        # Friendly name → GGUF filename
        filename = GGUF_MAP.get(model_size)
        if filename is None:
            raise ValueError(
                f"Unknown model size '{model_size}'. "
                f"Valid sizes: {list(GGUF_MAP.keys())}"
            )

        path = os.path.join(self._model_dir, filename)
        if not os.path.isfile(path):
            raise FileNotFoundError(
                f"GGUF model not found: {path}\n"
                f"Download with:\n"
                f"  mkdir -p {self._model_dir}\n"
                f"  wget -P {self._model_dir} {GGUF_BASE_URL}{filename}"
            )
        return path

    def list_downloaded_models(self) -> list[str]:
        """Return friendly names of GGUF models present in model_dir."""
        if not os.path.isdir(self._model_dir):
            return []
        present = []
        for friendly, filename in GGUF_MAP.items():
            if os.path.isfile(os.path.join(self._model_dir, filename)):
                present.append(friendly)
        return present

    def get_model_url(self, model_size: str) -> str:
        filename = GGUF_MAP.get(model_size, f"ggml-{model_size}.bin")
        return GGUF_BASE_URL + filename

    def configure_threads(self, n: int) -> None:
        self._threads = max(1, n)


# ── Helpers ────────────────────────────────────────────────────────────────

def _numpy_to_wav(audio: np.ndarray) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)  # 16-bit PCM
        wf.setframerate(16000)
        pcm = (audio * 32767).astype(np.int16)
        wf.writeframes(pcm.tobytes())
    return buf.getvalue()


def _device_to_flags(device: str) -> list[str]:
    device = device.lower()
    if device == "vulkan":
        return ["--gpu", "vulkan"]
    if device == "cuda":
        return ["--gpu", "cuda"]
    if device == "cpu":
        return []
    # 'auto' — let whisper-cli decide
    return []


def _parse_cpp_json(
    data: dict,
    audio: np.ndarray,
    inference_ms: int,
    word_timestamps: bool,
) -> TranscriptionResult:
    """Parse whisper-cli --output-json payload into a TranscriptionResult."""
    transcription = data.get("transcription", [])
    segments_text = [seg.get("text", "").strip() for seg in transcription]
    text = " ".join(t for t in segments_text if t)

    # whisper.cpp JSON may include language detection info
    result_info = data.get("result", {})
    language = result_info.get("language", "en")
    lang_prob = 1.0  # whisper.cpp doesn't always report confidence

    wts: list[WordTimestamp] | None = None
    if word_timestamps and transcription:
        wts = []
        for seg in transcription:
            ts = seg.get("timestamps", {})
            from_ts = _parse_timestamp(ts.get("from", "00:00:00,000"))
            to_ts = _parse_timestamp(ts.get("to", "00:00:00,000"))
            wts.append(
                WordTimestamp(
                    word=seg.get("text", "").strip(),
                    start_ms=from_ts,
                    end_ms=to_ts,
                    probability=1.0,
                )
            )

    return TranscriptionResult(
        text=text,
        language=language,
        language_probability=lang_prob,
        duration_ms=int(len(audio) / 16),
        inference_ms=inference_ms,
        word_timestamps=wts,
    )


def _parse_timestamp(ts: str) -> int:
    """Convert 'HH:MM:SS,mmm' → milliseconds."""
    try:
        hms, ms = ts.replace(".", ",").split(",")
        h, m, s = hms.split(":")
        return (int(h) * 3600 + int(m) * 60 + int(s)) * 1000 + int(ms)
    except Exception:
        return 0
