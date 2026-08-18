use std::collections::HashSet;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}};

use tokio::sync::Mutex;
use voxctrl_config::Config;
use voxctrl_routing::OutputTargetRouter;

/// All shared mutable state, behind Arc so it can be handed to Tauri commands.
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub router: Arc<OutputTargetRouter>,

    /// True while a hotkey hold/toggle is active (recording)
    pub recording: Arc<AtomicBool>,
    /// True while speech transcription/OpenAI post-processing is running
    pub processing: Arc<AtomicBool>,
    /// True while TTS is playing back
    pub speaking: Arc<AtomicBool>,
    /// Live mirror of `ui.show_overlay` so the hot status-forwarding loops can
    /// gate the native overlay without locking the config on every audio sample.
    pub overlay_enabled: Arc<AtomicBool>,
    /// True while MCP server is actively recording/listening to the microphone
    pub mcp_recording: Arc<AtomicBool>,
    /// True while the user is recording a new keybind in the settings UI.
    /// Any hotkey gesture event received while this is true is silently dropped,
    /// so the user cannot accidentally trigger dictation while pressing keys
    /// to set up a new binding.
    pub hotkeys_inhibited: Arc<AtomicBool>,
    /// True when dynamic stream has successfully opened and is active (Option A)
    pub audio_ready: Arc<AtomicBool>,
    /// Live sync atomic flag for dynamic stream preference
    pub dynamic_stream: Arc<AtomicBool>,
    /// True when Svelte settings Audio tab is actively monitoring audio level
    pub monitoring: Arc<AtomicBool>,
    /// Live input device index, mapped to u32::MAX when None (default system device)
    pub input_device_index: Arc<AtomicU32>,
    /// Live gain value, stored as f32 bits
    pub gain: Arc<AtomicU32>,

    /// Total words injected this session
    pub word_count: Arc<std::sync::atomic::AtomicU32>,

    /// Most recent transcription result (shown in history + overlay)
    pub last_text: Arc<Mutex<String>>,

    /// Monotonic counter — incremented each time last_text is written.
    /// MCP transcribe_voice uses this to detect a new result vs a stale one.
    pub last_text_version: Arc<std::sync::atomic::AtomicU64>,

    /// Currently active dictation target ID
    pub active_target: Arc<Mutex<String>>,

    /// Currently active keybind display name/label
    pub active_binding_label: Arc<Mutex<String>>,

    /// Currently active hotkey binding ID
    pub active_binding_id: Arc<Mutex<String>>,

    /// Currently configured target definitions (in-memory cache for fast lookups)
    pub targets: Arc<Mutex<Vec<voxctrl_routing::OutputTarget>>>,

    /// Transcript history — most recent first
    pub history: Arc<Mutex<Vec<HistoryEntry>>>,

    /// Channel sender to send empty audio chunks as sentinels to unblock the coordinator thread
    pub audio_tx: crossbeam_channel::Sender<Vec<f32>>,

    /// Playback engine handle
    pub tts_handle: Arc<Mutex<Option<voxctrl_tts::TtsEngineHandle>>>,

    /// Set of active FIFO response pipes currently being listened to
    pub active_fifos: Arc<Mutex<HashSet<String>>>,

    /// Channel for sending hotkey configurations directly to background threads
    pub hotkey_reloader: Arc<Mutex<Option<crossbeam_channel::Sender<Vec<voxctrl_routing::HotkeyBinding>>>>>,

    /// Channel for forwarding hotkey gestures from listener to app coordinator
    pub hotkey_gesture_tx: Arc<Mutex<Option<voxctrl_hotkeys::GestureSender>>>,

    /// Live view of whether the global hotkey listener can actually see a
    /// keyboard. Drives the "finish setup" warnings — without it a missing
    /// permission is indistinguishable from a hotkey the user never pressed.
    pub hotkey_health: Arc<voxctrl_hotkeys::ListenerHealth>,

    /// Channel sender to forward stdin messages to the native Slint overlay process
    pub overlay_tx: crossbeam_channel::Sender<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub text: String,
    pub target_id: String,
    pub timestamp: String,
    pub inference_ms: u32,
}

impl AppState {
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }

    pub fn is_hotkeys_inhibited(&self) -> bool {
        self.hotkeys_inhibited.load(Ordering::SeqCst)
    }

    pub fn set_hotkeys_inhibited(&self, v: bool) {
        self.hotkeys_inhibited.store(v, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::SeqCst)
    }

    pub fn set_processing(&self, v: bool) {
        self.processing.store(v, Ordering::SeqCst);
    }

    pub fn is_audio_ready(&self) -> bool {
        self.audio_ready.load(Ordering::SeqCst)
    }

    pub fn set_dynamic_stream(&self, v: bool) {
        self.dynamic_stream.store(v, Ordering::SeqCst);
    }

    pub fn set_monitoring(&self, v: bool) {
        self.monitoring.store(v, Ordering::SeqCst);
        if !v {
            let _ = self.audio_tx.send(Vec::new());
        }
    }

    pub fn set_input_device_index(&self, v: Option<u32>) {
        self.input_device_index.store(v.unwrap_or(u32::MAX), Ordering::SeqCst);
    }

    pub fn set_gain(&self, v: f32) {
        self.gain.store(v.to_bits(), Ordering::SeqCst);
    }

    pub fn set_recording(&self, v: bool) {
        self.recording.store(v, Ordering::SeqCst);
        if !v {
            let _ = self.audio_tx.send(Vec::new());
        }
    }

    pub fn set_speaking(&self, v: bool) {
        self.speaking.store(v, Ordering::SeqCst);
    }

    pub fn is_overlay_enabled(&self) -> bool {
        self.overlay_enabled.load(Ordering::SeqCst)
    }

    pub fn set_overlay_enabled(&self, v: bool) {
        self.overlay_enabled.store(v, Ordering::SeqCst);
    }

    pub fn is_mcp_recording(&self) -> bool {
        self.mcp_recording.load(Ordering::SeqCst)
    }

    pub fn set_mcp_recording(&self, v: bool) {
        self.mcp_recording.store(v, Ordering::SeqCst);
    }

    pub fn increment_words(&self, n: u32) {
        self.word_count.fetch_add(n, Ordering::SeqCst);
    }

    pub fn total_words(&self) -> u32 {
        self.word_count.load(Ordering::SeqCst)
    }

    pub async fn spawn_fifo_responders(&self, tts: voxctrl_tts::TtsEngineHandle) {
        let targets_guard = self.targets.lock().await;
        let mut active_fifos_guard = self.active_fifos.lock().await;

        for target in targets_guard.iter() {
            if let Some(ref pipe_path) = target.response_pipe {
                if !pipe_path.trim().is_empty() && !active_fifos_guard.contains(pipe_path) {
                    active_fifos_guard.insert(pipe_path.clone());
                    let tts_clone = tts.clone();
                    let pipe_path_clone = pipe_path.clone();
                    tokio::spawn(async move {
                        voxctrl_tts::run_fifo_responder(pipe_path_clone, tts_clone).await;
                    });
                }
            }
        }
    }
}
