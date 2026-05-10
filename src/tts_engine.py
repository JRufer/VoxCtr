"""
TTS Engine — Piper neural TTS with espeak-ng fallback.

Voice models are downloaded as GitHub release tarballs from the official
rhasspy/piper repository, extracted locally, and called via the `piper`
binary piped through `aplay`.

Verified source: https://github.com/rhasspy/piper/releases/tag/v0.0.2
"""

import io
import json
import os
import queue
import shutil
import subprocess
import tarfile
import tempfile
import threading
import urllib.request
from pathlib import Path
from typing import Callable, Optional

# ── Voice catalog ─────────────────────────────────────────────────────────────
# All entries verified reachable via HTTP HEAD at build time.
# tarball_name  : filename on the GitHub release page
# onnx_name     : filename of the .onnx model inside the tarball (dash-format)
# sample_rate   : extracted from each voice's .onnx.json "audio.sample_rate"

GITHUB_RELEASE_BASE = "https://github.com/rhasspy/piper/releases/download/v0.0.2/"

# ♀ = female voice   ♂ = male voice
VOICE_CATALOG: dict = {
    # ── Female voices ─────────────────────────────────────────────────────────
    "en-us-libritts-high": {
        "display": "LibriTTS ♀ — US English, High  (~115 MB)",
        "lang": "en-US",
        "quality": "high",
        "tarball": "voice-en-us-libritts-high.tar.gz",
        "onnx_name": "en-us-libritts-high.onnx",
        "sample_rate": 22050,
    },
    "en-us-amy-low": {
        "display": "Amy ♀ — US English, Low  (~5 MB)",
        "lang": "en-US",
        "quality": "low",
        "tarball": "voice-en-us-amy-low.tar.gz",
        "onnx_name": "en-us-amy-low.onnx",
        "sample_rate": 16000,
    },
    "en-us-kathleen-low": {
        "display": "Kathleen ♀ — US English, Low  (~5 MB)",
        "lang": "en-US",
        "quality": "low",
        "tarball": "voice-en-us-kathleen-low.tar.gz",
        "onnx_name": "en-us-kathleen-low.onnx",
        "sample_rate": 16000,
    },
    "en-gb-southern_english_female-low": {
        "display": "Southern English ♀ — GB English, Low  (~5 MB)",
        "lang": "en-GB",
        "quality": "low",
        "tarball": "voice-en-gb-southern_english_female-low.tar.gz",
        "onnx_name": "en-gb-southern_english_female-low.onnx",
        "sample_rate": 16000,
    },
    # ── Male voices ───────────────────────────────────────────────────────────
    "en-us-ryan-high": {
        "display": "Ryan ♂ — US English, High  (~100 MB)",
        "lang": "en-US",
        "quality": "high",
        "tarball": "voice-en-us-ryan-high.tar.gz",
        "onnx_name": "en-us-ryan-high.onnx",
        "sample_rate": 22050,
    },
    "en-us-ryan-medium": {
        "display": "Ryan ♂ — US English, Medium  (~55 MB)",
        "lang": "en-US",
        "quality": "medium",
        "tarball": "voice-en-us-ryan-medium.tar.gz",
        "onnx_name": "en-us-ryan-medium.onnx",
        "sample_rate": 22050,
    },
    "en-us-ryan-low": {
        "display": "Ryan ♂ — US English, Low  (~5 MB)",
        "lang": "en-US",
        "quality": "low",
        "tarball": "voice-en-us-ryan-low.tar.gz",
        "onnx_name": "en-us-ryan-low.onnx",
        "sample_rate": 16000,
    },
    "en-us-lessac-medium": {
        "display": "Lessac ♂ — US English, Medium  (~55 MB)",
        "lang": "en-US",
        "quality": "medium",
        "tarball": "voice-en-us-lessac-medium.tar.gz",
        "onnx_name": "en-us-lessac-medium.onnx",
        "sample_rate": 16000,
    },
    "en-us-lessac-low": {
        "display": "Lessac ♂ — US English, Low  (~5 MB)",
        "lang": "en-US",
        "quality": "low",
        "tarball": "voice-en-us-lessac-low.tar.gz",
        "onnx_name": "en-us-lessac-low.onnx",
        "sample_rate": 16000,
    },
    "en-us-danny-low": {
        "display": "Danny ♂ — US English, Low  (~5 MB)",
        "lang": "en-US",
        "quality": "low",
        "tarball": "voice-en-us-danny-low.tar.gz",
        "onnx_name": "en-us-danny-low.onnx",
        "sample_rate": 16000,
    },
    "en-gb-alan-low": {
        "display": "Alan ♂ — GB English, Low  (~5 MB)",
        "lang": "en-GB",
        "quality": "low",
        "tarball": "voice-en-gb-alan-low.tar.gz",
        "onnx_name": "en-gb-alan-low.onnx",
        "sample_rate": 16000,
    },
}

