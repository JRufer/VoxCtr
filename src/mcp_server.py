"""
Whisper-Wayland MCP Server

Exposes the app as an MCP (Model Context Protocol) tool server so AI clients
(Claude Desktop, custom agents) can trigger voice recording and TTS playback.

Transport: Unix domain socket at /tmp/whisper-wayland-mcp.sock
Protocol:  JSON-RPC 2.0 (MCP spec)

Tools exposed:
  transcribe_voice()              → records the user's voice and returns transcript
  speak_text(text)                → speaks text aloud via TTS
  get_status()                    → {"recording": bool, "speaking": bool}
  inject_text(text)               → types text into the active window
  cancel_tts()                    → stops TTS playback immediately
  get_last_transcript()           → returns the most recent transcription
  list_voices()                   → lists TTS voices with download status
  set_voice(voice_id)             → switches the active TTS voice
  transcribe_file(path)           → transcribes an audio file from disk
  get_config(key)                 → reads a config value
  set_config(key, value)          → writes a config value
  get_history(n)                  → returns recent transcriptions

Claude Desktop integration — add to ~/claude_desktop_config.json:
  {
    "mcpServers": {
      "whisper-wayland": {
        "command": "socat",
        "args": ["STDIO", "UNIX-CONNECT:/tmp/whisper-wayland-mcp.sock"]
      }
    }
  }
"""

import json
import os
import queue
import socket
import threading
import time
from typing import Any, Callable, Optional

SOCKET_PATH = "/tmp/whisper-wayland-mcp.sock"

