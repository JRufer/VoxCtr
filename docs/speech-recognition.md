# Speech Recognition

**Crate:** `crates/voxctrl-inference/`

## Overview

VoxCtrl uses **Whisper** (via `whisper-rs`, native bindings to whisper.cpp) for speech-to-text. Whisper runs entirely on-device using downloaded GGUF model files. No audio is ever sent to a remote server.

---

## Model Sizes

| Size | Approx RAM | Speed | Accuracy |
|---|---|---|---|
| `tiny` / `tiny.en` | ~75 MB | Fastest | Lowest |
| `base` / `base.en` | ~142 MB | Fast | Low |
| `small` / `small.en` | ~466 MB | Medium | Medium |
| `medium` / `medium.en` | ~1.5 GB | Slow | High |
| `large-v2` | ~3.1 GB | Slowest | High |
| `large-v3` | ~3.1 GB | Slowest | Highest |
| `large-v3-turbo` | ~1.6 GB | Medium | Near large-v3 |

The `.en` variants are English-only but slightly faster. `large-v3-turbo` is a distilled model offering near large-v3 quality at medium speed.

The default model is **`large-v3`**. Models are downloaded from Hugging Face as GGUF files on first use. By default they are cached at `~/.local/share/voxctrl/models/`; this path is configurable via `engine.whisper_cpp.model_dir`.

Change the active model via `engine.whisper_cpp.model_size` in config. Changing it takes effect on next recording.

---

## Hardware Backends

`engine.whisper_cpp.device` selects the compute backend:

| Value | Description |
|---|---|
| `auto` | Auto-detect: tries CUDA (nvidia-smi / /dev/nvidia0), then Vulkan (vulkaninfo / ICD dirs), then CPU |
| `cuda` | NVIDIA GPU via CUDA *(requires CUDA build — see below)* |
| `vulkan` | Any GPU via Vulkan (AMD/Intel/NVIDIA) |
| `cpu` | Force CPU |

On startup, VoxCtrl probes for CUDA (via `nvidia-smi`, `/proc/driver/nvidia/version`, `/dev/nvidia0`) and Vulkan (via `vulkaninfo`, `/usr/share/vulkan/icd.d`) to select the best backend.

> **CUDA is opt-in at compile time.** The default build runs on any machine without a GPU. To enable NVIDIA GPU acceleration, build with the `cuda` cargo feature:
> ```bash
> npm run tauri build -- --features cuda
> ```
> The `cuda` option only appears in the Settings → Engine device selector when the binary was compiled with this flag. If a previously saved config specifies `"cuda"` but the running binary is a CPU-only build, the app automatically resets the device to `"auto"` on launch.

---

## Inference Pipeline

When a recording session ends, the accumulated audio buffer is sent to the inference worker thread:

```
InferenceRequest {
    audio: Vec<f32>,           // 16 kHz mono PCM
    target_id: String,         // Which output target (comma-separated for multi-target)
    context_text: Option<String>, // AT-SPI context text, if enabled
}
```

The worker runs:

```
1. Empty audio check
   └─ audio.is_empty() → return ""

2. Noise gate (VAD)
   └─ rms_threshold = (1.0 - vad_threshold) * 0.006
   └─ rms(audio) < rms_threshold → return ""

3. Build Whisper initial prompt
   a. Target's initial_prompt (if set)
   b. Custom vocabulary words from features.custom_vocabulary
   c. AT-SPI context text (if context_prompt enabled)
   → Merged into a single prompt string for Whisper

4. whisper-rs transcription
   └─ Reuses pre-allocated WhisperState (KV cache + attention buffers loaded once at startup)
   └─ Returns raw_text with inference_ms and language

5. Post-processing pipeline (in order):
   a. Filler word removal (if enabled)
   b. Spoken punctuation conversion (if enabled)
   c. Auto-format list detection (if enabled)
   d. Snippet expansion (if snippets configured)
   e. Custom vocabulary fuzzy correction
   f. Code mode conversion (if enabled)

6. Silence hallucination filter
   └─ rms < 0.003 AND text is a known Whisper hallucination → return ""

7. Optional LLM post-processing via the OpenAI API (if target.processing.ollama_enabled)

8. Return InferenceOutput {
       text: String,            // Final processed text
       raw_text: String,        // Pre-processing Whisper output
       inference_ms: u32,       // Whisper wall time
       language: String,        // Detected language code
       target_id: String,
   }
```

---

## Post-Processing Details

