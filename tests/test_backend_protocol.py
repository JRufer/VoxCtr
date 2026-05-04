"""
Backend protocol conformance tests.

These tests verify structural correctness and type contracts without
requiring a real Whisper model to be loaded or heavy GPU hardware.
"""
import sys
import os
import numpy as np
import pytest

# Make src importable from the tests directory
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from backends.protocol import (
    TranscriptionBackend,
    TranscriptionResult,
    WordTimestamp,
    BackendCapabilities,
)
from backends.faster_whisper_backend import FasterWhisperBackend
from backends.whisper_cpp_backend import (
    WhisperCppBackend,
    _numpy_to_wav,
    _parse_timestamp,
    GGUF_MAP,
)
from backends.selector import (
    probe_gpu,
    _vulkan_available,
    _cuda_available,
    auto_compute_type,
)


# ── Dataclass shape tests ──────────────────────────────────────────────────

class TestDataclasses:
    def test_transcription_result_fields(self):
        r = TranscriptionResult(
            text="hello",
            language="en",
            language_probability=0.99,
            duration_ms=1000,
            inference_ms=50,
        )
        assert r.text == "hello"
        assert r.language == "en"
        assert 0.0 <= r.language_probability <= 1.0
        assert r.word_timestamps is None

    def test_word_timestamp_fields(self):
        w = WordTimestamp(word="hi", start_ms=0, end_ms=300, probability=0.95)
        assert w.word == "hi"
        assert w.start_ms == 0
        assert w.end_ms == 300

    def test_backend_capabilities_fields(self):
        caps = BackendCapabilities(
            word_timestamps=True,
            language_detection=True,
            initial_prompt=True,
            streaming=False,
            gpu_vendor_support=["nvidia", "cpu"],
        )
        assert isinstance(caps.gpu_vendor_support, list)
        assert isinstance(caps.streaming, bool)


# ── FasterWhisperBackend structural tests ─────────────────────────────────

class TestFasterWhisperBackend:
    def test_name(self):
        assert FasterWhisperBackend.name == "faster-whisper"

    def test_is_available_returns_bool(self):
        b = FasterWhisperBackend()
        assert isinstance(b.is_available, bool)

    def test_capabilities_shape(self):
        b = FasterWhisperBackend()
        caps = b.capabilities
        assert isinstance(caps, BackendCapabilities)
        assert isinstance(caps.word_timestamps, bool)
        assert "nvidia" in caps.gpu_vendor_support

    def test_satisfies_protocol(self):
        b = FasterWhisperBackend()
        assert isinstance(b, TranscriptionBackend)

    def test_transcribe_raises_without_model(self):
        b = FasterWhisperBackend()
        with pytest.raises(RuntimeError, match="load_model"):
            b.transcribe(np.zeros(16000, dtype=np.float32))

    def test_unload_clears_model(self):
        b = FasterWhisperBackend()
        b._model = object()  # fake model
        b.unload_model()
        assert b._model is None


# ── WhisperCppBackend structural tests ────────────────────────────────────

class TestWhisperCppBackend:
    def test_name(self):
        assert WhisperCppBackend.name == "whisper-cpp"

    def test_is_available_returns_bool(self):
        b = WhisperCppBackend()
        assert isinstance(b.is_available, bool)

    def test_capabilities_shape(self):
        b = WhisperCppBackend()
        caps = b.capabilities
        assert isinstance(caps, BackendCapabilities)
        assert "amd" in caps.gpu_vendor_support
        assert "intel" in caps.gpu_vendor_support

    def test_satisfies_protocol(self):
        b = WhisperCppBackend()
        assert isinstance(b, TranscriptionBackend)

    def test_transcribe_raises_without_model(self):
        b = WhisperCppBackend()
        with pytest.raises(RuntimeError, match="load_model"):
            b.transcribe(np.zeros(16000, dtype=np.float32))

    def test_unload_clears_model(self):
        b = WhisperCppBackend()
        b._model_path = "/fake/path"
        b.unload_model()
        assert b._model_path is None

    def test_configure_threads(self):
        b = WhisperCppBackend()
        b.configure_threads(8)
        assert b._threads == 8

    def test_configure_threads_minimum(self):
        b = WhisperCppBackend()
        b.configure_threads(0)
        assert b._threads >= 1

    def test_list_downloaded_models_missing_dir(self, tmp_path):
        b = WhisperCppBackend(model_dir=str(tmp_path / "nonexistent"))
        assert b.list_downloaded_models() == []

    def test_list_downloaded_models_present(self, tmp_path):
        filename = GGUF_MAP["base"]
        (tmp_path / filename).touch()
        b = WhisperCppBackend(model_dir=str(tmp_path))
        assert "base" in b.list_downloaded_models()

    def test_resolve_model_path_abs(self, tmp_path):
        f = tmp_path / "mymodel.bin"
        f.touch()
        b = WhisperCppBackend(model_dir=str(tmp_path))
        assert b._resolve_model_path(str(f)) == str(f)

    def test_resolve_model_path_unknown_size(self):
        b = WhisperCppBackend()
        with pytest.raises(ValueError, match="Unknown model size"):
            b._resolve_model_path("nonexistent-xyz")

    def test_resolve_model_path_missing_file(self, tmp_path):
        b = WhisperCppBackend(model_dir=str(tmp_path))
        with pytest.raises(FileNotFoundError):
            b._resolve_model_path("base")

    def test_get_model_url(self):
        b = WhisperCppBackend()
        url = b.get_model_url("base")
        assert "ggml-base" in url
        assert url.startswith("https://")


