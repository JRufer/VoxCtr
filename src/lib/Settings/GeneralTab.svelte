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

  type UpdateInfo = {
    version: string;
    current_version: string;
    can_self_update: boolean;
  };
  type UpdateCheckPayload = {
    current_version: string;
    update: UpdateInfo | null;
    skipped: boolean;
  };

  let checking = $state(false);
  let checkResult = $state<UpdateCheckPayload | null>(null);
  let checkError = $state<string | null>(null);

  async function checkForUpdate() {
    checking = true;
    checkError = null;
    checkResult = null;
    try {
      checkResult = await invoke<UpdateCheckPayload>("check_for_update");
    } catch (e) {
      checkError = `${e}`;
    } finally {
      checking = false;
    }
  }

  async function showUpdateWindow() {
    try {
      await invoke("open_update_window");
    } catch (e) {
      checkError = `${e}`;
    }
  }

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
    <h3>Updates</h3>
    <label class="field">
      <span>Check for a new version on launch</span>
      <input type="checkbox" bind:checked={cfg.updates.auto_check} onchange={markDirty} />
    </label>
    <p class="hint">
      Asks GitHub once, shortly after startup, whether a newer release has been published, and
      offers to install it. The request carries nothing about you or your machine, and it is the
      only network request VoxCtrl makes on its own. Turn it off and VoxCtrl never contacts GitHub
      unless you press "Check now".
    </p>
    <div class="field">
      <span>Check now</span>
      <button class="btn-action" onclick={checkForUpdate} disabled={checking}>
        {checking ? "Checking…" : "Check for updates"}
      </button>
    </div>
    {#if checkResult && !checkResult.update}
      <p class="hint">VoxCtrl {checkResult.current_version} is the latest release.</p>
    {:else if checkResult?.update}
      <p class="hint">
        Version {checkResult.update.version} is available (you have
        {checkResult.update.current_version}).
        <button class="link" onclick={showUpdateWindow}>See what's new</button>
      </p>
    {/if}
    {#if checkError}
      <p class="hint error">Could not check for updates: {checkError}</p>
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
    <p class="hint">
      How long <code>transcribe_voice</code> listens when the calling agent does not ask for a
      specific timeout. An explicit <code>timeout_seconds</code> in the tool call still wins.
    </p>
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

  .btn-action:disabled {
    @apply opacity-60 cursor-default;
  }

  .link {
    @apply text-[var(--color-accent-blue)] underline underline-offset-2 bg-transparent border-0 p-0 cursor-pointer text-inherit;
  }
</style>
