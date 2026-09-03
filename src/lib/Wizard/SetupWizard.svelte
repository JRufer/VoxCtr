<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import appIcon from "../../assets/app_icon.png";
  import { config, configLoaded } from "../../stores/config";
  import { wizard } from "./wizard-state.svelte";
  import { STEP_LABELS } from "./wizard-data";

  import "./wizard.css";

  import WelcomeStep from "./steps/WelcomeStep.svelte";
  import EngineStep from "./steps/EngineStep.svelte";
  import HotkeyStep from "./steps/HotkeyStep.svelte";
  import OverlayStep from "./steps/OverlayStep.svelte";
  import TestStep from "./steps/TestStep.svelte";
  import VoiceStep from "./steps/VoiceStep.svelte";
  import DoneStep from "./steps/DoneStep.svelte";

  // Steps report back what still has to happen before "Continue" may fire:
  // a model to fetch, a binding to write. Returning a promise keeps the button
  // in its busy state for exactly as long as the work takes.
  type StepGate = () => Promise<boolean>;
  let gates: Record<number, StepGate> = {};
  function registerGate(step: number, gate: StepGate | null) {
    if (gate) gates[step] = gate;
    else delete gates[step];
  }

  // A step can also veto the button outright (nothing recorded yet, a model
  // still downloading) and say why, so a dead button is never a mystery.
  let blockers = $state<Record<number, string | null>>({});
  function setBlocker(step: number, reason: string | null) {
    blockers = { ...blockers, [step]: reason };
  }

  let advancing = $state(false);
  let advanceError = $state<string | null>(null);

  const step = $derived(wizard.step);
  const blocker = $derived(blockers[step] ?? null);

  const nextLabel = $derived(
    step === 0
      ? "Get started →"
      : step === 5 && !$config.tts.enabled
        ? "Skip →"
        : step === STEP_LABELS.length - 1
          ? wizard.issues.length > 0
            ? "Finish anyway"
            : "Finish"
          : "Continue →",
  );

  async function next() {
    if (advancing) return;
    advanceError = null;

    if (step === STEP_LABELS.length - 1) {
      await finish();
      return;
    }

    const gate = gates[step];
    if (gate) {
      advancing = true;
      try {
        const ok = await gate();
        if (!ok) return;
      } catch (e) {
        advanceError = `${e}`;
        return;
      } finally {
        advancing = false;
      }
    }
    wizard.goTo(step + 1);
  }

  function back() {
    if (advancing) return;
    advanceError = null;
    wizard.goTo(step - 1);
  }

  let finishing = $state(false);

  async function finish() {
    if (finishing) return;
    finishing = true;
    try {
      // Flush anything the config store still has in its debounce window, so
      // the flag and the settings it accompanies land together.
      await invoke("save_config", { newConfig: $config });
      await invoke("finish_setup_wizard", { openSettings: false });
    } catch (e) {
      advanceError = `Could not finish setup: ${e}`;
      finishing = false;
    }
  }

  // Escaping the wizard has to leave a usable app behind: the choices already
  // made are saved, and the flag is set so it does not reappear on every
  // launch. It is reachable again from Settings → General.
  async function skipAll() {
    await finish();
  }
</script>