# ── Audio helper tests ─────────────────────────────────────────────────────

class TestAudioHelpers:
    def test_numpy_to_wav_produces_valid_wav(self):
        import wave, io
        audio = np.zeros(16000, dtype=np.float32)
        data = _numpy_to_wav(audio)
        with wave.open(io.BytesIO(data)) as wf:
            assert wf.getnchannels() == 1
            assert wf.getsampwidth() == 2
            assert wf.getframerate() == 16000
            assert wf.getnframes() == 16000

    def test_numpy_to_wav_round_trip_silence(self):
        audio = np.zeros(8000, dtype=np.float32)
        data = _numpy_to_wav(audio)
        assert len(data) > 44  # at least a valid WAV header

    def test_parse_timestamp_zero(self):
        assert _parse_timestamp("00:00:00,000") == 0

    def test_parse_timestamp_one_second(self):
        assert _parse_timestamp("00:00:01,000") == 1000

    def test_parse_timestamp_one_minute(self):
        assert _parse_timestamp("00:01:00,500") == 60500

    def test_parse_timestamp_malformed(self):
        assert _parse_timestamp("bad") == 0


# ── Selector tests ─────────────────────────────────────────────────────────

class TestSelector:
    def test_probe_gpu_returns_gpuinfo_or_none(self):
        from backends.selector import GpuInfo
        result = probe_gpu()
        assert result is None or isinstance(result, GpuInfo)

    def test_vulkan_available_returns_bool(self):
        assert isinstance(_vulkan_available(), bool)

    def test_cuda_available_returns_bool(self):
        assert isinstance(_cuda_available(), bool)

    def test_auto_compute_type_faster_whisper_cpu(self):
        ct = auto_compute_type("faster-whisper", None)
        assert ct == "int8"

    def test_auto_compute_type_whisper_cpp_cpu(self):
        ct = auto_compute_type("whisper-cpp", None)
        assert ct == "Q5_K_M"

    def test_auto_compute_type_nvidia_large_vram(self):
        from backends.selector import GpuInfo
        gpu = GpuInfo(vendor="nvidia", api="cuda", vram_mb=8192)
        ct = auto_compute_type("faster-whisper", gpu)
        assert ct == "float16"

    def test_auto_compute_type_nvidia_small_vram(self):
        from backends.selector import GpuInfo
        gpu = GpuInfo(vendor="nvidia", api="cuda", vram_mb=4096)
        ct = auto_compute_type("faster-whisper", gpu)
        assert ct == "int8"

    def test_auto_compute_type_amd_large_vram(self):
        from backends.selector import GpuInfo
        gpu = GpuInfo(vendor="amd", api="vulkan", vram_mb=10240)
        ct = auto_compute_type("whisper-cpp", gpu)
        assert ct == "Q8_0"

    def test_auto_compute_type_amd_small_vram(self):
        from backends.selector import GpuInfo
        gpu = GpuInfo(vendor="amd", api="vulkan", vram_mb=4096)
        ct = auto_compute_type("whisper-cpp", gpu)
        assert ct == "Q5_K_M"

    def test_select_backend_faster_whisper_forced(self):
        from backends.selector import select_backend

        class FakeConfig:
            def get(self, key, default=None):
                return {"backend_engine": "faster-whisper"}.get(key, default)

        b = select_backend(FakeConfig())
        assert isinstance(b, FasterWhisperBackend)

    def test_select_backend_whisper_cpp_forced(self, tmp_path):
        from backends.selector import select_backend

        class FakeConfig:
            def get(self, key, default=None):
                return {
                    "backend_engine": "whisper-cpp",
                    "whisper_cpp_binary": "whisper-cli",
                    "whisper_cpp_model_dir": str(tmp_path),
                    "whisper_cpp_threads": 0,
                }.get(key, default)

        b = select_backend(FakeConfig())
        assert isinstance(b, WhisperCppBackend)
