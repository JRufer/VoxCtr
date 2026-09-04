<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { get } from "svelte/store";
  import { config } from "../../../stores/config";
  import { status } from "../../../stores/status";
  import { patchConfig, wizard } from "../wizard-state.svelte";
  import {
    TTS_ENGINES,
    formatPercent,
    formatSize,
    modelSizeShare,
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

  /**
   * The single HuggingFace access token, shared by every gated model. Without
   * it those engines cannot be downloaded at all, which is why the wizard asks
   * for it here rather than leaving the user to discover the failure.
   *
   * A token exported as `HF_TOKEN` belongs to the session and wins at download
   * time, so it is shown here and the field goes read-only: it is never copied
   * into the config, and a value typed over it would be saved and then ignored.
   */
  let envToken = $state<string | null>(null);
  const fromEnv = $derived(!!envToken);
  const hfToken = $derived(envToken ?? ($config.tts.hf_token ?? "").trim());
  const hasHfToken = $derived(hfToken.length > 0);
  const gatedEngines = TTS_ENGINES.filter((e) => e.needsHfToken);

  function setHfToken(value: string) {
    // The environment's token is displayed, never stored: saving it would put a
    // copy in the config that the app ignores anyway.
    if (fromEnv) return;
    // A new token deserves a fresh verdict; leaving the old one up would read
    // as this one having been refused too, before it has been tried.
    tokenRejected = false;
    patchConfig((cfg) => {
      cfg.tts.hf_token = value.trim() ? value.trim() : null;
    });
  }

  /**
   * Whether a gated engine is still out of reach.
   *
   * The token is only needed to *fetch* the weights. Someone who downloaded
   * Breeze-TTS-2 or Pocket TTS on an earlier run — or in Settings, or with an
   * `HF_TOKEN` exported into a shell they are no longer in — already has them
   * on disk, and locking them out of a voice their machine can speak with
   * would be asking for a token to unlock something that needs no unlocking.
   */
  function locked(id: TtsEngineId) {
    const engine = TTS_ENGINES.find((e) => e.id === id);
    return !!engine?.needsHfToken && !hasHfToken && !ready[id];
  }

  /**
   * Set when HuggingFace turns our credentials away, cleared the moment the
   * token changes. The backend tags those failures so this does not depend on
   * the wording of an error from somewhere down in the HTTP stack.
   */
  let tokenRejected = $state(false);

  const HF_TOKEN_REJECTED_TAG = "hf-token-rejected";

  function isTokenRejection(error: unknown): boolean {
    return `${error}`.toLowerCase().includes(HF_TOKEN_REJECTED_TAG);
  }

  let playing = $state<string | null>(null);
  let playError = $state<string | null>(null);
  const bars = waveBars(14);

  /**
   * Getting back out of the "playing" state.
   *
   * A card left mid-playback disables every other engine, so this must not
   * depend on anything that can go missing — and in this window, pushed events
   * do. Neither `tts-playback-end` nor the 150ms status tick arrives here,
   * while `invoke` plainly works: the sample itself is played through it. So
   * the state is settled by asking, not by waiting to be told.
   *
   * The event stays wired as the fast path for windows where it does arrive.
   */
  let playWatchdog: ReturnType<typeof setTimeout> | null = null;
  let playPoll: ReturnType<typeof setInterval> | null = null;
  let playStartedAt = 0;
  /** Whether the engine has been observed actually speaking this run. */
  let sawSpeaking = false;

  /** How often to ask the backend whether it is still speaking. */
  const PLAY_POLL_MS = 300;

  /** Synthesis takes a moment before the first sample reaches the speakers, so
   *  "not speaking" is only meaningful once this has passed — otherwise every
   *  play would end the instant it began. */
  const PLAY_SETTLE_MS = 1500;

  /** Last resort, for an IPC call that never comes back at all. */
  const PLAY_TIMEOUT_MS = 30_000;

  function clearPlayTimers() {
    if (playWatchdog) {
      clearTimeout(playWatchdog);
      playWatchdog = null;
    }
    if (playPoll) {
      clearInterval(playPoll);
      playPoll = null;
    }
  }

  function stopPlaying() {
    playing = null;
    clearPlayTimers();
  }

  /**
   * Ask the backend whether it is still speaking, and finish when it is not.
   *
   * Two ways to be finished: the engine was heard speaking and has now stopped,
   * or it never started and the settle window has passed — which covers a
   * sample too short to catch between polls as well as an engine that failed
   * without saying so.
   */
  async function pollSpeaking() {
    if (!playing) return;
    let payload: { speaking?: boolean } | undefined;
    try {
      payload = await invoke<{ speaking?: boolean }>("get_status");
    } catch (e) {
      // A failed status call is not evidence of anything; the watchdog is the
      // backstop if they keep failing.
      console.error("Wizard: status poll failed:", e);
      return;
    }
    // Everything else in the window reads the same store, so keep it current
    // rather than holding a private copy of the answer.
    if (payload) status.set(payload as any);

    if (payload?.speaking) {
      sawSpeaking = true;
      return;
    }
    if (sawSpeaking || Date.now() - playStartedAt > PLAY_SETTLE_MS) stopPlaying();
  }

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
            hfToken: cfg.tts.hf_token,
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
            hfToken: cfg.tts.hf_token,
          });
          break;
        case "espeak":
          break;
      }
      // A download that reached the end proves the token was good, whatever an
      // earlier attempt said.
      tokenRejected = false;
      ready = { ...ready, [id]: true };
      wizard.clearIssue(`tts-download-${id}`);
    } catch (e) {
      const engine = TTS_ENGINES.find((t) => t.id === id);
      const rejected = isTokenRejection(e);
      if (rejected) tokenRejected = true;

      // Reported on the card rather than in a dialog: the backend lists every
      // URL it tried, and a modal would leave no way to read or copy it. A
      // refused token is the one failure with a single obvious cause, so it
      // gets a sentence the user can act on instead of the raw chain.
      setErr(
        id,
        rejected
          ? `HuggingFace did not accept that access token, so ${engine?.name ?? id} could not be ` +
            `downloaded. Check the token is a valid read token, and that the same account has ` +
            `accepted the licence at ${engine?.licenceUrl ?? "huggingface.co"}.`
          : `${e}`,
      );
      wizard.recordIssue({
        id: `tts-download-${id}`,
        step: STEP,
        title: rejected
          ? `${engine?.name ?? id} could not be downloaded — HuggingFace did not accept the access token.`
          : `${engine?.name ?? id} could not be downloaded — speech output will stay silent.`,
        detail: `engine=${id}\n${e}`,
      });
    } finally {
      downloading = null;
    }
  }

  function pick(id: TtsEngineId) {
    if (locked(id)) return;
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
    // A second press on the card that is speaking stops it. Without this the
    // only way out of a long sample is to wait it out.
    if (playing === id) {
      stopPlaying();
      void invoke("stop_tts").catch(() => {});
      return;
    }
    if (playing) return;
    pick(id);
    playError = null;
    playing = id;
    playStartedAt = Date.now();
    sawSpeaking = false;
    clearPlayTimers();
    playPoll = setInterval(() => void pollSpeaking(), PLAY_POLL_MS);
    playWatchdog = setTimeout(() => {
      stopPlaying();
      playError =
        "The sample never finished playing. The engine may have failed silently — " +
        "check Settings → TTS, or try another voice.";
    }, PLAY_TIMEOUT_MS);
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
      stopPlaying();
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
    if (locked(selected)) {
      setBlocker(STEP, "Enter a HuggingFace access token, or pick a voice that does not need one.");
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
    invoke<string | null>("hf_token_env")
      .then((t) => (envToken = t && t.trim() ? t.trim() : null))
      .catch(() => (envToken = null));
    invoke<boolean>("inflect_micro_available")
      .then((v) => (inflectAvailable = v))
      .catch(() => (inflectAvailable = false));

    const subs = [
      listen("tts-playback-end", () => stopPlaying()),
      listen<string>("tts-error", (e) => {
        playError = e.payload;
        stopPlaying();
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
      clearPlayTimers();
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

  <div
    class="hf"
    class:needed={enabled && (tokenRejected || gatedEngines.some((e) => locked(e.id)))}
    class:muted={!enabled}
  >
    <div class="hf-copy">
      <span class="hf-title">HuggingFace access token</span>
      <span class="hf-desc">
        {gatedEngines.map((e) => e.name).join(" and ")} are gated downloads: HuggingFace only
        serves their weights to an account that has accepted the licence. Create a token at
        <code>huggingface.co/settings/tokens</code>, accept the licence at
        {#each gatedEngines as engine, i}<code>{engine.licenceUrl}</code>{#if i < gatedEngines.length - 1}{" and "}{/if}{/each}, then paste the
        token here. One token covers both, and it is saved with your settings — the same place
        Settings → TTS keeps it. Export <code>HF_TOKEN</code> instead and VoxCtrl uses that: it is
        shown here, kept out of your config, and takes precedence over a saved token.
      </span>
    </div>
    <input
      class="hf-input"
      type="password"
      autocomplete="off"
      spellcheck="false"
      placeholder="hf_…"
      readonly={fromEnv}
      title={fromEnv ? "Set by the HF_TOKEN environment variable" : undefined}
      value={hfToken}
      oninput={(e) => setHfToken((e.currentTarget as HTMLInputElement).value)}
    />
    <span class="hf-state" class:ok={hasHfToken && !tokenRejected} class:bad={tokenRejected}>
      {#if tokenRejected}
        ✗ HuggingFace did not accept this token
      {:else if fromEnv}
        ✓ using the HF_TOKEN environment variable — not saved to your config
      {:else if hasHfToken}
        ✓ token saved
      {:else}
        no token — gated voices are locked
      {/if}
    </span>
  </div>

  <div class="grid" class:muted={!enabled}>
    {#each TTS_ENGINES as engine}
      {@const on = enabled && selected === engine.id}
      {@const isReady = !!ready[engine.id]}
      {@const busy = downloading === engine.id}
      {@const unavailable = engine.id === "inflect_micro" && !inflectAvailable}
      {@const needsToken = locked(engine.id)}
      <div
        class="vx-card card"
        class:vx-on={on}
        class:locked={needsToken}
        aria-disabled={needsToken}
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
              disabled={!!downloading || unavailable || needsToken}
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
            disabled={!isReady || unavailable || needsToken || (!!playing && playing !== engine.id)}
            title={playing === engine.id
              ? "Stop"
              : isReady
                ? "Play a sample"
                : "Download this voice first"}
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
          {#each [{ label: "quality", pct: Math.round(engine.quality * 100), value: formatPercent(engine.quality), color: "var(--vx-cyan-0)" }, { label: "speed", pct: Math.round(engine.speed * 100), value: ttsSpeedLabel(engine.speed), color: "var(--vx-cyan-2)" }, { label: "model size", pct: modelSizeShare(engine.mb), value: formatSize(engine.mb), color: "var(--vx-gold-1)" }] as m}
            <div class="metric">
              <div class="metric-head"><span>{m.label}</span><span>{m.value}</span></div>
              <div class="vx-meter"><div style:width="{m.pct}%" style:background={m.color}></div></div>
            </div>
          {/each}
        </div>

        <div class="note">
          {#if needsToken}
            <span class="bad">
              Needs a HuggingFace access token — enter one above to unlock this voice.
            </span>
          {:else if errors[engine.id]}
            <span class="bad">{errors[engine.id]}</span>
          {:else if engine.needsHfToken && !hasHfToken && isReady}
            <span class="ok">
              Already downloaded — the token is only needed to fetch the weights, so this voice
              works without one.
            </span>
          {:else if unavailable}
            <span class="bad">
              This build was compiled without the `inflect-micro` feature, so this engine cannot
              synthesize.
            </span>
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

  /* The HuggingFace token, above the engine grid: gated voices stay locked
     until it is filled in, so it has to read as a prerequisite, not a detail. */
  .hf {
    display: grid;
    grid-template-columns: 1fr minmax(220px, 320px);
    grid-template-areas: "copy input" "copy state";
    gap: 4px 18px;
    align-items: start;
    padding: 12px 14px;
    border: 1px solid var(--vx-line);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
  }

  .hf.muted {
    opacity: 0.18;
    filter: grayscale(1) blur(1px);
    pointer-events: none;
  }

  .hf.needed {
    border-color: color-mix(in srgb, var(--vx-gold-1) 45%, transparent);
    background: color-mix(in srgb, var(--vx-gold-1) 6%, transparent);
  }

  .hf-copy {
    grid-area: copy;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .hf-title {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }

  .hf-desc {
    font-size: 11px;
    line-height: 1.5;
    color: var(--vx-txt-2);
  }

  .hf-desc code {
    font-size: 10px;
    padding: 1px 4px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.35);
    color: var(--vx-cyan-0);
    word-break: break-all;
  }

  .hf-input {
    grid-area: input;
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    font-size: 12px;
    font-family: inherit;
    color: inherit;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid var(--vx-line);
    border-radius: 8px;
    outline: none;
  }

  .hf-input:focus {
    border-color: var(--vx-cyan-0);
  }

  .hf-input[readonly] {
    color: var(--vx-txt-2);
    cursor: not-allowed;
  }

  .hf-state {
    grid-area: state;
    font-size: 10px;
    letter-spacing: 0.02em;
    color: var(--vx-gold-1);
  }

  .hf-state.ok {
    color: var(--vx-good);
  }

  .hf-state.bad {
    color: var(--vx-bad);
  }

  @media (max-width: 720px) {
    .hf {
      grid-template-columns: 1fr;
      grid-template-areas: "copy" "input" "state";
    }
  }

  .card.locked {
    opacity: 0.55;
  }

  .card.locked .name {
    color: var(--vx-txt-2);
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

  .note .ok {
    color: var(--vx-good);
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