DEFAULT_VOICE = "en-us-lessac-medium"
VOICES_DIR = Path.home() / ".local" / "share" / "voxctl" / "piper-voices"
PIPER_LOCAL_DIR = Path.home() / ".local" / "share" / "voxctl" / "piper"
SAMPLE_TEXT = "Hello! This is how I sound. I am ready to be your voice assistant."


def _find_piper_binary() -> str | None:
    """Return the path to the piper binary, checking user-local install first."""
    local_piper = PIPER_LOCAL_DIR / "piper"
    if local_piper.is_file() and os.access(local_piper, os.X_OK):
        return str(local_piper)
    return shutil.which("piper")


# ── Download helpers ──────────────────────────────────────────────────────────

def get_voice_path(voice_id: str) -> Path:
    """Return the expected path of the .onnx file for a given voice_id."""
    info = VOICE_CATALOG.get(voice_id, {})
    onnx_name = info.get("onnx_name", f"{voice_id}.onnx")
    return VOICES_DIR / onnx_name


def get_voice_json_path(voice_id: str) -> Path:
    """Return the expected path of the .onnx.json config file."""
    return get_voice_path(voice_id).with_suffix(".onnx.json")


def is_voice_downloaded(voice_id: str) -> bool:
    """True only when both .onnx and .onnx.json are present on disk."""
    return get_voice_path(voice_id).exists() and get_voice_json_path(voice_id).exists()


def get_voice_sample_rate(voice_id: str) -> int:
    """
    Return the audio sample rate for a voice.
    Reads from the cached .onnx.json if available, falls back to catalog value.
    """
    json_path = get_voice_json_path(voice_id)
    if json_path.exists():
        try:
            with open(json_path) as f:
                meta = json.load(f)
            return int(meta.get("audio", {}).get("sample_rate", 22050))
        except Exception:
            pass
    return VOICE_CATALOG.get(voice_id, {}).get("sample_rate", 22050)


