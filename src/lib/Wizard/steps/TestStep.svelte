<script lang="ts">
  import { onMount } from "svelte";
  import { config } from "../../../stores/config";
  import { status } from "../../../stores/status";
  import { wizard } from "../wizard-state.svelte";
  import { keycapLabel, waveBars } from "../wizard-data";

  /**
   * A real end-to-end test, not a simulation: the user presses the binding they
   * just made, VoxCtrl records, transcribes and types the result into whatever
   * window has focus — which, if they clicked the box below, is this one. What
   * lands in the textarea is the actual transcription, delivered down the
   * actual injection path.
   */

  let textarea = $state<HTMLTextAreaElement | null>(null);
  let text = $state("");
  let elapsedMs = $state(0);

  type Phase = "idle" | "recording" | "transcribing" | "success";
  let phase = $state<Phase>("idle");
  let startedAt = 0;
  /** Length of the box when the last dictation started, so appended text is
   *  recognised even if the user had typed something first. */
  let lengthAtStart = 0;

  const bars = waveBars(28);
  const gesture = $derived(wizard.gestureInfo);
  const keys = $derived(wizard.combo ?? []);

  const engineSummary = $derived(
    $config.engine.backend === "moonshine"
      ? `Moonshine · ${$config.engine.moonshine.model_size}`
      : `whisper.cpp · ${$config.engine.whisper_cpp.model_size} · ${
          $config.engine.whisper_cpp.device === "cpu" ? "cpu" : "gpu/auto"
        }`,
  );

  const statusLabel = $derived(
    { idle: "waiting for hotkey", recording: "recording", transcribing: "transcribing", success: "delivered" }[
      phase
    ],
  );

  /** Previous value of `status.recording`, so a new dictation is recognised by
   *  the moment recording *starts* rather than by the phase we happen to be in.
   *  Comparing against the phase instead would knock a finished test back to
   *  "recording" the moment the transcript arrived while the mic was still
   *  open — which is exactly what a toggle-gesture dictation does. */
  let wasRecording = false;

  // The pipeline's own state drives the readout, so what the user sees here is
  // what the app is really doing rather than a timed animation.
  $effect(() => {
    const s = $status;
    if (s.recording && !wasRecording) {
      phase = "recording";
      startedAt = performance.now();
      lengthAtStart = text.length;
    } else if (!s.recording && s.processing && phase === "recording") {
      phase = "transcribing";
    }
    wasRecording = s.recording;
  });

  function onInput() {
    if (!textarea) return;
    const next = textarea.value;
    const grew = next.length > lengthAtStart && next.trim().length > 0;
    text = next;
    // Text arriving while (or just after) a dictation cycle is the transcript
    // landing. Typing by hand before ever pressing the hotkey is not.
    if (grew && (phase === "recording" || phase === "transcribing")) {
      elapsedMs = Math.max(1, Math.round(performance.now() - startedAt));
      phase = "success";
    }
  }

  function reset() {
    phase = "idle";
    text = "";
    lengthAtStart = 0;
    if (textarea) {
      textarea.value = "";
      textarea.focus();
    }
  }

  onMount(() => {
    // Focus is the whole point: the transcription goes to the focused window.
    setTimeout(() => textarea?.focus(), 60);
  });
</script>

