from llm_postprocessor import LLMPostprocessor
from backends.selector import select_backend, probe_gpu, auto_compute_type
from backends.faster_whisper_backend import FasterWhisperBackend
import numpy as np
import threading
import queue
import time
import os
import re

# Matches standalone filler sounds, with an optional trailing comma+space
_FILLER_RE = re.compile(
    r'\b(uh+|um+|hmm+|hm+|er+|ah)\b,?\s*',
    re.IGNORECASE
)

# Spoken punctuation: ordered longest-first so multi-word phrases match before single words
_PUNCT_MAP = [
    (re.compile(r'\bnew paragraph\b',       re.IGNORECASE), '\n\n'),
    (re.compile(r'\bnew line\b',            re.IGNORECASE), '\n'),
    (re.compile(r'\bopen paren(?:thesis)?\b', re.IGNORECASE), '('),
    (re.compile(r'\bclose paren(?:thesis)?\b', re.IGNORECASE), ')'),
    (re.compile(r'\bopen bracket\b',        re.IGNORECASE), '['),
    (re.compile(r'\bclose bracket\b',       re.IGNORECASE), ']'),
    (re.compile(r'\bopen brace\b',          re.IGNORECASE), '{'),
    (re.compile(r'\bclose brace\b',         re.IGNORECASE), '}'),
    (re.compile(r'\bexclamation(?: mark| point)?\b', re.IGNORECASE), '!'),
    (re.compile(r'\bquestion mark\b',       re.IGNORECASE), '?'),
    (re.compile(r'\bfull stop\b',           re.IGNORECASE), '.'),
    (re.compile(r'\bellipsis\b',            re.IGNORECASE), '...'),
    (re.compile(r'\bsemicolon\b',           re.IGNORECASE), ';'),
    (re.compile(r'\bcolon\b',               re.IGNORECASE), ':'),
    (re.compile(r'\bcomma\b',               re.IGNORECASE), ','),
    (re.compile(r'\bperiod\b',              re.IGNORECASE), '.'),
    (re.compile(r'\bdash\b',               re.IGNORECASE), '–'),
    (re.compile(r'\bhyphen\b',              re.IGNORECASE), '-'),
    (re.compile(r'\bat sign\b',             re.IGNORECASE), '@'),
    (re.compile(r'\bhash(?:tag)?\b',        re.IGNORECASE), '#'),
    (re.compile(r'\bampersand\b',           re.IGNORECASE), '&'),
    (re.compile(r'\bpercent(?: sign)?\b',   re.IGNORECASE), '%'),
    (re.compile(r'\bdollar(?: sign)?\b',    re.IGNORECASE), '$'),
]

# Numbered list detector: two or more "N. text" items
_LIST_ITEM_RE = re.compile(r'(?:^|\s)(\d+)\.\s+(.+?)(?=\s+\d+\.|$)', re.DOTALL)

# Code-mode spoken construct map (applied in order, longest first)
_CODE_MAP = [
    (re.compile(r'\bcamel case\b',      re.IGNORECASE), ''),          # join next word without space
    (re.compile(r'\bsnake case\b',       re.IGNORECASE), ''),          # join next word with _
    (re.compile(r'\bunderscore\b',       re.IGNORECASE), '_'),
    (re.compile(r'\bdouble underscore\b',re.IGNORECASE), '__'),
    (re.compile(r'\bdot\b',             re.IGNORECASE), '.'),
    (re.compile(r'\bslash\b',           re.IGNORECASE), '/'),
    (re.compile(r'\bbackslash\b',       re.IGNORECASE), '\\\\'),
    (re.compile(r'\bdash\b',            re.IGNORECASE), '-'),
    (re.compile(r'\bequals\b',          re.IGNORECASE), '='),
    (re.compile(r'\bcolon\b',           re.IGNORECASE), ':'),
    (re.compile(r'\bsemicolon\b',       re.IGNORECASE), ';'),
    (re.compile(r'\bopen paren\b',      re.IGNORECASE), '('),
    (re.compile(r'\bclose paren\b',     re.IGNORECASE), ')'),
    (re.compile(r'\bopen bracket\b',    re.IGNORECASE), '['),
    (re.compile(r'\bclose bracket\b',   re.IGNORECASE), ']'),
    (re.compile(r'\bopen brace\b',      re.IGNORECASE), '{'),
    (re.compile(r'\bclose brace\b',     re.IGNORECASE), '}'),
    (re.compile(r'\bnew line\b',         re.IGNORECASE), '\n'),
    (re.compile(r'\btab\b',             re.IGNORECASE), '\t'),
    (re.compile(r'\bspace\b',           re.IGNORECASE), ' '),
    (re.compile(r'\bgreater than\b',    re.IGNORECASE), '>'),
    (re.compile(r'\bless than\b',       re.IGNORECASE), '<'),
    (re.compile(r'\bampersand\b',       re.IGNORECASE), '&'),
    (re.compile(r'\bpipe\b',            re.IGNORECASE), '|'),
    (re.compile(r'\bat sign\b',         re.IGNORECASE), '@'),
    (re.compile(r'\bhash\b',            re.IGNORECASE), '#'),
    (re.compile(r'\bstar\b',            re.IGNORECASE), '*'),
]


