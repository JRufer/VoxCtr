"""
Whisper-Wayland MCP Server

Exposes the app as an MCP (Model Context Protocol) tool server so AI clients
(Claude Desktop, custom agents) can trigger voice recording and TTS playback.

Transport: Unix domain socket at /tmp/whisper-wayland-mcp.sock
Protocol:  JSON-RPC 2.0 (MCP spec)

Tools exposed:
  transcribe_voice()          → records the user's voice and returns transcript
  speak_text(text: str)       → speaks text aloud via TTS
  get_status()                → {"recording": bool, "speaking": bool}

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
from typing import Callable, Optional

SOCKET_PATH = "/tmp/whisper-wayland-mcp.sock"

_TOOL_LIST = {
    "tools": [
        {
            "name": "transcribe_voice",
            "description": (
                "Records the user's voice through their microphone and returns the transcribed text.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  Calling this tool immediately activates the user's microphone. The user speaks, "
                "and when they stop (or the timeout is reached) the audio is transcribed locally "
                "using Whisper and the text is returned. The microphone indicator in the app's "
                "system tray will show a recording state while this tool is active.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - Whenever you need a spoken response or clarification from the user.\n"
                "  - To conduct a voice-driven conversation: speak a question with speak_text, "
                "then call transcribe_voice to capture the answer.\n"
                "  - When the user has indicated they prefer to respond by voice rather than typing.\n"
                "  - To capture dictated content such as notes, messages, or commands.\n"
                "\n"
                "WHEN NOT TO USE:\n"
                "  - Do not call while get_status shows recording=true (a recording is already in "
                "progress). Check status first if unsure.\n"
                "  - Do not call while get_status shows speaking=true; wait for TTS to finish so "
                "the microphone does not pick up the synthesised voice.\n"
                "  - Do not loop rapidly on empty results; if '(no speech detected)' is returned "
                "twice in a row, inform the user and wait for a typed prompt instead.\n"
                "\n"
                "PARAMETERS:\n"
                "  timeout_seconds (number, optional, default 15): Maximum wall-clock seconds to "
                "wait for the user to finish speaking. Use a shorter value (5–8 s) for quick "
                "yes/no questions. Use a longer value (30–60 s) when asking the user to dictate "
                "a paragraph or give detailed instructions.\n"
                "\n"
                "RETURN VALUE:\n"
                "  A plain-text string containing the transcribed speech. If no speech was "
                "detected within the timeout the string will be '(no speech detected)'. "
                "The transcript may contain minor errors from the speech model; treat it as "
                "lightly noisy text and correct obvious errors from context before acting on it.\n"
                "\n"
                "EXAMPLE FLOW — voice Q&A:\n"
                "  1. speak_text(\"What city are you in?\")\n"
                "  2. transcribe_voice(timeout_seconds=10)  → \"I'm in Seattle\"\n"
                "  3. Use 'Seattle' in subsequent tool calls or responses.\n"
                "\n"
                "EXAMPLE FLOW — voice dictation:\n"
                "  1. speak_text(\"Please dictate your message now.\")\n"
                "  2. transcribe_voice(timeout_seconds=45)  → full dictated message\n"
                "  3. Present the transcript back to the user for confirmation before sending."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "timeout_seconds": {
                        "type": "number",
                        "description": (
                            "Maximum seconds to wait for the user to finish speaking. "
                            "Defaults to 15. Use 5–8 for short answers, 30–60 for dictation."
                        ),
                    }
                },
            },
        },
        {
            "name": "speak_text",
            "description": (
                "Converts text to speech and plays it aloud through the user's speakers.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  The text is queued for playback using the locally configured TTS engine "
                "(piper neural TTS by default, espeak-ng as fallback). Playback happens "
                "asynchronously — this tool returns immediately while audio plays in the "
                "background. The app's system tray will show a speaking indicator. Only one "
                "utterance plays at a time; additional calls are queued and played in order.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - To read your response aloud so the user does not have to look at the screen.\n"
                "  - To prompt the user before calling transcribe_voice (speak the question, "
                "then listen).\n"
                "  - To confirm actions: \"Done — I've sent your message.\"\n"
                "  - For accessibility: whenever the user has indicated they prefer audio output.\n"
                "  - To narrate step-by-step instructions the user can follow hands-free.\n"
                "\n"
                "WHEN NOT TO USE:\n"
                "  - Do not pass extremely long text (>500 words) in a single call; break long "
                "content into logical paragraphs and call speak_text once per paragraph so the "
                "user can interrupt between them.\n"
                "  - Do not include markdown syntax (**, ##, -, etc.) — it will be read aloud "
                "literally. Strip formatting before speaking.\n"
                "  - Do not include URLs, file paths, or code snippets; summarise them in plain "
                "prose instead.\n"
                "  - If get_status shows speaking=true and you need to ask a follow-up question, "
                "queue the next speak_text call; do not call transcribe_voice until speaking=false.\n"
                "\n"
                "PARAMETERS:\n"
                "  text (string, required): The plain-text content to speak. Should be natural "
                "prose — complete sentences, no markdown, no raw symbols. Punctuation is used by "
                "the TTS engine for pacing; commas and periods produce natural pauses.\n"
                "\n"
                "RETURN VALUE:\n"
                "  Returns the string 'spoken' when the text has been successfully queued for "
                "playback. This does NOT mean playback is complete — use get_status to check "
                "speaking=false if you need to wait before recording.\n"
                "\n"
                "EXAMPLE — confirm then record:\n"
                "  1. speak_text(\"I'll listen for your answer. Go ahead.\")\n"
                "  2. # Poll until speaking=false before recording\n"
                "  3. transcribe_voice(timeout_seconds=15)\n"
                "\n"
                "EXAMPLE — multi-part narration:\n"
                "  1. speak_text(\"Here are your three reminders for today.\")\n"
                "  2. speak_text(\"First: team standup at 9 AM.\")\n"
                "  3. speak_text(\"Second: review the pull request from Jordan.\")\n"
                "  4. speak_text(\"Third: submit your expense report by 5 PM.\")"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": (
                            "Plain-text content to speak aloud. Use natural prose with punctuation "
                            "for pacing. No markdown, no code, no URLs."
                        ),
                    }
                },
                "required": ["text"],
            },
        },
        {
            "name": "get_status",
            "description": (
                "Returns the current state of the voice interface as a JSON object.\n"
                "\n"
                "HOW IT WORKS:\n"
                "  Queries the running Whisper-Wayland app for its live recording and TTS state. "
                "This is a lightweight, non-blocking call that returns immediately.\n"
                "\n"
                "WHEN TO USE:\n"
                "  - Before calling transcribe_voice: confirm recording=false so you do not "
                "start a second recording session while one is already active.\n"
                "  - Before calling transcribe_voice after speak_text: confirm speaking=false "
                "so the microphone does not capture TTS audio.\n"
                "  - To implement a wait loop: poll get_status every 1–2 seconds until "
                "speaking=false before proceeding to record.\n"
                "  - To diagnose unexpected behaviour: if transcribe_voice returns no speech, "
                "check whether the app is still in a speaking state.\n"
                "\n"
                "RETURN VALUE:\n"
                "  A JSON object with the following fields:\n"
                "    recording (boolean): true while the microphone is actively capturing audio "
                "for transcription. Only one recording session can run at a time.\n"
                "    speaking (boolean): true while TTS audio is currently playing through the "
                "speakers. The queue may contain additional utterances that will play after the "
                "current one finishes.\n"
                "\n"
                "EXAMPLE RESPONSE:\n"
                "  {\"recording\": false, \"speaking\": false}  — idle, safe to record or speak\n"
                "  {\"recording\": true,  \"speaking\": false}  — recording in progress\n"
                "  {\"recording\": false, \"speaking\": true}   — TTS playing, wait before recording\n"
                "\n"
                "RECOMMENDED PATTERN — speak then record safely:\n"
                "  1. speak_text(\"Your question here.\")\n"
                "  2. Loop: get_status → if speaking=true, wait 1 s and repeat\n"
                "  3. transcribe_voice(timeout_seconds=15)\n"
                "\n"
                "NOTE: Both fields can briefly be false between a speak_text call returning and "
                "the audio actually starting. If precise synchronisation matters, add a short "
                "delay (0.5 s) after speak_text before beginning to poll."
            ),
            "inputSchema": {"type": "object", "properties": {}},
        },
    ]
}


class WhisperMCPServer:
    """
    In-process MCP server backed by a Unix domain socket.

    Callbacks injected from main.py:
      on_record(timeout_seconds) -> str   triggers recording, blocks until done
      on_speak(text)                      queues text for TTS playback
      get_status() -> dict                {"recording": bool, "speaking": bool}
    """

    def __init__(
        self,
        on_record: Callable[[float], str],
        on_speak: Callable[[str], None],
        get_status: Callable[[], dict],
    ):
        self._on_record = on_record
        self._on_speak = on_speak
        self._get_status = get_status
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
        # Clean up stale socket
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
                # Split on newlines (each JSON-RPC message is one line)
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

        # Notifications (no id) — no response needed
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
        pass  # reserved for future use

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

        raise ValueError(f"Unknown tool: {name!r}")

    # ── Claude Desktop config helper ──────────────────────────────────────────

    @staticmethod
    def write_claude_desktop_config() -> str:
        """
        Writes a socat-based entry to the Claude Desktop MCP config file.
        Returns the config file path.
        """
        import platform
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
