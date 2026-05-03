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
        self.recording = False
        self.running = True
        self.chunk_size = 1024
        self.target_rate = 16000 # Whisper expects 16kHz
        self.actual_rate = 16000 # Will be updated on start
        self.visualizer_callback = None
        self._lock = threading.Lock()

    def start_recording(self):
        if self.recording:
            return
        
        try:
            device_index = self.config.get("input_device_index")
            print(f"Starting recording on device index: {device_index}")
            
            # Try to find supported sample rate
            rates_to_try = [16000, 44100, 48000, 32000, 22050]
            self.stream = None
            
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
                    print(f"Successfully opened stream at {rate}Hz")
                    break
                except Exception:
                    continue
            
            if not self.stream:
                raise Exception("Could not find a supported sample rate for this device.")
                
            self.recording = True
            print("Recording started...")
        except Exception as e:
            print(f"Failed to start recording: {e}")

    def stop_recording(self):
        with self._lock:
            if not self.recording:
                return
            self.recording = False
            if self.stream:
                try:
                    self.stream.stop_stream()
                    self.stream.close()
                except:
                    pass
                self.stream = None
        print("Recording stopped.")

    def run(self):
        while self.running:
            data = None
            # Capture stream reference under lock, then read outside it so
            # stop_recording() is never blocked waiting for a chunk to finish.
            with self._lock:
                stream = self.stream if self.recording else None

            if stream:
                try:
                    data = stream.read(self.chunk_size, exception_on_overflow=False)
                except Exception as e:
                    if self.recording:
                        print(f"Error reading audio: {e}")
                    if "Unanticipated host error" in str(e):
                        self.recording = False

            if data:
                try:
                    # Apply Gain (Boost); double up for quiet/whisper mode (P0.4)
                    gain = self.config.get("mic_gain", 1.0)
                    if self.config.get("quiet_mode", False):
                        gain = min(gain * 2.5, 10.0)  # cap at 10x to avoid clipping
                    audio_data = np.frombuffer(data, dtype=np.int16).astype(np.float32)

                    if gain != 1.0:
                        audio_data *= gain
                        audio_data = np.clip(audio_data, -32768, 32767)

                    # P2.2: Noise suppression (stationary mode — low latency)
                    if _HAS_NOISEREDUCE and self.config.get("noise_suppression", False):
                        try:
                            # noisereduce expects float32 in [-1, 1]
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

                    if self.visualizer_callback:
                        self.visualizer_callback(audio_data)

                    if self.actual_rate != self.target_rate:
                        # Resample to 16kHz
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
                    print(f"Error processing audio data: {e}")
            else:
                time.sleep(0.01)

    def stop(self):
        self.running = False
        self.stop_recording()
        self.p.terminate()