class InferenceEngine(threading.Thread):
    def __init__(self, config, audio_queue, text_queue, realtime_text_queue):
        super().__init__(daemon=True)
        self.config = config
        self.audio_queue = audio_queue
        self.text_queue = text_queue
        self.realtime_text_queue = realtime_text_queue
        self.running = True
        self.recording = False
        self.buffer = bytearray()
        self._buffer_lock = threading.Lock()
        self._model_lock = threading.Lock()

        self.actual_device = "Unknown"
        self.actual_compute_type = "Unknown"
        self.active_backend_name = "Unknown"

        # Routing: current session target
        self._current_target_id: str = 'default'
        self._current_post_processing: str = 'default'
        self._current_initial_prompt_override: str | None = None
        self._atspi_context_at_start = None  # FocusContext snapshot taken on recording start

        # P2.1: LLM post-processor (probe happens lazily on first use)
        self.llm = LLMPostprocessor(config)

        # Setup CUDA environment for pip-installed libraries
        self._setup_cuda_env()

        # Select and load backend
        self._backend = None
        self._load_backend()
        print("Model loaded.")

        # Kick off Ollama probe in background so it doesn't delay startup
        threading.Thread(
            target=self.llm.probe, daemon=True, name="ollama-probe"
        ).start()

    def _setup_cuda_env(self):
        """
        Arch Linux workaround: Discover and preload pip-installed CUDA/cuDNN libraries
        to satisfy ctranslate2's dependencies when system versions are mismatched.
        """
        import ctypes
        import glob

        home = os.path.expanduser("~")
        python_version = f"{os.sys.version_info.major}.{os.sys.version_info.minor}"
        pip_base = os.path.join(home, ".local", "lib", f"python{python_version}", "site-packages", "nvidia")

        lib_search_paths = [
            os.path.join(pip_base, "cublas", "lib"),
            os.path.join(pip_base, "cudnn", "lib"),
            os.path.join(pip_base, "cuda_runtime", "lib"),
        ]

        existing_ld = os.environ.get("LD_LIBRARY_PATH", "")
        new_paths = []

        for p in lib_search_paths:
            if os.path.isdir(p):
                new_paths.append(p)
                for lib_name in ["libcublas.so.*", "libcublasLt.so.*", "libcudnn.so.*"]:
                    for lib_path in glob.glob(os.path.join(p, lib_name)):
                        try:
                            ctypes.CDLL(lib_path, mode=ctypes.RTLD_GLOBAL)
                        except Exception:
                            pass

        if new_paths:
            os.environ["LD_LIBRARY_PATH"] = ":".join(new_paths + ([existing_ld] if existing_ld else []))
            print(f"CUDA Environment: Injected {len(new_paths)} library paths from ~/.local")

    def _load_backend(self):
        """Select and load the appropriate transcription backend."""
        backend = select_backend(self.config)
        model_size, device, compute_type = self._resolve_load_params(backend)

        try:
            print(f"Loading backend '{backend.name}' model='{model_size}' device='{device}'...")
            self._verify_and_load(backend, model_size, device, compute_type)
        except FileNotFoundError as e:
            # Model file missing — fall back to faster-whisper gracefully
            print(f"[Engine] Model not found: {e}")
            print("[Engine] Falling back to faster-whisper backend...")
            from backends.faster_whisper_backend import FasterWhisperBackend as _FW
            backend = _FW()
            model_size = self.config.get("model_size", "base")
            device = self.config.get("device", "auto")
            gpu = probe_gpu()
            compute_type = auto_compute_type("faster-whisper", gpu)
            self._verify_and_load(backend, model_size, device, compute_type)
        except Exception as e:
            # Attempt CPU fallback if loading failed on GPU
            if isinstance(backend, FasterWhisperBackend) and device != "cpu":
                print(f"GPU load failed ({e}), falling back to CPU...")
                self.config.set("device", "cpu")
                self.config.set("compute_type", "int8")
                self.config.save()
                device, compute_type = "cpu", "int8"
                self._verify_and_load(backend, model_size, device, compute_type)
            else:
                raise

        self._backend = backend
        self.active_backend_name = backend.name
        self.actual_device = device
        self.actual_compute_type = compute_type

    def _resolve_load_params(self, backend) -> tuple[str, str, str]:
        """Return (model_size, device, compute_type) for the given backend."""
        from backends.whisper_cpp_backend import WhisperCppBackend

        if isinstance(backend, WhisperCppBackend):
            model_size = self.config.get("whisper_cpp_model_size", "large-v3")
            device = self.config.get("whisper_cpp_device", "auto")
            gpu = probe_gpu()
            compute_type = auto_compute_type("whisper-cpp", gpu)
            return model_size, device, compute_type

        # FasterWhisperBackend
        model_size = self.config.get("model_size", "base")
        device = self.config.get("device", "auto")
        compute_type = self.config.get("compute_type", "default")
        if compute_type == "default":
            gpu = probe_gpu()
            compute_type = auto_compute_type("faster-whisper", gpu)
        return model_size, device, compute_type

    def _verify_and_load(self, backend, model_size, device, compute_type):
        backend.load_model(model_size, device, compute_type)

        # Smoke test: verify the model actually produces output
        if isinstance(backend, FasterWhisperBackend):
            dummy = np.zeros(16000, dtype=np.float32)
            backend.transcribe_with_vad(dummy)

    def switch_backend(self, new_engine: str):
        """
        Hot-swap the active backend at runtime (called from the settings UI).
        Disabling recording while the swap is in progress is the caller's responsibility.
        """
        with self._model_lock:
            if self._backend is not None:
                self._backend.unload_model()
                self._backend = None

            self.config.set("backend_engine", new_engine)
            self.config.save()
            self._load_backend()

    def _build_initial_prompt(self):
        if self._current_initial_prompt_override is not None:
            return self._current_initial_prompt_override

        parts = []

        # Prepend surrounding text captured at recording start so Whisper can
        # match vocabulary and sentence style to the current document context.
        ctx = self._atspi_context_at_start
        if ctx and ctx.surrounding_text.strip():
            parts.append(ctx.surrounding_text.strip())

        vocab = self.config.get("custom_vocabulary", [])
        if vocab:
            parts.append(", ".join(vocab))

        return " ".join(parts) if parts else None

    def _apply_spoken_punctuation(self, text):
        for pattern, symbol in _PUNCT_MAP:
            text = pattern.sub(lambda m: symbol, text)
        text = re.sub(r' +', ' ', text).strip()
        return text

    def _apply_list_formatting(self, text):
        items = _LIST_ITEM_RE.findall(text)
        if len(items) < 2:
            return text
        return '\n'.join(f"{num}. {content.strip()}" for num, content in items)

    def _apply_snippets(self, text):
        snippets = self.config.get("snippets", {})
        if not snippets:
            return text
        for trigger in sorted(snippets, key=len, reverse=True):
            expansion = snippets[trigger]
            pattern = re.compile(re.escape(trigger.strip()), re.IGNORECASE)
            text = pattern.sub(lambda _: expansion, text)
        return text

    def _apply_code_mode(self, text):
        for pattern, symbol in _CODE_MAP:
            text = pattern.sub(symbol, text)
        text = re.sub(r'[ \t]+', ' ', text).strip()
        return text

    def _postprocess(self, text):
        pp = self._current_post_processing

        if pp == 'none':
            return text

        if pp == 'strip_fillers':
            text = _FILLER_RE.sub('', text)
            text = re.sub(r'\s+', ' ', text).strip()
            if text:
                text = text[0].upper() + text[1:]
            return text

        if pp == 'snippets_only':
            return self._apply_snippets(text)

        if pp == 'ollama_only':
            return text  # Ollama applied later in process_buffer

        # 'default' or unknown: full pipeline
        mode = self.config.get("dictation_mode", "normal")
        if getattr(self, '_atspi_forced_code_mode', False):
            mode = "code"
        if mode == "code":
            text = self._apply_code_mode(text)
        else:
            if self.config.get("remove_fillers", True):
                text = _FILLER_RE.sub('', text)
                text = re.sub(r'\s+', ' ', text).strip()
                if text:
                    text = text[0].upper() + text[1:]
            if self.config.get("spoken_punctuation", True):
                text = self._apply_spoken_punctuation(text)
            if self.config.get("auto_format_lists", True):
                text = self._apply_list_formatting(text)

        text = self._apply_snippets(text)
        return text

    def set_recording(self, recording, target_id='default',
                      post_processing='default', initial_prompt_override=None):
        self.recording = recording
        if recording:
            self._current_target_id = target_id
            self._current_post_processing = post_processing
            self._current_initial_prompt_override = initial_prompt_override
            self._atspi_context_at_start = self._capture_atspi_context()
        else:
            self.process_buffer(incremental=False)
            with self._buffer_lock:
                self.buffer.clear()

    def _capture_atspi_context(self):
        """Snapshot the focused widget's AT-SPI2 context at the moment recording starts."""
        if not self.config.get('atspi_context_prompt', True):
            return None
        try:
            import atspi_context
            ctx = atspi_context.get_focused_context(max_chars=300)
            if ctx is None:
                return None
            # Auto-switch to code mode when focused on a terminal or IDE text widget,
            # unless the user has explicitly overridden dictation_mode in settings.
            if (ctx.is_code_context
                    and self.config.get('atspi_auto_code_mode', True)
                    and self.config.get('dictation_mode', 'normal') == 'normal'):
                self._atspi_forced_code_mode = True
            else:
                self._atspi_forced_code_mode = False
            return ctx
        except Exception:
            return None

    @property
    def last_language(self):
        return getattr(self, '_last_language', None)

    @property
    def last_language_prob(self):
        return getattr(self, '_last_language_prob', None)

    def process_buffer(self, incremental=True):
        with self._buffer_lock:
            if not self.buffer:
                return
            audio_bytes = bytes(self.buffer)

        audio_data = np.frombuffer(audio_bytes, dtype=np.int16).astype(np.float32) / 32768.0

        try:
            quiet = self.config.get("quiet_mode", False)
            vad_threshold = 0.2 if quiet else self.config.get("vad_threshold", 0.5)

            with self._model_lock:
                backend = self._backend

            if backend is None:
                return

            # FasterWhisperBackend has an extended method exposing VAD filtering
            if isinstance(backend, FasterWhisperBackend):
                result = backend.transcribe_with_vad(
                    audio_data,
                    vad_parameters=dict(
                        min_silence_duration_ms=self.config.get("min_silence_duration_ms", 500),
                        threshold=vad_threshold,
                    ),
                    initial_prompt=self._build_initial_prompt(),
                )
            else:
                result = backend.transcribe(
                    audio_data,
                    initial_prompt=self._build_initial_prompt(),
                )

            self._last_language = result.language
            self._last_language_prob = result.language_probability

            full_text = self._postprocess(result.text.strip())

            if full_text and not incremental:
                full_text = self.llm.process(full_text) or full_text

            if full_text:
                if incremental:
                    self.realtime_text_queue.put(full_text)
                else:
                    target_id = self._current_target_id
                    self.text_queue.put((full_text, target_id))
            elif not incremental:
                self.realtime_text_queue.put("")

        except Exception as e:
            print(f"Inference error: {e}")
            if "libcublas" in str(e) or "cuda" in str(e).lower():
                print("Critical GPU error during transcription. Attempting CPU fallback...")
                try:
                    with self._model_lock:
                        if self._backend is not None:
                            self._backend.unload_model()
                        self.config.set("device", "cpu")
                        self.config.set("compute_type", "int8")
                        self.config.save()
                        self._load_backend()
                except Exception as reload_err:
                    print(f"Failed to reload backend: {reload_err}")

    def run(self):
        last_proc_time = time.time()
        while self.running:
            try:
                try:
                    while True:
                        chunk = self.audio_queue.get_nowait()
                        if self.recording:
                            with self._buffer_lock:
                                self.buffer.extend(chunk)
                except queue.Empty:
                    pass

                mode = self.config.get("inference_mode", "Balanced")
                interval = 0.5 if mode == "Aggressive" else 1.5

                if self.recording and time.time() - last_proc_time > interval:
                    self.process_buffer(incremental=True)
                    last_proc_time = time.time()

                time.sleep(0.05)
            except Exception as e:
                print(f"Error in inference loop: {e}")

    def stop(self):
        self.running = False
