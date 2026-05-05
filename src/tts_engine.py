"""
TTS Engine — Piper neural TTS with espeak-ng fallback.

Manages voice model catalog, downloads, playback queue, and stop control.
All public methods are thread-safe.
"""

import os
import queue
import shutil
import subprocess
import threading
import urllib.request
from pathlib import Path
from typing import Callable, Optional

# ── Voice catalog ─────────────────────────────────────────────────────────────
# HuggingFace rhasspy/piper-voices v1.0.0 — (name, display, sample_rate, path)
VOICE_CATALOG: dict = {
    "en_US-lessac-medium": {
        "display": "Lessac — US English, Medium",
        "lang": "en_US",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/medium/en_US-lessac-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json",
    },
    "en_US-amy-medium": {
        "display": "Amy — US English, Medium",
        "lang": "en_US",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/amy/medium/en_US-amy-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/amy/medium/en_US-amy-medium.onnx.json",
    },
    "en_US-ryan-high": {
        "display": "Ryan — US English, High",
        "lang": "en_US",
        "quality": "high",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/high/en_US-ryan-high.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/high/en_US-ryan-high.onnx.json",
    },
    "en_US-joe-medium": {
        "display": "Joe — US English, Medium",
        "lang": "en_US",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/joe/medium/en_US-joe-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/joe/medium/en_US-joe-medium.onnx.json",
    },
    "en_GB-alan-medium": {
        "display": "Alan — GB English, Medium",
        "lang": "en_GB",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_GB/alan/medium/en_GB-alan-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_GB/alan/medium/en_GB-alan-medium.onnx.json",
    },
    "en_GB-jenny_dioco-medium": {
        "display": "Jenny — GB English, Medium",
        "lang": "en_GB",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_GB/jenny_dioco/medium/en_GB-jenny_dioco-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_GB/jenny_dioco/medium/en_GB-jenny_dioco-medium.onnx.json",
    },
    "en_US-arctic-medium": {
        "display": "Arctic — US English, Medium (multi-speaker)",
        "lang": "en_US",
        "quality": "medium",
        "sample_rate": 22050,
        "url_onnx": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/arctic/medium/en_US-arctic-medium.onnx",
        "url_json": "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/arctic/medium/en_US-arctic-medium.onnx.json",
    },
}

VOICES_DIR = Path.home() / ".local" / "share" / "whisper-wayland" / "piper-voices"
SAMPLE_TEXT = "Hello! This is how I sound. I'm ready to be your voice assistant."


# ── Download helpers ──────────────────────────────────────────────────────────

def get_voice_path(voice_id: str) -> Path:
    return VOICES_DIR / f"{voice_id}.onnx"


def is_voice_downloaded(voice_id: str) -> bool:
    p = get_voice_path(voice_id)
    json_p = p.with_suffix(".onnx.json")
    return p.exists() and json_p.exists()


def download_voice(
    voice_id: str,
    progress_cb: Optional[Callable[[int, int], None]] = None,
) -> None:
    """Download .onnx and .onnx.json for voice_id. Raises on failure."""
    info = VOICE_CATALOG.get(voice_id)
    if not info:
        raise ValueError(f"Unknown voice: {voice_id}")
    VOICES_DIR.mkdir(parents=True, exist_ok=True)

    def _dl(url: str, dest: Path):
        def _reporthook(block, bsize, total):
            if progress_cb and total > 0:
                progress_cb(block * bsize, total)
        urllib.request.urlretrieve(url, dest, reporthook=_reporthook)

    onnx_path = VOICES_DIR / f"{voice_id}.onnx"
    json_path = VOICES_DIR / f"{voice_id}.onnx.json"
    _dl(info["url_onnx"], onnx_path)
    _dl(info["url_json"], json_path)


def available_tts_engine() -> str:
    """Return 'piper', 'espeak', or 'none'."""
    if shutil.which("piper"):
        return "piper"
    if shutil.which("espeak-ng"):
        return "espeak"
    return "none"


# ── TTS Engine ────────────────────────────────────────────────────────────────

