# Audio Pipeline

**Crate:** `crates/voxctrl-audio/`

## Responsibilities

- Enumerate and select audio input devices
- Stream raw PCM from the microphone via CPAL
- Resample from the hardware rate to 16 kHz (Whisper's required input rate)
- Compute RMS levels for the VU meter and VAD noise gate
- Suppress background noise with RNNoise when `audio.noise_suppression` is on
- Support two streaming modes: dynamic (on-demand) and always-on

---

## Device Selection

On startup, `test_and_detect_active_device()` probes devices in priority order:

1. **Configured device** — `audio.input_device_index` from config (if non-null)
2. **Default input device** — CPAL's `default_input_device()`
3. **First enumerable device** — iterates all input devices, picks first that builds a stream

A test stream is opened on each candidate to confirm it is functional before committing. If none succeed, CPAL's default device is used as a last resort.

Devices are enumerated with `list_input_devices()`, which returns `(index: u32, name: String)` pairs for the Settings → Audio tab.

The device can be **hot-reloaded** at runtime: if `audio.input_device_index` changes (via the UI), the capture loop detects the change and re-opens the stream on the new device without restarting.

---

## Streaming Modes

### Dynamic Streaming (default, `dynamic_stream: true`)
The microphone stream is opened when recording starts and closed when it stops.

- Lower CPU/battery usage when idle
- A small startup latency (~30ms polling interval) on hotkey press
- Suitable for most users

### Always-On Streaming (`dynamic_stream: false`)
The stream stays open permanently. Chunks are forwarded to the recording buffer only while recording is active; they are discarded otherwise.

- Zero startup latency
- Higher idle CPU usage
- The stream also runs during VU meter monitoring (Settings → Audio tab)

Both modes also serve the live audio monitoring flag used by the VU meter in the Settings → Audio tab.

---

## Audio Processing Chain

```
Hardware input (e.g. 48000 Hz, f32 samples)
        │
        ▼
   apply_gain()         — multiply each sample by gain (atomic f32, live-updated)
        │
        ▼
   rms() → level_tx     — f32 RMS forwarded to UI every ~30ms for VU meter
        │
        ▼
   resample_chunk()      — linear interpolation to 16000 Hz (if hardware rate differs)
        │
        ▼
   audio_tx.send(chunk)  — Vec<f32> forwarded to lib.rs accumulator (only if recording=true)
```

### Resampling

`resample_chunk(input, from_hz, to_hz)` uses simple linear interpolation to convert from the hardware sample rate to 16 kHz. This is fast and low-latency, which is more important than perfect fidelity for speech recognition.

### Gain Control

`audio.gain` is stored as an `AtomicU32` (bit-cast from f32) in `AppState`, allowing live updates from the UI without locking the audio thread.

---

## Noise Suppression

With `audio.noise_suppression` on, each captured buffer goes through RNNoise
(the [`nnnoiseless`](https://crates.io/crates/nnnoiseless) port) before it
reaches inference. RNNoise applies per-band gains learned to keep voiced speech
and pull down steady broadband noise — fans, hiss, hum, air conditioning.

The denoiser lives in `crates/voxctrl-audio/src/denoise.rs` and reconciles three
mismatched formats inside the capture callback:

```
mic buffer (hardware rate, [-1.0, 1.0])
  → resample to 48 kHz            RNNoise runs at 48 kHz only
  → scale to 16-bit range         RNNoise wants [-32768.0, 32767.0] floats
  → process in 480-sample frames  a whole frame at a time; the remainder is
                                  carried into the next callback
  → scale back, resample to 16 kHz
```

The first frame out of RNNoise is discarded (it carries fade-in artifacts), and
because only whole frames are processed, up to 10 ms is still buffered when a
recording ends.

State is per stream: opening a stream builds a fresh denoiser, so a device
change never continues with the spectral estimate of the device it left.

**Build requirement.** RNNoise is compiled in behind the `noisereduce` cargo
feature on `voxctrl-audio`. The app enables it (`src-tauri/Cargo.toml`), so
released builds have it. A build without it logs a warning when the setting is
on and passes audio through unchanged.

**Live toggle.** `AppState.noise_suppression` mirrors the setting as an
`AtomicBool` the capture callback reads per buffer, so switching it in Settings
applies to the next recording without a restart or a stream rebuild.

---

## Noise Gate / VAD

Voice Activity Detection is applied in the inference layer (not capture), after the full recording is accumulated:

```
rms_threshold = (1.0 - audio.vad_threshold) * 0.006

IF rms(audio) < rms_threshold
THEN discard — return empty string
```

**VAD threshold interpretation:**
- `0.5` (default) → rms_threshold = 0.003 (comfortable speech easily passes)
- `1.0` (maximum sensitivity) → rms_threshold = 0.0 (no gate; all audio processed)
- `0.0` (minimum sensitivity) → rms_threshold = 0.006 (only loud audio passes)

Setting it too low (high sensitivity) can cause Whisper to transcribe silence as hallucinated text. The default 0.5 is well-calibrated for typical microphone setups.

---

## Configuration Options

All under `audio` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `input_device_index` | integer or null | `null` | CPAL device index; null = auto-detect |
| `evdev_device` | string or null | `null` | Linux evdev keyboard path, e.g. `"/dev/input/event4"`. Only used by the evdev hotkey fallback; ignored when the desktop portal is available (the normal case) |
| `gain` | float | `1.0` | Microphone amplification multiplier |
| `vad_threshold` | float | `0.5` | Sensitivity 0.0–1.0; higher = more sensitive (0.0 RMS gate at 1.0) |
| `noise_suppression` | bool | `false` | Run captured audio through RNNoise before transcription (see [Noise Suppression](#noise-suppression)) |
| `dynamic_stream` | bool | `true` | Open/close mic on demand vs. always-on |

---

## AudioRecorder

`AudioRecorder` is constructed with shared atomics from `AppState` so it reacts to live config changes without restarts:

```rust
pub struct AudioRecorder {
    config: AudioConfig,
    recording: Arc<AtomicBool>,
    monitoring: Arc<AtomicBool>,
    dynamic_stream: Arc<AtomicBool>,
    input_device_index: Arc<AtomicU32>,
    gain: Arc<AtomicU32>,
    noise_suppression: Arc<AtomicBool>,
}
```

It is started via `.run(audio_tx, level_tx, audio_ready)` which spawns the `capture_loop` on a dedicated OS thread. The loop polls every 30ms to check for device index changes, dynamic stream preference changes, and recording/monitoring state transitions.
