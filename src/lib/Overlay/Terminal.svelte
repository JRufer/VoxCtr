<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, active = true } = $props();

  const ASCII_METER_WIDTH = 20;

  let meter = $state("·".repeat(ASCII_METER_WIDTH));
  let cursorOn = $state(false);
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  // Mirrors ascii_meter() in src-tauri/src/overlay.rs
  function asciiMeter(level: number, phase: number, rec: boolean, proc: boolean, ready: boolean): string {
    const w = ASCII_METER_WIDTH;
    if (proc) {
      const cycle = 2 * w;
      const raw = Math.floor(phase * 10) % cycle;
      const pos = raw < w ? raw : cycle - 1 - raw;
      return Array.from({ length: w }, (_, i) => (i === pos ? "█" : "·")).join("");
    } else if (rec && !ready) {
      return Array.from({ length: w }, (_, i) => (i % 4 === 0 ? "·" : " ")).join("");
    } else if (rec) {
      const filled = Math.round(Math.min(1, Math.max(0, level)) * w);
      return Array.from({ length: w }, (_, i) => (i < filled ? "█" : "·")).join("");
    } else {
      return "·".repeat(w);
    }
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
      meter = asciiMeter(currentVolume, phase, recording, $status.processing, ready);
      cursorOn = Math.sin(phase * 5.5) > 0;

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div class="terminal" class:on={active}>
  <div class="titlebar">
    <span class="dot"></span>
    <span class="dot"></span>
    <span class="dot"></span>
    <span class="titletext">VOXCTRL — /dev/mic0</span>
  </div>
  <div class="body">
    <div class="line1">$ voxctrl listen --target "{targetLabel}"</div>
    <div class="line2" class:processing={$status.processing} class:standby={recording && !isReady}>
      {#if $status.processing}
        [PROC]
      {:else if recording && !isReady}
        [INIT]
      {:else}
        [REC ]
      {/if}
      {meter}
    </div>
    <div class="line3">
      {#if $status.processing}
        transcribing audio stream
      {:else if recording && !isReady}
        connecting input device
      {:else}
        streaming to output
      {/if}{cursorOn ? "_" : " "}
    </div>
  </div>
</div>

<style>
  /* DOS-blue console: drops down from the top edge on load, retracts on unload */
  .terminal {
    width: 380px;
    height: 130px;
    background: #0d1b4c;
    border: 1.5px solid rgba(125, 211, 252, 0.35);
    border-radius: 8px;
    box-shadow: 0 0 20px rgba(8, 15, 48, 0.6);
    overflow: hidden;
    pointer-events: none;
    user-select: none;
    transform: scaleY(0.04);
    transform-origin: top center;
    opacity: 0;
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.16s ease;
  }

  .terminal.on {
    transform: scaleY(1);
    opacity: 1;
  }

  .titlebar {
    height: 24px;
    background: rgba(255, 255, 255, 0.06);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.22);
  }

  .titletext {
    color: rgba(224, 242, 254, 0.55);
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .body {
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .line1 {
    color: rgba(186, 230, 253, 0.7);
    font-size: 10px;
    font-weight: 600;
    font-family: 'SF Mono', Menlo, Consolas, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .line2 {
    color: #ffffff;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
    font-family: 'SF Mono', Menlo, Consolas, monospace;
  }

  .line2.standby {
    color: #fcd34d;
  }

  .line2.processing {
    color: #7dd3fc;
  }

  .line3 {
    color: rgba(186, 230, 253, 0.45);
    font-size: 9.5px;
    font-weight: 500;
    font-family: 'SF Mono', Menlo, Consolas, monospace;
  }
</style>
