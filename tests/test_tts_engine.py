"""Tests for tts_engine — voice catalog, download helpers, TTSEngine."""
import io
import json
import os
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch, call

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from tts_engine import (
    GITHUB_RELEASE_BASE,
    DEFAULT_VOICE,
    VOICE_CATALOG,
    VOICES_DIR,
    TTSEngine,
    available_tts_engine,
    download_voice,
    get_voice_json_path,
    get_voice_path,
    get_voice_sample_rate,
    is_voice_downloaded,
)


# ── Helpers ───────────────────────────────────────────────────────────────────

def _fake_config(enabled=True, engine="piper", voice="en-us-lessac-medium"):
    cfg = MagicMock()
    cfg.get.side_effect = lambda k, d=None: {
        "tts_enabled": enabled,
        "tts_engine": engine,
        "tts_voice": voice,
    }.get(k, d)
    return cfg


def _make_tarball_bytes(onnx_name: str, sample_rate: int = 16000) -> bytes:
    """Build a minimal in-memory .tar.gz with a fake .onnx and .onnx.json."""
    buf = io.BytesIO()
    json_name = onnx_name + ".json"
    meta = json.dumps({"audio": {"sample_rate": sample_rate}}).encode()
    with tarfile.open(fileobj=buf, mode="w:gz") as tf:
        # .onnx (tiny fake binary)
        onnx_data = b"\x00" * 64
        ti = tarfile.TarInfo(name=onnx_name)
        ti.size = len(onnx_data)
        tf.addfile(ti, io.BytesIO(onnx_data))
        # .onnx.json
        tj = tarfile.TarInfo(name=json_name)
        tj.size = len(meta)
        tf.addfile(tj, io.BytesIO(meta))
    return buf.getvalue()


# ── Voice catalog tests ───────────────────────────────────────────────────────

class TestVoiceCatalog(unittest.TestCase):
    def test_all_entries_have_required_keys(self):
        for vid, info in VOICE_CATALOG.items():
            for key in ("display", "lang", "quality", "tarball", "onnx_name", "sample_rate"):
                self.assertIn(key, info, f"Voice {vid!r} missing key {key!r}")

    def test_tarball_urls_use_correct_base(self):
        for vid, info in VOICE_CATALOG.items():
            expected_url = GITHUB_RELEASE_BASE + info["tarball"]
            # URL must use the verified GitHub release base
            self.assertTrue(
                expected_url.startswith("https://github.com/rhasspy/piper/releases/download/"),
                f"Voice {vid!r} URL does not use expected base: {expected_url}",
            )

    def test_onnx_names_use_dash_locale_format(self):
        """Locale portion (lang-region) must use dashes not underscores.

        Some voice names legitimately contain underscores (e.g.
        en-gb-southern_english_female-low.onnx); the restriction only applies
        to the lang-region prefix (must NOT be en_US-... style).
        """
        for vid, info in VOICE_CATALOG.items():
            onnx = info["onnx_name"]
            parts = onnx.split("-", 2)  # lang, region, rest
            self.assertGreaterEqual(len(parts), 2,
                f"Voice {vid!r} onnx_name {onnx!r} has unexpected format")
            # lang and region parts must not contain underscores
            self.assertNotIn("_", parts[0],
                f"Voice {vid!r} lang part {parts[0]!r} uses underscore")
            self.assertNotIn("_", parts[1],
                f"Voice {vid!r} region part {parts[1]!r} uses underscore")

    def test_sample_rates_are_valid(self):
        for vid, info in VOICE_CATALOG.items():
            self.assertIn(
                info["sample_rate"],
                (16000, 22050),
                f"Voice {vid!r} has unexpected sample_rate {info['sample_rate']}",
            )

    def test_default_voice_is_in_catalog(self):
        self.assertIn(DEFAULT_VOICE, VOICE_CATALOG)

    def test_no_duplicate_onnx_names(self):
        names = [info["onnx_name"] for info in VOICE_CATALOG.values()]
        self.assertEqual(len(names), len(set(names)), "Duplicate onnx_name entries")


# ── Path helpers ──────────────────────────────────────────────────────────────