### Filler Word Removal
Enabled via `features.remove_fillers`.

Strips common verbal fillers using a regex with repetition variants:
- `uh`, `um`, `hmm`, `er`, `ah`, `ugh`, `mhm` (e.g. `"uhhh"`, `"umm"` also matched)
- Cleans up resulting double spaces

### Spoken Punctuation Conversion
Enabled via `features.spoken_punctuation`.

Converts spoken words to their symbol equivalents (case-insensitive, word boundaries):

| Spoken | Output | | Spoken | Output |
|---|---|---|---|---|
| "period" / "full stop" | `. ` | | "open bracket" / "open paren" | `(` |
| "comma" | `, ` | | "close bracket" / "close paren" | `)` |
| "question mark" | `? ` | | "new line" | `\n` |
| "exclamation mark" / "exclamation point" | `! ` | | "new paragraph" | `\n\n` |
| "colon" | `: ` | | "tab" | `\t` |
| "semicolon" | `; ` | | "dash" | ` — ` |
| "hyphen" | `-` | | "ellipsis" | `...` |
| "slash" | `/` | | "backslash" | `\` |
| "at sign" | `@` | | "hash" | `#` |
| "percent" | `%` | | "ampersand" | `&` |
| "asterisk" | `*` | | "plus sign" | `+` |
| "equals sign" | `=` | | "less than" | `<` |
| "greater than" | `>` | | | |

### Auto-Format Lists
Enabled via `features.auto_format_lists`.

Detects ordinal pattern words (`first`, `second`, `third`, `fourth`, `fifth`, `finally`, including `firstly`, `secondly`, etc.) and reformats the text as a **numbered list**:

Input: `"First do this then second check that and finally submit"`
Output:
```
1. do this then
2. check that and
3. submit
```

### Snippet Expansion
Enabled whenever `features.snippets` is non-empty.

Short codes in transcribed text are replaced with their expansions (case-insensitive, word boundaries):

```json
"snippets": {
  "addr": "123 Main St, Springfield",
  "sig": "Best regards,\nJane"
}
```

### Custom Vocabulary Correction
Enabled whenever `features.custom_vocabulary` is non-empty.

After transcription, each word is compared against the vocabulary list using **Levenshtein distance fuzzy matching**:

| Word length | Max edit distance allowed |
|---|---|
| 1–3 chars | 0 (exact match only) |
| 4 chars | 1 |
| 5+ chars | 2 |

This corrects Whisper's phonetic approximations of proper nouns, names, and domain-specific terms. Example: vocabulary `["Rufer"]` would correct `"Rufur"` or `"Rupher"` to `"Rufer"`.

### Code Mode
Enabled via target `processing.code_mode = true`.

Converts spoken phrases to code-style syntax:
- Maps spoken operators: `"equals"` → `=`, `"plus"` → `+`, `"minus"` → `-`, `"times"` → `*`, `"divided by"` → `/`, `"modulo"` → `%`
- Converts multi-word lowercase phrases to camelCase: `"my function name"` → `"myFunctionName"`

---

## Silence Hallucination Filter

Whisper generates text like "Thank you." or "Thanks for watching." when given near-silent input. VoxCtrl applies a filter after post-processing:

```
IF rms_energy < 0.003 (absolute room silence)
AND processed_text ∈ ["thank you", "thanks for watching", "thank you for watching"]
THEN discard result → return ""
```

This threshold (0.003 RMS) is intentionally below any genuine speech energy, so saying "thank you" aloud will still be transcribed correctly.

---

## Context Prompting

When `atspi.context_prompt = true`, the surrounding text from the focused widget (read via AT-SPI2) is included in the Whisper initial prompt. This improves continuity with existing text in the field.

The Whisper initial prompt also incorporates:
1. The target's `initial_prompt` field (if set)
2. The `features.custom_vocabulary` list, formatted as: `"Vocabulary: word1, word2, ..."`
3. The AT-SPI2 surrounding text

---

## Configuration Options

Under `engine.whisper_cpp` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"large-v3"` | Whisper model |
| `device` | string | `"auto"` | Compute device |
| `threads` | integer | `0` | CPU threads (0 = auto) |
| `model_dir` | string | `""` | Custom model storage path; empty = `~/.local/share/voxctrl/models/`. Supports `~` expansion (e.g. `~/.whisper-models`). The directory must already exist. |

Language detection is automatic when using whisper-cpp; use the `engine.moonshine.language` field for the Moonshine backend.
