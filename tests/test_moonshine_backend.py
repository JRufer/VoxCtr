"""
Tests for the Moonshine transcription backend.

All tests run without a real Moonshine model loaded or moonshine-voice installed —
structural correctness, contract compliance, and helper-function behaviour only.
"""
import sys
import os
import numpy as np
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from backends.protocol import TranscriptionBackend, BackendCapabilities, TranscriptionResult
from backends.moonshine_backend import (
    MoonshineBackend,
    parse_model_size_language,
    encode_model_size_language,
    is_model_downloaded,
    MODEL_SIZES,
    SUPPORTED_LANGUAGES,
    LANGUAGE_NAMES,
    _ARCH_MAP,
    _model_cache_dir,
)


# ── Dataclass / constant tests ─────────────────────────────────────────────

class TestMoonshineConstants:
    def test_model_sizes_non_empty(self):
        assert len(MODEL_SIZES) > 0

    def test_tiny_and_base_present(self):
        assert "tiny" in MODEL_SIZES
        assert "base" in MODEL_SIZES

    def test_supported_languages_includes_english(self):
        assert "en" in SUPPORTED_LANGUAGES

    def test_language_names_keys_match_supported(self):
        for lang in SUPPORTED_LANGUAGES:
            assert lang in LANGUAGE_NAMES, f"Missing display name for language '{lang}'"

    def test_arch_map_keys_match_model_sizes(self):
        assert set(_ARCH_MAP.keys()) == set(MODEL_SIZES)

    def test_arch_map_values_are_ints(self):
        for v in _ARCH_MAP.values():
            assert isinstance(v, int)


# ── parse_model_size_language tests ───────────────────────────────────────

class TestParseModelSizeLanguage:
    def test_plain_base_defaults_to_english(self):
        assert parse_model_size_language("base") == ("base", "en")

    def test_plain_tiny_defaults_to_english(self):
        assert parse_model_size_language("tiny") == ("tiny", "en")

    def test_base_en(self):
        assert parse_model_size_language("base-en") == ("base", "en")

    def test_tiny_es(self):
        assert parse_model_size_language("tiny-es") == ("tiny", "es")

    def test_base_zh(self):
        assert parse_model_size_language("base-zh") == ("base", "zh")

    def test_tiny_streaming_en(self):
        assert parse_model_size_language("tiny-streaming-en") == ("tiny-streaming", "en")

    def test_small_streaming_es(self):
        assert parse_model_size_language("small-streaming-es") == ("small-streaming", "es")

    def test_unknown_falls_back_to_base_en(self):
        result = parse_model_size_language("totally-unknown")
        # Should not raise; returns a safe default
        assert isinstance(result, tuple)
        assert len(result) == 2


class TestEncodeModelSizeLanguage:
    def test_base_en(self):
        assert encode_model_size_language("base", "en") == "base-en"

    def test_tiny_streaming_es(self):
        assert encode_model_size_language("tiny-streaming", "es") == "tiny-streaming-es"

    def test_round_trip(self):
        for size in ["tiny", "base", "tiny-streaming", "small-streaming"]:
            for lang in ["en", "es", "ja"]:
                encoded = encode_model_size_language(size, lang)
                decoded_size, decoded_lang = parse_model_size_language(encoded)
                assert decoded_size == size, f"Round-trip failed for {encoded}"
                assert decoded_lang == lang, f"Round-trip language failed for {encoded}"


# ── is_model_downloaded / cache-dir tests ─────────────────────────────────

