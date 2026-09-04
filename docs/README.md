# VoxCtrl Documentation

VoxCtrl is a high-performance, privacy-first voice-to-text dictation application and programmable voice input broker. All processing happens 100% on-device with zero telemetry or cloud dependencies.

Global shortcuts are registered with your desktop through the XDG `GlobalShortcuts` portal, so **VoxCtrl does not read your keyboard** and needs no permission setup at all. See [Privacy & Security](./privacy.md) for exactly what it can see and how to verify it.

---

## Wiki Index

| Document | Description |
|---|---|
| [Overview](./overview.md) | What VoxCtrl does, key features, and design principles |
| [Privacy & Security](./privacy.md) | What VoxCtrl can see, what the installer touches, and how to verify both |
| [Architecture](./architecture.md) | System design, crate layout, data flow, concurrency model |
| [Audio Pipeline](./audio.md) | Audio capture, device management, VAD, resampling |
| [Speech Recognition](./speech-recognition.md) | Whisper engine, models, inference pipeline, post-processing |
| [Routing](./routing.md) | Output targets, hotkey bindings, delivery types |
| [Hotkeys](./hotkeys.md) | Global shortcuts via the desktop portal, gestures, platform support |
| [Text-to-Speech](./tts.md) | TTS engines, voice packs, playback |
| [Integrations](./integrations.md) | MCP server, DBus service, OpenAI-compatible LLM API, webhooks |
| [UI & Windows](./ui.md) | Svelte frontend, overlay, settings |
| [API Reference](./api.md) | Tauri IPC commands and frontend events |
| [Configuration](./configuration.md) | All config files, schemas, and options |
| [Installation & Setup](./installation.md) | Dependencies, building, running |
| [Development Guide](./development.md) | Dev environment, build system, crate structure |

---

## Quick Summary

```
Microphone → Audio Capture → Whisper Inference → Post-Processing → Output Router
                                                                         │
                                               ┌────────────────────────┤
                                               │                        │
                                          Inject text            Clipboard/File/
                                          to window              HTTP/Webhook/Socket/
                                                                 DBus/MCP/Exec/Pipe
```

**Tech Stack:**
- **Frontend:** Svelte 5 + Tailwind CSS 4 + Vite 5
- **Desktop Shell:** Tauri 2 (Rust + WebView)
- **Backend:** Rust (Tokio async), ~10 specialized crates
- **Speech:** whisper.cpp (GGUF models, CPU/CUDA/Vulkan)
- **TTS:** Piper (ONNX neural voices) + Espeak-ng fallback
- **Config:** TOML + JSON, hot-reloadable
