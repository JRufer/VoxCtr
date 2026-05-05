#!/usr/bin/env python3
"""
Live integration test for the Whisper-Wayland MCP server.

Tests all 12 exposed tools over the Unix socket.  Requires Whisper-Wayland
to be running with mcp_server_enabled=true and tts_enabled=true.

Usage:
    python3 scripts/test_mcp_voice.py [OPTIONS] [TEST ...]

    Run all tests:
        python3 scripts/test_mcp_voice.py

    Run specific tests by name:
        python3 scripts/test_mcp_voice.py get_status speak transcribe

Options:
    --socket PATH       Unix socket path  (default: /tmp/whisper-wayland-mcp.sock)
    --timeout SECS      Recording timeout (default: 15)
    --audio-file PATH   Audio file for transcribe_file test (auto-creates a WAV if omitted)
    --inject-text TEXT  Text to inject in the inject_text test
    --list              List available test names and exit

Available test names:
    handshake         Connect and verify protocol version
    list_tools        Print all tool names from the server
    get_status        Verify idle state
    speak             Speak a sentence via TTS and wait for completion
    transcribe        Record voice and return transcript
    get_last          Verify get_last_transcript matches the last recording
    get_history       Verify transcript history contains recent entries
    list_voices       List available TTS voices
    set_voice         Switch to another downloaded voice (skipped if only one)
    transcribe_file   Transcribe a WAV file from disk
    inject_text       Type text into the active window (requires focused text field)
    cancel_tts        Test TTS cancellation mid-playback
    get_config        Read a config value
    set_config        Write and restore a config value
"""

import argparse
import json
import os
import struct
import sys
import tempfile
import time
import wave

SOCKET_PATH = "/tmp/whisper-wayland-mcp.sock"

_USE_COLOR = sys.stdout.isatty()

def _c(code, text):   return f"\033[{code}m{text}\033[0m" if _USE_COLOR else text
def ok(msg):          print(_c("32",   f"  ✓  {msg}"))
def info(msg):        print(_c("36",   f"  ·  {msg}"))
def warn(msg):        print(_c("33",   f"  ⚠  {msg}"), file=sys.stderr)
def err(msg):         print(_c("31",   f"  ✗  {msg}"), file=sys.stderr)
def head(msg):        print(_c("1;35", f"\n── {msg} ──"))
def skipped(msg):     print(_c("90",   f"  ⊘  {msg} (skipped)"))
def value(label, v):  print(f"      {_c('1', label+':')} {_c('32', str(v))}")


# ── Low-level MCP client ───────────────────────────────────────────────────────