class TestModelCacheHelpers:
    def test_cache_dir_contains_language(self):
        path = _model_cache_dir("base", "en")
        assert "en" in path

    def test_cache_dir_contains_arch_int(self):
        path = _model_cache_dir("base", "en")
        arch_int = _ARCH_MAP["base"]
        assert str(arch_int) in path

    def test_is_model_downloaded_missing_dir(self, tmp_path, monkeypatch):
        import backends.moonshine_backend as mb
        monkeypatch.setattr(mb, "_CACHE_DIR", str(tmp_path / "nonexistent"))
        assert not mb.is_model_downloaded("base", "en")

    def test_is_model_downloaded_empty_dir(self, tmp_path, monkeypatch):
        import backends.moonshine_backend as mb
        monkeypatch.setattr(mb, "_CACHE_DIR", str(tmp_path))
        cache = tmp_path / "download.moonshine.ai" / "model" / "en" / str(_ARCH_MAP["base"])
        cache.mkdir(parents=True)
        # No .ort files present
        assert not mb.is_model_downloaded("base", "en")

    def test_is_model_downloaded_with_ort_file(self, tmp_path, monkeypatch):
        import backends.moonshine_backend as mb
        monkeypatch.setattr(mb, "_CACHE_DIR", str(tmp_path))
        cache = tmp_path / "download.moonshine.ai" / "model" / "en" / str(_ARCH_MAP["base"])
        cache.mkdir(parents=True)
        (cache / "model.ort").touch()
        assert mb.is_model_downloaded("base", "en")

    def test_list_downloaded_models_empty(self, tmp_path, monkeypatch):
        import backends.moonshine_backend as mb
        monkeypatch.setattr(mb, "_CACHE_DIR", str(tmp_path / "empty"))
        result = MoonshineBackend.list_downloaded_models("en")
        assert result == []

    def test_list_downloaded_models_finds_cached(self, tmp_path, monkeypatch):
        import backends.moonshine_backend as mb
        monkeypatch.setattr(mb, "_CACHE_DIR", str(tmp_path))
        for size in ["tiny", "base"]:
            cache = (
                tmp_path / "download.moonshine.ai" / "model" / "en" / str(_ARCH_MAP[size])
            )
            cache.mkdir(parents=True)
            (cache / "model.ort").touch()
        result = MoonshineBackend.list_downloaded_models("en")
        assert "tiny" in result
        assert "base" in result


# ── MoonshineBackend structural tests ─────────────────────────────────────

class TestMoonshineBackendStructure:
    def test_name(self):
        assert MoonshineBackend.name == "moonshine"

    def test_is_available_returns_bool(self):
        b = MoonshineBackend()
        assert isinstance(b.is_available, bool)

    def test_capabilities_shape(self):
        b = MoonshineBackend()
        caps = b.capabilities
        assert isinstance(caps, BackendCapabilities)
        assert isinstance(caps.word_timestamps, bool)
        assert isinstance(caps.language_detection, bool)
        assert isinstance(caps.initial_prompt, bool)
        assert isinstance(caps.streaming, bool)
        assert isinstance(caps.gpu_vendor_support, list)

    def test_capabilities_cpu_support(self):
        b = MoonshineBackend()
        assert "cpu" in b.capabilities.gpu_vendor_support

    def test_satisfies_protocol(self):
        b = MoonshineBackend()
        assert isinstance(b, TranscriptionBackend)

    def test_transcribe_raises_without_model(self):
        b = MoonshineBackend()
        with pytest.raises(RuntimeError, match="load_model"):
            b.transcribe(np.zeros(16000, dtype=np.float32))

    def test_unload_clears_transcriber(self):
        b = MoonshineBackend()
        b._transcriber = object()  # fake transcriber
        b.unload_model()
        assert b._transcriber is None

    def test_unload_noop_when_not_loaded(self):
        b = MoonshineBackend()
        b.unload_model()  # should not raise
        assert b._transcriber is None

    def test_initial_state(self):
        b = MoonshineBackend()
        assert b._transcriber is None
        assert b._model_size == "base"
        assert b._language == "en"
        assert b._partial_text == ""
        assert b._stream_samples == 0

    def test_word_timestamps_not_supported(self):
        b = MoonshineBackend()
        assert b.capabilities.word_timestamps is False

    def test_language_detection_not_supported(self):
        b = MoonshineBackend()
        assert b.capabilities.language_detection is False

    def test_initial_prompt_not_supported(self):
        b = MoonshineBackend()
        assert b.capabilities.initial_prompt is False

    def test_streaming_capability_true(self):
        b = MoonshineBackend()
        assert b.capabilities.streaming is True

    def test_satisfies_streaming_protocol(self):
        from backends.protocol import StreamingTranscriptionBackend
        b = MoonshineBackend()
        assert isinstance(b, StreamingTranscriptionBackend)


# ── Streaming interface structural tests ───────────────────────────────────

class FakeTranscript:
    def __init__(self, lines):
        self.lines = lines

