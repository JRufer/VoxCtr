<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty, saveConfig } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

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
    
    try {
      await saveConfig(cfg);
    } catch (err) {
      ttsError = `Failed to save configuration: ${err}`;
      return;
    }
    
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
    } else if (cfg.tts.engine === "voxcpm2") {
      engineName = "Vox C P M 2";
      voice = null;
    } else if (cfg.tts.engine === "breeze_tts_2") {
      engineName = "Breeze-TTS-2";
      voice = null;
    } else if (cfg.tts.engine === "inflect_micro") {
      engineName = "Inflect Micro";
      voice = null;
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

  // Why the Test button is unavailable, or null when it is usable. Returning a
  // reason rather than a bare boolean means a greyed-out button can say what is
  // wrong instead of leaving the user to guess.
  function testTtsDisabledReason(): string | null {
    if (!cfg.tts.enabled) return "Enable text-to-speech above first.";
    if (engineSwitching) return "Switching engine...";
    if (voiceSpeaking) return null;
    if (testing) return "Already speaking.";

    if (cfg.tts.engine === "inflect_micro") {
      if (!inflectAvailable) {
        return "This build was compiled without the `inflect-micro` feature, so this engine cannot synthesize. Rebuild with: npm run tauri dev -- --features inflect-micro";
      }
      if (inflectChecking) return "Checking local model files...";
      if (inflectDownloading) return "Downloading the model...";
      if (!inflectReady) return "Download the model first.";
    }
    if (cfg.tts.engine === "breeze_tts_2") {
      if (breezeChecking) return "Checking local model files...";
      if (breezeDownloading) return "Downloading the model...";
      if (!breezeReady) return "Download the model first.";
    }
    if (cfg.tts.engine === "voxcpm2") {
      if (voxcpmStatus && !voxcpmStatus.compiled) {
        return "This build was compiled without the `voxcpm2` feature, so this engine cannot synthesize. Rebuild with: npm run tauri dev -- --features voxcpm2";
      }
      if (voxcpmChecking) return "Checking local model files...";
      if (voxcpmDownloading) return "Downloading the model...";
      if (!voxcpmReady) return "Download the model first.";
      if (cfg.tts.voxcpm2.voice_mode === "clone" && !cfg.tts.voxcpm2.cloned_voice) {
        return "Pick a reference clip to clone, or switch to voice design.";
      }
    }
    return null;
  }

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
    if (cfg.tts.engine === "breeze_tts_2") {
      return breezeChecking || breezeDownloading || !breezeReady;
    }
    if (cfg.tts.engine === "voxcpm2") {
      if (voxcpmChecking || voxcpmDownloading || !voxcpmReady) return true;
      if (voxcpmStatus && !voxcpmStatus.compiled) return true;
      return cfg.tts.voxcpm2.voice_mode === "clone" && !cfg.tts.voxcpm2.cloned_voice;
    }
    if (cfg.tts.engine === "inflect_micro") {
      return inflectChecking || inflectDownloading || !inflectReady || !inflectAvailable;
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
    { value: "voxcpm2", label: "VoxCPM2 (neural, voice design + cloning)" },
    { value: "breeze_tts_2", label: "Breeze-TTS-2 (neural, voice design)" },
    { value: "pocket_tts", label: "Pocket-TTS (neural, voice cloning)" },
    { value: "piper", label: "Piper (neural, high quality)" },
    { value: "inflect_micro", label: "Inflect Micro (neural, 38 MB)" },
    { value: "espeak", label: "eSpeak-NG (lightweight)" }
  ];

  // ── Inflect-Micro-v2 ───────────────────────────────────────────────────────
  //
  // A fixed-voice model, so there is no voice picker here — the knobs are the
  // sampling seed and the two VITS noise scales. `inflectAvailable` reports
  // whether the app was built with the `inflect-micro` feature; without it the
  // engine can be selected but never synthesizes, so the UI says so up front.

  let inflectAvailable = $state(true);
  let inflectReady = $state(false);
  let inflectChecking = $state(false);
  let inflectDownloading = $state(false);
  let inflectSignature = $state<string | null>(null);
  let inflectInspecting = $state(false);
  let inflectError = $state<string | null>(null);

  async function checkInflectAvailable() {
    try {
      inflectAvailable = await invoke<boolean>("inflect_micro_available");
    } catch (e) {
      console.error("inflect_micro_available:", e);
      inflectAvailable = false;
    }
  }

  async function checkInflectReady() {
    inflectChecking = true;
    try {
      inflectReady = await invoke<boolean>("check_inflect_micro_downloaded", {
        modelDir: cfg.tts.inflect_micro.model_dir,
      });
    } catch (e) {
      console.error("check_inflect_micro_downloaded:", e);
      inflectReady = false;
    } finally {
      inflectChecking = false;
    }
  }

  async function downloadInflect() {
    if (inflectDownloading) return;
    inflectDownloading = true;
    inflectError = null;
    try {
      await invoke("download_inflect_micro", {
        modelDir: cfg.tts.inflect_micro.model_dir,
      });
      inflectReady = true;
    } catch (e) {
      // Reported inline rather than through alert(): the backend lists every URL
      // it tried, which is far too long for a modal, and a blocking dialog here
      // leaves the user with no way to copy the detail out.
      inflectError = `${e}`;
    } finally {
      inflectDownloading = false;
    }
  }

  // Diagnostic: report the tensor names the downloaded export actually declares.
  // Useful when synthesis fails because the graph's naming doesn't match what
  // the Rust side binds against.
  async function inspectInflect() {
    if (inflectInspecting) return;
    inflectInspecting = true;
    inflectSignature = null;
    try {
      const sig = await invoke<unknown>("inflect_micro_inspect", {
        modelDir: cfg.tts.inflect_micro.model_dir,
      });
      inflectSignature = JSON.stringify(sig, null, 2);
    } catch (e) {
      inflectSignature = `${e}`;
    } finally {
      inflectInspecting = false;
    }
  }

  function onInflectSettingChanged() { markDirty(); }

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

  // ── Breeze-TTS-2 ───────────────────────────────────────────────────────────

  let breezeReady = $state(false);
  let breezeChecking = $state(false);
  let breezeDownloading = $state(false);

  async function checkBreezeReady() {
    breezeChecking = true;
    try {
      breezeReady = await invoke<boolean>("check_breeze_tts_2_ready", {
        modelDir: cfg.tts.breeze_tts_2.model_dir,
      });
    } catch (e) {
      console.error("check_breeze_tts_2_ready:", e);
      breezeReady = false;
    } finally {
      breezeChecking = false;
    }
  }

  async function downloadBreezeTts2() {
    if (breezeDownloading) return;
    breezeDownloading = true;
    try {
      const token = cfg.tts.breeze_tts_2.hf_token || cfg.tts.pocket_tts.hf_token;
      await invoke("download_breeze_tts_2", {
        modelDir: cfg.tts.breeze_tts_2.model_dir,
        hfToken: token,
      });
      breezeReady = true;
    } catch (e) {
      alert(`Failed to download Breeze-TTS-2 assets: ${e}`);
    } finally {
      breezeDownloading = false;
    }
  }

  // ── VoxCPM2 ────────────────────────────────────────────────────────────────
  //
  // Runs in pure Rust via the `voxcpm-rs` (Burn) crate, so unlike the other
  // neural engines there is no token and no gated licence to accept — the
  // weights are Apache-2.0. `voxcpm2_status` answers in one round trip whether
  // the backend was compiled in, which device it will use, and which checkpoint
  // files are still missing.

  interface VoxCpm2Status {
    compiled: boolean;
    backend: string;
    ready: boolean;
    missing: string[];
    model_dir: string;
  }

  let voxcpmStatus = $state<VoxCpm2Status | null>(null);
  let voxcpmReady = $state(false);
  let voxcpmChecking = $state(false);
  let voxcpmDownloading = $state(false);
  let voxcpmError = $state<string | null>(null);

  async function checkVoxcpmStatus() {
    voxcpmChecking = true;
    try {
      voxcpmStatus = await invoke<VoxCpm2Status>("voxcpm2_status", {
        modelDir: cfg.tts.voxcpm2.model_dir,
      });
      voxcpmReady = voxcpmStatus.ready;
    } catch (e) {
      console.error("voxcpm2_status:", e);
      voxcpmStatus = null;
      voxcpmReady = false;
    } finally {
      voxcpmChecking = false;
    }
  }

  async function downloadVoxcpm2() {
    if (voxcpmDownloading) return;
    voxcpmDownloading = true;
    voxcpmError = null;
    try {
      await invoke("download_voxcpm2", {
        modelDir: cfg.tts.voxcpm2.model_dir,
        repo: cfg.tts.voxcpm2.model_repo,
        hfToken: cfg.tts.voxcpm2.hf_token,
      });
      await checkVoxcpmStatus();
    } catch (e) {
      // Reported inline rather than through alert(): a failed multi-gigabyte
      // download names the file and the HTTP status, which is worth copying.
      voxcpmError = `${e}`;
    } finally {
      voxcpmDownloading = false;
    }
  }

  function onVoxcpmSettingChanged() { markDirty(); }

  // Each patch is ~80 ms of audio, so the slider can state what it is really
  // choosing rather than leaving the unit as folklore.
  let voxcpmChunkMs = $derived(Math.round(cfg.tts.voxcpm2.chunk_patches * 80));

  function onHfTokenChanged(e: Event) {
    const val = (e.target as HTMLInputElement).value;
    const tokenVal = val.trim() ? val.trim() : null;
    cfg.tts.pocket_tts.hf_token = tokenVal;
    cfg.tts.breeze_tts_2.hf_token = tokenVal;
    markDirty();
  }

  async function onEngineChanged() {
    markDirty();
    engineSwitching = true;
    try {
      if (cfg.tts.engine === "breeze_tts_2") {
        breezeReady = false;
        await checkBreezeReady();
      } else if (cfg.tts.engine === "voxcpm2") {
        voxcpmReady = false;
        await loadPocketTtsVoices();
        await checkVoxcpmStatus();
      } else if (cfg.tts.engine === "pocket_tts") {
        pocketTtsReady = false;
        await loadPocketTtsVoices();
        await checkPocketTtsReady();
      } else if (cfg.tts.engine === "inflect_micro") {
        inflectReady = false;
        await checkInflectAvailable();
        await checkInflectReady();
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

    if (cfg.tts.engine === "breeze_tts_2") {
      checkBreezeReady();
    }

    if (cfg.tts.engine === "voxcpm2") {
      await loadPocketTtsVoices();
      checkVoxcpmStatus();
    }

    if (cfg.tts.engine === "pocket_tts") {
      await loadPocketTtsVoices();
      checkPocketTtsReady();
    }

    if (cfg.tts.engine === "inflect_micro") {
      await checkInflectAvailable();
      checkInflectReady();
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

    // Playback-end also clears `testing`. Otherwise an utterance that completes
    // without ever starting playback — a run that produces no audio but also no
    // error — leaves the button stuck on "Speaking..." indefinitely, since only
    // playback-start cleared it.
    unlistenTtsEnd = await listen<void>("tts-playback-end", () => {
      voiceSpeaking = false;
      if (testing) {
        clearInterval(timerId);
        isCounting = false;
        testing = false;
      }
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

  // TTS Snippets & Dictionary editing
  let ttsSnippetList = $state<{key: string, val: string}[]>(
    Object.entries(cfg.tts.snippets || {}).map(([k, v]) => ({ key: k, val: v as string }))
  );

  let isTtsSnippetInitialized = false;
  $effect(() => {
    const list = ttsSnippetList;
    const newSnippets: Record<string, string> = {};
    for (const {key, val} of list) {
      if (key.trim()) {
        newSnippets[key.trim()] = val.trim();
      }
    }

    const existing = cfg.tts.snippets || {};
    const existingKeys = Object.keys(existing);
    const newKeys = Object.keys(newSnippets);
    let changed = existingKeys.length !== newKeys.length;
    if (!changed) {
      for (const k of newKeys) {
        if (existing[k] !== newSnippets[k]) {
          changed = true;
          break;
        }
      }
    }

    if (changed) {
      cfg.tts.snippets = newSnippets;
      if (isTtsSnippetInitialized) {
        markDirty();
      }
    }
    isTtsSnippetInitialized = true;
  });

  function addEmptyTtsSnippetRow() {
    ttsSnippetList = [...ttsSnippetList, { key: "", val: "" }];
  }

  function removeTtsSnippetRow(index: number) {
    ttsSnippetList = ttsSnippetList.filter((_, i) => i !== index);
  }

  let ttsCustomVocabString = $derived(
    cfg.tts.custom_vocabulary ? cfg.tts.custom_vocabulary.join(", ") : ""
  );

  function onTtsCustomVocabChange(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    cfg.tts.custom_vocabulary = target.value
      .split(",")
      .map(w => w.trim())
      .filter(w => w.length > 0);
    markDirty();
  }

  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
    const timer = setTimeout(resize, 0);

    return {
      update() {
        resize();
      },
      destroy() {
        clearTimeout(timer);
        node.removeEventListener("input", resize);
      }
    };
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
      <button class="btn-preview" onclick={testTts} disabled={isTestTtsDisabled()} title={testTtsDisabledReason() ?? "Speak a test phrase"}>
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
    {#if !ttsError && isTestTtsDisabled() && testTtsDisabledReason()}
      <p class="hint">Test TTS unavailable: {testTtsDisabledReason()}</p>
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

  <!-- ── VoxCPM2 section ────────────────────────────────────────────────── -->
  {#if cfg.tts.engine === "voxcpm2"}
  <div class="field-group">
    <h3>VoxCPM2 Voice</h3>

    {#if voxcpmStatus && !voxcpmStatus.compiled}
      <p class="field-error-msg">
        ❌ This build was compiled without the <code>voxcpm2</code> feature, so this engine
        cannot synthesize. The model still downloads and these settings still save.
        Rebuild with <code>--features voxcpm2</code> (GPU) or <code>--features voxcpm2-cpu</code>.
      </p>
    {/if}

    <div class="field col">
      <span class="field-title">Voice Selection Method</span>
      <div class="engine-radio-group">
        <label class="engine-radio-option {cfg.tts.voxcpm2.voice_mode !== 'clone' ? 'selected' : ''}">
          <div class="engine-radio-header">
            <input
              type="radio"
              name="voxcpm2_voice_mode"
              value="design"
              checked={cfg.tts.voxcpm2.voice_mode !== 'clone'}
              onchange={() => { cfg.tts.voxcpm2.voice_mode = 'design'; markDirty(); }}
            />
            <span class="engine-radio-name">🗣️ Voice Design (Prompt)</span>
          </div>
          <span class="engine-radio-desc">Describe vocal characteristics in natural language</span>
        </label>

        <label class="engine-radio-option {cfg.tts.voxcpm2.voice_mode === 'clone' ? 'selected' : ''}">
          <div class="engine-radio-header">
            <input
              type="radio"
              name="voxcpm2_voice_mode"
              value="clone"
              checked={cfg.tts.voxcpm2.voice_mode === 'clone'}
              onchange={() => { cfg.tts.voxcpm2.voice_mode = 'clone'; markDirty(); loadPocketTtsVoices(); }}
            />
            <span class="engine-radio-name">🎙️ Voice Cloning (Shared Folder)</span>
          </div>
          <span class="engine-radio-desc">Clone voice from reference .wav audio clip</span>
        </label>
      </div>
    </div>

    {#if cfg.tts.voxcpm2.voice_mode === 'clone'}
      <label class="field col">
        <span class="field-title">Cloned Voice Reference Clip</span>
        <CustomSelect
          bind:value={cfg.tts.voxcpm2.cloned_voice}
          options={pocketTtsVoiceOptions}
          onchange={markDirty}
        />
      </label>

      <div class="field">
        <span>Shared Voice Folder (leave blank for default)</span>
        <input
          type="text"
          bind:value={cfg.tts.voxcpm2.voice_dir}
          onchange={() => { markDirty(); validatePocketTtsVoiceDir(); }}
        />
      </div>
      <p class="hint">Default directory: <code>~/.local/share/voxctrl/pocket-tts-voices/</code> — shared with Pocket-TTS and Breeze-TTS-2.</p>

      <label class="field col">
        <span class="field-title">Style Instruction (optional)</span>
        <textarea
          bind:value={cfg.tts.voxcpm2.style_prompt}
          onchange={markDirty}
          placeholder="e.g. slightly faster, cheerful tone"
          rows="2"
          class="field-input-textarea"
        ></textarea>
      </label>
      <p class="hint" style="margin-top: -4px;">
        Shapes delivery without changing who the cloned voice is. Leave blank to reproduce the clip as-is.
      </p>

      <div class="license-warning-card" style="margin-top: 4px; margin-bottom: 8px;">
        <p class="license-title">💡 Voice Cloning Transcript Requirement</p>
        <p class="license-text">
          Drop reference <code>.wav</code> audio files into your shared voice folder. Placing a matching
          text file (e.g. <code>voice_name.txt</code>) containing the spoken transcript alongside the
          <code>.wav</code> lets VoxCPM2 <em>continue</em> the recording rather than merely imitate it,
          which tracks the reference speaker noticeably more closely.
        </p>
      </div>
    {:else}
      <label class="field col">
        <span class="field-title">Speaker Voice Prompt (Voice Design)</span>
        <textarea
          bind:value={cfg.tts.voxcpm2.design_prompt}
          onchange={markDirty}
          placeholder="Describe the voice of the speaker in natural language..."
          rows="2"
          class="field-input-textarea"
        ></textarea>
      </label>
      <p class="hint" style="margin-top: -4px;">
        Natural language description used by VoxCPM2 to generate the speaker's voice (e.g. <em>"A young woman, gentle and sweet voice"</em>
        or <em>"A deep, confident male narrator"</em>). No reference audio required.
      </p>
    {/if}

    <div class="voice-status-container">
      {#if voxcpmChecking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if voxcpmDownloading}
        <span class="status-downloading">⏳ Downloading VoxCPM2 checkpoint (~4 GB, may take a while)...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={voxcpmReady ? "status-downloaded" : "status-missing"}>
            {voxcpmReady ? "✔ Model weights downloaded and ready" : "❌ Model files missing"}
          </span>
          <button class="btn-download" onclick={downloadVoxcpm2} disabled={voxcpmReady || voxcpmDownloading}>
            {voxcpmReady ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    {#if voxcpmError}
      <p class="field-error-msg">❌ {voxcpmError}</p>
    {/if}
    {#if voxcpmStatus && !voxcpmReady && voxcpmStatus.missing.length > 0 && !voxcpmDownloading}
      <p class="hint">Missing: {voxcpmStatus.missing.join(", ")}</p>
    {/if}
    {#if voxcpmStatus}
      <p class="hint">Compute backend: <code>{voxcpmStatus.backend}</code></p>
    {/if}

    <div class="license-warning-card" style="margin-top: 4px; margin-bottom: 8px;">
      <p class="license-title">⚡ Latency</p>
      <p class="license-text">
        VoxCPM2 is a 2B-parameter model that runs in pure Rust with no Python and no subprocess.
        Keep <strong>Pre-warm</strong> on: loading the checkpoint takes 20–25 seconds, and prewarming
        moves that to startup so the first spoken response does not pay for it. <strong>Lead
        Buffer</strong> below decides how soon speech starts; the other sliders decide how fast the
        model generates it.
      </p>
    </div>

    <label class="field">
      <span>Pre-warm Model on Startup</span>
      <input type="checkbox" bind:checked={cfg.tts.voxcpm2.prewarm} onchange={markDirty} />
    </label>
    <p class="hint" style="margin-top: -6px;">Loads the checkpoint and compiles GPU shaders at startup so the first utterance is fast.</p>

    <label class="field">
      <span>Lead Buffer ({cfg.tts.voxcpm2.prebuffer_ms} ms)</span>
      <input
        type="range" min="200" max="3000" step="50"
        bind:value={cfg.tts.voxcpm2.prebuffer_ms}
        onchange={onVoxcpmSettingChanged}
        class="range-input"
      />
    </label>
    <p class="hint" style="margin-top: -6px;">
      Audio buffered before speech starts, and the main time-to-first-sound control.
      <strong>If speech stalls or breaks up part-way through, raise this.</strong> Once playback
      begins the audio device consumes sound in real time, and a lead this long is what absorbs
      the pauses between generated chunks. When generation is measured to be slower than
      realtime the engine extends this automatically. Default is 400 ms.
    </p>

    <label class="field">
      <span>Chunk Size ({cfg.tts.voxcpm2.chunk_patches} patches ≈ {voxcpmChunkMs} ms)</span>
      <input
        type="range" min="1" max="10" step="1"
        bind:value={cfg.tts.voxcpm2.chunk_patches}
        onchange={onVoxcpmSettingChanged}
        class="range-input"
      />
    </label>
    <p class="hint" style="margin-top: -6px;">
      How much audio the model generates at a time. This affects throughput rather than when
      speech starts: larger chunks do less repeated decoding, so they generate faster. Default is 4.
    </p>

    <label class="field">
      <span>Diffusion Steps ({cfg.tts.voxcpm2.inference_timesteps})</span>
      <input
        type="range" min="4" max="16" step="1"
        bind:value={cfg.tts.voxcpm2.inference_timesteps}
        onchange={onVoxcpmSettingChanged}
        class="range-input"
      />
    </label>
    <p class="hint" style="margin-top: -6px;">
      Sampling steps per audio patch. Cost is linear, so this scales the whole generation.
      Below 6 quality degrades audibly. Default is 6.
    </p>

    <label class="field">
      <span>Guidance Scale ({cfg.tts.voxcpm2.cfg_value.toFixed(2)})</span>
      <input
        type="range" min="1.0" max="3.0" step="0.1"
        bind:value={cfg.tts.voxcpm2.cfg_value}
        onchange={onVoxcpmSettingChanged}
        class="range-input"
      />
    </label>
    <p class="hint" style="margin-top: -6px;">How closely the output follows the text and voice prompt. Default is 2.00.</p>

    <div class="field">
      <span>Model directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.voxcpm2.model_dir}
        onchange={() => { markDirty(); checkVoxcpmStatus(); }}
      />
    </div>
    <p class="hint">Default directory: <code>~/.local/share/voxctrl/models/voxcpm2/</code></p>

    <div class="field">
      <span>HuggingFace repository</span>
      <input
        type="text"
        bind:value={cfg.tts.voxcpm2.model_repo}
        onchange={markDirty}
      />
    </div>
    <p class="hint">
      Weights are Apache-2.0 and ungated, so no access token is needed. Change this only to use a
      mirror or a fine-tune of <code>openbmb/VoxCPM2</code>.
    </p>
  </div>
  {/if}

  <!-- ── Breeze-TTS-2 section ───────────────────────────────────────────── -->
  {#if cfg.tts.engine === "breeze_tts_2"}
  <div class="field-group">
    <h3>Breeze-TTS-2 Voice</h3>

    <div class="non-commercial-warning">
      <div class="warning-header">
        <span class="warning-icon">⚠️</span>
        <strong>Non-Commercial License Warning</strong>
      </div>
      <p class="warning-text">
        Breeze-TTS-2 model weights are released under the <strong>BreezeBlue Research and Non-Commercial License</strong>.
        Commercial use requires a personal or commercial license directly from the creator (RESONIA, INC.).
      </p>
    </div>

    <div class="field col">
      <span class="field-title">Voice Selection Method</span>
      <div class="engine-radio-group">
        <label class="engine-radio-option {cfg.tts.breeze_tts_2.voice_mode !== 'clone' ? 'selected' : ''}">
          <div class="engine-radio-header">
            <input
              type="radio"
              name="breeze_voice_mode"
              value="prompt"
              checked={cfg.tts.breeze_tts_2.voice_mode !== 'clone'}
              onchange={() => { cfg.tts.breeze_tts_2.voice_mode = 'prompt'; markDirty(); }}
            />
            <span class="engine-radio-name">🗣️ Voice Design (Prompt)</span>
          </div>
          <span class="engine-radio-desc">Describe vocal characteristics in natural language</span>
        </label>

        <label class="engine-radio-option {cfg.tts.breeze_tts_2.voice_mode === 'clone' ? 'selected' : ''}">
          <div class="engine-radio-header">
            <input
              type="radio"
              name="breeze_voice_mode"
              value="clone"
              checked={cfg.tts.breeze_tts_2.voice_mode === 'clone'}
              onchange={() => { cfg.tts.breeze_tts_2.voice_mode = 'clone'; markDirty(); loadPocketTtsVoices(); }}
            />
            <span class="engine-radio-name">🎙️ Voice Cloning (Shared Folder)</span>
          </div>
          <span class="engine-radio-desc">Clone voice from reference .wav audio clip</span>
        </label>
      </div>
    </div>

    {#if cfg.tts.breeze_tts_2.voice_mode === 'clone'}
      <label class="field col">
        <span class="field-title">Cloned Voice Reference Clip</span>
        <CustomSelect
          bind:value={cfg.tts.breeze_tts_2.cloned_voice}
          options={pocketTtsVoiceOptions}
          onchange={markDirty}
        />
      </label>

      <div class="field">
        <span>Shared Voice Folder (leave blank for default)</span>
        <input
          type="text"
          bind:value={cfg.tts.breeze_tts_2.voice_dir}
          onchange={() => { markDirty(); validatePocketTtsVoiceDir(); }}
        />
      </div>
      <p class="hint">Default directory: <code>~/.local/share/voxctrl/pocket-tts-voices/</code></p>

      <div class="license-warning-card" style="margin-top: 4px; margin-bottom: 8px;">
        <p class="license-title">💡 Voice Cloning Transcript Requirement</p>
        <p class="license-text">
          Drop reference <code>.wav</code> audio files into your shared voice folder. For best cloning accuracy, place a matching text file (e.g. <code>voice_name.txt</code>) containing the spoken transcript of the audio file in the exact same folder alongside your <code>.wav</code> file.
        </p>
      </div>
    {:else}
      <label class="field col">
        <span class="field-title">Speaker Voice Prompt (Voice Design)</span>
        <textarea
          bind:value={cfg.tts.breeze_tts_2.speaker_prompt}
          onchange={markDirty}
          placeholder="Describe the voice of the speaker in natural language..."
          rows="2"
          class="field-input-textarea"
        ></textarea>
      </label>
      <p class="hint" style="margin-top: -4px;">
        Natural language description used by Breeze-TTS-2 to generate the speaker's voice (e.g. <em>"A calm female voice speaking clearly with a gentle tone"</em> or <em>"A deep, confident male narrator"</em>).
      </p>
    {/if}

    <div class="voice-status-container">
      {#if breezeChecking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if breezeDownloading}
        <span class="status-downloading">⏳ Downloading Breeze-TTS-2 model weights from HuggingFace...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={breezeReady ? "status-downloaded" : "status-missing"}>
            {breezeReady ? "✔ Model weights downloaded and ready" : "❌ Model files missing"}
          </span>
          <button class="btn-download" onclick={downloadBreezeTts2} disabled={breezeReady || breezeDownloading}>
            {breezeReady ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>HuggingFace access token</span>
      <input
        type="password"
        value={cfg.tts.breeze_tts_2.hf_token || cfg.tts.pocket_tts.hf_token || ""}
        oninput={onHfTokenChanged}
      />
    </div>
    <p class="hint">
      Breeze-TTS-2 model weights are hosted on HuggingFace. Create a token at
      <code>huggingface.co/settings/tokens</code> and accept the license at
      <code>huggingface.co/BreezeBlue/Breeze-TTS-2</code> before downloading. This token is shared with Pocket-TTS.
    </p>

    <label class="field">
      <span>GPU Acceleration (CUDA)</span>
      <input type="checkbox" bind:checked={cfg.tts.breeze_tts_2.gpu} onchange={markDirty} />
    </label>
    <p class="hint" style="margin-top: -6px;">Use NVIDIA CUDA GPU acceleration for fastest inference speed.</p>

    <label class="field">
      <span>Pre-warm Model on Startup</span>
      <input type="checkbox" bind:checked={cfg.tts.breeze_tts_2.prewarm} onchange={markDirty} />
    </label>
    <p class="hint" style="margin-top: -6px;">Pre-loads model tensors into GPU VRAM on startup so the first speech generation is instant.</p>

    <label class="field">
      <span>Sampling Temperature ({cfg.tts.breeze_tts_2.temperature.toFixed(2)})</span>
      <input
        type="range" min="0.1" max="1.0" step="0.05"
        bind:value={cfg.tts.breeze_tts_2.temperature}
        onchange={markDirty}
        class="range-input"
      />
    </label>
    <p class="hint" style="margin-top: -6px;">Controls voice expressiveness and variation. Default is 0.70.</p>

    <div class="field">
      <span>Model directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.breeze_tts_2.model_dir}
        onchange={markDirty}
      />
    </div>
    <p class="hint">Default directory: <code>~/.local/share/voxctrl/models/breeze-tts-2/</code></p>
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

  {#if cfg.tts.engine === "inflect_micro"}
  <div class="field-group">
    <h3>Inflect Micro</h3>
    <p class="hint" style="margin-top: 0;">
      A 9.4M-parameter VITS model (38 MB) with a single fixed English voice at 24 kHz, so
      there is no voice to choose. Needs <code>espeak-ng</code> installed for phonemization.
    </p>

    {#if !inflectAvailable}
      <p class="field-error-msg">
        <strong>This build cannot run this engine.</strong> The ONNX half is behind an opt-in
        cargo feature, so Test TTS stays disabled until the app is rebuilt with it:
        <br /><code>npm run tauri dev -- --features inflect-micro</code>
        <br /><code>npm run tauri build -- --features inflect-micro</code>
        <br />The <code>--</code> is required, or npm consumes the flag itself. Downloading the
        model works either way — only synthesis needs the feature.
      </p>
    {/if}

    <div class="voice-status-container">
      {#if inflectChecking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if inflectDownloading}
        <span class="status-downloading">⏳ Downloading Inflect-Micro-v2 model (~38 MB)...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={inflectReady ? "status-downloaded" : "status-missing"}>
            {inflectReady ? "✔ Model downloaded and ready" : "❌ Model files missing"}
          </span>
          <button class="btn-download" onclick={downloadInflect} disabled={inflectReady || inflectDownloading}>
            {inflectReady ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    {#if inflectError}
      <pre class="field-error-msg" style="white-space: pre-wrap; overflow-x: auto;">❌ {inflectError}</pre>
    {/if}

    <label class="field">
      <span>Sampling seed</span>
      <input
        type="number"
        min="0"
        step="1"
        bind:value={cfg.tts.inflect_micro.seed}
        onchange={onInflectSettingChanged}
      />
    </label>
    <p class="hint">
      The model is deterministic for a fixed seed, so the same text always produces identical
      audio. Change it to resample the prosody.
    </p>

    <label class="field">
      <span>Variation (0.0 – 1.0)</span>
      <input
        type="number"
        min="0"
        max="1"
        step="0.01"
        bind:value={cfg.tts.inflect_micro.noise_scale}
        onchange={onInflectSettingChanged}
      />
    </label>

    <p class="hint">
      Higher values give more expressive but less predictable delivery. Default is 0.667.
    </p>

    <label class="field">
      <span>Pre-warm on startup</span>
      <input
        type="checkbox"
        bind:checked={cfg.tts.inflect_micro.prewarm}
        onchange={onInflectSettingChanged}
      />
    </label>
    <p class="hint">
      Loads the ONNX graphs at launch so the first spoken response has no load delay.
    </p>

    <div class="field">
      <span>Model directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.inflect_micro.model_dir}
        onchange={onInflectSettingChanged}
      />
    </div>
    <p class="hint">
      Point this at an existing copy of the model to skip downloading. Default:
      <code>~/.local/share/voxctrl/models/inflect-micro/</code>
    </p>

    <div class="field">
      <span>Model diagnostics</span>
      <button class="btn-download" onclick={inspectInflect} disabled={!inflectReady || inflectInspecting}>
        {inflectInspecting ? "Inspecting..." : "🔍 Inspect graphs"}
      </button>
    </div>
    <p class="hint">
      Reports the tensor names the downloaded export declares. Useful if synthesis fails with a
      message about an input that could not be mapped.
    </p>
    {#if inflectSignature}
      <pre class="hint" style="white-space: pre-wrap; overflow-x: auto;">{inflectSignature}</pre>
    {/if}
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

  <div class="field-group mt-6">
    <h3>TTS Custom Dictionary</h3>
    <p class="hint">Provide a comma-separated list of words (e.g. jargon or names) that the TTS engine should try to correct phonetic spellings for when speaking.</p>
    <textarea 
      class="custom-vocab-input"
      placeholder="e.g. Waylin, Rufer, Enola, Kenz"
      value={ttsCustomVocabString}
      oninput={onTtsCustomVocabChange}
      use:autoResize
    ></textarea>
  </div>

  <div class="field-group mt-6">
    <div class="field-label-row">
      <div style="display: flex; flex-direction: column;">
        <h3 style="margin-bottom: 0;">TTS Snippets (Pronunciation Guide)</h3>
        <p class="hint" style="margin-top: 4px;">Type a word (e.g. "voxctrl") ➔ its spoken expansion/pronunciation (e.g. "vox control"). Only affects speech playback.</p>
      </div>
      <button class="btn-add-inline" type="button" onclick={addEmptyTtsSnippetRow}>
        ＋ Add Pronunciation
      </button>
    </div>

    <div class="dynamic-list">
      {#each ttsSnippetList as snippet, idx}
        <div class="dynamic-list-row">
          <input 
            type="text" 
            placeholder="Word / Abbreviation" 
            bind:value={ttsSnippetList[idx].key} 
            style="flex: 0.4;"
          />
          <span style="color: var(--text-muted);">→</span>
          <input 
            type="text" 
            placeholder="Spoken pronunciation" 
            bind:value={ttsSnippetList[idx].val} 
            style="flex: 1;"
          />
          <button class="btn-remove-inline" type="button" onclick={() => removeTtsSnippetRow(idx)}>✕</button>
        </div>
      {/each}
      {#if ttsSnippetList.length === 0}
        <div class="empty-state" style="padding: 20px; grid-column: 1 / -1;">
          <p>No pronunciation snippets defined.</p>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  @reference "../../app.css";

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

  .custom-vocab-input {
    @apply w-full min-h-[80px] bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] resize-y mt-2 outline-none box-border transition-all duration-200 ease-out;
  }

  .custom-vocab-input:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .custom-vocab-input::placeholder {
    @apply text-[var(--text-muted)] opacity-50;
  }

  .non-commercial-warning {
    @apply bg-amber-950/30 border border-amber-500/40 rounded-[var(--radius)] p-3 mb-2 flex flex-col gap-1.5;
  }
  .warning-header {
    @apply flex items-center gap-2 text-amber-400 text-xs font-semibold;
  }
  .warning-icon {
    @apply text-sm;
  }
  .warning-text {
    @apply text-[12px] text-amber-200/80 leading-relaxed m-0 max-w-none;
  }

  .field-input-textarea {
    @apply w-full bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] resize-y mt-1 outline-none box-border transition-all duration-200 ease-out;
  }
  .field-input-textarea:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .engine-radio-group {
    @apply flex flex-col gap-2 mt-1.5 w-full;
  }
  .engine-radio-option {
    @apply flex flex-col gap-1 p-3 bg-[var(--bg)] border border-[var(--border)] rounded-[var(--radius)] cursor-pointer transition-all duration-200 ease-out;
  }
  .engine-radio-option:hover {
    @apply border-[var(--accent2)] bg-[var(--surface2)];
  }
  .engine-radio-option.selected {
    @apply border-[var(--accent)] bg-[var(--surface2)];
  }
  .engine-radio-header {
    @apply flex items-center gap-2.5;
  }
  .engine-radio-name {
    @apply font-medium text-sm text-[var(--text)];
  }
  .engine-radio-desc {
    @apply text-xs text-[var(--text-muted)] ml-6 leading-normal;
  }

</style>