_TOOL_LIST = {
    "tools": [
        # ── Voice I/O ─────────────────────────────────────────────────────────
        {
            "name": "transcribe_voice",
            "description": (
                "Records the user's voice through their microphone and returns the transcribed text.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  Calling this tool immediately activates the user's microphone. The user speaks, "
                "and when they stop (or the timeout is reached) the audio is transcribed locally "
                "using Whisper and the text is returned.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - Whenever you need a spoken response or clarification from the user.\n"
                "  - To conduct a voice-driven conversation: speak a question with speak_text, "
                "then call transcribe_voice to capture the answer.\n"
                "\n"
                "WHEN NOT TO USE:\n"
                "  - Do not call while get_status shows recording=true.\n"
                "  - Do not call while get_status shows speaking=true; wait for TTS to finish.\n"
                "  - Do not loop rapidly on empty results.\n"
                "\n"
                "PARAMETERS:\n"
                "  timeout_seconds (number, optional, default 15): Maximum seconds to wait.\n"
                "\n"
                "RETURN VALUE:\n"
                "  Plain-text transcript. Returns '(no speech detected)' if silent."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "timeout_seconds": {
                        "type": "number",
                        "description": "Maximum seconds to wait for speech. Default 15.",
                    }
                },
            },
        },
        {
            "name": "speak_text",
            "description": (
                "Converts text to speech and plays it aloud through the user's speakers.\n"
                "\n"
                "Returns as soon as the text is queued — does not block until playback finishes. "
                "Use get_status to poll speaking=false before calling transcribe_voice.\n"
                "\n"
                "PARAMETERS:\n"
                "  text (string, required): Plain prose — no markdown, no code, no URLs.\n"
                "\n"
                "RETURN VALUE:\n"
                "  'spoken' when successfully queued."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Plain-text content to speak aloud.",
                    }
                },
                "required": ["text"],
            },
        },
        {
            "name": "get_status",
            "description": (
                "Returns the current state of the voice interface.\n"
                "\n"
                "RETURN VALUE:\n"
                "  JSON object: {\"recording\": bool, \"speaking\": bool}\n"
                "  recording — true while the mic is open\n"
                "  speaking  — true while TTS audio is playing"
            ),
            "inputSchema": {"type": "object", "properties": {}},
        },
        # ── Text injection ────────────────────────────────────────────────────
        {
            "name": "inject_text",
            "description": (
                "Types text directly into the currently focused window using the system keyboard "
                "input method (wtype on Wayland, xdotool on X11).\n"
                "\n"
                "HOW IT WORKS:\n"
                "  The text is injected character-by-character as synthetic keystrokes into "
                "whatever window currently has keyboard focus. This is equivalent to the user "
                "typing the text manually.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - To deliver a transcribed voice command directly into a text field, "
                "terminal, or editor without the user having to copy-paste.\n"
                "  - To fill forms or compose messages hands-free after transcribing the user's "
                "dictation.\n"
                "  - When the user has explicitly asked for voice-to-text insertion.\n"
                "\n"
                "WHEN NOT TO USE:\n"
                "  - Do not inject into unknown windows without the user's awareness — always "
                "confirm the target before injecting.\n"
                "  - Do not inject passwords or sensitive data.\n"
                "  - Prefer clipboard delivery for very long texts.\n"
                "\n"
                "PARAMETERS:\n"
                "  text (string, required): The text to type. Plain text only.\n"
                "\n"
                "RETURN VALUE:\n"
                "  'injected' on success, or 'error: <reason>' if the injection method is "
                "unavailable (wtype / xdotool not installed, or no Wayland/X11 display)."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to type into the focused window.",
                    }
                },
                "required": ["text"],
            },
        },
        # ── TTS control ───────────────────────────────────────────────────────
        {
            "name": "cancel_tts",
            "description": (
                "Immediately stops any TTS audio that is currently playing and clears the "
                "entire playback queue.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  Kills the active piper/espeak-ng subprocess and discards any queued "
                "utterances. Returns instantly — no audio will play after this call until "
                "speak_text is called again.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - When the user interrupts a long TTS response (e.g., says 'stop').\n"
                "  - Before starting a new speak_text sequence when old audio may still be "
                "queued from a previous turn.\n"
                "  - To recover from a stuck TTS state.\n"
                "\n"
                "RETURN VALUE:\n"
                "  'stopped' always (even if no audio was playing)."
            ),
            "inputSchema": {"type": "object", "properties": {}},
        },
        # ── Transcript history ────────────────────────────────────────────────
        {
            "name": "get_last_transcript",
            "description": (
                "Returns the text from the most recent transcription session, regardless of "
                "how it was triggered (hotkey, MCP, DBus).\n"
                "\n"
                "WHEN TO USE:\n"
                "  - To retrieve what the user just said after a hotkey-triggered recording.\n"
                "  - To confirm the transcript before acting on it.\n"
                "  - When reconnecting mid-session to pick up the last user utterance.\n"
                "\n"
                "RETURN VALUE:\n"
                "  The last transcribed string, or '(no transcript yet)' if no recording has "
                "completed in this session."
            ),
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "get_history",
            "description": (
                "Returns recent transcription entries from the current session as a JSON array.\n"
                "\n"
                "PARAMETERS:\n"
                "  n (integer, optional, default 10): Number of recent entries to return. "
                "Capped at 100.\n"
                "\n"
                "RETURN VALUE:\n"
                "  JSON array of plain-text transcript strings, oldest first. Empty array if "
                "no recordings have completed this session."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "n": {
                        "type": "integer",
                        "description": "Number of recent transcriptions to return (default 10, max 100).",
                    }
                },
            },
        },
        # ── Voice management ──────────────────────────────────────────────────
        {
            "name": "list_voices",
            "description": (
                "Returns all TTS voices in the catalog with their download status.\n"
                "\n"
                "RETURN VALUE:\n"
                "  JSON array of voice objects:\n"
                "    id        — voice ID string (pass to set_voice)\n"
                "    display   — human-readable name and description\n"
                "    lang      — BCP-47 language tag (e.g. 'en-US')\n"
                "    quality   — 'low', 'medium', or 'high'\n"
                "    downloaded — true if the model file is present on disk\n"
                "\n"
                "Only voices with downloaded=true can be activated with set_voice."
            ),
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "set_voice",
            "description": (
                "Switches the active TTS voice used by speak_text.\n"
                "\n"
                "The voice must already be downloaded (downloaded=true in list_voices). "
                "The change takes effect immediately for the next speak_text call and is "
                "persisted to config.json.\n"
                "\n"
                "PARAMETERS:\n"
                "  voice_id (string, required): A voice ID from list_voices "
                "(e.g. 'en-us-ryan-medium').\n"
                "\n"
                "RETURN VALUE:\n"
                "  'voice set to <voice_id>' on success.\n"
                "  Raises an error if the voice_id is unknown or not downloaded."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "voice_id": {
                        "type": "string",
                        "description": "Voice ID to activate (must be downloaded).",
                    }
                },
                "required": ["voice_id"],
            },
        },
        # ── File transcription ────────────────────────────────────────────────
        {
            "name": "transcribe_file",
            "description": (
                "Transcribes an audio file from disk using the loaded Whisper model.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  Loads the audio file, resamples it to 16 kHz mono, and runs it through "
                "the same Whisper pipeline used for live recordings — including the full "
                "post-processing pipeline (filler removal, spoken punctuation, etc.).\n"
                "\n"
                "SUPPORTED FORMATS:\n"
                "  WAV, MP3, OGG, FLAC, M4A, and any format supported by PyAV/libavcodec.\n"
                "\n"
                "PARAMETERS:\n"
                "  path (string, required): Absolute path to the audio file on disk.\n"
                "\n"
                "RETURN VALUE:\n"
                "  Plain-text transcript. Returns '(no speech detected)' if the file is "
                "silent or contains no recognisable speech."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to an audio file (WAV, MP3, OGG, FLAC, etc.).",
                    }
                },
                "required": ["path"],
            },
        },
        # ── Configuration ─────────────────────────────────────────────────────
        {
            "name": "get_config",
            "description": (
                "Reads a configuration value from the running app.\n"
                "\n"
                "PARAMETERS:\n"
                "  key (string, required): The config key to read "
                "(e.g. 'tts_voice', 'vad_threshold').\n"
                "\n"
                "RETURN VALUE:\n"
                "  JSON-encoded value, or null if the key is unknown."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Config key to read.",
                    }
                },
                "required": ["key"],
            },
        },
        {
            "name": "set_config",
            "description": (
                "Updates a writable configuration value in the running app and persists it "
                "to config.json.\n"
                "\n"
                "WRITABLE KEYS (partial list):\n"
                "  tts_enabled, tts_engine, tts_voice, tts_response_overlay\n"
                "  vad_threshold, min_silence_duration_ms, mcp_record_timeout\n"
                "  remove_fillers, spoken_punctuation, auto_format_lists\n"
                "  quiet_mode, dictation_mode, inference_mode\n"
                "  overlay_style, show_overlay\n"
                "  ollama_enabled, ollama_model, ollama_mode\n"
                "\n"
                "Hardware/security keys (hotkey, evdev_device, etc.) are read-only via MCP.\n"
                "\n"
                "PARAMETERS:\n"
                "  key   (string, required): Config key to update.\n"
                "  value (any,    required): New value (string, number, or boolean).\n"
                "\n"
                "RETURN VALUE:\n"
                "  'config.<key> = <value>' on success."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Config key to update.",
                    },
                    "value": {
                        "description": "New value (string, number, or boolean).",
                    },
                },
                "required": ["key", "value"],
            },
        },
    ]
}