class TestMoonshineBackendStreaming:
    def test_start_stream_raises_without_model(self):
        b = MoonshineBackend()
        with pytest.raises(RuntimeError, match="load_model"):
            b.start_stream()

    def test_end_stream_raises_without_model(self):
        b = MoonshineBackend()
        with pytest.raises(RuntimeError, match="load_model"):
            b.end_stream()

    def test_feed_audio_returns_none_without_model(self):
        b = MoonshineBackend()
        chunk = (np.zeros(1024, dtype=np.int16)).tobytes()
        result = b.feed_audio(chunk)
        assert result is None

    def test_start_stream_resets_state(self):
        b = MoonshineBackend()
        b._partial_text = "stale text"
        b._stream_samples = 99999

        # Fake a loaded transcriber so start_stream() can proceed
        class FakeTranscriber:
            def start(self): pass
            def stop(self): pass
            def add_audio(self, *a, **k): pass
            def update_transcription(self): return FakeTranscript([])
            def transcribe_without_streaming(self, *a, **k): return FakeTranscript([])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        b.start_stream()
        assert b._partial_text == ""
        assert b._stream_samples == 0

    def test_feed_audio_accumulates_samples(self):
        b = MoonshineBackend()

        class FakeTranscriber:
            def add_audio(self, audio, sample_rate): pass
            def update_transcription(self): return FakeTranscript([])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        chunk = (np.zeros(1024, dtype=np.int16)).tobytes()
        b.feed_audio(chunk)
        assert b._stream_samples == 1024

    def test_feed_audio_returns_none_when_text_unchanged(self):
        b = MoonshineBackend()

        class FakeTranscriber:
            def add_audio(self, audio, sample_rate): pass
            def update_transcription(self): return FakeTranscript([])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        b._partial_text = ""  # same as what update_transcription returns
        chunk = (np.zeros(256, dtype=np.int16)).tobytes()
        result = b.feed_audio(chunk)
        assert result is None

    def test_feed_audio_returns_text_when_changed(self):
        b = MoonshineBackend()

        class FakeLine:
            text = "hello world"

        class FakeTranscriber:
            def add_audio(self, audio, sample_rate): pass
            def update_transcription(self): return FakeTranscript([FakeLine()])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        b._partial_text = ""
        chunk = (np.zeros(256, dtype=np.int16)).tobytes()
        result = b.feed_audio(chunk)
        assert result == "hello world"
        assert b._partial_text == "hello world"

    def test_feed_audio_returns_none_when_text_same(self):
        b = MoonshineBackend()

        class FakeLine:
            text = "hello world"

        class FakeTranscriber:
            def add_audio(self, audio, sample_rate): pass
            def update_transcription(self): return FakeTranscript([FakeLine()])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        b._partial_text = "hello world"  # already up to date
        chunk = (np.zeros(256, dtype=np.int16)).tobytes()
        result = b.feed_audio(chunk)
        assert result is None

    def test_end_stream_returns_transcription_result(self):
        b = MoonshineBackend()
        b._language = "en"
        b._stream_samples = 16000  # 1 second

        class FakeLine:
            text = "final result"

        class FakeTranscriber:
            def stop(self): pass
            def update_transcription(self): return FakeTranscript([FakeLine()])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        result = b.end_stream()
        from backends.protocol import TranscriptionResult
        assert isinstance(result, TranscriptionResult)
        assert result.text == "final result"
        assert result.language == "en"
        assert result.duration_ms == 1000  # 16000 samples / 16 = 1000 ms

    def test_end_stream_resets_streaming_state(self):
        b = MoonshineBackend()
        b._partial_text = "some partial"
        b._stream_samples = 8000

        class FakeTranscriber:
            def stop(self): pass
            def update_transcription(self): return FakeTranscript([])
            def close(self): pass

        b._transcriber = FakeTranscriber()
        b.end_stream()
        assert b._partial_text == ""
        assert b._stream_samples == 0


# ── Selector integration ───────────────────────────────────────────────────

class TestSelectorMoonshine:
    def test_select_backend_moonshine_forced(self):
        from backends.selector import select_backend

        class FakeConfig:
            def get(self, key, default=None):
                return {"backend_engine": "moonshine"}.get(key, default)

        b = select_backend(FakeConfig())
        # If moonshine-voice is not installed, selector falls back to FasterWhisperBackend
        assert b is not None

    def test_auto_compute_type_moonshine(self):
        from backends.selector import auto_compute_type
        ct = auto_compute_type("moonshine", None)
        assert ct == "onnx"