class TTSEngine:
    """
    Thread-safe TTS engine.

    speak(text) — queue text for playback (non-blocking)
    stop()      — kill active playback and clear queue
    is_speaking — True while audio is playing

    Callbacks (called from worker thread; schedule to Qt main thread if needed):
      on_started(text: str)
      on_finished()
    """

    def __init__(self, config):
        self.config = config
        self._lock = threading.Lock()
        self._procs: list = []       # active subprocesses
        self._speaking = False
        self._q: queue.Queue = queue.Queue()
        self._stopped = threading.Event()
        self.on_started: Optional[Callable[[str], None]] = None
        self.on_finished: Optional[Callable[[], None]] = None

        self._worker = threading.Thread(target=self._run, daemon=True, name="tts-worker")
        self._worker.start()

    # ── Public API ────────────────────────────────────────────────────────────

    def speak(self, text: str) -> None:
        if not self.config.get("tts_enabled", False):
            return
        text = text.strip()
        if text:
            self._q.put(text)

    def stop(self) -> None:
        """Immediately halt all playback and clear the queue."""
        with self._lock:
            for proc in self._procs:
                try:
                    proc.kill()
                except Exception:
                    pass
            self._procs.clear()
        # Drain queue
        while True:
            try:
                self._q.get_nowait()
            except queue.Empty:
                break
        with self._lock:
            if self._speaking:
                self._speaking = False
                if self.on_finished:
                    try:
                        self.on_finished()
                    except Exception:
                        pass

    def shutdown(self) -> None:
        self.stop()
        self._q.put(None)   # sentinel

    @property
    def is_speaking(self) -> bool:
        with self._lock:
            return self._speaking

    # ── Worker loop ───────────────────────────────────────────────────────────

    def _run(self):
        while True:
            item = self._q.get()
            if item is None:
                break
            self._do_speak(item)

    def _do_speak(self, text: str):
        with self._lock:
            self._speaking = True
        if self.on_started:
            try:
                self.on_started(text)
            except Exception:
                pass
        try:
            engine = self.config.get("tts_engine", "piper")
            voice = self.config.get("tts_voice", "en_US-lessac-medium")
            if engine == "piper" and shutil.which("piper"):
                self._speak_piper(text, voice)
            elif shutil.which("espeak-ng"):
                self._speak_espeak(text)
            else:
                print(f"[TTS] No TTS engine available (piper/espeak-ng not found)")
        except Exception as e:
            print(f"[TTS] Playback error: {e}")
        finally:
            with self._lock:
                self._speaking = False
                self._procs.clear()
            if self.on_finished:
                try:
                    self.on_finished()
                except Exception:
                    pass

    def _speak_piper(self, text: str, voice: str):
        voice_path = get_voice_path(voice)
        if not voice_path.exists():
            print(f"[TTS] Voice not downloaded ({voice}), falling back to espeak-ng")
            self._speak_espeak(text)
            return
        info = VOICE_CATALOG.get(voice, {})
        rate = str(info.get("sample_rate", 22050))

        piper = subprocess.Popen(
            ["piper", "--model", str(voice_path), "--output_raw"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        aplay = subprocess.Popen(
            ["aplay", "-r", rate, "-f", "S16_LE", "-t", "raw", "-"],
            stdin=piper.stdout,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        piper.stdout.close()  # allow piper to receive SIGPIPE if aplay dies
        with self._lock:
            self._procs = [piper, aplay]
        try:
            piper.stdin.write(text.encode("utf-8"))
            piper.stdin.close()
        except BrokenPipeError:
            pass
        aplay.wait()
        piper.wait()

    def _speak_espeak(self, text: str):
        proc = subprocess.Popen(
            ["espeak-ng", "-s", "150", "--", text],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        with self._lock:
            self._procs = [proc]
        proc.wait()

    # ── One-shot test (blocking, used by settings UI) ─────────────────────────

    def speak_test(self, voice: str, text: str = SAMPLE_TEXT) -> None:
        """Blocking test playback. Returns when audio finishes or is stopped."""
        engine = self.config.get("tts_engine", "piper")
        with self._lock:
            self._speaking = True
        try:
            if engine == "piper" and shutil.which("piper"):
                self._speak_piper(text, voice)
            elif shutil.which("espeak-ng"):
                self._speak_espeak(text)
        finally:
            with self._lock:
                self._speaking = False
                self._procs.clear()