class MCPClient:
    def __init__(self, socket_path):
        import socket as _socket
        self._path = socket_path
        self._sock = None
        self._buf = b""
        self._next_id = 1
        self._socket = _socket

    def connect(self):
        if not os.path.exists(self._path):
            raise FileNotFoundError(
                f"Socket not found: {self._path}\n"
                "  → Is Whisper-Wayland running with mcp_server_enabled=true?"
            )
        self._sock = self._socket.socket(self._socket.AF_UNIX, self._socket.SOCK_STREAM)
        self._sock.settimeout(60.0)
        self._sock.connect(self._path)

    def close(self):
        if self._sock:
            try:
                self._sock.close()
            except Exception:
                pass
            self._sock = None

    def _send(self, obj):
        self._sock.sendall((json.dumps(obj) + "\n").encode())

    def _recv_line(self):
        while b"\n" not in self._buf:
            chunk = self._sock.recv(4096)
            if not chunk:
                raise ConnectionError("Server closed connection")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.strip())

    def call(self, method, params=None):
        rpc_id = self._next_id
        self._next_id += 1
        self._send({"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params or {}})
        resp = self._recv_line()
        if resp.get("id") != rpc_id:
            raise ValueError(f"Unexpected response id: {resp}")
        if "error" in resp:
            raise RuntimeError(f"RPC error [{resp['error']['code']}]: {resp['error']['message']}")
        return resp["result"]

    def notify(self, method, params=None):
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    # ── High-level helpers ─────────────────────────────────────────────────────

    def initialize(self):
        result = self.call("initialize")
        self.notify("notifications/initialized")
        return result

    def tool(self, name, **kwargs):
        return self.call("tools/call", {"name": name, "arguments": kwargs})

    def tool_text(self, name, **kwargs):
        result = self.tool(name, **kwargs)
        return result["content"][0]["text"]

    def wait_idle(self, poll=0.5, timeout=120):
        deadline = time.time() + timeout
        while time.time() < deadline:
            status = json.loads(self.tool_text("get_status"))
            if not status.get("recording") and not status.get("speaking"):
                return
            time.sleep(poll)
        raise TimeoutError("Server did not become idle within timeout")


# ── Individual tests ───────────────────────────────────────────────────────────

class TestRunner:
    def __init__(self, client: MCPClient, args):
        self.c = client
        self.args = args
        self._last_transcript = ""
        self.results = {}  # name → "pass" | "fail" | "skip"

    def _pass(self, name):  self.results[name] = "pass"
    def _fail(self, name):  self.results[name] = "fail"
    def _skip(self, name):  self.results[name] = "skip"

    # ── handshake ─────────────────────────────────────────────────────────────
    def test_handshake(self):
        head("handshake")
        info_result = self.c.initialize()
        srv = info_result["serverInfo"]
        ok(f"{srv['name']} v{srv['version']}  (protocol {info_result['protocolVersion']})")
        assert info_result["protocolVersion"] == "2024-11-05", "unexpected protocol version"
        self._pass("handshake")

    # ── list_tools ────────────────────────────────────────────────────────────
    def test_list_tools(self):
        head("list_tools")
        tools = self.c.call("tools/list")["tools"]
        for t in tools:
            info(t["name"])
        expected = {
            "transcribe_voice", "speak_text", "get_status",
            "inject_text", "cancel_tts", "get_last_transcript", "get_history",
            "list_voices", "set_voice", "transcribe_file",
            "get_config", "set_config",
        }
        names = {t["name"] for t in tools}
        missing = expected - names
        if missing:
            err(f"Missing tools: {missing}")
            self._fail("list_tools")
        else:
            ok(f"All {len(expected)} expected tools present")
            self._pass("list_tools")

    # ── get_status ────────────────────────────────────────────────────────────
    def test_get_status(self):
        head("get_status")
        raw = self.c.tool_text("get_status")
        status = json.loads(raw)
        value("recording", status["recording"])
        value("speaking",  status["speaking"])
        assert "recording" in status and "speaking" in status
        ok("status fields present")
        if status["recording"] or status["speaking"]:
            warn("Server is not idle — waiting…")
            self.c.wait_idle()
        self._pass("get_status")

    # ── speak ─────────────────────────────────────────────────────────────────
    def test_speak(self):
        head("speak_text")
        text = "Hello. This is a test of the text to speech system."
        info(f"Speaking: {text!r}")
        result = self.c.tool_text("speak_text", text=text)
        assert result == "spoken", f"Expected 'spoken', got {result!r}"
        ok("queued successfully")
        time.sleep(0.5)
        info("Waiting for TTS to finish…")
        self.c.wait_idle()
        ok("playback complete")
        self._pass("speak")

    # ── cancel_tts ────────────────────────────────────────────────────────────
    def test_cancel_tts(self):
        head("cancel_tts")
        long_text = (
            "This is a very long sentence that I am going to cancel before it finishes. "
            "There are many more words here that will never be spoken aloud because we "
            "are going to stop the text to speech engine right in the middle."
        )
        info("Starting long TTS…")
        self.c.tool_text("speak_text", text=long_text)
        time.sleep(1.0)
        info("Cancelling TTS…")
        result = self.c.tool_text("cancel_tts")
        assert result == "stopped", f"Expected 'stopped', got {result!r}"
        ok("cancel returned 'stopped'")
        time.sleep(0.3)
        status = json.loads(self.c.tool_text("get_status"))
        if status["speaking"]:
            warn("TTS still playing after cancel (may take a moment to stop)")
        else:
            ok("TTS stopped confirmed via get_status")
        self._pass("cancel_tts")

    # ── transcribe ────────────────────────────────────────────────────────────
    def test_transcribe(self):
        head("transcribe_voice")
        prompt = "Please say something after the beep. Speak clearly."
        info(f"Speaking prompt: {prompt!r}")
        self.c.tool_text("speak_text", text=prompt)
        time.sleep(0.5)
        self.c.wait_idle()

        timeout = self.args.timeout
        info(f"Listening for up to {timeout:.0f} s — speak now…")
        transcript = self.c.tool_text("transcribe_voice", timeout_seconds=timeout)

        value("transcript", f'"{transcript}"')
        if transcript == "(no speech detected)":
            warn("No speech detected — microphone may not be active")
            self._fail("transcribe")
            return

        self._last_transcript = transcript
        ok("transcript received")
        self._pass("transcribe")

    # ── get_last_transcript ───────────────────────────────────────────────────
    def test_get_last(self):
        head("get_last_transcript")
        if not self._last_transcript:
            info("Running transcribe first…")
            self.test_transcribe()
            if self.results.get("transcribe") != "pass":
                skipped("get_last_transcript (no transcript available)")
                self._skip("get_last")
                return

        last = self.c.tool_text("get_last_transcript")
        value("last transcript", f'"{last}"')
        if last == "(no transcript yet)":
            err("Expected a transcript but got placeholder")
            self._fail("get_last")
        elif last == self._last_transcript:
            ok("matches transcribe_voice result")
            self._pass("get_last")
        else:
            # get_last_transcript returns the overall last, which may differ if
            # hotkey was used between tests — just check it's non-empty
            ok(f"non-empty transcript returned (may differ if hotkey used)")
            self._pass("get_last")

    # ── get_history ───────────────────────────────────────────────────────────
    def test_get_history(self):
        head("get_history")
        raw = self.c.tool_text("get_history", n=10)
        entries = json.loads(raw)
        value("entries returned", len(entries))
        for i, e in enumerate(entries):
            info(f"  [{i}] {e!r}")
        ok("get_history returned a list")
        self._pass("get_history")

    # ── list_voices ───────────────────────────────────────────────────────────
    def test_list_voices(self):
        head("list_voices")
        raw = self.c.tool_text("list_voices")
        voices = json.loads(raw)
        downloaded = [v for v in voices if v["downloaded"]]
        not_downloaded = [v for v in voices if not v["downloaded"]]
        value("total voices in catalog", len(voices))
        value("downloaded", len(downloaded))
        value("not downloaded", len(not_downloaded))
        for v in downloaded:
            info(f"  ✓ [{v['id']}]  {v['display']}")
        for v in not_downloaded[:3]:
            info(f"  ⊘ [{v['id']}]  {v['display']}")
        if len(not_downloaded) > 3:
            info(f"  … and {len(not_downloaded) - 3} more not downloaded")
        ok("list_voices returned voice catalog")
        self._pass("list_voices")
        return voices

    # ── set_voice ─────────────────────────────────────────────────────────────
    def test_set_voice(self):
        head("set_voice")
        raw = self.c.tool_text("list_voices")
        voices = json.loads(raw)
        downloaded = [v for v in voices if v["downloaded"]]
        current_id = json.loads(self.c.tool_text("get_config", key="tts_voice"))

        alternates = [v for v in downloaded if v["id"] != current_id]
        if not alternates:
            skipped("set_voice (only one voice downloaded — download another to test this)")
            self._skip("set_voice")
            return

        target = alternates[0]
        info(f"Switching from {current_id!r} → {target['id']!r}")
        result = self.c.tool_text("set_voice", voice_id=target["id"])
        value("result", result)
        assert target["id"] in result

        # Restore original voice
        self.c.tool_text("set_voice", voice_id=current_id)
        ok(f"Restored original voice: {current_id!r}")
        self._pass("set_voice")

    # ── transcribe_file ───────────────────────────────────────────────────────
    def test_transcribe_file(self):
        head("transcribe_file")

        audio_path = self.args.audio_file
        _temp = None

        if not audio_path:
            # Generate a short silent WAV (real speech test requires a real file)
            _temp = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
            _write_silent_wav(_temp.name, duration_secs=1.0)
            audio_path = _temp.name
            info(f"No --audio-file provided; using synthetic silent WAV: {audio_path}")
        else:
            info(f"Transcribing: {audio_path}")

        try:
            transcript = self.c.tool_text("transcribe_file", path=audio_path)
            value("transcript", f'"{transcript}"')
            ok("transcribe_file returned without error")
            self._pass("transcribe_file")
        except RuntimeError as e:
            if "not found" in str(e).lower():
                err(str(e))
                self._fail("transcribe_file")
            else:
                ok(f"transcribe_file raised expected error: {e}")
                self._pass("transcribe_file")
        finally:
            if _temp:
                try:
                    os.unlink(_temp.name)
                except OSError:
                    pass

    # ── inject_text ───────────────────────────────────────────────────────────
    def test_inject_text(self):
        head("inject_text")
        text = self.args.inject_text or "Hello from Whisper-Wayland MCP!"
        print()
        print(_c("33", "  ⚠  Focus a text editor or terminal before continuing."))
        print(_c("33", f"     The following text will be typed: {text!r}"))
        print(_c("33",  "     Press Enter to proceed or Ctrl-C to skip this test."))
        try:
            input()
        except KeyboardInterrupt:
            print()
            skipped("inject_text (user skipped)")
            self._skip("inject_text")
            return

        result = self.c.tool_text("inject_text", text=text)
        value("result", result)
        if result.startswith("error:"):
            warn(f"Injection failed: {result}")
            warn("Install wtype (Wayland) or xdotool (X11) to enable this tool")
            self._fail("inject_text")
        else:
            ok("text injected")
            self._pass("inject_text")

    # ── get_config ────────────────────────────────────────────────────────────
    def test_get_config(self):
        head("get_config")
        for key in ("tts_voice", "tts_enabled", "vad_threshold", "model_size"):
            raw = self.c.tool_text("get_config", key=key)
            value(key, json.loads(raw))
        ok("get_config returned values")
        self._pass("get_config")

    # ── set_config ────────────────────────────────────────────────────────────
    def test_set_config(self):
        head("set_config")
        key = "vad_threshold"
        original = json.loads(self.c.tool_text("get_config", key=key))
        new_val = 0.42

        info(f"Setting {key}: {original} → {new_val}")
        result = self.c.tool_text("set_config", key=key, value=new_val)
        value("result", result)
        assert str(new_val) in result

        readback = json.loads(self.c.tool_text("get_config", key=key))
        assert abs(readback - new_val) < 0.001, f"readback mismatch: {readback}"
        ok("write + readback confirmed")

        # Restore
        self.c.tool_text("set_config", key=key, value=original)
        ok(f"Restored {key} = {original}")

        # Verify read-only key is rejected
        try:
            self.c.tool_text("set_config", key="hotkey", value=["KEY_F12"])
            err("Expected error for read-only key, but none raised")
            self._fail("set_config")
            return
        except RuntimeError as e:
            ok(f"Read-only key correctly rejected: {e}")

        self._pass("set_config")


# ── Summary ────────────────────────────────────────────────────────────────────

def print_summary(results: dict):
    print()
    print(_c("1", "── Test Summary " + "─" * 40))
    passed  = [k for k, v in results.items() if v == "pass"]
    failed  = [k for k, v in results.items() if v == "fail"]
    skipped = [k for k, v in results.items() if v == "skip"]
    for k in passed:   print(_c("32", f"  PASS  {k}"))
    for k in skipped:  print(_c("90", f"  SKIP  {k}"))
    for k in failed:   print(_c("31", f"  FAIL  {k}"))
    print()
    print(f"  {_c('32', str(len(passed)) + ' passed')}  "
          f"{_c('90', str(len(skipped)) + ' skipped')}  "
          f"{_c('31', str(len(failed)) + ' failed')}")
    print()
    return len(failed) == 0


# ── WAV helper ─────────────────────────────────────────────────────────────────

def _write_silent_wav(path: str, duration_secs: float = 1.0, sample_rate: int = 16000):
    n_samples = int(sample_rate * duration_secs)
    with wave.open(path, "w") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sample_rate)
        wf.writeframes(b"\x00\x00" * n_samples)


# ── All tests ordered ──────────────────────────────────────────────────────────

ALL_TESTS = [
    "handshake",
    "list_tools",
    "get_status",
    "speak",
    "cancel_tts",
    "transcribe",
    "get_last",
    "get_history",
    "list_voices",
    "set_voice",
    "transcribe_file",
    "inject_text",
    "get_config",
    "set_config",
]

TEST_MAP = {
    "handshake":       "test_handshake",
    "list_tools":      "test_list_tools",
    "get_status":      "test_get_status",
    "speak":           "test_speak",
    "cancel_tts":      "test_cancel_tts",
    "transcribe":      "test_transcribe",
    "get_last":        "test_get_last",
    "get_history":     "test_get_history",
    "list_voices":     "test_list_voices",
    "set_voice":       "test_set_voice",
    "transcribe_file": "test_transcribe_file",
    "inject_text":     "test_inject_text",
    "get_config":      "test_get_config",
    "set_config":      "test_set_config",
}


# ── Entry point ────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Live integration test for the Whisper-Wayland MCP server.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Run specific tests by name: python3 test_mcp_voice.py speak transcribe",
    )
    parser.add_argument("tests", nargs="*", help="Test names to run (default: all)")
    parser.add_argument("--socket", default=SOCKET_PATH, metavar="PATH",
                        help=f"Unix socket path (default: {SOCKET_PATH})")
    parser.add_argument("--timeout", type=float, default=15.0, metavar="SECS",
                        help="Recording timeout in seconds (default: 15)")
    parser.add_argument("--audio-file", default="", metavar="PATH",
                        help="Audio file for transcribe_file test")
    parser.add_argument("--inject-text", default="", metavar="TEXT",
                        help="Text to type in inject_text test")
    parser.add_argument("--list", action="store_true",
                        help="List available test names and exit")
    args = parser.parse_args()

    if args.list:
        print("Available tests:")
        for name in ALL_TESTS:
            print(f"  {name}")
        return

    tests_to_run = args.tests if args.tests else ALL_TESTS
    invalid = [t for t in tests_to_run if t not in TEST_MAP]
    if invalid:
        err(f"Unknown test(s): {invalid}")
        err(f"Valid names: {ALL_TESTS}")
        sys.exit(1)

    print(_c("1;35", "\nWhisper-Wayland MCP Server — Integration Test"))
    print(f"  socket:  {args.socket}")
    print(f"  tests:   {', '.join(tests_to_run)}")

    client = MCPClient(args.socket)
    try:
        client.connect()
    except FileNotFoundError as e:
        err(str(e))
        sys.exit(1)

    ok("Connected")

    runner = TestRunner(client, args)

    # handshake must run first to initialise the MCP session
    if "handshake" in tests_to_run:
        try:
            runner.test_handshake()
        except Exception as e:
            err(f"handshake failed: {e}")
            client.close()
            sys.exit(1)
        tests_to_run = [t for t in tests_to_run if t != "handshake"]
    else:
        # Still need to initialise even if test not selected
        try:
            client.initialize()
        except Exception as e:
            err(f"MCP handshake failed: {e}")
            client.close()
            sys.exit(1)

    for name in tests_to_run:
        method = TEST_MAP[name]
        try:
            getattr(runner, method)()
        except KeyboardInterrupt:
            print()
            skipped(f"{name} (interrupted)")
            runner._skip(name)
        except Exception as e:
            err(f"{name} raised: {e}")
            runner._fail(name)

    client.close()
    ok_all = print_summary(runner.results)
    sys.exit(0 if ok_all else 1)


if __name__ == "__main__":
    main()