def download_voice(
    voice_id: str,
    progress_cb: Optional[Callable[[int, int], None]] = None,
) -> None:
    """
    Download and extract the piper voice tarball for *voice_id*.

    Files are extracted from the GitHub release tarball directly into
    VOICES_DIR.  Both .onnx and .onnx.json are written atomically using a
    temp file so a partial download never leaves a corrupt model on disk.

    Raises ValueError for unknown voice_id, OSError / urllib.error on failure.
    """
    info = VOICE_CATALOG.get(voice_id)
    if not info:
        raise ValueError(f"Unknown voice id: {voice_id!r}. "
                         f"Valid ids: {list(VOICE_CATALOG)}")

    VOICES_DIR.mkdir(parents=True, exist_ok=True)
    url = GITHUB_RELEASE_BASE + info["tarball"]

    # Stream the tarball, reporting progress.
    # GitHub redirects to a CDN; the CDN returns Content-Length on the final
    # response.  Call progress_cb unconditionally so the UI can switch to an
    # indeterminate spinner when total is unknown.
    req = urllib.request.Request(url, headers={"User-Agent": "voxctl/1.0"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        raw_len = resp.headers.get("Content-Length") or resp.headers.get("content-length")
        total = int(raw_len) if raw_len and raw_len.strip().isdigit() else 0
        downloaded = 0
        chunks = []
        while True:
            chunk = resp.read(65536)
            if not chunk:
                break
            chunks.append(chunk)
            downloaded += len(chunk)
            if progress_cb:
                progress_cb(downloaded, total)

    raw = b"".join(chunks)

    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tf:
        for member in tf.getmembers():
            name = os.path.basename(member.name)
            if not name.endswith((".onnx", ".onnx.json")):
                continue
            dest = VOICES_DIR / name
            # Atomic write via temp file
            tmp_fd, tmp_path = tempfile.mkstemp(dir=VOICES_DIR, suffix=".tmp")
            try:
                with os.fdopen(tmp_fd, "wb") as out:
                    f = tf.extractfile(member)
                    if f:
                        shutil.copyfileobj(f, out)
                os.replace(tmp_path, dest)
            except Exception:
                try:
                    os.unlink(tmp_path)
                except OSError:
                    pass
                raise


def available_tts_engine() -> str:
    """Return 'piper', 'espeak', or 'none' depending on what is available."""
    if _find_piper_binary():
        return "piper"
    if shutil.which("espeak-ng"):
        return "espeak"
    return "none"


# ── TTS Engine ────────────────────────────────────────────────────────────────

class TTSEngine:
    """
    Thread-safe TTS engine.

    speak(text) — queue text for playback (non-blocking).
    stop()      — immediately kill active playback and drain the queue.
    is_speaking — True while audio subprocess is running.

    Callbacks (called from the worker thread; use QTimer.singleShot to
    marshal to the Qt main thread from UI code):
      on_started(text: str)
      on_finished()
    """

    def __init__(self, config):
        self.config = config
        self._lock = threading.Lock()
        self._procs: list = []
        self._speaking = False
        self._q: queue.Queue = queue.Queue()
        # on_started(text, source_label) — source_label is the routing target label
        self.on_started: Optional[Callable] = None
        self.on_finished: Optional[Callable[[], None]] = None

        self._worker = threading.Thread(
            target=self._run, daemon=True, name="tts-worker"
        )
        self._worker.start()

    # ── Public API ────────────────────────────────────────────────────────────

    def speak(self, text: str, source_label: str = "") -> None:
        """Queue *text* for TTS playback. No-op if TTS is disabled in config.

        Args:
            text: Text to speak.
            source_label: Human-readable name of the routing target that produced
                          this response (shown in the TTS overlay badge).
        """
        if not self.config.get("tts_enabled", False):
            return
        text = text.strip()
        if text:
            self._q.put((text, source_label))

    def stop(self) -> None:
        """Kill active subprocesses and drain the pending queue immediately."""
        with self._lock:
            for proc in self._procs:
                try:
                    proc.kill()
                except Exception:
                    pass
            self._procs.clear()
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
        """Stop playback and terminate the worker thread."""
        self.stop()
        self._q.put(None)  # sentinel

    @property
    def is_speaking(self) -> bool:
        with self._lock:
            return self._speaking

    # ── Worker ────────────────────────────────────────────────────────────────

    def _run(self):
        while True:
            item = self._q.get()
            if item is None:
                break
            text, source_label = item if isinstance(item, tuple) else (item, "")
            self._do_speak(text, source_label)

    def _do_speak(self, text: str, source_label: str = ""):
        with self._lock:
            self._speaking = True
        if self.on_started:
            try:
                self.on_started(text, source_label)
            except Exception:
                pass
        try:
            engine = self.config.get("tts_engine", "piper")
            voice = self.config.get("tts_voice", DEFAULT_VOICE)
            if engine == "piper" and _find_piper_binary():
                self._speak_piper(text, voice)
            elif shutil.which("espeak-ng"):
                self._speak_espeak(text)
            else:
                print("[TTS] No TTS engine available (piper / espeak-ng not on PATH)")
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

    def _speak_piper(self, text: str, voice: str, verbose: bool = False):
        voice_path = get_voice_path(voice)
        if not voice_path.exists():
            print(f"[TTS] Voice model not found ({voice_path}); falling back to espeak-ng")
            self._speak_espeak(text, verbose=verbose)
            return

        rate = str(get_voice_sample_rate(voice))
        stderr_dest = subprocess.PIPE if verbose else subprocess.DEVNULL

        piper_bin = _find_piper_binary() or "piper"
        piper = subprocess.Popen(
            [piper_bin, "--model", str(voice_path), "--output_raw"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr_dest,
        )

        try:
            aplay = subprocess.Popen(
                ["aplay", "-r", rate, "-f", "S16_LE", "-t", "raw", "-"],
                stdin=piper.stdout,
                stdout=subprocess.DEVNULL,
                stderr=stderr_dest,
            )
        except FileNotFoundError:
            piper.kill()
            piper.wait()
            print("[TTS] aplay not found; falling back to espeak-ng")
            self._speak_espeak(text, verbose=verbose)
            return

        piper.stdout.close()

        with self._lock:
            self._procs = [piper, aplay]

        try:
            piper.stdin.write(text.encode("utf-8"))
            piper.stdin.close()
        except BrokenPipeError:
            pass

        aplay.wait()
        piper.wait()

        piper_err = (piper.stderr.read() if piper.stderr else b"").decode(errors="replace").strip()
        aplay_err = (aplay.stderr.read() if aplay.stderr else b"").decode(errors="replace").strip()
        if verbose:
            print(f"[TTS test] piper exit={piper.returncode}  stderr: {piper_err or '(none)'}")
            print(f"[TTS test] aplay  exit={aplay.returncode}  stderr: {aplay_err or '(none)'}")

        # exit 127 = shared library / binary not executable; fall back rather than silently fail
        if piper.returncode == 127:
            print("[TTS] piper failed to start (missing shared lib?); falling back to espeak-ng")
            self._speak_espeak(text, verbose=verbose)

    def _speak_espeak(self, text: str, verbose: bool = False):
        stderr_dest = subprocess.PIPE if verbose else subprocess.DEVNULL
        proc = subprocess.Popen(
            ["espeak-ng", "-s", "150", "--", text],
            stdout=subprocess.DEVNULL,
            stderr=stderr_dest,
        )
        with self._lock:
            self._procs = [proc]
        proc.wait()
        if verbose:
            err = (proc.stderr.read() if proc.stderr else b"").decode(errors="replace").strip()
            print(f"[TTS test] espeak-ng exit={proc.returncode}  stderr: {err or '(none)'}")

    # ── Blocking test (used by settings UI preview) ───────────────────────────

    def speak_test(self, voice: str, text: str = SAMPLE_TEXT) -> None:
        """
        Blocking test playback — returns when audio finishes or is stopped.
        Used by the Settings → Voice Out → Test Voice button.
        Raises RuntimeError if no usable TTS engine is found.
        """
        piper_bin   = _find_piper_binary()
        espeak_bin  = shutil.which("espeak-ng")
        aplay_bin   = shutil.which("aplay")
        voice_path  = get_voice_path(voice)

        print(f"[TTS test] voice={voice!r}  model={voice_path}  exists={voice_path.exists()}")
        print(f"[TTS test] piper={piper_bin}  aplay={aplay_bin}  espeak-ng={espeak_bin}")

        with self._lock:
            self._speaking = True
        try:
            if piper_bin and aplay_bin and voice_path.exists():
                print("[TTS test] using piper")
                self._speak_piper(text, voice, verbose=True)
            elif espeak_bin:
                print("[TTS test] using espeak-ng (piper/aplay/model unavailable)")
                self._speak_espeak(text, verbose=True)
            else:
                raise RuntimeError(
                    "No TTS engine available.\n"
                    f"  piper binary : {'✓' if piper_bin else '✗ not found'}\n"
                    f"  aplay binary : {'✓' if aplay_bin else '✗ not found (alsa-utils)'}\n"
                    f"  voice model  : {'✓' if voice_path.exists() else '✗ not downloaded'}\n"
                    f"  espeak-ng    : {'✓' if espeak_bin else '✗ not found'}"
                )
        finally:
            with self._lock:
                self._speaking = False
                self._procs.clear()