<div class="vx-wizard vx-root">
  <!-- title bar -->
  <header class="vx-titlebar">
    <img src={appIcon} alt="" class="vx-logo" />
    <span class="vx-brand">vox<span>ctrl</span></span>
    <span class="vx-mono vx-dim">// setup</span>
    <div class="vx-spacer"></div>
    <span class="vx-mono vx-faint">
      step {step + 1} / {STEP_LABELS.length} · every choice editable later in Settings
    </span>
  </header>

  <!-- screen -->
  <main class="vx-screen" class:vx-leaving={wizard.leaving}>
    {#if !$configLoaded}
      <div class="vx-loading"><span class="vx-spinner"></span> Loading your settings…</div>
    {:else if step === 0}
      <WelcomeStep />
    {:else if step === 1}
      <EngineStep {registerGate} {setBlocker} />
    {:else if step === 2}
      <HotkeyStep {registerGate} {setBlocker} />
    {:else if step === 3}
      <OverlayStep />
    {:else if step === 4}
      <TestStep />
    {:else if step === 5}
      <VoiceStep {setBlocker} />
    {:else}
      <DoneStep />
    {/if}
  </main>

  <!-- progress tracker -->
  <footer class="vx-footer">
    <div class="vx-nav-left">
      {#if step > 0}
        <button class="vx-btn vx-ghost" onclick={back} disabled={advancing}>← Back</button>
      {:else}
        <button class="vx-btn vx-ghost vx-skip" onclick={skipAll} disabled={finishing}>
          Skip setup
        </button>
      {/if}
    </div>

    <div class="vx-track">
      {#each STEP_LABELS as label, i}
        <div class="vx-track-cell" style:flex={i < STEP_LABELS.length - 1 ? 1 : "none"}>
          <button
            class="vx-track-step"
            class:vx-cur={i === step}
            class:vx-done={i < step}
            disabled={i > wizard.visited || advancing}
            onclick={() => wizard.goTo(i)}
          >
            <span class="vx-ring">{i < step ? "✓" : String(i + 1).padStart(2, "0")}</span>
            <span class="vx-track-label">{label}</span>
          </button>
          {#if i < STEP_LABELS.length - 1}
            <div class="vx-track-line"><div style:width={i < step ? "100%" : "0%"}></div></div>
          {/if}
        </div>
      {/each}
    </div>

    <div class="vx-nav-right">
      {#if blocker && !advancing}
        <span class="vx-blocker" title={blocker}>{blocker}</span>
      {/if}
      <button
        class="vx-btn vx-primary"
        onclick={next}
        disabled={advancing || finishing || !!blocker}
      >
        {#if advancing || finishing}
          <span class="vx-spinner"></span> Working…
        {:else}
          {nextLabel}
        {/if}
      </button>
    </div>
  </footer>

  {#if advanceError}
    <div class="vx-toast">
      <span>⚠ {advanceError}</span>
      <button onclick={() => (advanceError = null)} aria-label="Dismiss">✕</button>
    </div>
  {/if}
</div>

<style>
  .vx-root {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .vx-titlebar {
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 18px;
    border-bottom: 1px solid var(--vx-line);
    background: rgba(14, 17, 22, 0.7);
    z-index: 2;
  }

  .vx-logo {
    width: 20px;
    height: 20px;
    border-radius: 5px;
  }

  .vx-brand {
    font-weight: 600;
    font-size: 14.5px;
    letter-spacing: -0.01em;
  }

  .vx-brand span {
    color: var(--vx-cyan-0);
  }

  .vx-mono {
    font-family: var(--vx-mono);
    font-size: 11px;
    letter-spacing: 0.1em;
  }

  .vx-dim {
    color: var(--vx-txt-2);
  }

  .vx-faint {
    color: var(--vx-txt-3);
  }

  .vx-spacer {
    flex: 1;
  }

  .vx-screen {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 22px 32px 16px;
    overflow: auto;
    opacity: 1;
    transform: none;
    transition: opacity 0.2s ease, transform 0.2s var(--vx-ease);
  }

  .vx-screen.vx-leaving {
    opacity: 0;
    transform: translateY(10px);
  }

  .vx-loading {
    margin: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--vx-txt-2);
    font-size: 13.5px;
  }

  .vx-footer {
    flex: none;
    display: grid;
    grid-template-columns: 170px 1fr auto;
    align-items: center;
    gap: 28px;
    border-top: 1px solid var(--vx-line);
    background: rgba(14, 17, 22, 0.8);
    padding: 10px 32px 12px;
    z-index: 3;
  }

  .vx-nav-left,
  .vx-nav-right {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .vx-nav-right {
    justify-content: flex-end;
  }

  .vx-skip {
    font-weight: 500;
    color: var(--vx-txt-3);
  }

  .vx-blocker {
    max-width: 340px;
    font-size: 12px;
    line-height: 1.35;
    color: var(--vx-txt-2);
    text-align: right;
  }

  .vx-track {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .vx-track-cell {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .vx-track-step {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 62px;
    padding: 4px 6px;
    border: 0;
    background: transparent;
    font: inherit;
    color: var(--vx-txt-3);
    cursor: pointer;
    transition: color 0.3s;
  }

  .vx-track-step:disabled {
    cursor: default;
  }

  .vx-track-step.vx-done {
    color: var(--vx-txt-1);
  }

  .vx-track-step.vx-cur {
    color: var(--vx-txt-0);
  }

  .vx-ring {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-family: var(--vx-mono);
    font-size: 12px;
    border: 1px solid var(--vx-line-2);
    background: transparent;
    color: var(--vx-txt-3);
    transition: all 0.35s var(--vx-ease);
  }

  .vx-track-step.vx-done .vx-ring {
    border-color: rgba(34, 212, 239, 0.4);
    background: rgba(34, 212, 239, 0.1);
    color: var(--vx-cyan-1);
  }

  .vx-track-step.vx-cur .vx-ring {
    border-color: var(--vx-cyan-0);
    background: var(--vx-cyan-0);
    color: #00222a;
    box-shadow: 0 0 16px rgba(34, 212, 239, 0.45);
  }

  .vx-track-label {
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
  }

  .vx-track-step.vx-cur .vx-track-label {
    font-weight: 600;
  }

  .vx-track-line {
    flex: 1;
    min-width: 8px;
    height: 2px;
    margin: 0 2px 19px;
    border-radius: 1px;
    background: var(--vx-bg-4);
    overflow: hidden;
  }

  .vx-track-line > div {
    height: 100%;
    background: var(--vx-cyan-0);
    transition: width 0.5s var(--vx-ease);
  }

  .vx-toast {
    position: fixed;
    left: 50%;
    bottom: 84px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 14px;
    max-width: 70vw;
    padding: 11px 14px;
    border-radius: 10px;
    border: 1px solid rgba(244, 99, 110, 0.45);
    background: #2a1215;
    color: #ffb3b8;
    font-size: 13px;
    box-shadow: var(--vx-panel-shadow);
    animation: vxPop 0.25s var(--vx-ease);
    z-index: 20;
  }

  .vx-toast button {
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 13px;
  }

  /* Narrow windows: the tracker labels are the first thing worth losing. */
  @media (max-width: 1000px) {
    .vx-footer {
      grid-template-columns: 110px 1fr auto;
      gap: 16px;
      padding: 10px 18px 12px;
    }

    .vx-track-label {
      display: none;
    }

    .vx-track-line {
      margin-bottom: 0;
    }

    .vx-screen {
      padding: 18px 20px 14px;
    }
  }
</style>
