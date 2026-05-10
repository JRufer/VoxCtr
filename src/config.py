import json
import os
from pathlib import Path

DEFAULT_CONFIG = {
    "hotkey": ["KEY_LEFTMETA", "KEY_SPACE"],
    "toggle_hotkey": ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"],
    "dictation_mode": "normal",
    "engine": {
        "backend": "auto",
        "inference_mode": "Balanced",
        "faster_whisper": {
            "model_size": "base",
            "device": "auto",
            "compute_type": "default"
        },
        "whisper_cpp": {
            "binary": "whisper-cli",
            "model_dir": "",
            "model_size": "large-v3",
            "device": "auto",
            "threads": 0,
            "use_bindings": True
        },
        "moonshine": {
            "model_size": "base",
            "language": "en"
        }
    },
    "audio": {
        "vad_threshold": 0.5,
        "min_silence_duration_ms": 500,
        "input_device_index": None,
        "evdev_device": None,
        "noise_suppression": False
    },
    "ui": {
        "show_overlay": True,
        "overlay_style": "voice_card"
    },
    "features": {
        "remove_fillers": True,
        "custom_vocabulary": [],
        "spoken_punctuation": True,
        "auto_format_lists": True,
        "quiet_mode": False,
        "show_notification": False,
        "snippets": {}
    },
    "ollama": {
        "enabled": False,
        "model": "llama3.2:1b",
        "mode": "clean"
    },
    "tts": {
        "enabled": False,
        "engine": "piper",
        "voice": "en-us-lessac-medium",
        "stop_key": ["KEY_ESCAPE"],
        "response_overlay": True
    },
    "mcp": {
        "server_enabled": False,
        "record_timeout": 15.0
    },
    "atspi": {
        "injection": True,
        "context_prompt": True,
        "auto_code_mode": True
    }
}

CONFIG_PATH = Path.home() / ".config" / "voxctl" / "config.json"

class Config:
    _MAPPING = {
        "model_size": "engine.faster_whisper.model_size",
        "device": "engine.faster_whisper.device",
        "compute_type": "engine.faster_whisper.compute_type",
        "backend_engine": "engine.backend",
        "inference_mode": "engine.inference_mode",
        "whisper_cpp_binary": "engine.whisper_cpp.binary",
        "whisper_cpp_model_dir": "engine.whisper_cpp.model_dir",
        "whisper_cpp_model_size": "engine.whisper_cpp.model_size",
        "whisper_cpp_device": "engine.whisper_cpp.device",
        "whisper_cpp_threads": "engine.whisper_cpp.threads",
        "whisper_cpp_use_bindings": "engine.whisper_cpp.use_bindings",
        "moonshine_model_size": "engine.moonshine.model_size",
        "moonshine_language": "engine.moonshine.language",
        "vad_threshold": "audio.vad_threshold",
        "min_silence_duration_ms": "audio.min_silence_duration_ms",
        "input_device_index": "audio.input_device_index",
        "evdev_device": "audio.evdev_device",
        "noise_suppression": "audio.noise_suppression",
        "show_overlay": "ui.show_overlay",
        "overlay_style": "ui.overlay_style",
        "remove_fillers": "features.remove_fillers",
        "custom_vocabulary": "features.custom_vocabulary",
        "spoken_punctuation": "features.spoken_punctuation",
        "auto_format_lists": "features.auto_format_lists",
        "quiet_mode": "features.quiet_mode",
        "show_notification": "features.show_notification",
        "snippets": "features.snippets",
        "ollama_enabled": "ollama.enabled",
        "ollama_model": "ollama.model",
        "ollama_mode": "ollama.mode",
        "tts_enabled": "tts.enabled",
        "tts_engine": "tts.engine",
        "tts_voice": "tts.voice",
        "tts_stop_key": "tts.stop_key",
        "tts_response_overlay": "tts.response_overlay",
        "mcp_server_enabled": "mcp.server_enabled",
        "mcp_record_timeout": "mcp.record_timeout",
        "atspi_injection": "atspi.injection",
        "atspi_context_prompt": "atspi.context_prompt",
        "atspi_auto_code_mode": "atspi.auto_code_mode",
    }

    def __init__(self):
        import copy
        self.config = copy.deepcopy(DEFAULT_CONFIG)
        self.load()

    def load(self):
        if CONFIG_PATH.exists():
            try:
                with open(CONFIG_PATH, "r") as f:
                    user_config = json.load(f)
                    self._merge_and_migrate(user_config)
                    self._sanitize()
            except Exception as e:
                print(f"Error loading config: {e}")

    def _merge_and_migrate(self, user_config):
        """Merge user config into default config, handling migration from flat to nested."""
        for k, v in user_config.items():
            if k in self._MAPPING:
                # Migrate old flat key to new nested path
                path = self._MAPPING[k].split(".")
                curr = self.config
                for part in path[:-1]:
                    curr = curr[part]
                curr[path[-1]] = v
            elif isinstance(v, dict) and k in self.config and isinstance(self.config[k], dict):
                # Deep merge for nested dicts
                self._deep_update(self.config[k], v)
            else:
                # New keys or keys that didn't change
                self.config[k] = v

    def _deep_update(self, d, u):
        for k, v in u.items():
            if isinstance(v, dict) and k in d and isinstance(d[k], dict):
                self._deep_update(d[k], v)
            else:
                d[k] = v

    def _sanitize(self):
        """Ensure critical string keys didn't accidentally become lists/objects."""
        # This is less critical now with mapping but good to keep for top-level if any
        pass

    def save(self):
        CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        try:
            with open(CONFIG_PATH, "w") as f:
                json.dump(self.config, f, indent=4)
        except Exception as e:
            print(f"Error saving config: {e}")

    def get(self, key, default=None):
        # Apply mapping for backward compatibility
        if key in self._MAPPING:
            key = self._MAPPING[key]

        if "." in key:
            parts = key.split(".")
            curr = self.config
            for part in parts:
                if isinstance(curr, dict) and part in curr:
                    curr = curr[part]
                else:
                    return default if default is not None else self._get_default(key)
            return curr
        return self.config.get(key, default if default is not None else self.config.get(key))

    def _get_default(self, key):
        if key in self._MAPPING:
            key = self._MAPPING[key]

        if "." in key:
            parts = key.split(".")
            curr = DEFAULT_CONFIG
            for part in parts:
                if isinstance(curr, dict) and part in curr:
                    curr = curr[part]
                else:
                    return None
            return curr
        return DEFAULT_CONFIG.get(key)

    def set(self, key, value):
        if key in self._MAPPING:
            key = self._MAPPING[key]

        if "." in key:
            parts = key.split(".")
            curr = self.config
            for part in parts[:-1]:
                if part not in curr:
                    curr[part] = {}
                curr = curr[part]
            curr[parts[-1]] = value
        else:
            self.config[key] = value
        self.save()

