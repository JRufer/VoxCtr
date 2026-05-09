import json
import os
from pathlib import Path

DEFAULT_CONFIG = {
    "hotkey": ["KEY_LEFTMETA", "KEY_SPACE"],
    "toggle_hotkey": ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"],
    "inference_mode": "Balanced",
    "model_size": "base",
    "device": "auto",  # cuda or cpu
    "compute_type": "default",
    "vad_threshold": 0.5,
    "min_silence_duration_ms": 500,
    "input_device_index": None,  # For PyAudio
    "evdev_device": None,         # e.g. /dev/input/event0
    "show_overlay": True,
    "remove_fillers": True,
    "custom_vocabulary": [],
    # P0 features
    "spoken_punctuation": True,
    "auto_format_lists": True,
    "quiet_mode": False,
    "show_notification": False,
    # P1 features
    "snippets": {},             # {"trigger phrase": "expanded text"}
    "dictation_mode": "normal", # "normal" | "code"
    # P2.1 Ollama LLM post-processing
    "ollama_enabled": False,
    "ollama_model": "llama3.2:1b",
    "ollama_mode": "clean",
    # P2.2 Noise suppression
    "noise_suppression": False,  # Requires: pip install noisereduce
    # Overlay UI
    "overlay_style": "voice_card",  # built-in: "waveform" | "pulse" | "voice_card" | custom stem name
    # Multi-backend engine selection
    "backend_engine": "auto",           # 'auto' | 'moonshine' | 'faster-whisper' | 'whisper-cpp'
    # Moonshine backend settings
    "moonshine_model_size": "medium",   # 'tiny' (34M) | 'small' (123M) | 'medium' (245M)
    "moonshine_language": "en",         # 'en' | 'es' | 'zh' | 'ja' | 'ko' | 'vi' | 'uk' | 'ar'
    "whisper_cpp_binary": "whisper-cli",
    "whisper_cpp_model_dir": "",        # empty = use default ~/.local/share/…/models
    "whisper_cpp_model_size": "large-v3",
    "whisper_cpp_device": "auto",       # 'auto' | 'vulkan' | 'cuda' | 'cpu'
    "whisper_cpp_threads": 0,           # 0 = auto (half of logical cores)
    "whisper_cpp_use_bindings": True,   # prefer pywhispercpp when available
    # TTS / Voice Output
    "tts_enabled": False,
    "tts_engine": "piper",              # 'piper' | 'espeak'
    "tts_voice": "en-us-lessac-medium",
    "tts_stop_key": ["KEY_ESCAPE"],     # evdev key(s) to stop TTS playback
    "tts_response_overlay": True,       # show overlay while TTS is playing
    # MCP Server
    "mcp_server_enabled": False,
    "mcp_record_timeout": 15.0,         # max seconds for MCP-triggered recording
    # AT-SPI2 accessibility integration (requires: pip install pyatspi)
    "atspi_injection": True,            # try AT-SPI2 Text.insertText before wtype/xdotool
    "atspi_context_prompt": True,       # feed surrounding text to Whisper as initial_prompt
    "atspi_auto_code_mode": True,       # auto-switch to code mode for terminal/IDE widgets
}

CONFIG_PATH = Path.home() / ".config" / "voxctl" / "config.json"

class Config:
    def __init__(self):
        self.config = DEFAULT_CONFIG.copy()
        self.load()

    def load(self):
        if CONFIG_PATH.exists():
            try:
                with open(CONFIG_PATH, "r") as f:
                    user_config = json.load(f)
                    self.config.update(user_config)
                    self._sanitize()
            except Exception as e:
                print(f"Error loading config: {e}")

    def _sanitize(self):
        """Ensure critical string keys didn't accidentally become lists/objects."""
        string_keys = [
            "model_size", "device", "compute_type", "backend_engine",
            "whisper_cpp_binary", "whisper_cpp_model_size", "whisper_cpp_device",
            "moonshine_model_size", "moonshine_language",
            "dictation_mode", "ollama_model", "ollama_mode", "overlay_style"
        ]
        for k in string_keys:
            val = self.config.get(k)
            if isinstance(val, list):
                if val:
                    self.config[k] = str(val[0])
                else:
                    self.config[k] = DEFAULT_CONFIG.get(k)
            elif val is None:
                self.config[k] = DEFAULT_CONFIG.get(k)

    def save(self):
        CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        try:
            with open(CONFIG_PATH, "w") as f:
                json.dump(self.config, f, indent=4)
        except Exception as e:
            print(f"Error saving config: {e}")

    def get(self, key, default=None):
        return self.config.get(key, default if default is not None else DEFAULT_CONFIG.get(key))

    def set(self, key, value):
        # Sanitize string keys that must not be lists
        _string_keys = {
            "model_size", "device", "compute_type", "backend_engine",
            "whisper_cpp_binary", "whisper_cpp_model_size", "whisper_cpp_device",
            "moonshine_model_size", "moonshine_language",
            "dictation_mode", "ollama_model", "ollama_mode", "overlay_style"
        }
        if key in _string_keys and isinstance(value, list):
            value = str(value[0]) if value else DEFAULT_CONFIG.get(key, "")
        self.config[key] = value
        self.save()
