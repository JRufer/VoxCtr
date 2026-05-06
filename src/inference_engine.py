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

        # Routing: current session target (full OutputTarget object stored on record start)
        self._current_target_id: str = 'default'
        self._current_target = None          # OutputTarget | None
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

    # ── Effective processing config ───────────────────────────────────────────

    def _build_effective_processing(self, target=None) -> dict:
        """Merge the global config with per-target processing overrides.

        Returns a flat dict of resolved processing settings.  For globally-gated
        optional features (Ollama, noise suppression) the global master switch
        acts as an absolute OFF — a target can request the feature but it will
        not run if globally disabled.
        """
        p = target.processing if target is not None else None

        def resolve(attr, config_key, default):
            """Return target override if set, else global config value."""
            if p is not None:
                val = getattr(p, attr, None)
                if val is not None:
                    return val
            return self.config.get(config_key, default)

        # Globally-gated features: global OFF wins unconditionally
        global_ollama = self.config.get("ollama_enabled", False)
        global_noise  = self.config.get("noise_suppression", False)

        target_wants_ollama = resolve("ollama_enabled", "ollama_enabled", False)
        target_wants_noise  = resolve("noise_suppression", "noise_suppression", False)

        return {
            # ── Preprocessing ───────────────────────────────────────────────
            "noise_suppression": global_noise and target_wants_noise,
            "quiet_mode":        resolve("quiet_mode", "quiet_mode", False),
            "atspi_context":     resolve("atspi_context", "atspi_context_prompt", True),

            # ── Postprocessing ──────────────────────────────────────────────
            "remove_fillers":     resolve("remove_fillers", "remove_fillers", True),
            "spoken_punctuation": resolve("spoken_punctuation", "spoken_punctuation", True),
            "auto_format_lists":  resolve("auto_format_lists", "auto_format_lists", True),
            "apply_snippets":     resolve("apply_snippets", "apply_snippets", True),
            "code_mode":          resolve("code_mode", "dictation_mode", "normal") == "code"
                                  if p is None or p.code_mode is None
                                  else bool(p.code_mode),

            # ── Ollama / LLM ────────────────────────────────────────────────
            # global OFF → feature off; target can opt-in within global budget
            "ollama_enabled": global_ollama and target_wants_ollama,
            "ollama_model":   resolve("ollama_model", "ollama_model", "llama3.2:1b"),
            "ollama_mode":    resolve("ollama_mode", "ollama_mode", "clean"),
            "ollama_prompt":  p.ollama_prompt if p is not None else None,
        }

    # ── Initial prompt ────────────────────────────────────────────────────────

    def _build_initial_prompt(self, effective: dict):
        if self._current_initial_prompt_override is not None:
            return self._current_initial_prompt_override

        parts = []

        if effective.get("atspi_context", True):
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

    def _postprocess(self, text, effective: dict) -> str:
        """Apply the full postprocessing pipeline using resolved effective settings."""
        # Force code mode when AT-SPI2 detected an IDE/terminal (unless overridden by target)
        force_code = (
            getattr(self, '_atspi_forced_code_mode', False)
            and self._current_target is not None
            and self._current_target.processing.code_mode is None
        )
        use_code_mode = effective.get("code_mode", False) or force_code

        if use_code_mode:
            text = self._apply_code_mode(text)
        else:
            if effective.get("remove_fillers", True):
                text = _FILLER_RE.sub('', text)
                text = re.sub(r'\s+', ' ', text).strip()
                if text:
                    text = text[0].upper() + text[1:]
            if effective.get("spoken_punctuation", True):
                text = self._apply_spoken_punctuation(text)
            if effective.get("auto_format_lists", True):
                text = self._apply_list_formatting(text)

        if effective.get("apply_snippets", True):
            text = self._apply_snippets(text)

        return text

    # ── Recording state ───────────────────────────────────────────────────────

    def set_recording(self, recording, target_id='default',
                      target=None, initial_prompt_override=None):
        """Start or stop a recording session.

        Args:
            recording: True to start, False to stop.
            target_id: ID of the OutputTarget for this session.
            target: Full OutputTarget object (for per-target processing config).
            initial_prompt_override: Explicit Whisper initial prompt; None = auto-build.
        """
        self.recording = recording
        if recording:
            self._current_target_id = target_id
            self._current_target = target
            self._current_initial_prompt_override = (
                initial_prompt_override
                if initial_prompt_override is not None
                else (target.initial_prompt if target else None)
            )
            self._atspi_context_at_start = self._capture_atspi_context(
                target.processing if target else None
            )
        else:
            self.process_buffer(incremental=False)
            with self._buffer_lock:
                self.buffer.clear()

    def _capture_atspi_context(self, processing=None):
        """Snapshot the focused widget's AT-SPI2 context at the moment recording starts."""
        # Per-target override takes precedence; fall back to global setting
        use_atspi = (
            processing.atspi_context
            if processing is not None and processing.atspi_context is not None
            else self.config.get('atspi_context_prompt', True)
        )
        if not use_atspi:
            return None
        try:
            import atspi_context
            ctx = atspi_context.get_focused_context(max_chars=300)
            if ctx is None:
                return None
            if (ctx.is_code_context
                    and self.config.get('atspi_auto_code_mode', True)
                    and self.config.get('dictation_mode', 'normal') == 'normal'
                    and (processing is None or processing.code_mode is None)):
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
            effective = self._build_effective_processing(self._current_target)

            with self._model_lock:
                backend = self._backend

            if backend is None:
                return

            # VAD threshold: per-target quiet_mode overrides global setting
            quiet = effective.get("quiet_mode", False)
            vad_threshold = 0.2 if quiet else self.config.get("vad_threshold", 0.5)

            initial_prompt = self._build_initial_prompt(effective)

            if isinstance(backend, FasterWhisperBackend):
                result = backend.transcribe_with_vad(
                    audio_data,
                    vad_parameters=dict(
                        min_silence_duration_ms=self.config.get("min_silence_duration_ms", 500),
                        threshold=vad_threshold,
                    ),
                    initial_prompt=initial_prompt,
                )
            else:
                result = backend.transcribe(
                    audio_data,
                    initial_prompt=initial_prompt,
                )

            self._last_language = result.language
            self._last_language_prob = result.language_probability

            full_text = self._postprocess(result.text.strip(), effective)

            if full_text and not incremental:
                full_text = self.llm.process_with_target(full_text, effective) or full_text

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
