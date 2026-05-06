"""
TTS Responder — watches a named pipe for AI-generated responses and speaks them.

One ResponseListener is created per routing target that has `response_pipe` set.
The listener runs as a daemon thread and remains alive for the lifetime of the app.
"""

import os
import threading
import time
from typing import Callable, Optional


class ResponseListener(threading.Thread):
    """
    Reads newline-delimited text from a FIFO and passes each line to the TTS engine.

    Args:
        pipe_path:      Path to the FIFO the AI agent writes responses to.
        tts_speak:      Callable that accepts a str and queues it for TTS playback.
        on_response:    Optional callback fired with each response line (for UI).
        label:          Human-readable target label (for logging).
    """

    def __init__(
        self,
        pipe_path: str,
        tts_speak: Callable[[str], None],
        on_response: Optional[Callable[[str], None]] = None,
        label: str = "",
    ):
        super().__init__(daemon=True, name=f"tts-resp-{label or pipe_path}")
        self.pipe_path = os.path.expanduser(pipe_path)
        self.tts_speak = tts_speak
        self.on_response = on_response
        self.label = label
        self.running = True

    def run(self):
        print(f"[ResponseListener] Watching {self.pipe_path!r} for AI responses")
        while self.running:
            # Wait for the FIFO to exist (agent may not have started yet)
            if not os.path.exists(self.pipe_path):
                time.sleep(1.0)
                continue

            try:
                # Open in blocking read mode; re-opens on EOF (agent closed its end)
                with open(self.pipe_path, "r", encoding="utf-8", errors="replace") as f:
                    for line in f:
                        if not self.running:
                            break
                        text = line.strip()
                        if not text:
                            continue
                        print(f"[ResponseListener:{self.label}] → TTS: {text[:80]!r}")
                        self.tts_speak(text, self.label)
                        if self.on_response:
                            try:
                                self.on_response(text)
                            except Exception:
                                pass
            except OSError as e:
                if self.running:
                    print(f"[ResponseListener] FIFO error: {e}")
                    time.sleep(0.5)

    def stop(self):
        self.running = False