class TestVoicePathHelpers(unittest.TestCase):
    def test_get_voice_path_uses_onnx_name(self):
        for vid, info in VOICE_CATALOG.items():
            expected = VOICES_DIR / info["onnx_name"]
            self.assertEqual(get_voice_path(vid), expected)

    def test_get_voice_json_path_suffix(self):
        path = get_voice_json_path("en-us-lessac-medium")
        self.assertTrue(str(path).endswith(".onnx.json"))

    def test_is_voice_downloaded_false_when_missing(self):
        self.assertFalse(is_voice_downloaded("en-us-lessac-medium"))

    def test_is_voice_downloaded_true_when_both_present(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmpdir = Path(tmp)
            vid = "en-us-lessac-medium"
            info = VOICE_CATALOG[vid]
            (tmpdir / info["onnx_name"]).write_bytes(b"\x00")
            (tmpdir / (info["onnx_name"] + ".json")).write_text("{}")

            with patch("tts_engine.VOICES_DIR", tmpdir):
                self.assertTrue(is_voice_downloaded(vid))

    def test_is_voice_downloaded_false_when_only_onnx(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmpdir = Path(tmp)
            vid = "en-us-lessac-medium"
            (tmpdir / VOICE_CATALOG[vid]["onnx_name"]).write_bytes(b"\x00")
            with patch("tts_engine.VOICES_DIR", tmpdir):
                self.assertFalse(is_voice_downloaded(vid))


# ── Sample rate helper ────────────────────────────────────────────────────────

class TestGetVoiceSampleRate(unittest.TestCase):
    def test_reads_from_json_when_present(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmpdir = Path(tmp)
            vid = "en-us-ryan-high"
            # Build the expected json filename from the onnx_name
            json_filename = VOICE_CATALOG[vid]["onnx_name"] + ".json"
            json_file = tmpdir / json_filename
            json_file.write_text(json.dumps({"audio": {"sample_rate": 99999}}))
            with patch("tts_engine.VOICES_DIR", tmpdir):
                rate = get_voice_sample_rate(vid)
            self.assertEqual(rate, 99999)

    def test_falls_back_to_catalog_when_json_missing(self):
        with patch("tts_engine.VOICES_DIR", Path("/nonexistent/dir")):
            rate = get_voice_sample_rate("en-us-ryan-high")
        self.assertEqual(rate, VOICE_CATALOG["en-us-ryan-high"]["sample_rate"])


# ── download_voice ────────────────────────────────────────────────────────────

class TestDownloadVoice(unittest.TestCase):
    def test_raises_for_unknown_voice(self):
        with self.assertRaises(ValueError) as ctx:
            download_voice("nonexistent-voice-xyz")
        self.assertIn("nonexistent-voice-xyz", str(ctx.exception))

    def test_extracts_onnx_and_json(self):
        vid = "en-us-lessac-medium"
        info = VOICE_CATALOG[vid]
        tarball_data = _make_tarball_bytes(info["onnx_name"], sample_rate=16000)

        class FakeResponse:
            headers = {"Content-Length": str(len(tarball_data))}
            def read(self, n=-1):
                return tarball_data if n == -1 else tarball_data[:n]
            def __enter__(self): return self
            def __exit__(self, *a): pass

        with tempfile.TemporaryDirectory() as tmp:
            tmpdir = Path(tmp)
            with patch("tts_engine.VOICES_DIR", tmpdir), \
                 patch("urllib.request.urlopen", return_value=FakeResponse()):
                download_voice(vid)
            self.assertTrue((tmpdir / info["onnx_name"]).exists())
            self.assertTrue((tmpdir / (info["onnx_name"] + ".json")).exists())

    def test_progress_callback_called(self):
        vid = "en-us-ryan-medium"
        info = VOICE_CATALOG[vid]
        tarball_data = _make_tarball_bytes(info["onnx_name"])

        class FakeResponse:
            headers = {"Content-Length": str(len(tarball_data))}
            def read(self, n=-1):
                return tarball_data
            def __enter__(self): return self
            def __exit__(self, *a): pass

        calls = []
        with tempfile.TemporaryDirectory() as tmp:
            tmpdir = Path(tmp)
            with patch("tts_engine.VOICES_DIR", tmpdir), \
                 patch("urllib.request.urlopen", return_value=FakeResponse()):
                download_voice(vid, progress_cb=lambda done, total: calls.append((done, total)))
        self.assertGreater(len(calls), 0)
        # Final call should report complete
        self.assertEqual(calls[-1][0], calls[-1][1])

    def test_tarball_url_matches_catalog(self):
        """download_voice must request the URL from the catalog."""
        vid = "en-us-lessac-medium"
        info = VOICE_CATALOG[vid]
        expected_url = GITHUB_RELEASE_BASE + info["tarball"]
        tarball_data = _make_tarball_bytes(info["onnx_name"])

        opened_urls = []

        class FakeResponse:
            headers = {"Content-Length": str(len(tarball_data))}
            def read(self, n=-1): return tarball_data
            def __enter__(self): return self
            def __exit__(self, *a): pass

        def fake_urlopen(req, timeout=None):
            opened_urls.append(req.full_url)
            return FakeResponse()

        with tempfile.TemporaryDirectory() as tmp:
            with patch("tts_engine.VOICES_DIR", Path(tmp)), \
                 patch("urllib.request.urlopen", side_effect=fake_urlopen):
                download_voice(vid)

        self.assertEqual(len(opened_urls), 1)
        self.assertEqual(opened_urls[0], expected_url)


# ── available_tts_engine ──────────────────────────────────────────────────────

class TestAvailableTTSEngine(unittest.TestCase):
    def test_returns_piper_when_piper_found(self):
        with patch("shutil.which", side_effect=lambda x: "/usr/bin/piper" if x == "piper" else None):
            self.assertEqual(available_tts_engine(), "piper")

    def test_returns_espeak_when_no_piper(self):
        with patch("shutil.which", side_effect=lambda x: "/usr/bin/espeak-ng" if x == "espeak-ng" else None):
            self.assertEqual(available_tts_engine(), "espeak")

    def test_returns_none_when_neither(self):
        with patch("shutil.which", return_value=None):
            self.assertEqual(available_tts_engine(), "none")


# ── TTSEngine ─────────────────────────────────────────────────────────────────

class TestTTSEngine(unittest.TestCase):
    def _make_engine(self, enabled=True, engine="piper", voice="en-us-lessac-medium"):
        return TTSEngine(_fake_config(enabled=enabled, engine=engine, voice=voice))

    def test_speak_noop_when_disabled(self):
        eng = TTSEngine(_fake_config(enabled=False))
        # Queue should stay empty
        eng.speak("hello")
        self.assertEqual(eng._q.qsize(), 0)
        eng.shutdown()

    def test_speak_queues_text_when_enabled(self):
        eng = TTSEngine(_fake_config(enabled=True))
        with patch.object(eng, "_do_speak") as mock_speak:
            eng._q.put("test text")
            time.sleep(0.2)
        eng.shutdown()

    def test_stop_drains_queue(self):
        eng = self._make_engine()
        for _ in range(10):
            eng._q.put("queued item")
        eng.stop()
        self.assertEqual(eng._q.qsize(), 0)
        eng.shutdown()

    def test_is_speaking_false_initially(self):
        eng = self._make_engine()
        self.assertFalse(eng.is_speaking)
        eng.shutdown()

    def test_on_started_called(self):
        eng = self._make_engine()
        started_texts = []
        finished_flag = threading.Event()

        eng.on_started = lambda t: started_texts.append(t)
        eng.on_finished = finished_flag.set

        with patch.object(eng, "_speak_piper", side_effect=lambda t, v: None), \
             patch("shutil.which", return_value="/usr/bin/piper"):
            eng._q.put("hello world")
            finished_flag.wait(timeout=3.0)

        self.assertEqual(started_texts, ["hello world"])
        eng.shutdown()

    def test_on_finished_called_after_speak(self):
        eng = self._make_engine()
        done = threading.Event()
        eng.on_finished = done.set

        with patch.object(eng, "_speak_piper", side_effect=lambda t, v: None), \
             patch("shutil.which", return_value="/usr/bin/piper"):
            eng._q.put("test")
            self.assertTrue(done.wait(timeout=3.0))
        eng.shutdown()

    def test_stop_sets_speaking_false(self):
        eng = self._make_engine()
        with eng._lock:
            eng._speaking = True
        eng.stop()
        self.assertFalse(eng.is_speaking)
        eng.shutdown()

    def test_fallback_to_espeak_when_piper_missing(self):
        eng = self._make_engine()
        espeak_called = threading.Event()

        def fake_espeak(text):
            espeak_called.set()

        with patch.object(eng, "_speak_espeak", side_effect=fake_espeak), \
             patch("shutil.which", side_effect=lambda x: None if x == "piper" else "/usr/bin/espeak-ng"):
            eng._q.put("fallback test")
            espeak_called.wait(timeout=3.0)

        self.assertTrue(espeak_called.is_set())
        eng.shutdown()

    def test_fallback_to_espeak_when_voice_missing(self):
        eng = self._make_engine()
        espeak_called = threading.Event()

        def fake_espeak(text):
            espeak_called.set()

        with patch.object(eng, "_speak_espeak", side_effect=fake_espeak), \
             patch("shutil.which", return_value="/usr/bin/piper"), \
             patch("tts_engine.get_voice_path", return_value=Path("/nonexistent/voice.onnx")):
            eng._q.put("missing voice")
            espeak_called.wait(timeout=3.0)

        self.assertTrue(espeak_called.is_set())
        eng.shutdown()

    def test_speak_test_is_blocking(self):
        eng = self._make_engine()
        call_order = []

        def slow_espeak(text):
            time.sleep(0.05)
            call_order.append("speak")

        with patch.object(eng, "_speak_espeak", side_effect=slow_espeak), \
             patch("shutil.which", side_effect=lambda x: "/usr/bin/espeak-ng" if x == "espeak-ng" else None):
            eng.speak_test("en-us-lessac-medium", text="hi")
            call_order.append("after")

        self.assertEqual(call_order, ["speak", "after"])
        eng.shutdown()


if __name__ == "__main__":
    unittest.main()
