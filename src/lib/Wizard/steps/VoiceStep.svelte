<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { get } from "svelte/store";
  import { config } from "../../../stores/config";
  import { patchConfig, wizard } from "../wizard-state.svelte";
  import {
    TTS_ENGINES,
    formatPercent,
    formatSize,
    ttsSpeedLabel,
    waveBars,
    type TtsEngineId,
  } from "../wizard-data";

  let {
    setBlocker,
  }: {
    setBlocker: (step: number, reason: string | null) => void;
  } = $props();

  const STEP = 5;

  const enabled = $derived($config.tts.enabled);
  const selected = $derived($config.tts.engine as TtsEngineId);

  /** engine id → its model/voice files are on disk. */
  let ready = $state<Record<string, boolean>>({ espeak: true });
  let checking = $state<Record<string, boolean>>({});
  let downloading = $state<string | null>(null);
  let errors = $state<Record<string, string>>({});

  /** Whether this build compiled the Inflect Micro engine at all. */
  let inflectAvailable = $state(true);

  let playing = $state<string | null>(null);
  let playError = $state<string | null>(null);
  const bars = waveBars(14);

  function setErr(id: string, msg: string | null) {
    const next = { ...errors };
    if (msg) next[id] = msg;
    else delete next[id];
    errors = next;
  }

  /** Ask the backend whether one engine's assets are already present. */
  async function checkEngine(id: TtsEngineId): Promise<boolean> {
    const cfg = get(config);
    try {
      switch (id) {
        case "espeak":
          return true;
        case "piper":
          return await invoke<boolean>("check_voice_downloaded", {
            voiceName: cfg.tts.voice,
            voiceDir: cfg.tts.voice_dir,
          });
        case "pocket_tts":
          return await invoke<boolean>("check_pocket_tts_ready", {
            voice: cfg.tts.pocket_tts.voice,
            voiceDir: cfg.tts.pocket_tts.voice_dir,
          });
        case "inflect_micro":
          return await invoke<boolean>("check_inflect_micro_downloaded", {
            modelDir: cfg.tts.inflect_micro.model_dir,
          });
        case "breeze_tts_2":
          return await invoke<boolean>("check_breeze_tts_2_ready", {
            modelDir: cfg.tts.breeze_tts_2.model_dir,
          });
      }
    } catch (e) {
      console.error("Wizard: TTS readiness check failed for", id, e);
      return false;
    }
  }

  async function refreshAll() {
    for (const engine of TTS_ENGINES) {
      checking = { ...checking, [engine.id]: true };
      const ok = await checkEngine(engine.id);
      ready = { ...ready, [engine.id]: ok };
      checking = { ...checking, [engine.id]: false };
    }
  }

  /** Fetch one engine's assets. Each card drives its own download so the user
   *  can audition several voices before committing to one. */
  async function download(id: TtsEngineId) {
    if (downloading) return;
    downloading = id;
    setErr(id, null);
    const cfg = get(config);
    try {
      switch (id) {
        case "piper":
          await invoke("download_voice", {
            voiceName: cfg.tts.voice,
            voiceDir: cfg.tts.voice_dir,
          });
          break;
        case "pocket_tts":
          await invoke("download_pocket_tts", {
            voice: cfg.tts.pocket_tts.voice,
            voiceDir: cfg.tts.pocket_tts.voice_dir,
            hfToken: cfg.tts.pocket_tts.hf_token,
          });
          break;
        case "inflect_micro":
          await invoke("download_inflect_micro", {
            modelDir: cfg.tts.inflect_micro.model_dir,
          });
          break;
        case "breeze_tts_2":
          await invoke("download_breeze_tts_2", {
            modelDir: cfg.tts.breeze_tts_2.model_dir,
            hfToken: cfg.tts.breeze_tts_2.hf_token ?? cfg.tts.pocket_tts.hf_token,
          });
          break;
        case "espeak":
          break;
      }
      ready = { ...ready, [id]: true };
      wizard.clearIssue(`tts-download-${id}`);
    } catch (e) {
      // Reported on the card rather than in a dialog: the backend lists every
      // URL it tried, and a modal would leave no way to read or copy it.
      setErr(id, `${e}`);
      wizard.recordIssue({
        id: `tts-download-${id}`,
        step: STEP,
        title: `${TTS_ENGINES.find((t) => t.id === id)?.name ?? id} could not be downloaded — speech output will stay silent.`,
        detail: `engine=${id}\n${e}`,
      });
    } finally {
      downloading = null;
    }
  }

  function pick(id: TtsEngineId) {
    patchConfig((cfg) => {
      cfg.tts.enabled = true;
      cfg.tts.engine = id;
    });
  }

  function setEnabled(on: boolean) {
    patchConfig((cfg) => {
      cfg.tts.enabled = on;
    });
  }

  /**
   * Speak a sample through the engine the user just picked — the same path
   * Settings → TTS uses, so a sample that works here works in the app.
   */
  async function play(id: TtsEngineId) {
    if (playing) return;
    pick(id);
    playError = null;
    playing = id;
    const cfg = get(config);
    const engine = TTS_ENGINES.find((e) => e.id === id)!;
    const voice =
      id === "piper" ? cfg.tts.voice : id === "pocket_tts" ? cfg.tts.pocket_tts.voice : null;
    try {
      // The worker is (re)started from the saved config, so the engine has to
      // be on disk in its final form before the sample is requested.
      await invoke("save_config", { newConfig: cfg });
      await invoke("speak_text", { text: `Hi this is ${engine.name} speaking from VoxCtrl`, voice });
    } catch (e) {
      playError = `${e}`;
      playing = null;
      wizard.recordIssue({
        id: `tts-speak-${id}`,
        step: STEP,
        title: `${engine.name} failed to speak the sample.`,
        detail: `engine=${id} voice=${voice ?? "(fixed)"}\n${e}`,
      });
    }
  }

  $effect(() => {
    if (!enabled) {
      setBlocker(STEP, null);
      return;
    }
    if (downloading) {
      setBlocker(STEP, `Downloading ${TTS_ENGINES.find((e) => e.id === downloading)?.name}…`);
      return;
    }
    if (selected === "inflect_micro" && !inflectAvailable) {
      setBlocker(STEP, "This build has no Inflect Micro engine — pick another voice or skip.");
      return;
    }
    setBlocker(
      STEP,
      ready[selected]
        ? null
        : "Download the voice you picked, or turn speech output off to continue.",
    );
  });

  onMount(() => {
    void refreshAll();
    invoke<boolean>("inflect_micro_available")
      .then((v) => (inflectAvailable = v))
      .catch(() => (inflectAvailable = false));

    const subs = [
      listen("tts-playback-end", () => (playing = null)),
      listen<string>("tts-error", (e) => {
        playError = e.payload;
        playing = null;
        wizard.recordIssue({
          id: "tts-runtime",
          step: STEP,
          title: "The text-to-speech engine reported an error while speaking.",
          detail: `engine=${get(config).tts.engine}\n${e.payload}`,
        });
      }),
    ];
    return () => {
      setBlocker(STEP, null);
      for (const s of subs) void s.then((off) => off()).catch(() => {});
      void invoke("stop_tts").catch(() => {});
    };
  });
