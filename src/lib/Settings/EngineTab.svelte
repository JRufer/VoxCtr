<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  const MODEL_SIZES = [
    "tiny",
    "tiny.en",
    "base",
    "base.en",
    "small",
    "small.en",
    "medium",
    "medium.en",
    "large-v2",
    "large-v3",
    "large-v3-turbo",
  ];

  const backendOptions = [
    { value: "auto", label: "Auto-detect" },
    { value: "whisper-cpp", label: "Whisper.cpp" },
    { value: "moonshine", label: "Moonshine (CPU only)" }
  ];

  let whisperModelSizeOptions = $derived(
    MODEL_SIZES.map(s => ({
      value: s,
      label: `${s}${downloadedMap[s] ? " ✔" : ""}`
    }))
  );

  let cudaEnabled = $state(false);

  let deviceOptions = $derived([
    { value: "auto", label: "Auto" },
    ...(cudaEnabled ? [{ value: "cuda", label: "CUDA (NVIDIA)" }] : []),
    { value: "vulkan", label: "Vulkan (AMD/Intel)" },
    { value: "cpu", label: "CPU" }
  ]);

  const moonshineModelSizeOptions = [
    { value: "base", label: "Base" },
    { value: "tiny", label: "Tiny" }
  ];

  let downloadedMap = $state<Record<string, boolean>>({});
  let checking = $state(false);
  let downloading = $state(false);
  let modelDirError = $state<string | null>(null);

  // ── Moonshine ────────────────────────────────────────────────────────────
  // Whether the app was built with the Moonshine backend. When false, choosing
  // Moonshine silently runs whisper-cpp, so we surface that to the user.
  let moonshineAvailable = $state(true);
  let moonshineDownloadedMap = $state<Record<string, boolean>>({});
  let moonshineChecking = $state(false);
  let moonshineDownloading = $state(false);

  async function checkMoonshineDownloaded() {
    moonshineChecking = true;
    const newMap: Record<string, boolean> = {};
    for (const m of moonshineModelSizeOptions) {
      try {
        newMap[m.value] = await invoke<boolean>("check_moonshine_downloaded", {
          modelSize: m.value,
        });
      } catch (e) {
        console.error("Failed to check Moonshine download status for " + m.value, e);
        newMap[m.value] = false;
      }
    }
    moonshineDownloadedMap = newMap;
    moonshineChecking = false;
  }

  async function triggerMoonshineDownload(model: string) {
    if (moonshineDownloading) return;
    moonshineDownloading = true;
    try {
      await invoke("download_moonshine_model", { modelSize: model });
      moonshineDownloadedMap[model] = true;
    } catch (e) {
      alert(`Failed to download Moonshine model: ${e}`);
    } finally {
      moonshineDownloading = false;
    }
  }

  async function onMoonshineModelChanged() {
    markDirty();
    const selected = cfg.engine.moonshine.model_size;
    if (moonshineAvailable && !moonshineDownloadedMap[selected]) {
      await triggerMoonshineDownload(selected);
    }
  }

  async function checkAllModelsDownloaded() {
    checking = true;
    const newMap: Record<string, boolean> = {};
    for (const m of MODEL_SIZES) {
      try {
        newMap[m] = await invoke<boolean>("check_model_downloaded", {
          modelSize: m,
          modelDir: cfg.engine.whisper_cpp.model_dir,
        });
      } catch (e) {
        console.error("Failed to check download status for model " + m, e);
        newMap[m] = false;
      }
    }
    downloadedMap = newMap;
    checking = false;
  }

  async function triggerDownload(model: string) {
    if (downloading) return;
    downloading = true;
    try {
      await invoke("download_model", {
        modelSize: model,
        modelDir: cfg.engine.whisper_cpp.model_dir,
      });
      downloadedMap[model] = true;
    } catch (e) {
      alert(`Failed to download model: ${e}`);
    } finally {
      downloading = false;
    }
  }

  async function onModelChanged() {
    markDirty();
    const selected = cfg.engine.whisper_cpp.model_size;
    if (!downloadedMap[selected]) {
      await triggerDownload(selected);
    }
  }

  async function validateModelDir() {
    const path = cfg.engine.whisper_cpp.model_dir;
    if (!path) {
      modelDirError = null;
      return;
    }
    const exists = await invoke<boolean>("check_directory_exists", { path });
    modelDirError = exists
      ? null
      : "This folder does not exist. Please create it first or leave blank for the default location.";
    if (!modelDirError) {
      await checkAllModelsDownloaded();
    }
  }

  function onModelDirChange() {
    markDirty();
  }

  function onModelDirKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      (e.currentTarget as HTMLInputElement).blur();
    }
  }

  onMount(async () => {
    checkAllModelsDownloaded();
    try {
      moonshineAvailable = await invoke<boolean>("moonshine_available");
    } catch (e) {
      console.error("Failed to query Moonshine availability", e);
    }
    checkMoonshineDownloaded();
    cudaEnabled = await invoke<boolean>("cuda_enabled");
    // If this is a CPU build but config still says "cuda", reset to "auto"
    if (!cudaEnabled && cfg.engine.whisper_cpp.device === "cuda") {
      cfg.engine.whisper_cpp.device = "auto";
      markDirty();
    }
  });
</script>

