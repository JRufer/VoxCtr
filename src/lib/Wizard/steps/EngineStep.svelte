<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { config } from "../../../stores/config";
  import { patchConfig, wizard } from "../wizard-state.svelte";
  import {
    STT_ENGINES,
    accuracyBars,
    formatPercent,
    formatSize,
    speedLabel,
    type ModelOption,
    type SttEngineId,
  } from "../wizard-data";

  let {
    registerGate,
    setBlocker,
  }: {
    registerGate: (step: number, gate: (() => Promise<boolean>) | null) => void;
    setBlocker: (step: number, reason: string | null) => void;
  } = $props();

  const STEP = 1;

  /** Whether this build actually contains the Moonshine backend. Without it a
   *  "moonshine" selection silently runs whisper.cpp, which the user deserves
   *  to know before they pick it. */
  let moonshineAvailable = $state(true);
  let cudaEnabled = $state(false);

  /** model id → on disk, per engine. */
  let whisperDownloaded = $state<Record<string, boolean>>({});
  let moonshineDownloaded = $state<Record<string, boolean>>({});

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  const selectedEngine = $derived<SttEngineId>(
    $config.engine.backend === "moonshine" ? "moonshine" : "whisper-cpp",
  );
  const gpuOn = $derived($config.engine.whisper_cpp.device !== "cpu");
  const gpuPath = $derived(cudaEnabled ? "CUDA" : "Vulkan");

  /** The model the current engine will actually load. */
  const selectedModel = $derived(
    selectedEngine === "moonshine" && moonshineAvailable
      ? $config.engine.moonshine.model_size
      : $config.engine.whisper_cpp.model_size,
  );

  const selectedReady = $derived(
    selectedEngine === "moonshine" && moonshineAvailable
      ? !!moonshineDownloaded[selectedModel]
      : !!whisperDownloaded[selectedModel],
  );

  function isSelected(engine: SttEngineId, model: ModelOption): boolean {
    if (engine !== selectedEngine) return false;
    return model.id === (engine === "moonshine"
      ? $config.engine.moonshine.model_size
      : $config.engine.whisper_cpp.model_size);
  }

  function downloadedFor(engine: SttEngineId, model: ModelOption): boolean {
    return engine === "moonshine" ? !!moonshineDownloaded[model.id] : !!whisperDownloaded[model.id];
  }

  function pickEngine(id: SttEngineId) {
    patchConfig((cfg) => {
      cfg.engine.backend = id;
    });
  }

  function pickModel(engine: SttEngineId, model: ModelOption) {
    patchConfig((cfg) => {
      cfg.engine.backend = engine;
      if (engine === "moonshine") cfg.engine.moonshine.model_size = model.id;
      else cfg.engine.whisper_cpp.model_size = model.id;
    });
    downloadError = null;
  }

  function toggleGpu() {
    patchConfig((cfg) => {
      // "auto" lets the engine pick CUDA or Vulkan by what the machine has,
      // which is a better answer than pinning one of them from here.
      cfg.engine.whisper_cpp.device = gpuOn ? "cpu" : "auto";
    });
  }

  async function refreshWhisper() {
    const dir = $config.engine.whisper_cpp.model_dir;
    const next: Record<string, boolean> = {};
    for (const m of STT_ENGINES[0].models) {
      try {
        next[m.id] = await invoke<boolean>("check_model_downloaded", {
          modelSize: m.id,
          modelDir: dir,
        });
      } catch (e) {
        console.error("Wizard: whisper model check failed for", m.id, e);
        next[m.id] = false;
      }
    }
    whisperDownloaded = next;
  }

  async function refreshMoonshine() {
    const next: Record<string, boolean> = {};
    for (const m of STT_ENGINES[1].models) {
      try {
        next[m.id] = await invoke<boolean>("check_moonshine_downloaded", { modelSize: m.id });
      } catch (e) {
        console.error("Wizard: moonshine model check failed for", m.id, e);
        next[m.id] = false;
      }
    }
    moonshineDownloaded = next;
  }

  /**
   * Fetch whichever model the user just chose, and only then let the wizard
   * move on. Downloading here rather than in the background is the point of
   * the step: the next screen asks them to press a hotkey and speak, and that
   * cannot work without weights on disk.
   */
  async function ensureModel(): Promise<boolean> {
    if (selectedReady) return true;
    const engine = selectedEngine;
    const model = selectedModel;
    downloading = model;
    downloadError = null;
    try {
      if (engine === "moonshine" && moonshineAvailable) {
        await invoke("download_moonshine_model", { modelSize: model });
        moonshineDownloaded = { ...moonshineDownloaded, [model]: true };
      } else {
        await invoke("download_model", {
          modelSize: model,
          modelDir: $config.engine.whisper_cpp.model_dir,
        });
        whisperDownloaded = { ...whisperDownloaded, [model]: true };
      }
      wizard.clearIssue("model-download");
      return true;
    } catch (e) {
      downloadError = `${e}`;
      // Logged for the final screen: a missing model is the difference between
      // a working install and a hotkey that records and then produces nothing.
      wizard.recordIssue({
        id: "model-download",
        step: STEP,
        title: `Speech model "${model}" could not be downloaded — dictation will produce no text.`,
        detail: `engine=${engine} model=${model} model_dir=${$config.engine.whisper_cpp.model_dir || "(default)"}\n${e}`,
      });
      return false;
    } finally {
      downloading = null;
    }
  }

  // The Continue button stays live while a model is missing — pressing it is
  // what starts the download — and is only blocked while one is in flight.
  $effect(() => {
    setBlocker(STEP, downloading ? `Downloading ${downloading}…` : null);
  });

  onMount(() => {
    registerGate(STEP, ensureModel);
    invoke<boolean>("moonshine_available")
      .then((v) => (moonshineAvailable = v))
      .catch(() => (moonshineAvailable = false));
    invoke<boolean>("cuda_enabled")
      .then((v) => (cudaEnabled = v))
      .catch(() => (cudaEnabled = false));
    void refreshWhisper();
    void refreshMoonshine();
    return () => {
      registerGate(STEP, null);
      setBlocker(STEP, null);
    };
  });

  /** Metrics for one engine card, recomputed from the model it has selected. */
  function metricsFor(engine: (typeof STT_ENGINES)[number]) {
    const chosen =
      engine.id === "moonshine"
        ? $config.engine.moonshine.model_size
        : $config.engine.whisper_cpp.model_size;
    const model = engine.models.find((m) => m.id === chosen) ?? engine.models[0];
    const gpu = engine.gpu && gpuOn;
    const speed = Math.min(1, model.speed + (gpu ? 0.3 : 0));
    const ram = gpu ? model.mb * 0.3 : model.mb;
    const vram = gpu ? model.mb * 1.1 : 0;
    return {
      model,
      rows: [
        { label: "speed", pct: Math.round(speed * 100), value: speedLabel(speed), color: "var(--vx-cyan-0)", dim: false },
        { label: "accuracy", pct: Math.round(model.accuracy * 100), value: formatPercent(model.accuracy), color: "var(--vx-cyan-2)", dim: false },
        { label: "RAM", pct: Math.max(2, Math.min(100, Math.round((ram / 3400) * 100))), value: formatSize(ram), color: "var(--vx-gold-1)", dim: false },
        {
          label: "VRAM",
          pct: vram ? Math.max(2, Math.min(100, Math.round((vram / 3400) * 100))) : 0,
          value: vram ? formatSize(vram) : engine.gpu ? "off" : "cpu only",
          color: "var(--vx-gold-0)",
          dim: !vram,
        },
      ],
      quiet: accuracyBars(12, model.accuracy),
      noisy: accuracyBars(12, model.accuracy * engine.noiseRetention),
      quietPct: formatPercent(model.accuracy),
      noisyPct: formatPercent(model.accuracy * engine.noiseRetention),
    };
  }
