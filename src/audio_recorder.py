import pyaudio
import threading
import queue
import time
import numpy as np

# P2.2: Optional noise suppression via noisereduce (pip install noisereduce)
try:
    import noisereduce as _nr
    _HAS_NOISEREDUCE = True
except ImportError:
    _nr = None
    _HAS_NOISEREDUCE = False

class AudioRecorder(threading.Thread):
    def __init__(self, config, audio_queue):
        super().__init__(daemon=True)
        self.config = config
        self.audio_queue = audio_queue
        self.p = pyaudio.PyAudio()
        self.stream = None
        self.actual_rate = 16000 # Will be updated on start
        self.target_rate = 16000
        self.chunk_size = 1024
        self.running = True
        self.recording = False
        self.monitoring = False
        self._last_rms = 0.0
        self.visualizer_callbacks = []  # List of functions(np.array)
        self._lock = threading.Lock()

    def start_recording(self):
        with self._lock:
            self.recording = True
            if self.stream is None:
                self._open_stream()

    def stop_recording(self):
        """Mark recording stopped. The stream stays open to avoid ALSA xrun crashes.
        The run() loop owns the stream; we only set the flag here."""
        with self._lock:
            self.recording = False
            # Do NOT close the stream here — closing from a different thread while
            # run() holds a reference to it causes ALSA xrun / mmap errors.
            # The stream will close when monitoring also stops, or on full stop().
            if not self.monitoring:
                # Schedule a deferred close via a flag; run() will close safely.
                self._pending_close = True

    def start_monitoring(self):
        """Opens mic for VU meter/visuals without sending to transcription queue."""
        with self._lock:
            self._pending_close = False
            self.monitoring = True
            if self.stream is None:
                self._open_stream()

    def stop_monitoring(self):
        with self._lock:
            self.monitoring = False
            if not self.recording:
                self._pending_close = True

    def _open_stream(self):
        try:
            device_index = self.config.get("input_device_index")
            print(f"[Audio] Opening device index: {device_index}")
            
            # Try to find supported sample rate
            rates_to_try = [16000, 44100, 48000, 32000, 22050]
            for rate in rates_to_try:
                try:
                    self.stream = self.p.open(
                        format=pyaudio.paInt16,
                        channels=1,
                        rate=rate,
                        input=True,
                        input_device_index=device_index,
                        frames_per_buffer=self.chunk_size
                    )
                    self.actual_rate = rate
                    print(f"[Audio] Opened stream at {rate}Hz")
                    break
                except Exception:
                    continue
            
            if not self.stream:
                print("[Audio] Error: No supported sample rate found.")
        except Exception as e:
            print(f"[Audio] Failed to open stream: {e}")

    def _close_stream(self):
        """Must only be called from within the run() thread or stop()."""
        if self.stream:
            try:
                self.stream.stop_stream()
                self.stream.close()
            except Exception:
                pass
            self.stream = None
            print("[Audio] Stream closed.")

    def run(self):
        self._pending_close = False
        while self.running:
            # Check if a deferred close was requested by stop_recording()
            with self._lock:
                pending = getattr(self, '_pending_close', False)
                if pending and not self.recording and not self.monitoring:
                    self._pending_close = False
                    self._close_stream()

            with self._lock:
                stream = self.stream
                is_recording = self.recording

            if stream:
                try:
                    data = stream.read(self.chunk_size, exception_on_overflow=False)
                except Exception as e:
                    if self.recording or self.monitoring:
                        print(f"[Audio] Read error: {e}")
                    # On fatal stream error, close safely from this thread
                    with self._lock:
                        self.recording = False
                        self.monitoring = False
                        self._pending_close = False
                    self._close_stream()
                    continue
            else:
                time.sleep(0.01)
                continue

            try:
                # Apply Gain
                gain = self.config.get("mic_gain", 1.0)
                if self.config.get("quiet_mode", False):
                    gain = min(gain * 2.5, 10.0)
                
                audio_data = np.frombuffer(data, dtype=np.int16).astype(np.float32)

                if gain != 1.0:
                    audio_data *= gain
                    audio_data = np.clip(audio_data, -32768, 32767)

                # Noise suppression
                if _HAS_NOISEREDUCE and self.config.get("noise_suppression", False):
                    try:
                        normalised = audio_data / 32768.0
                        cleaned = _nr.reduce_noise(
                            y=normalised,
                            sr=self.actual_rate,
                            stationary=True,
                            prop_decrease=0.75,
                        )
                        audio_data = (cleaned * 32768.0).astype(np.float32)
                    except Exception as nr_err:
                        print(f"[NR] noise reduction skipped: {nr_err}")

                # Calculate RMS for VU meter
                rms = np.sqrt(np.mean(audio_data**2))
                self._last_rms = rms / 32768.0  # Normalized to 0.0-1.0 approx

                # Dispatch to visualizers
                for cb in self.visualizer_callbacks:
                    try:
                        cb(audio_data)
                    except Exception:
                        pass

                # Only queue for transcription if recording is actually ON
                if is_recording:
                    if self.actual_rate != self.target_rate:
                        num_samples = int(len(audio_data) * self.target_rate / self.actual_rate)
                        resampled_audio = np.interp(
                            np.linspace(0.0, 1.0, num_samples, endpoint=False),
                            np.linspace(0.0, 1.0, len(audio_data), endpoint=False),
                            audio_data
                        ).astype(np.int16)
                        self.audio_queue.put(resampled_audio.tobytes())
                    else:
                        self.audio_queue.put(audio_data.astype(np.int16).tobytes())
            except Exception as e:
                print(f"[Audio] Process error: {e}")

    def get_rms_level(self) -> float:
        """Returns normalized RMS level (0.0 to 1.0) of the last captured chunk."""
        return self._last_rms

    def stop(self):
        self.running = False
        with self._lock:
            self.recording = False
            self.monitoring = False
            self._pending_close = False
        # Give the run() loop one iteration to exit, then close from here
        time.sleep(0.05)
        self._close_stream()
        self.p.terminate()