<section>
  <h2>Inference Engine</h2>

  {#if cfg.engine.backend !== "moonshine" && !checking && !downloadedMap[cfg.engine.whisper_cpp.model_size]}
    <div
      class="flex items-center gap-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl p-6 mb-5 animate-in fade-in slide-in-from-top-1 duration-300"
    >
      <span
        class="text-2xl leading-none text-yellow-500 drop-shadow-[0_0_6px_rgba(234,179,8,0.3)]"
        >⚠️</span
      >
      <div class="flex-1">
        <strong class="block text-yellow-200 font-semibold text-sm mb-1"
          >Voice Model Not Downloaded</strong
        >
        <p class="m-0 text-slate-200 text-xs leading-relaxed">
          The configuration specifies a voice model (<strong
            >{cfg.engine.whisper_cpp.model_size}</strong
          >) that is not currently downloaded. Please select the model size to
          use and download it below, or choose another model.
        </p>
      </div>
    </div>
  {/if}

  <div class="field-group">
    <h3>Backend</h3>
    <label class="field">
      <span>Backend</span>
      <CustomSelect bind:value={cfg.engine.backend} options={backendOptions} onchange={markDirty} />
    </label>
  </div>

  {#if cfg.engine.backend !== "moonshine"}
    <div class="field-group">
      <h3>Whisper.cpp Settings</h3>
      <label class="field">
        <span>Model size</span>
        <CustomSelect bind:value={cfg.engine.whisper_cpp.model_size} options={whisperModelSizeOptions} onchange={onModelChanged} />
      </label>

      <div class="model-status-container">
        {#if checking}
          <span class="status-checking">⏳ Checking local model files...</span>
        {:else if downloading}
          <span class="status-downloading"
            >⏳ Downloading {cfg.engine.whisper_cpp.model_size} (GGUF format)...</span
          >
        {:else if downloadedMap[cfg.engine.whisper_cpp.model_size]}
          <span class="status-downloaded">✔ Model downloaded and ready</span>
        {:else}
          <div class="status-missing-wrapper">
            <span class="status-missing">❌ Model file missing</span>
            <button
              class="btn-download"
              onclick={() => triggerDownload(cfg.engine.whisper_cpp.model_size)}
            >
              📥 Download Model
            </button>
          </div>
        {/if}
      </div>

      <label class="field">
        <span>Device</span>
        <CustomSelect bind:value={cfg.engine.whisper_cpp.device} options={deviceOptions} onchange={markDirty} />
      </label>
      <div class="field">
        <span>Model directory (leave blank for default)</span>
        <input
          type="text"
          bind:value={cfg.engine.whisper_cpp.model_dir}
          onchange={onModelDirChange}
          onblur={validateModelDir}
          onkeydown={onModelDirKeydown}
          class:field-input-error={!!modelDirError}
        />
        {#if modelDirError}
          <p class="field-error-msg">{modelDirError}</p>
        {/if}
      </div>
      <label class="field">
        <span>Threads (0 = auto)</span>
        <input
          type="number"
          min="0"
          max="64"
          bind:value={cfg.engine.whisper_cpp.threads}
          onchange={markDirty}
        />
      </label>
      <p class="hint">
        Default model directory: <code>~/.local/share/voxctrl/models/</code>
      </p>
    </div>
  {:else}
    <div class="field-group">
      <h3>Moonshine Settings</h3>

      {#if !moonshineAvailable}
        <div
          class="flex items-center gap-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl p-4 mb-4"
        >
          <span class="text-2xl leading-none text-yellow-500">⚠️</span>
          <div class="flex-1">
            <strong class="block text-yellow-200 font-semibold text-sm mb-1"
              >Moonshine backend not included in this build</strong
            >
            <p class="m-0 text-slate-200 text-xs leading-relaxed">
              This build was compiled without Moonshine, so selecting it will fall
              back to Whisper.cpp (using the Whisper model configured above).
              Rebuild with <code>--features moonshine</code> to enable it.
            </p>
          </div>
        </div>
      {/if}

      <label class="field">
        <span>Model size</span>
        <CustomSelect bind:value={cfg.engine.moonshine.model_size} options={moonshineModelSizeOptions} onchange={onMoonshineModelChanged} />
      </label>

      {#if moonshineAvailable}
        <div class="model-status-container">
          {#if moonshineChecking}
            <span class="status-checking">⏳ Checking local model files...</span>
          {:else if moonshineDownloading}
            <span class="status-downloading"
              >⏳ Downloading Moonshine {cfg.engine.moonshine.model_size} (ONNX)...</span
            >
          {:else if moonshineDownloadedMap[cfg.engine.moonshine.model_size]}
            <span class="status-downloaded">✔ Model downloaded</span>
          {:else}
            <div class="status-missing-wrapper">
              <span class="status-missing">Model not downloaded</span>
              <button
                class="btn-download"
                onclick={() => triggerMoonshineDownload(cfg.engine.moonshine.model_size)}
              >
                Download {cfg.engine.moonshine.model_size}
              </button>
            </div>
          {/if}
        </div>
      {/if}

      <label class="field">
        <span>Language</span>
        <input
          type="text"
          bind:value={cfg.engine.moonshine.language}
          onchange={markDirty}
        />
      </label>
    </div>
  {/if}
</section>

<style>
  @reference "../../app.css";

  .model-status-container {
    @apply flex items-center bg-[var(--bg)] border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 text-[13px] min-h-[42px] mb-3;
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
  .btn-download:hover {
    @apply bg-[var(--accent2)];
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
</style>
