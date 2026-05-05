#!/usr/bin/env python3
"""
Hermes voice loop test.

Sends text to a local Hermes model via Ollama, then speaks the response
aloud through the Whisper-Wayland MCP server.

Usage:
    # Make sure Whisper-Wayland is running, then:
    python scripts/test_hermes_voice.py

    # Override model or socket path:
    python scripts/test_hermes_voice.py --model hermes3:8b
    python scripts/test_hermes_voice.py --socket /tmp/whisper-wayland-mcp.sock
"""

import argparse
import json
import socket
import sys
import urllib.request
import urllib.error


SOCKET_PATH = "/tmp/whisper-wayland-mcp.sock"
OLLAMA_URL  = "http://localhost:11434/api/chat"
MODEL       = "hermes3"

SYSTEM_PROMPT = (
    "You are a helpful voice assistant. Keep your answers concise and conversational — "
    "two to four sentences at most. Do not use markdown, bullet points, code blocks, or "
    "any special formatting. Write in plain prose that sounds natural when spoken aloud. "
    "Do not include URLs."
)


# ── MCP client ────────────────────────────────────────────────────────────────

class MCPClient:
    def __init__(self, socket_path: str):
        self._path = socket_path
        self._id   = 0

    def _call(self, method: str, params: dict) -> dict:
        self._id += 1
        req = {"jsonrpc": "2.0", "id": self._id, "method": method, "params": params}
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            try:
                s.connect(self._path)
            except FileNotFoundError:
                raise RuntimeError(
                    f"MCP socket not found at {self._path}.\n"
                    "Is Whisper-Wayland running?"
                )
            s.sendall((json.dumps(req) + "\n").encode())
            s.settimeout(30.0)
            data = b""
            while True:
                chunk = s.recv(4096)
                if not chunk:
                    break
                data += chunk
                if b"\n" in data:
                    break
        return json.loads(data.split(b"\n")[0])

    def initialize(self):
        self._call("initialize", {})

    def speak(self, text: str) -> None:
        resp = self._call("tools/call", {"name": "speak_text", "arguments": {"text": text}})
        if "error" in resp:
            raise RuntimeError(f"speak_text error: {resp['error']['message']}")

    def get_status(self) -> dict:
        resp = self._call("tools/call", {"name": "get_status", "arguments": {}})
        return json.loads(resp["result"]["content"][0]["text"])

    def wait_until_idle(self, poll_interval: float = 0.5) -> None:
        import time
        while True:
            status = self.get_status()
            if not status.get("speaking") and not status.get("recording"):
                break
            time.sleep(poll_interval)

    def transcribe(self, timeout_seconds: float = 15.0) -> str:
        resp = self._call(
            "tools/call",
            {"name": "transcribe_voice", "arguments": {"timeout_seconds": timeout_seconds}},
        )
        return resp["result"]["content"][0]["text"]


# ── Ollama client ─────────────────────────────────────────────────────────────

def ask_hermes(prompt: str, history: list, model: str) -> str:
    messages = [{"role": "system", "content": SYSTEM_PROMPT}]
    messages.extend(history)
    messages.append({"role": "user", "content": prompt})

    body = json.dumps({"model": model, "messages": messages, "stream": False}).encode()
    req  = urllib.request.Request(
        OLLAMA_URL,
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read())
    except urllib.error.URLError as e:
        raise RuntimeError(
            f"Cannot reach Ollama at {OLLAMA_URL}.\n"
            f"Is Ollama running?  (ollama serve)\n"
            f"Detail: {e}"
        )

    return data["message"]["content"].strip()


def check_ollama_model(model: str) -> None:
    try:
        req = urllib.request.Request("http://localhost:11434/api/tags", method="GET")
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read())
        names = [m["name"].split(":")[0] for m in data.get("models", [])]
        base  = model.split(":")[0]
        if base not in names:
            print(f"Warning: model '{model}' not found in Ollama. Available: {names}")
            print(f"  Run:  ollama pull {model}")
    except Exception:
        pass  # Ollama reachability check is best-effort


# ── Main loop ────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Hermes voice loop test")
    parser.add_argument("--model",  default=MODEL,       help="Ollama model name")
    parser.add_argument("--socket", default=SOCKET_PATH, help="MCP socket path")
    parser.add_argument("--voice",  action="store_true", help="Use microphone for input")
    args = parser.parse_args()

    print(f"Hermes voice test")
    print(f"  Model : {args.model}")
    print(f"  Socket: {args.socket}")
    print(f"  Input : {'microphone' if args.voice else 'keyboard'}")
    print("  Type your message (or 'quit' to exit)\n")

    check_ollama_model(args.model)

    mcp = MCPClient(args.socket)
    mcp.initialize()

    history = []

    while True:
        try:
            if args.voice:
                print("Listening... (speak now)")
                user_text = mcp.transcribe(timeout_seconds=15.0)
                print(f"You said: {user_text}")
                if "(no speech detected)" in user_text:
                    print("No speech detected, try again.\n")
                    continue
            else:
                user_text = input("You: ").strip()

            if not user_text or user_text.lower() in ("quit", "exit", "q"):
                print("Exiting.")
                break

            print("Hermes is thinking...")
            reply = ask_hermes(user_text, history, args.model)
            print(f"Hermes: {reply}\n")

            # Keep conversation history for context
            history.append({"role": "user",      "content": user_text})
            history.append({"role": "assistant",  "content": reply})

            # Speak the reply and wait for it to finish
            mcp.speak(reply)
            mcp.wait_until_idle()

        except KeyboardInterrupt:
            print("\nInterrupted.")
            break
        except RuntimeError as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)


if __name__ == "__main__":
    main()
