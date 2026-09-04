<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  interface AudioDevice { index: number; name: string; }
  let devices = $state<AudioDevice[]>([]);

  let audioDeviceOptions = $derived([
    { value: -1, label: "Default system device" },
    ...devices.map(d => ({ value: d.index, label: d.name }))
  ]);

  // VU Meter States using Svelte 5 Runes
  let rawLevel = $state(0);
  let smoothedLevel = $state(0);
  let peakLevel = $state(0);
  let peakHoldTime = 0;
  let unlistenFn = $state<(() => void) | null>(null);

  let rafId: number;

  function updateVisuals() {
    const target = rawLevel;
    // Fast attack, slower decay physics
    if (target > smoothedLevel) {
      smoothedLevel += (target - smoothedLevel) * 0.45;
    } else {
      smoothedLevel += (target - smoothedLevel) * 0.12;
    }

    // Keep peak level with smooth decay
    if (smoothedLevel > peakLevel) {
      peakLevel = smoothedLevel;
      peakHoldTime = 25; // hold for ~0.4s
    } else {
      if (peakHoldTime > 0) {
        peakHoldTime--;
      } else {
        peakLevel = Math.max(0, peakLevel - 0.012);
      }
    }

    rafId = requestAnimationFrame(updateVisuals);
  }

  onMount(async () => {
    devices = await invoke<AudioDevice[]>("list_audio_devices");
    
    // Start backend microphone monitoring
    try {
      await invoke("start_monitoring_audio");
      unlistenFn = await listen<number>("audio-level", (event) => {
        rawLevel = event.payload;
      });
    } catch (e) {
      console.error("Failed to start audio monitoring:", e);
    }

    rafId = requestAnimationFrame(updateVisuals);
  });

  onDestroy(async () => {
    if (rafId) {
      cancelAnimationFrame(rafId);
    }
    if (unlistenFn) {
      unlistenFn();
    }
    // Stop backend microphone monitoring
    try {
      await invoke("stop_monitoring_audio");
    } catch (e) {
      console.error("Failed to stop audio monitoring:", e);
    }
  });
</script>

<section>
  <h2>Audio</h2>

  <div class="field-group">
    <h3>Input Device</h3>
    <label class="field col">
      <span class="field-title">Microphone</span>
      <CustomSelect
        value={cfg.audio.input_device_index ?? -1}
        options={audioDeviceOptions}
        onchange={(v) => {
          cfg.audio.input_device_index = v < 0 ? null : v;
          markDirty();
        }}
      />
    </label>

    <!-- Hardware-inspired real-time VU meter -->
    <div class="vu-meter-container">
      <div class="vu-meter-info">
        <span class="vu-label">Microphone Monitor</span>
        <span class="vu-status-dot" class:active={rawLevel > 0.005}></span>
        <span class="vu-status-text">{rawLevel > 0.005 ? "Signal Detected" : "Listening..."}</span>
      </div>
      <div class="vu-meter-bar-wrapper">
        <div class="vu-meter-bar">
          {#each Array(24) as _, i}
            {@const threshold = i / 24}
            {@const isActive = smoothedLevel >= threshold}
            {@const isOrange = i >= 17}
            <div
              class="vu-segment"
              class:active={isActive}
              class:orange={isOrange}
            ></div>
          {/each}
          
          <!-- Peak marker line -->
          {#if peakLevel > 0.01}
            <div
              class="vu-peak-marker"
              style="left: {Math.min(0.99, peakLevel) * 100}%;"
            ></div>
          {/if}
        </div>
      </div>
    </div>

    <label class="field">
      <span>Dynamic Stream (Open on trigger)</span>
      <input type="checkbox" bind:checked={cfg.audio.dynamic_stream} onchange={markDirty} />
    </label>
    <p class="field-help" style="margin: -4px 0 12px 16px; opacity: 0.75; font-size: 11px; line-height: 1.45; color: var(--text-muted, #888); max-width: 480px;">
      Only opens the microphone stream when recording is triggered. If your first words are being clipped due to hardware delay, disable this feature to keep the stream always open.
    </p>
  </div>

  <div class="field-group">
    <h3>Voice Activity Detection</h3>
    <label class="field">
      <span>VAD threshold (0.0 – 1.0)</span>
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        bind:value={cfg.audio.vad_threshold}
        onchange={markDirty}
      />
      <span class="val">{cfg.audio.vad_threshold.toFixed(2)}</span>
    </label>
  </div>

  <div class="field-group">
    <h3>Processing</h3>
    <label class="field">
      <span>Noise suppression (RNNoise)</span>
      <input type="checkbox" bind:checked={cfg.audio.noise_suppression} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Input gain</span>
      <input type="range" min="0.5" max="4.0" step="0.1"
        bind:value={cfg.audio.gain} onchange={markDirty} />
      <span class="val">{cfg.audio.gain.toFixed(1)}×</span>
    </label>
  </div>
</section>

<style>
  @reference "../../app.css";

  .val {
    @apply text-xs text-[var(--text-muted)] min-w-[36px] text-right;
  }

  .vu-meter-container {
    @apply flex flex-col gap-2 py-1 my-1 mb-3;
  }

  .vu-meter-info {
    @apply flex items-center gap-1.5 text-[10px] font-mono uppercase tracking-wide;
  }

  .vu-label {
    @apply text-[var(--text-muted)] font-semibold;
  }

  .vu-status-dot {
    @apply w-1.5 h-1.5 bg-[#444] transition-colors duration-150 ease-out;
  }

  .vu-status-dot.active {
    @apply bg-[#39ff14] shadow-[0_0_8px_#39ff14];
  }

  .vu-status-text {
    @apply text-[var(--text)] opacity-70 ml-auto;
  }

  .vu-meter-bar-wrapper {
    @apply relative w-full h-3.5 bg-black/40 p-[3px] box-border;
  }

  .vu-meter-bar {
    @apply relative flex gap-[2px] w-full h-full;
  }

  .vu-segment {
    @apply flex-1 h-full bg-white/[0.05] transition-colors duration-[50ms] ease-out;
  }

  /* Neon Acid Green for safe range */
  .vu-segment.active {
    @apply bg-[#39ff14] shadow-[0_0_4px_rgba(57,255,20,0.3)];
  }

  /* Signal Orange for warning range */
  .vu-segment.active.orange {
    @apply bg-[#ff6d00] shadow-[0_0_4px_rgba(255,109,0,0.5)];
  }

  .vu-peak-marker {
    @apply absolute -top-0.5 -bottom-0.5 w-[2px] bg-[#ffab00] shadow-[0_0_6px_#ffab00] pointer-events-none transition-[left] duration-[50ms] ease-linear;
  }
</style>
