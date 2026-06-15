<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, active = true } = $props();

  const SPECTRUM_BARS = 16;
  const SPECTRUM_MIN = 6;
  const SPECTRUM_MAX = 96;

  let barHeights = $state<number[]>(Array(SPECTRUM_BARS).fill(SPECTRUM_MIN));
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  // Mirrors spectrum_bar_height() in src-tauri/src/overlay.rs
  function spectrumBarHeight(
    i: number,
    n: number,
    level: number,
    phase: number,
    rec: boolean,
    proc: boolean,
    ready: boolean,
    noise: number
  ): number {
    const band = i / (n - 1);
    let amp: number;
    if (proc) {
      amp = Math.pow(Math.sin(phase * 2.4 - band * 6.0) * 0.5 + 0.5, 1.5);
    } else if (rec && !ready) {
      amp = noise * 0.06;
    } else if (rec) {
      const bandFreq = 2.0 + band * 9.0;
      const bandPhase = i * 0.7;
      const wobble = Math.sin(phase * bandFreq + bandPhase) * 0.5 + 0.5;
      const bandGain = 1.0 - band * 0.35;
      amp = Math.min(1, Math.max(0, level * bandGain * (0.35 + 0.65 * wobble) * (0.7 + 0.6 * noise)));
    } else {
      amp = 0;
    }
    return SPECTRUM_MIN + (SPECTRUM_MAX - SPECTRUM_MIN) * amp;
  }

  onMount(() => {
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let phase = 0;

    function update() {
      phase += 0.016;
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;

      const ready = $status.audio_ready !== false;
      const heights = new Array(SPECTRUM_BARS);
      for (let i = 0; i < SPECTRUM_BARS; i++) {
        heights[i] = spectrumBarHeight(i, SPECTRUM_BARS, currentVolume, phase, recording, $status.processing, ready, Math.random());
      }
      barHeights = heights;

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div class="spectrum" class:on={active}>
  <div class="header">
    <span class="led" class:processing={$status.processing} class:standby={recording && !isReady}></span>
    <span class="title">SPECTRUM // EQ-16</span>
    <span class="subtitle">
      {#if $status.processing}
        · ANALYZING
      {:else if recording && !isReady}
        · WARMING UP
      {:else}
        · LIVE
      {/if}
    </span>
    <span class="spacer"></span>
    <span class="chip">
      <span class="tgt">OUT ▸</span>
      <span class="tgt-label">{targetLabel}</span>
    </span>
  </div>

  <div class="floor"></div>
  <div class="bars">
    {#each barHeights as h}
      <div class="bar" style="height: {h}px; opacity: {0.45 + 0.55 * (h / SPECTRUM_MAX)}"></div>
    {/each}
  </div>
</div>

<style>
  /* Neon equalizer: rises up from the floor on load, sinks back on unload */
  .spectrum {
    width: 440px;
    height: 132px;
    background: linear-gradient(160deg, #1b0c2e 0%, #0a0614 100%);
    border: 1px solid rgba(232, 121, 249, 0.25);
    border-radius: 16px;
    box-shadow: 0 0 26px rgba(192, 132, 252, 0.22);
    position: relative;
    overflow: hidden;
    pointer-events: none;
    user-select: none;
    transform: scaleY(0.04);
    transform-origin: bottom center;
    opacity: 0;
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.16s ease;
  }

  .spectrum.on {
    transform: scaleY(1);
    opacity: 1;
  }

  .header {
    position: absolute;
    left: 20px;
    top: 12px;
    right: 20px;
    height: 16px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .led {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #e879f9;
    box-shadow: 0 0 6px #e879f9;
    animation: spectrum-blink 1.2s ease-in-out infinite;
    flex-shrink: 0;
  }

  .led.standby {
    background: #f59e0b;
    box-shadow: 0 0 6px #f59e0b;
  }

  .led.processing {
    background: #38bdf8;
    box-shadow: 0 0 6px #38bdf8;
  }

  @keyframes spectrum-blink {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .title {
    color: #f5d0fe;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.15em;
  }

  .subtitle {
    color: rgba(245, 208, 254, 0.35);
    font-size: 9px;
    font-weight: 500;
    letter-spacing: 0.1em;
  }

  .spacer {
    flex: 1;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: rgba(232, 121, 249, 0.08);
    border: 1px solid rgba(232, 121, 249, 0.3);
    border-radius: 4px;
    padding: 2px 8px;
  }

  .tgt {
    color: #e879f9;
    font-size: 8.5px;
    font-weight: 800;
    letter-spacing: 0.1em;
  }

  .tgt-label {
    color: #fae8ff;
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    max-width: 150px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bars {
    position: absolute;
    left: 20px;
    right: 20px;
    bottom: 16px;
    display: flex;
    gap: 6.5px;
    align-items: flex-end;
    height: 96px;
  }

  .bar {
    width: 19px;
    border-radius: 3px;
    background: linear-gradient(180deg, #f0abfc 0%, #c084fc 45%, #38bdf8 100%);
    transition: height 0.05s linear;
  }

  .floor {
    position: absolute;
    left: 20px;
    right: 20px;
    bottom: 16px;
    height: 1px;
    background: rgba(232, 121, 249, 0.15);
  }
</style>