</script>

<div class="voice-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 05 · text to speech · optional</span>
      <h2 class="vx-title">Should VoxCtrl talk back?</h2>
      <p class="vx-lede">
        Agents can stream replies back through a response pipe and VoxCtrl speaks them aloud. Richer
        voices need bigger models and more time per sentence; lighter engines answer instantly.
        Download one and play a sample before deciding.
      </p>
    </div>

    <div class="choice">
      <button class="vx-card mode" class:vx-on={enabled} onclick={() => setEnabled(true)}>
        <span class="mode-glyph on">⊕</span>
        <span>
          <span class="mode-title">Enable speech output</span>
          <span class="mode-desc">Downloads once · runs offline</span>
        </span>
      </button>
      <button class="vx-card mode" class:off-on={!enabled} onclick={() => setEnabled(false)}>
        <span class="mode-glyph">⊘</span>
        <span>
          <span class="mode-title">Skip for now</span>
          <span class="mode-desc">Enable later in Settings → TTS</span>
        </span>
      </button>
    </div>
  </div>

  <div class="grid" class:muted={!enabled}>
    {#each TTS_ENGINES as engine}
      {@const on = enabled && selected === engine.id}
      {@const isReady = !!ready[engine.id]}
      {@const busy = downloading === engine.id}
      {@const unavailable = engine.id === "inflect_micro" && !inflectAvailable}
      <div
        class="vx-card card"
        class:vx-on={on}
        role="radio"
        aria-checked={on}
        tabindex="0"
        onclick={() => pick(engine.id)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            pick(engine.id);
          }
        }}
      >
        <div class="card-head">
          <div>
            <div class="name">{engine.name}</div>
            <div class="kind">{engine.kind}</div>
          </div>
          <div class="vx-check"><span>✓</span></div>
        </div>

        <div class="actions">
          {#if engine.mb === 0}
            <div class="bundled">bundled · nothing to download</div>
          {:else if busy}
            <button class="vx-btn dl" disabled><span class="vx-spinner"></span> Downloading…</button>
          {:else if isReady}
            <div class="bundled ok">✓ downloaded</div>
          {:else}
            <button
              class="vx-btn dl"
              disabled={!!downloading || unavailable}
              onclick={(e) => {
                e.stopPropagation();
                void download(engine.id);
              }}
            >
              ↓ Download {formatSize(engine.mb)}
            </button>
          {/if}

          <button
            class="play"
            class:playing={playing === engine.id}
            disabled={!isReady || unavailable || (!!playing && playing !== engine.id)}
            title={isReady ? "Play a sample" : "Download this voice first"}
            onclick={(e) => {
              e.stopPropagation();
              void play(engine.id);
            }}
          >
            {#if playing === engine.id}
              <span class="play-bars">
                {#each bars as b}
                  <div style:animation-duration="{b.d}s" style:animation-delay="{b.dl}s"></div>
                {/each}
              </span>
            {:else}
              <span class="tri">▶</span><span class="play-label">play sample</span>
            {/if}
          </button>
        </div>

        <div class="metrics">
          {#each [{ label: "quality", pct: Math.round(engine.quality * 100), value: formatPercent(engine.quality), color: "var(--vx-cyan-0)" }, { label: "speed", pct: Math.round(engine.speed * 100), value: ttsSpeedLabel(engine.speed), color: "var(--vx-cyan-2)" }, { label: "model size", pct: Math.max(2, Math.round((Math.log10(engine.mb + 1) / Math.log10(1300)) * 100)), value: formatSize(engine.mb), color: "var(--vx-gold-1)" }] as m}
            <div class="metric">
              <div class="metric-head"><span>{m.label}</span><span>{m.value}</span></div>
              <div class="vx-meter"><div style:width="{m.pct}%" style:background={m.color}></div></div>
            </div>
          {/each}
        </div>

        <div class="note">
          {#if unavailable}
            <span class="bad">
              This build was compiled without the `inflect-micro` feature, so this engine cannot
              synthesize.
            </span>
          {:else if errors[engine.id]}
            <span class="bad">{errors[engine.id]}</span>
          {:else}
            {engine.note}
          {/if}
        </div>
      </div>
    {/each}
  </div>

  {#if playError}
    <div class="play-error">⚠ {playError}</div>
  {/if}
</div>

<style>
  .voice-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .head {
    display: flex;
    gap: 24px;
    align-items: flex-end;
    justify-content: space-between;
    flex: none;
    min-width: 0;
  }

  .copy {
    max-width: 720px;
    min-width: 0;
  }

  .choice {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    flex: none;
    width: 520px;
  }

  .mode {
    height: 66px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .mode.off-on {
    border-color: var(--vx-line-2);
    background: var(--vx-bg-3);
    box-shadow: 0 0 0 1px var(--vx-line-2);
  }

  .mode-glyph {
    font-family: var(--vx-mono);
    font-size: 22px;
    color: var(--vx-txt-2);
  }

  .mode-glyph.on {
    color: var(--vx-cyan-1);
  }

  .mode-title {
    display: block;
    font-weight: 600;
    font-size: 14px;
  }

  .mode-desc {
    display: block;
    font-size: 12px;
    color: var(--vx-txt-2);
  }

  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 10px;
    transition: opacity 0.4s, filter 0.4s;
  }

  .grid.muted {
    opacity: 0.18;
    filter: grayscale(1) blur(1px);
    pointer-events: none;
  }

  .card {
    padding: 16px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
  }

  .card-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }

  .name {
    font-weight: 600;
    font-size: 15px;
    letter-spacing: -0.01em;
  }

  .kind {
    font-family: var(--vx-mono);
    font-size: 10.5px;
    color: var(--vx-txt-2);
    margin-top: 3px;
  }

  .actions {
    display: grid;
    gap: 8px;
  }

  .dl {
    height: 36px;
    width: 100%;
    font-size: 12px;
    padding: 0 10px;
  }

  .bundled {
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 10px;
    border: 1px dashed var(--vx-line);
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-3);
    text-align: center;
  }

  .bundled.ok {
    border-style: solid;
    border-color: rgba(106, 212, 138, 0.35);
    color: var(--vx-good);
  }

  .play {
    height: 62px;
    border-radius: 12px;
    border: 1px solid var(--vx-line-2);
    background: rgba(255, 255, 255, 0.03);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: inherit;
    font: inherit;
    transition: all 0.25s;
  }

  .play:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .play:hover:not(:disabled) {
    border-color: var(--vx-cyan-b);
  }

  .play.playing {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.08);
  }

  .tri {
    font-family: var(--vx-mono);
    font-size: 20px;
    color: var(--vx-cyan-1);
  }

  .play-label {
    font-family: var(--vx-mono);
    font-size: 11.5px;
    color: var(--vx-txt-1);
  }

  .play-bars {
    display: flex;
    align-items: center;
    gap: 3px;
    height: 28px;
  }

  .play-bars > div {
    width: 4px;
    height: 28px;
    border-radius: 2px;
    background: var(--vx-cyan-0);
    transform-origin: center;
    animation-name: vxBar;
    animation-iteration-count: infinite;
    animation-timing-function: ease-in-out;
  }

  .metrics {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .metric-head {
    display: flex;
    justify-content: space-between;
    font-family: var(--vx-mono);
    font-size: 10.5px;
    color: var(--vx-txt-2);
    margin-bottom: 5px;
  }

  .metric-head span:last-child {
    color: var(--vx-txt-1);
  }

  .note {
    font-size: 12px;
    color: var(--vx-txt-2);
    line-height: 1.45;
    margin-top: auto;
  }

  .bad {
    color: var(--vx-bad);
    word-break: break-word;
  }

  .play-error {
    flex: none;
    font-size: 12.5px;
    color: var(--vx-bad);
  }

  @media (max-width: 1200px) {
    .grid {
      grid-template-columns: repeat(3, 1fr);
    }
  }

  @media (max-width: 1000px) {
    .head {
      flex-direction: column;
      align-items: stretch;
    }

    .choice {
      width: 100%;
    }

    .grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
