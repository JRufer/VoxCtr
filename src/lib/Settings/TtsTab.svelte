<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  // ── Run Speed Timer ────────────────────────────────────────────────────────
  let runSpeed = $state<number | null>(null);
  let elapsed = $state(0);
  let isCounting = $state(false);
  let timerId: any = null;
  let unlistenTtsStart: (() => void) | null = null;
  let unlistenTtsEnd: (() => void) | null = null;
  let unlistenTtsError: (() => void) | null = null;
  let voiceSpeaking = $state(false);
  let ttsError = $state<string | null>(null);
  let startTime = 0;

  function startTimer() {
    isCounting = true;
    elapsed = 0;
    runSpeed = null;
    startTime = performance.now();
    
    if (timerId) clearInterval(timerId);
    timerId = setInterval(() => {
      elapsed = Math.round(performance.now() - startTime);
      if (elapsed > 10000) {
        clearInterval(timerId);
        isCounting = false;
        runSpeed = null;
      }
    }, 10);
  }

  // ── Piper ──────────────────────────────────────────────────────────────────

  const PIPER_VOICES = [
    "en-us-libritts-high",
    "en-us-amy-low",
    "en-us-kathleen-low",
    "en-gb-southern_english_female-low",
    "en-us-ryan-high",
    "en-us-ryan-medium",
    "en-us-ryan-low",
    "en-us-lessac-medium",
    "en-us-lessac-low",
    "en-us-danny-low",
    "en-gb-alan-low"
  ];

  let downloadedMap = $state<Record<string, boolean>>({});
  let checking = $state(false);
  let downloading = $state(false);
  let testing = $state(false);
  let voiceDirError = $state<string | null>(null);

  async function checkAllVoicesDownloaded() {
    checking = true;
    const newMap: Record<string, boolean> = {};
    for (const v of PIPER_VOICES) {
      try {
        newMap[v] = await invoke<boolean>("check_voice_downloaded", {
          voiceName: v,
          voiceDir: cfg.tts.voice_dir,
        });
      } catch (e) {
        console.error("Failed to check download status for voice " + v, e);
        newMap[v] = false;
      }
    }
    downloadedMap = newMap;
    checking = false;
  }

  async function triggerDownload(voice: string) {
    if (downloading) return;
    downloading = true;
    try {
      await invoke("download_voice", {
        voiceName: voice,
        voiceDir: cfg.tts.voice_dir,
      });
      downloadedMap[voice] = true;
    } catch (e) {
      alert(`Failed to download voice: ${e}`);
    } finally {
      downloading = false;
    }
  }

  async function validateVoiceDir() {
    const path = cfg.tts.voice_dir;
    if (!path) {
      voiceDirError = null;
      return;
    }
    const exists = await invoke<boolean>("check_directory_exists", { path });
    voiceDirError = exists ? null : "This folder does not exist. Please create it first or leave blank for the default location.";
    if (!voiceDirError) {
      await checkAllVoicesDownloaded();
    }
  }

  function onVoiceDirChange() { markDirty(); }

  function onVoiceDirKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
  }

  async function onVoiceChanged() {
    markDirty();
  }

  async function testTts() {
    if (testing) return;
    
    if (voiceSpeaking) {
      voiceSpeaking = false;
      try {
        await invoke("stop_tts");
      } catch (err) {
        console.error("Failed to stop TTS:", err);
      }
    }
    
    testing = true;
    ttsError = null;
    startTimer();

    let engineName = "TTS";
    let voice: string | null = null;
    
    if (cfg.tts.engine === "piper") {
      engineName = "Piper";
      voice = cfg.tts.voice;
    } else if (cfg.tts.engine === "pocket_tts") {
      engineName = "Pocket-TTS";
      voice = cfg.tts.pocket_tts.voice;
    } else if (cfg.tts.engine === "espeak") {
      engineName = "eSpeak-NG";
      voice = null;
    }
    
    const textToSpeak = `Hi this is ${engineName} speaking from VoxCtrl`;
    
    try {
      await invoke("speak_text", {
        text: textToSpeak,
        voice: voice,
      });
    } catch (e) {
      ttsError = `${e}`;
      clearInterval(timerId);
      isCounting = false;
      testing = false;
    }
  }

  let engineSwitching = $state(false);

  function isTestTtsDisabled() {
    if (!cfg.tts.enabled || engineSwitching) return true;
    if (voiceSpeaking) return false;
    if (testing) return true;
    
    if (cfg.tts.engine === "piper") {
      return checking || downloading || !downloadedMap[cfg.tts.voice];
    }
    if (cfg.tts.engine === "pocket_tts") {
      return pocketTtsChecking || pocketTtsDownloading || !pocketTtsReady;
    }
    return false;
  }

  // ── Pocket-TTS ─────────────────────────────────────────────────────────────

  let pocketTtsVoices = $state<{ id: string; label: string }[]>([]);
  let pocketTtsVoiceDirError = $state<string | null>(null);

  async function loadPocketTtsVoices() {
    try {
      pocketTtsVoices = await invoke<{ id: string; label: string }[]>("list_pocket_tts_voices", {
        voiceDir: cfg.tts.pocket_tts.voice_dir,
      });
    } catch (e) {
      console.error("list_pocket_tts_voices:", e);
    }
  }

  async function validatePocketTtsVoiceDir() {
    const path = cfg.tts.pocket_tts.voice_dir;
    if (!path) {
      pocketTtsVoiceDirError = null;
      await loadPocketTtsVoices();
      return;
    }
    const exists = await invoke<boolean>("check_directory_exists", { path });
    pocketTtsVoiceDirError = exists ? null : "This folder does not exist. Please create it first or leave blank for the default location.";
    if (!pocketTtsVoiceDirError) {
      await loadPocketTtsVoices();
    }
  }

  function onPocketTtsVoiceDirChange() {
    markDirty();
  }

  function onPocketTtsVoiceDirKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
  }

  let piperVoiceOptions = $derived(
    PIPER_VOICES.map(v => ({
      value: v,
      label: `${v}${downloadedMap[v] ? " ✔" : ""}`
    }))
  );

  let pocketTtsVoiceOptions = $derived(
    pocketTtsVoices.map(v => ({
      value: v.id,
      label: v.label
    }))
  );

  const engineOptions = [
    { value: "pocket_tts", label: "Pocket-TTS (neural, voice cloning)" },
    { value: "piper", label: "Piper (neural, high quality)" },
    { value: "espeak", label: "eSpeak-NG (lightweight)" }
  ];

  let pocketTtsReady = $state(false);
  let pocketTtsChecking = $state(false);
  let pocketTtsDownloading = $state(false);

  async function checkPocketTtsReady() {
    pocketTtsChecking = true;
    try {
      pocketTtsReady = await invoke<boolean>("check_pocket_tts_ready", {
        voice: cfg.tts.pocket_tts.voice,
        voiceDir: cfg.tts.pocket_tts.voice_dir,
      });
    } catch (e) {
      console.error("check_pocket_tts_ready:", e);
      pocketTtsReady = false;
    } finally {
      pocketTtsChecking = false;
    }
  }

  async function downloadPocketTts() {
    if (pocketTtsDownloading) return;
    pocketTtsDownloading = true;
    try {
      await invoke("download_pocket_tts", {
        voice: cfg.tts.pocket_tts.voice,
        voiceDir: cfg.tts.pocket_tts.voice_dir,
        hfToken: cfg.tts.pocket_tts.hf_token,
      });
      pocketTtsReady = true;
    } catch (e) {
      alert(`Failed to download Pocket-TTS assets: ${e}`);
    } finally {
      pocketTtsDownloading = false;
    }
  }

  function onPocketTtsVoiceChanged() {
    markDirty();
    pocketTtsReady = false;
    checkPocketTtsReady();
  }

  function onPocketTtsTokenChanged() { markDirty(); }

  async function onEngineChanged() {
    markDirty();
    engineSwitching = true;
    try {
      if (cfg.tts.engine === "pocket_tts") {
        pocketTtsReady = false;
        await loadPocketTtsVoices();
        await checkPocketTtsReady();
      } else if (cfg.tts.engine === "piper") {
        if (cfg.tts.voice_dir) {
          await validateVoiceDir();
        } else {
          await checkAllVoicesDownloaded();
        }
      }
    } finally {
      // Add a small 400ms delay to allow the backend save_config to run
      setTimeout(() => {
        engineSwitching = false;
      }, 400);
    }
  }

  onMount(async () => {
    if (cfg.tts.voice_dir) {
      validateVoiceDir();
    } else {
      checkAllVoicesDownloaded();
    }

    if (cfg.tts.engine === "pocket_tts") {
      await loadPocketTtsVoices();
      checkPocketTtsReady();
    }

    unlistenTtsStart = await listen<void>("tts-playback-start", () => {
      if (isCounting) {
        clearInterval(timerId);
        runSpeed = elapsed;
        isCounting = false;
      }
      testing = false;
      voiceSpeaking = true;
    });

    unlistenTtsEnd = await listen<void>("tts-playback-end", () => {
      voiceSpeaking = false;
    });

    // Speak errors happen asynchronously in the TTS worker thread (missing
    // engine binary, voice not downloaded, no audio device, ...). Without this
    // the Test button hangs on "Speaking..." with no feedback at all.
    unlistenTtsError = await listen<string>("tts-error", (event) => {
      ttsError = event.payload;
      clearInterval(timerId);
      isCounting = false;
      runSpeed = null;
      testing = false;
      voiceSpeaking = false;
    });
  });

  onDestroy(() => {
    if (timerId) clearInterval(timerId);
    if (unlistenTtsStart) unlistenTtsStart();
    if (unlistenTtsEnd) unlistenTtsEnd();
    if (unlistenTtsError) unlistenTtsError();
  });

  // ── Stop Key Recorder ───────────────────────────────────────────────────────────

  let isRecordingStopKey = $state(false);
  let currentlyPressedStopKeys = $state<string[]>([]);

  function mapBrowserKeyToEvdev(key: string, code: string): string {
    const codeUpper = code.toUpperCase();
    if (key === "Control") return "KEY_LEFTCTRL";
    if (key === "Alt") return "KEY_LEFTALT";
    if (key === "Shift") return "KEY_LEFTSHIFT";
    if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";
    if (codeUpper === "SPACE") return "KEY_SPACE";
    if (codeUpper === "ENTER") return "KEY_ENTER";
    if (codeUpper === "ESCAPE" || codeUpper === "ESC") return "KEY_ESC";
    if (codeUpper === "TAB") return "KEY_TAB";
    if (codeUpper === "BACKSPACE") return "KEY_BACKSPACE";
    if (codeUpper === "DELETE") return "KEY_DELETE";
    if (codeUpper.startsWith("KEY")) return codeUpper;
    if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
    if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
    if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;
    if (key.length === 1) return `KEY_${key.toUpperCase()}`;
    return `KEY_${codeUpper}`;
  }

  function handleStopKeyDown(e: KeyboardEvent) {
    if (!isRecordingStopKey) return;
    e.preventDefault();
    e.stopPropagation();
    const evdevKey = mapBrowserKeyToEvdev(e.key, e.code);
    if (!currentlyPressedStopKeys.includes(evdevKey)) {
      currentlyPressedStopKeys = [...currentlyPressedStopKeys, evdevKey];
    }
    // Escape triggers browser blur before keyup fires, so commit immediately
    // on keydown for single-key combos where Escape is the key pressed.
    // For multi-key combos, keyup still handles commit as normal.
    if (e.key === "Escape") {
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
      currentlyPressedStopKeys = [];
      isRecordingStopKey = false;
    }
  }

  function handleStopKeyUp(e: KeyboardEvent) {
    if (!isRecordingStopKey) return;
    e.preventDefault();
    e.stopPropagation();
    if (currentlyPressedStopKeys.length > 0) {
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
    }
    currentlyPressedStopKeys = [];
    isRecordingStopKey = false;
  }

  function handleStopKeyBlur() {
    // Safety net: if blur fires while we have pending keys (e.g. Escape blur race),
    // commit whatever was captured rather than discarding it silently.
    if (currentlyPressedStopKeys.length > 0) {
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
      currentlyPressedStopKeys = [];
    }
    isRecordingStopKey = false;
  }

