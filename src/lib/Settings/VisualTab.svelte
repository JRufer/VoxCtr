<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  interface MonitorInfo {
    name: string | null;
    width: number;
    height: number;
    is_primary: boolean;
  }

  let customOverlays = $state<CustomOverlay[]>([]);
  let monitors = $state<MonitorInfo[]>([]);

  let overlayStyleOptions = $derived([
    { value: "voice_card", label: "Voice Card" },
    { value: "waveform", label: "Waveform" },
    { value: "pulse", label: "Pulse Ring" },
    { value: "blue_wave", label: "Ocean Wave" },
    { value: "mono_bars", label: "Mono Bars" },
    { value: "spectrum", label: "Neon Spectrum" },
    { value: "terminal", label: "Retro Terminal" },
    { value: "vinyl", label: "Analog VU" },
    ...customOverlays.map(o => ({ value: o.name, label: o.name }))
  ]);

  const overlayPositionOptions = [
    { value: "top", label: "Top of screen" },
    { value: "center", label: "Center of screen" },
    { value: "bottom", label: "Bottom of screen" }
  ];

  let overlayMonitorOptions = $derived([
    { value: "primary", label: "Primary Monitor" },
    ...monitors.filter(mon => mon.name).map(mon => ({
      value: mon.name!,
      label: `${mon.name} (${mon.width}x${mon.height})${mon.is_primary ? ' [Primary]' : ''}`
    }))
  ]);

  onMount(async () => {
    try {
      customOverlays = await invoke<CustomOverlay[]>("get_custom_overlays");
    } catch (e) {
      console.error("Failed to fetch custom overlays:", e);
    }
    try {
      monitors = await invoke<MonitorInfo[]>("get_available_monitors");
    } catch (e) {
      console.error("Failed to fetch available monitors:", e);
    }
  });

  function markDirty() {
    configDirty.set(true);
  }
</script>

<section>
  <h2>Visual & Feedback</h2>

  <div class="field-group">
    <h3>Overlay & HUD</h3>
    <label class="field">
      <span>Show overlay while speaking</span>
      <input type="checkbox" bind:checked={cfg.ui.show_overlay} onchange={markDirty} />
    </label>
    
    <label class="field">
      <span>Overlay style</span>
      <CustomSelect bind:value={cfg.ui.overlay_style} options={overlayStyleOptions} onchange={markDirty} />
    </label>

    <label class="field">
      <span>Overlay position</span>
      <CustomSelect bind:value={cfg.ui.overlay_position} options={overlayPositionOptions} onchange={markDirty} />
    </label>

    <label class="field">
      <span>Overlay display</span>
      <CustomSelect bind:value={cfg.ui.overlay_monitor} options={overlayMonitorOptions} onchange={markDirty} />
    </label>

    {#if cfg.ui.overlay_monitor !== 'primary' && monitors.length > 0 && !monitors.some(m => m.name === cfg.ui.overlay_monitor)}
      <div class="warning-alert">
        <span>⚠️ Configured monitor "{cfg.ui.overlay_monitor}" is disconnected. Using Primary Monitor.</span>
      </div>
    {/if}
  </div>

  <div class="field-group">
    <h3>Alerts & OS Integration</h3>
    <label class="field">
      <span>Show system notifications on transcription</span>
      <input type="checkbox" bind:checked={cfg.ui.show_notification} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Application Window</h3>
    <label class="field">
      <span>Automatically open settings at launch</span>
      <input type="checkbox" bind:checked={cfg.ui.auto_show_settings} onchange={markDirty} />
    </label>
  </div>
</section>

<style>
</style>