class WhisperMCPServer:
    """
    In-process MCP server backed by a Unix domain socket.

    Required callbacks (injected from main.py):
      on_record(timeout_seconds) -> str   triggers recording, blocks until done
      on_speak(text)                      queues text for TTS playback
      get_status() -> dict                {"recording": bool, "speaking": bool}

    Optional callbacks — tools are disabled (return an error) when None:
      on_inject(text) -> dict             {"success": bool, "error": str|None}
      on_stop_tts()                       stops TTS immediately
      get_last_transcript() -> str        returns most recent transcript
      list_voices() -> list[dict]         returns voice catalog with download status
      on_set_voice(voice_id) -> str       switches active voice, returns confirmation
      on_transcribe_file(path) -> str     transcribes an audio file
      get_config(key) -> Any              reads a config value
      on_set_config(key, value) -> str    writes a config value, returns confirmation
      get_history(n) -> list[str]         returns last n transcriptions
    """

    def __init__(
        self,
        on_record: Callable[[float], str],
        on_speak: Callable[[str], None],
        get_status: Callable[[], dict],
        on_inject: Optional[Callable[[str], dict]] = None,
        on_stop_tts: Optional[Callable[[], None]] = None,
        get_last_transcript: Optional[Callable[[], str]] = None,
        list_voices: Optional[Callable[[], list]] = None,
        on_set_voice: Optional[Callable[[str], str]] = None,
        on_transcribe_file: Optional[Callable[[str], str]] = None,
        get_config: Optional[Callable[[str], Any]] = None,
        on_set_config: Optional[Callable[[str, Any], str]] = None,
        get_history: Optional[Callable[[int], list]] = None,
    ):
        self._on_record = on_record
        self._on_speak = on_speak
        self._get_status = get_status
        self._on_inject = on_inject
        self._on_stop_tts = on_stop_tts
        self._get_last_transcript = get_last_transcript
        self._list_voices = list_voices
        self._on_set_voice = on_set_voice
        self._on_transcribe_file = on_transcribe_file
        self._get_config = get_config
        self._on_set_config = on_set_config
        self._get_history = get_history

        self._socket_path = SOCKET_PATH
        self._server_sock: Optional[socket.socket] = None
        self._thread: Optional[threading.Thread] = None
        self._running = False

    # ── Lifecycle ─────────────────────────────────────────────────────────────

    def start(self) -> None:
        if self._running:
            return
        self._running = True
        self._thread = threading.Thread(
            target=self._serve, daemon=True, name="mcp-server"
        )
        self._thread.start()
        print(f"[MCP] Server started on {self._socket_path}")

    def stop(self) -> None:
        self._running = False
        if self._server_sock:
            try:
                self._server_sock.close()
            except Exception:
                pass
        try:
            os.unlink(self._socket_path)
        except OSError:
            pass

    # ── Socket server loop ────────────────────────────────────────────────────

    def _serve(self):
        try:
            os.unlink(self._socket_path)
        except OSError:
            pass

        self._server_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._server_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server_sock.bind(self._socket_path)
        os.chmod(self._socket_path, 0o600)
        self._server_sock.listen(4)
        self._server_sock.settimeout(1.0)

        while self._running:
            try:
                conn, _ = self._server_sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            t = threading.Thread(
                target=self._handle_connection,
                args=(conn,),
                daemon=True,
                name="mcp-conn",
            )
            t.start()

    def _handle_connection(self, conn: socket.socket):
        buf = b""
        try:
            while self._running:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        req = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    resp = self._dispatch(req)
                    if resp is not None:
                        conn.sendall((json.dumps(resp) + "\n").encode("utf-8"))
        except Exception as e:
            print(f"[MCP] Connection error: {e}")
        finally:
            try:
                conn.close()
            except Exception:
                pass

    # ── JSON-RPC dispatcher ───────────────────────────────────────────────────

    def _dispatch(self, req: dict) -> Optional[dict]:
        method = req.get("method", "")
        rpc_id = req.get("id")

        if rpc_id is None and method not in ("initialize",):
            self._handle_notification(method, req.get("params", {}))
            return None

        try:
            result = self._handle_method(method, req.get("params") or {})
            return {"jsonrpc": "2.0", "id": rpc_id, "result": result}
        except Exception as e:
            return {
                "jsonrpc": "2.0",
                "id": rpc_id,
                "error": {"code": -32603, "message": str(e)},
            }

    def _handle_notification(self, method: str, params: dict):
        pass

    def _handle_method(self, method: str, params: dict) -> dict:
        if method == "initialize":
            return {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "whisper-wayland",
                    "version": "1.0.0",
                },
                "capabilities": {"tools": {}},
            }

        if method == "tools/list":
            return _TOOL_LIST

        if method == "tools/call":
            name = params.get("name", "")
            args = params.get("arguments") or {}
            return self._call_tool(name, args)

        if method == "notifications/initialized":
            return {}

        raise ValueError(f"Unknown method: {method!r}")

    def _call_tool(self, name: str, args: dict) -> dict:
        # ── Original three tools ──────────────────────────────────────────────
        if name == "transcribe_voice":
            timeout = float(args.get("timeout_seconds", 15.0))
            text = self._on_record(timeout)
            return {
                "content": [{"type": "text", "text": text or "(no speech detected)"}]
            }

        if name == "speak_text":
            text = args.get("text", "")
            if not text:
                raise ValueError("speak_text requires 'text' argument")
            self._on_speak(text)
            return {"content": [{"type": "text", "text": "spoken"}]}

        if name == "get_status":
            status = self._get_status()
            return {
                "content": [{"type": "text", "text": json.dumps(status)}]
            }

        # ── inject_text ───────────────────────────────────────────────────────
        if name == "inject_text":
            text = args.get("text", "")
            if not text:
                raise ValueError("inject_text requires 'text' argument")
            if self._on_inject is None:
                raise ValueError("inject_text not available in this configuration")
            result = self._on_inject(text)
            msg = "injected" if result.get("success") else f"error: {result.get('error') or 'unknown'}"
            return {"content": [{"type": "text", "text": msg}]}

        # ── cancel_tts ────────────────────────────────────────────────────────
        if name == "cancel_tts":
            if self._on_stop_tts is None:
                raise ValueError("cancel_tts not available in this configuration")
            self._on_stop_tts()
            return {"content": [{"type": "text", "text": "stopped"}]}

        # ── get_last_transcript ───────────────────────────────────────────────
        if name == "get_last_transcript":
            if self._get_last_transcript is None:
                raise ValueError("get_last_transcript not available in this configuration")
            text = self._get_last_transcript()
            return {"content": [{"type": "text", "text": text or "(no transcript yet)"}]}

        # ── get_history ───────────────────────────────────────────────────────
        if name == "get_history":
            if self._get_history is None:
                raise ValueError("get_history not available in this configuration")
            n = int(args.get("n", 10))
            n = max(1, min(n, 100))
            entries = self._get_history(n)
            return {"content": [{"type": "text", "text": json.dumps(entries)}]}

        # ── list_voices ───────────────────────────────────────────────────────
        if name == "list_voices":
            if self._list_voices is None:
                raise ValueError("list_voices not available in this configuration")
            voices = self._list_voices()
            return {"content": [{"type": "text", "text": json.dumps(voices)}]}

        # ── set_voice ─────────────────────────────────────────────────────────
        if name == "set_voice":
            voice_id = args.get("voice_id", "")
            if not voice_id:
                raise ValueError("set_voice requires 'voice_id' argument")
            if self._on_set_voice is None:
                raise ValueError("set_voice not available in this configuration")
            msg = self._on_set_voice(voice_id)
            return {"content": [{"type": "text", "text": msg}]}

        # ── transcribe_file ───────────────────────────────────────────────────
        if name == "transcribe_file":
            path = args.get("path", "")
            if not path:
                raise ValueError("transcribe_file requires 'path' argument")
            if self._on_transcribe_file is None:
                raise ValueError("transcribe_file not available in this configuration")
            text = self._on_transcribe_file(path)
            return {"content": [{"type": "text", "text": text or "(no speech detected)"}]}

        # ── get_config ────────────────────────────────────────────────────────
        if name == "get_config":
            key = args.get("key", "")
            if not key:
                raise ValueError("get_config requires 'key' argument")
            if self._get_config is None:
                raise ValueError("get_config not available in this configuration")
            value = self._get_config(key)
            return {"content": [{"type": "text", "text": json.dumps(value)}]}

        # ── set_config ────────────────────────────────────────────────────────
        if name == "set_config":
            key = args.get("key", "")
            if not key:
                raise ValueError("set_config requires 'key' argument")
            if "value" not in args:
                raise ValueError("set_config requires 'value' argument")
            if self._on_set_config is None:
                raise ValueError("set_config not available in this configuration")
            msg = self._on_set_config(key, args["value"])
            return {"content": [{"type": "text", "text": msg}]}

        raise ValueError(f"Unknown tool: {name!r}")

    # ── Claude Desktop config helper ──────────────────────────────────────────

    @staticmethod
    def write_claude_desktop_config() -> str:
        """
        Writes a socat-based entry to the Claude Desktop MCP config file.
        Returns the config file path.
        """
        config_path_candidates = [
            os.path.expanduser("~/.config/claude/claude_desktop_config.json"),
            os.path.expanduser("~/Library/Application Support/Claude/claude_desktop_config.json"),
        ]
        config_path = config_path_candidates[0]
        for p in config_path_candidates:
            if os.path.exists(p):
                config_path = p
                break

        try:
            with open(config_path, "r") as f:
                cfg = json.load(f)
        except (FileNotFoundError, json.JSONDecodeError):
            cfg = {}

        cfg.setdefault("mcpServers", {})
        cfg["mcpServers"]["whisper-wayland"] = {
            "command": "socat",
            "args": ["STDIO", f"UNIX-CONNECT:{SOCKET_PATH}"],
        }

        os.makedirs(os.path.dirname(config_path), exist_ok=True)
        with open(config_path, "w") as f:
            json.dump(cfg, f, indent=2)

        return config_path