</script>

<div class="engine-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 01 · transcription engine</span>
      <h2 class="vx-title">Which ears should VoxCtrl use?</h2>
      <p class="vx-lede">
        Both run 100% on-device. <b>whisper.cpp</b> is most accurate in a quiet room and scales to
        very large models. <b>Moonshine</b> keeps its accuracy in noisy rooms and stays fast on CPU.
        Pick a model size inside each card — larger is more accurate, smaller is faster and lighter.
      </p>
    </div>

    <button class="vx-card gpu-toggle" class:vx-on={gpuOn} onclick={toggleGpu}>
      <span class="switch" class:on={gpuOn}><span class="knob"></span></span>
      <span class="gpu-copy">
        <span class="gpu-title">
          <span class="glyph">∗</span> GPU offloading
          <span class="state" class:on={gpuOn}>{gpuOn ? `ON · ${gpuPath}` : "OFF · CPU"}</span>
        </span>
        <span class="gpu-desc">
          Moves whisper.cpp weights from RAM to your GPU: faster, less RAM, uses VRAM. Moonshine is
          CPU-native and unaffected.
        </span>
      </span>
    </button>
  </div>

  <div class="engines">
    {#each STT_ENGINES as engine}
      {@const m = metricsFor(engine)}
      {@const on = selectedEngine === engine.id}
      <div
        class="vx-card engine-card"
        class:vx-on={on}
        role="radio"
        aria-checked={on}
        tabindex="0"
        onclick={() => pickEngine(engine.id)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            pickEngine(engine.id);
          }
        }}
      >
        <div class="vx-check corner"><span>✓</span></div>

        <div class="engine-head">
          <div class="name-row">
            <span class="engine-glyph">{engine.glyph}</span>
            <span class="engine-name">{engine.name}</span>
          </div>
          <div class="tagline">{engine.tagline}</div>
          {#if engine.id === "moonshine" && !moonshineAvailable}
            <div class="warn">
              This build was compiled without the Moonshine backend — choosing it runs whisper.cpp
              with the model above instead.
            </div>
          {/if}
        </div>

        <div class="left-col">
          <div class="vx-label">model size</div>
          <div class="sizes">
            {#each engine.models as model}
              <button
                class="size"
                class:on={isSelected(engine.id, model)}
                onclick={(e) => {
                  e.stopPropagation();
                  pickModel(engine.id, model);
                }}
              >
                <span class="size-id">{model.id}</span>
                <span class="size-mb">
                  {formatSize(model.mb)}{downloadedFor(engine.id, model) ? " ✔" : ""}
                </span>
              </button>
            {/each}
          </div>

          <div class="metrics">
            {#each m.rows as row}
              <div class="metric" class:dim={row.dim}>
                <span class="metric-label">{row.label}</span>
                <div class="vx-meter">
                  <div style:width="{row.pct}%" style:background={row.color}></div>
                </div>
                <span class="metric-value">{row.value}</span>
              </div>
            {/each}
          </div>
        </div>

        <div class="right-col">
          <div class="spark">
            <div class="spark-head"><span>◌ quiet room</span><span>{m.quietPct}</span></div>
            <div class="spark-bars">
              {#each m.quiet as b}
                <div style:height="{b.h}%" style:opacity={b.o}></div>
              {/each}
            </div>
          </div>
          <div class="spark">
            <div class="spark-head"><span>≋ noisy room</span><span>{m.noisyPct}</span></div>
            <div class="spark-bars">
              {#each m.noisy as b}
                <div style:height="{b.h}%" style:opacity={b.o}></div>
              {/each}
            </div>
          </div>
        </div>
      </div>
    {/each}
  </div>

  <div class="status-row">
    {#if downloading}
      <span class="vx-pill vx-busy"><span class="vx-spinner"></span> Downloading {downloading} — this can take a few minutes</span>
    {:else if downloadError}
      <span class="vx-pill vx-err">✕ Download failed: {downloadError}</span>
    {:else if selectedReady}
      <span class="vx-pill vx-ok">✓ {selectedModel} is on disk and ready</span>
    {:else}
      <span class="vx-pill">↓ {selectedModel} will download when you continue</span>
    {/if}
  </div>
</div>

<style>
  .engine-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .head {
    display: flex;
    gap: 28px;
    align-items: flex-end;
    justify-content: space-between;
  }

  .copy {
    max-width: 760px;
  }

  .gpu-toggle {
    flex: none;
    width: 400px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
  }

  .switch {
    width: 50px;
    height: 30px;
    border-radius: 999px;
    position: relative;
    flex: none;
    background: var(--vx-bg-4);
    border: 1px solid var(--vx-line-2);
    transition: background 0.3s;
  }

  .switch.on {
    background: var(--vx-cyan-0);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
    transition: left 0.28s var(--vx-ease);
  }

  .switch.on .knob {
    left: 23px;
  }

  .gpu-copy {
    display: block;
  }

  .gpu-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 14px;
  }

  .gpu-title .glyph {
    font-family: var(--vx-mono);
    color: var(--vx-cyan-1);
  }

  .state {
    font-family: var(--vx-mono);
    font-size: 10.5px;
    letter-spacing: 0.1em;
    color: var(--vx-txt-3);
    transition: color 0.3s;
  }

  .state.on {
    color: var(--vx-cyan-1);
  }

  .gpu-desc {
    display: block;
    font-size: 12px;
    color: var(--vx-txt-2);
    margin-top: 2px;
    line-height: 1.4;
  }

  .engines {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }

  .engine-card {
    position: relative;
    padding: 20px 22px;
    border-radius: 18px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 18px 22px;
    align-content: start;
  }

  .corner {
    position: absolute;
    top: 16px;
    right: 16px;
    width: 26px;
    height: 26px;
  }

  .engine-head {
    grid-column: 1 / -1;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 6px;
  }

  .engine-glyph {
    font-family: var(--vx-mono);
    font-size: 24px;
    color: var(--vx-cyan-1);
  }

  .engine-name {
    font-size: 21px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .tagline {
    font-size: 13px;
    color: var(--vx-txt-2);
    line-height: 1.45;
    max-width: 90%;
  }

  .warn {
    margin-top: 10px;
    padding: 8px 11px;
    border-radius: 9px;
    border: 1px solid rgba(255, 180, 84, 0.3);
    background: rgba(255, 180, 84, 0.06);
    color: var(--vx-warn);
    font-size: 12px;
    line-height: 1.45;
  }

  .sizes {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
  }

  .size {
    height: 46px;
    padding: 0 13px;
    border-radius: 10px;
    border: 1px solid var(--vx-line);
    background: rgba(255, 255, 255, 0.02);
    color: var(--vx-txt-1);
    font-family: var(--vx-mono);
    font-size: 12.5px;
    cursor: pointer;
    transition: all 0.22s;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 1px;
    line-height: 1.1;
  }

  .size:hover {
    border-color: var(--vx-line-2);
  }

  .size.on {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.12);
    color: var(--vx-cyan-1);
  }

  .size-mb {
    font-size: 10px;
    opacity: 0.7;
  }

  .metrics {
    margin-top: 16px;
    display: flex;
    flex-direction: column;
    gap: 9px;
  }

  .metric {
    display: grid;
    grid-template-columns: 64px 1fr 58px;
    align-items: center;
    gap: 10px;
    transition: opacity 0.3s;
  }

  .metric.dim {
    opacity: 0.45;
  }

  .metric-label {
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-2);
  }

  .metric-value {
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-1);
    text-align: right;
  }

  .right-col {
    display: grid;
    grid-template-rows: 1fr 1fr;
    gap: 10px;
  }

  .spark {
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--vx-line);
    background: rgba(0, 0, 0, 0.25);
    display: flex;
    flex-direction: column;
  }

  .spark-head {
    display: flex;
    justify-content: space-between;
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-2);
    margin-bottom: 6px;
  }

  .spark-head span:last-child {
    color: var(--vx-txt-0);
  }

  .spark-bars {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    flex: 1;
    min-height: 28px;
  }

  .spark-bars > div {
    flex: 1;
    border-radius: 2px;
    background: var(--vx-cyan-0);
    transition: height 0.6s var(--vx-ease), opacity 0.4s;
  }

  .status-row {
    flex: none;
    display: flex;
    justify-content: flex-end;
  }

  @media (max-width: 1100px) {
    .head {
      flex-direction: column;
      align-items: stretch;
      gap: 14px;
    }

    .gpu-toggle {
      width: 100%;
    }

    .engines {
      grid-template-columns: 1fr;
    }
  }
</style>
