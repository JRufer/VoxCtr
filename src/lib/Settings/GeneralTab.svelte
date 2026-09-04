<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  let wizardError = $state<string | null>(null);

  async function runSetupWizard() {
    wizardError = null;
    try {
      await invoke("open_setup_wizard");
    } catch (e) {
      wizardError = `${e}`;
    }
  }
</script>

<section>
  <h2>General</h2>

  <div class="field-group">
    <h3>Setup</h3>
    <div class="field">
      <span>Run the first-launch setup wizard again</span>
      <button class="btn-action" onclick={runSetupWizard}>Open setup wizard</button>
    </div>
    <p class="hint">
      Walks through the engine, model, hotkey, overlay and voice choices in one flow, and downloads
      whatever it needs. Your current settings stay in place until you change them in the wizard.
    </p>
    {#if wizardError}
      <p class="hint error">Could not open the wizard: {wizardError}</p>
    {/if}
  </div>


  <div class="field-group">
    <h3>MCP Server</h3>
    <label class="field">
      <span>Enable MCP JSON-RPC server</span>
      <input type="checkbox" bind:checked={cfg.mcp.server_enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Visual Feedback</span>
      <input type="checkbox" bind:checked={cfg.mcp.visual_feedback} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Record timeout (seconds)</span>
      <input
        type="number"
        min="1"
        max="120"
        bind:value={cfg.mcp.record_timeout}
        onchange={markDirty}
      />
    </label>
    <p class="hint">Socket: <code>/tmp/voxctrl-mcp.sock</code> (Linux) / <code>\\.\pipe\voxctrl-mcp</code> (Windows)</p>
  </div>
</section>

<style>
  @reference "../../app.css";

  .btn-action {
    @apply bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs font-semibold cursor-pointer transition-all duration-150 ease-out;
  }
  .btn-action:hover {
    @apply bg-[var(--border)] border-[var(--text-muted)];
  }

  .hint.error {
    @apply text-red-400;
  }
</style>