<div class="test-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 04 · live test</span>
      <h2 class="vx-title">Say something.</h2>
      <p class="vx-lede">
        Click into the box, then <b>{gesture.verb}</b> your binding and speak. Your words land where
        the cursor is.
      </p>
    </div>

    <div class="combo">
      {#each keys as key, i}
        <div class="vx-keycap">{keycapLabel(key)}</div>
        {#if i < keys.length - 1}<span class="vx-plus">+</span>{/if}
      {/each}
      <span class="gesture-pill">{gesture.name}</span>
    </div>
  </div>

  <div class="box" class:live={phase === "recording" || phase === "transcribing"} class:won={phase === "success"}>
    <div class="box-head">
      <span class="file">▸ test.txt</span>
      <span class="state {phase}"><span class="dot"></span>{statusLabel}</span>
    </div>

    <textarea
      bind:this={textarea}
      oninput={onInput}
      placeholder="Click here, then use your hotkey…"
      spellcheck="false"
    ></textarea>

    {#if phase === "recording" || phase === "transcribing"}
      <div class="wave">
        {#each bars as b}
          <div style:animation-duration="{b.d}s" style:animation-delay="{b.dl}s"></div>
        {/each}
      </div>
    {/if}

    {#if phase === "success"}
      <div class="won-overlay">
        <div>
          <div class="tick">✓</div>
          <div class="won-title">It works.</div>
          <div class="won-sub">Transcribed on-device in {elapsedMs} ms · delivered via inject</div>
          <button class="vx-btn" onclick={reset}>Try again</button>
        </div>
      </div>
    {/if}
  </div>

  <div class="foot">
    <span>
      Nothing happening? The overlay and tray icon show whether VoxCtrl heard the key. You can
      continue and come back to this from Settings → Hotkeys at any time.
    </span>
    <span class="engine">{engineSummary}</span>
  </div>
</div>

<style>
  .test-step {
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
  }

  .copy {
    max-width: 640px;
  }

  .combo {
    display: flex;
    gap: 10px;
    align-items: center;
    flex: none;
  }

  .gesture-pill {
    margin-left: 10px;
    font-family: var(--vx-mono);
    font-size: 11.5px;
    color: var(--vx-txt-2);
    padding: 6px 12px;
    border: 1px solid var(--vx-line);
    border-radius: 999px;
  }

  .box {
    position: relative;
    flex: 1;
    min-height: 0;
    border-radius: 18px;
    border: 1px solid var(--vx-line);
    background: var(--vx-bg-1);
    transition: all 0.4s var(--vx-ease);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .box.live {
    border-color: var(--vx-cyan-b);
    box-shadow: 0 0 40px rgba(34, 212, 239, 0.15);
  }

  .box.won {
    border-color: rgba(106, 212, 138, 0.5);
    box-shadow: 0 0 40px rgba(106, 212, 138, 0.15);
  }

  .box-head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 18px;
    border-bottom: 1px solid var(--vx-line);
  }

  .file {
    font-family: var(--vx-mono);
    font-size: 11.5px;
    color: var(--vx-txt-2);
  }

  .state {
    font-family: var(--vx-mono);
    font-size: 11px;
    letter-spacing: 0.1em;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--vx-txt-3);
    transition: color 0.3s;
  }

  .state.recording {
    color: var(--vx-cyan-0);
  }

  .state.transcribing {
    color: var(--vx-gold-1);
  }

  .state.success {
    color: var(--vx-good);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
    animation: vxPulse 1.4s infinite;
  }

  textarea {
    display: block;
    flex: 1;
    min-height: 180px;
    width: 100%;
    padding: 22px;
    border: 0;
    background: transparent;
    color: var(--vx-txt-0);
    font-family: inherit;
    font-size: 19px;
    line-height: 1.6;
    resize: none;
    caret-color: var(--vx-cyan-0);
    outline: none;
  }

  .wave {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    background: linear-gradient(180deg, transparent, rgba(34, 212, 239, 0.1));
    pointer-events: none;
  }

  .wave > div {
    width: 4px;
    height: 30px;
    border-radius: 2px;
    background: var(--vx-cyan-0);
    transform-origin: center;
    animation-name: vxBar;
    animation-iteration-count: infinite;
    animation-timing-function: ease-in-out;
  }

  .won-overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
    background: rgba(14, 17, 22, 0.86);
    animation: vxPop 0.35s var(--vx-ease);
  }

  .tick {
    width: 84px;
    height: 84px;
    margin: 0 auto 16px;
    border-radius: 50%;
    border: 2px solid var(--vx-good);
    display: grid;
    place-items: center;
    color: var(--vx-good);
    font-family: var(--vx-mono);
    font-size: 36px;
    box-shadow: 0 0 30px rgba(106, 212, 138, 0.35);
    animation: vxRing 0.5s var(--vx-ease);
  }

  .won-title {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .won-sub {
    font-size: 14px;
    color: var(--vx-txt-2);
    margin: 6px 0 18px;
  }

  .foot {
    flex: none;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 20px;
    font-size: 12.5px;
    color: var(--vx-txt-2);
  }

  .engine {
    font-family: var(--vx-mono);
    font-size: 11px;
    white-space: nowrap;
  }

  @media (max-width: 1000px) {
    .head {
      flex-direction: column;
      align-items: stretch;
      gap: 12px;
    }
  }
</style>