</script>

<section>
  <h2>Text to Speech</h2>

  <div class="field-group">
    <h3>TTS Engine</h3>
    <label class="field">
      <span>Enable TTS</span>
      <input type="checkbox" bind:checked={cfg.tts.enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Engine</span>
      <CustomSelect bind:value={cfg.tts.engine} options={engineOptions} onchange={onEngineChanged} />
    </label>
    {#if cfg.tts.engine === "piper"}
    <label class="field">
      <span>GPU Acceleration</span>
      <input type="checkbox" bind:checked={cfg.tts.gpu} onchange={markDirty} />
    </label>
    <p class="hint" style="margin-top: -6px; margin-bottom: 12px;">Use CUDA GPU acceleration (ONNX Runtime). Falls back to CPU if unavailable.</p>
    {/if}
    <label class="field">
      <span>Speed ({cfg.tts.speed.toFixed(2)}×)</span>
      <input
        type="range" min="0.5" max="2.0" step="0.05"
        bind:value={cfg.tts.speed}
        onchange={markDirty}
        class="range-input"
      />
    </label>
    <div class="row tts-test-row">
      <button class="btn-preview" onclick={testTts} disabled={isTestTtsDisabled()}>
        {testing ? "Speaking..." : voiceSpeaking ? "⏹ Stop & Test" : "Test TTS"}
      </button>
      {#if isCounting || runSpeed !== null}
        <div class="run-speed-container">
          <span class="run-speed-label">Run speed</span>
          <span class="run-speed-value" class:counting={isCounting}>
            {isCounting ? `${elapsed} ms` : `${runSpeed} ms`}
          </span>
        </div>
      {/if}
    </div>
    {#if ttsError}
      <p class="field-error-msg tts-error-msg">❌ {ttsError}</p>
    {/if}
  </div>

  <!-- ── Piper section ──────────────────────────────────────────────────── -->
  {#if cfg.tts.engine === "piper"}
  <div class="field-group">
    <h3>Piper Voice</h3>
    <label class="field col">
      <span class="field-title">Voice</span>
      <CustomSelect bind:value={cfg.tts.voice} options={piperVoiceOptions} onchange={onVoiceChanged} />
    </label>

    <div class="voice-status-container">
      {#if checking}
        <span class="status-checking">⏳ Checking local voice files...</span>
      {:else if downloading}
        <span class="status-downloading">⏳ Downloading {cfg.tts.voice} (model + config)...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={downloadedMap[cfg.tts.voice] ? "status-downloaded" : "status-missing"}>
            {downloadedMap[cfg.tts.voice] ? "✔ Voice downloaded and ready" : "❌ Voice files missing"}
          </span>
          <button class="btn-download" onclick={() => triggerDownload(cfg.tts.voice)} disabled={downloadedMap[cfg.tts.voice] || downloading}>
            {downloadedMap[cfg.tts.voice] ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>Voice directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.voice_dir}
        onchange={onVoiceDirChange}
        onblur={validateVoiceDir}
        onkeydown={onVoiceDirKeydown}
        class:field-input-error={!!voiceDirError}
      />
      {#if voiceDirError}
        <p class="field-error-msg">{voiceDirError}</p>
      {/if}
    </div>
    <p class="hint">Default voice directory: <code>~/.local/share/voxctrl/piper-voices/</code></p>
  </div>
  {/if}

  <!-- ── Pocket-TTS section ─────────────────────────────────────────────── -->
  {#if cfg.tts.engine === "pocket_tts"}
  <div class="field-group">
    <h3>Pocket-TTS Voice</h3>
    <label class="field col">
      <span class="field-title">Voice</span>
      <CustomSelect bind:value={cfg.tts.pocket_tts.voice} options={pocketTtsVoiceOptions} onchange={onPocketTtsVoiceChanged} />
    </label>

    <div class="voice-status-container">
      {#if pocketTtsChecking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if pocketTtsDownloading}
        <span class="status-downloading">⏳ Downloading Pocket-TTS model &amp; voice clip (may take a few minutes)...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={pocketTtsReady ? "status-downloaded" : "status-missing"}>
            {pocketTtsReady ? "✔ Model and voice clip downloaded and ready" : "❌ Model files missing"}
          </span>
          <button class="btn-download" onclick={downloadPocketTts} disabled={pocketTtsReady || pocketTtsDownloading}>
            {pocketTtsReady ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>HuggingFace access token</span>
      <input
        type="password"
        bind:value={cfg.tts.pocket_tts.hf_token}
        onchange={onPocketTtsTokenChanged}
      />
    </div>
    <p class="hint">
      Pocket-TTS model weights are hosted on a gated HuggingFace repo. Create a token at
      <code>huggingface.co/settings/tokens</code> and accept the license at
      <code>huggingface.co/kyutai/pocket-tts</code> before downloading.
    </p>

    <div class="field">
      <span>Custom voice directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.pocket_tts.voice_dir}
        onchange={onPocketTtsVoiceDirChange}
        onblur={validatePocketTtsVoiceDir}
        onkeydown={onPocketTtsVoiceDirKeydown}
        class:field-input-error={!!pocketTtsVoiceDirError}
      />
      {#if pocketTtsVoiceDirError}
        <p class="field-error-msg">{pocketTtsVoiceDirError}</p>
      {/if}
    </div>
    <p class="hint">
      Drop a <code>.wav</code> reference clip into this folder to add it to the voice list —
      the filename (without extension) becomes the voice's id, e.g. <code>narrator.wav</code> adds
      "Narrator (Custom)". Naming a clip after a built-in voice (e.g. <code>alba.wav</code>) replaces
      that voice's reference clip. Default: <code>~/.local/share/voxctrl/pocket-tts-voices/</code>
    </p>
  </div>
  {/if}

  <div class="field-group">
    <h3>Playback</h3>
    <label class="field">
      <span>Show response overlay</span>
      <input type="checkbox" bind:checked={cfg.tts.response_overlay} onchange={markDirty} />
    </label>

    <div class="border-t border-white/5 pt-[14px] flex flex-col gap-2">
      <h5 class="mb-1 text-[11px] font-bold uppercase text-accent-blue tracking-[0.06em]">Stop Key Bind</h5>
      <p class="hint" style="margin: 0 0 8px 0;">Press a key combo to immediately stop TTS playback — works even when this window is hidden.</p>
      <div
        class={[
          "border-2 rounded-desktop p-6 text-center cursor-pointer outline-none transition-all duration-200 flex flex-col items-center justify-center min-h-[80px]",
          isRecordingStopKey
            ? "border-solid border-[#f43f5e] bg-[rgba(244,63,94,0.05)] animate-border-pulse"
            : "border-dashed border-white/5 bg-black/25 hover:border-accent-blue hover:bg-black/35 focus:border-accent-blue focus:bg-black/35"
        ].join(" ")}
        tabindex="0"
        role="button"
        aria-label="Stop key recorder"
        onclick={() => isRecordingStopKey = true}
        onfocus={() => isRecordingStopKey = true}
        onblur={handleStopKeyBlur}
        onkeydown={handleStopKeyDown}
        onkeyup={handleStopKeyUp}
      >
        {#if isRecordingStopKey}
          <div class="flex items-center gap-[10px]">
            <span class="w-2 h-2 bg-accent-blue rounded-full animate-flash"></span>
            <span class="text-[13px] font-semibold text-accent-blue">
              {currentlyPressedStopKeys.length > 0
                ? currentlyPressedStopKeys.join(" + ").replace(/KEY_/g, "")
                : "Press your physical shortcut combination now..."}
            </span>
          </div>
        {:else}
          <span class="text-[12px] text-obsidian-300 flex flex-col gap-2 items-center">
            {#if cfg.tts.stop_key.length > 0}
              <div class="flex gap-1.5">
                {#each cfg.tts.stop_key as k}
                  <kbd class="px-1.5! py-0.5! text-[12px] bg-accent-blue text-black border-0 font-extrabold rounded">{k.replace("KEY_", "")}</kbd>
                {/each}
              </div>
              <span class="text-[10px] text-accent-blue opacity-80">(Click / Tab here to record a new stop key)</span>
            {:else}
              ⚠️ Click/Focus here to press a stop key!
            {/if}
          </span>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  @reference "tailwindcss";

  .row {
    @apply flex gap-2 mt-2;
  }
  .btn-preview {
    @apply bg-[var(--surface2)] border border-[var(--border)] text-[var(--text)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs cursor-pointer transition-all duration-200 ease-out;
  }
  .btn-preview:hover:not(:disabled) {
    @apply bg-[var(--border)] text-[var(--accent)];
  }
  .btn-preview:disabled {
    @apply opacity-40 cursor-not-allowed;
  }

  .voice-status-container {
    @apply flex items-center bg-[var(--bg)] border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 text-[13px] min-h-[42px];
  }
  .status-downloaded {
    @apply text-emerald-400 font-semibold;
  }
  .status-downloading {
    @apply text-[var(--accent2)];
  }
  .status-checking {
    @apply text-[var(--text-muted)];
  }
  .status-missing-wrapper {
    @apply flex items-center justify-between w-full;
  }
  .status-missing {
    @apply text-red-400;
  }
  .btn-download {
    @apply bg-[var(--accent)] border-none text-white rounded-[var(--radius)] p-1.5 px-3 text-xs cursor-pointer font-semibold transition-colors duration-200;
  }
  .btn-download:hover:not(:disabled) {
    @apply bg-[var(--accent2)];
  }
  .btn-download:disabled {
    @apply bg-[var(--surface2)] border border-[var(--border)] text-[var(--text-muted)] opacity-50 cursor-not-allowed;
  }
  .field-input-error {
    @apply border-red-500!;
  }
  .field-input-error:focus {
    @apply border-red-500 shadow-[0_0_0_2px_rgba(239,68,68,0.15),_inset_0_2px_4px_rgba(0,0,0,0.2)];
  }
  .field-error-msg {
    @apply mt-1 text-sm leading-5 text-red-400;
  }
  .range-input {
    @apply w-full accent-[var(--accent)];
  }
  .number-input {
    @apply w-20;
  }

  .tts-test-row {
    @apply flex justify-between items-center w-full;
  }
  .tts-error-msg {
    @apply w-full break-words;
  }
  .run-speed-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
  }
  .run-speed-label {
    color: var(--text-muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .run-speed-value {
    color: var(--accent);
    font-family: 'JetBrains Mono', monospace;
    font-weight: 600;
  }
  .run-speed-value.counting {
    color: var(--accent2);
    text-shadow: 0 0 8px rgba(56, 189, 248, 0.3);
  }

</style>
