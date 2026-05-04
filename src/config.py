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
    # Multi-backend engine selection
    "backend_engine": "auto",           # 'auto' | 'faster-whisper' | 'whisper-cpp'
    "whisper_cpp_binary": "whisper-cli",
    "whisper_cpp_model_dir": "",        # empty = use default ~/.local/share/…/models
    "whisper_cpp_model_size": "large-v3",
    "whisper_cpp_device": "auto",       # 'auto' | 'vulkan' | 'cuda' | 'cpu'
    "whisper_cpp_threads": 0,           # 0 = auto (half of logical cores)
    "whisper_cpp_use_bindings": True,   # prefer pywhispercpp when available
}

CONFIG_PATH = Path.home() / ".config" / "whisper-wayland" / "config.json"

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
            except Exception as e:
                print(f"Error loading config: {e}")

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
        self.config[key] = value
        self.save()
