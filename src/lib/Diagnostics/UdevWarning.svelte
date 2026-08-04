<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  type Permissions = {
    is_configured: boolean;
    rule_exists: boolean;
    in_group: boolean;
    needs_relogin: boolean;
    rule_is_current: boolean;
    devices_total: number;
    devices_readable: number;
    can_relaunch: boolean;
    pkexec_available: boolean;
    session_type: string;
    detail: string;
    manual_commands: string;
  };

  type SetupStatus = {
    permissions: Permissions;
    hotkeys_active: boolean;
    model_ready: boolean;
    model_size: string;
    model_auto_downloads: boolean;
    missing_injection_tool: string | null;
    is_complete: boolean;
  };

  type StepState = "ok" | "busy" | "todo";

  let setup = $state<SetupStatus | null>(null);
  let installing = $state(false);
  let restarting = $state(false);
  let downloading = $state(false);
  let errorMsg = $state<string | null>(null);
  let showManual = $state(false);
  let copied = $state(false);

  // Polled rather than fetched once: permissions can start working while this
  // window is open — the user runs the setup, or fixes it in a terminal — and
  // a stale "not configured" screen is exactly the confusion to avoid.
  let poll: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      setup = await invoke<SetupStatus>("get_setup_status");
    } catch (err) {
      console.error("Failed to read setup status:", err);
    }
  }

  onMount(() => {
    refresh();
    poll = setInterval(refresh, 2000);
  });

  onDestroy(() => {
    if (poll) clearInterval(poll);
  });

  async function handleClose() {
    try {
      await getCurrentWindow().close();
    } catch (err) {
      console.error("Failed to close window natively:", err);
    }
  }

  async function handleSetup() {
    installing = true;
    errorMsg = null;
    try {
      await invoke("install_system_integration");
      await refresh();
    } catch (err: any) {
      errorMsg = err?.toString() ?? "Setup failed";
      showManual = true;
    } finally {
      installing = false;
    }
  }

  async function handleRestart() {
    restarting = true;
    errorMsg = null;
    try {
      await invoke("restart_for_permissions");
    } catch (err: any) {
      restarting = false;
      errorMsg = err?.toString() ?? "Restart failed";
    }
  }

  async function handleDownloadModel() {
    downloading = true;
    errorMsg = null;
    try {
      await invoke("download_configured_model");
      await refresh();
    } catch (err: any) {
      errorMsg = err?.toString() ?? "Download failed";
    } finally {
      downloading = false;
    }
  }

  async function handleChooseModel() {
    try {
      await invoke("open_settings_tab", { tab: "engine" });
    } catch (err) {
      console.error("Failed to open the Engine settings tab:", err);
    }
  }

  async function copyManual() {
    if (!setup) return;
    try {
      await navigator.clipboard.writeText(setup.permissions.manual_commands);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (err) {
      console.error("Clipboard write failed:", err);
    }
  }

  // Permissions only count as done once the listener actually holds a
  // keyboard; the rule being on disk is not the same as hotkeys working.
  const permissionState = $derived<StepState>(
    !setup ? "busy" : setup.permissions.is_configured && setup.hotkeys_active ? "ok" : "todo",
  );
  const injectionState = $derived<StepState>(
    !setup ? "busy" : setup.missing_injection_tool ? "todo" : "ok",
  );
  // A small default model downloads itself in the background, so "not on disk
  // yet" is progress rather than something the user has to act on.
  const modelState = $derived<StepState>(
    !setup ? "busy" : setup.model_ready ? "ok" : setup.model_auto_downloads ? "busy" : "todo",
  );

  const icon = (s: StepState) => (s === "ok" ? "✅" : s === "busy" ? "⏳" : "⚠️");
</script>

<div class="diagnostic-window">
  {#if installing || restarting}
    <div class="loading-container">
      <div class="spinner"></div>
      <span class="loading-label">
        {restarting ? "Restarting VoxCtrl with keyboard access…" : "Configuring hotkey permissions…"}
      </span>
      {#if installing}
        <p class="hint">You may be prompted for your administrator password.</p>
      {/if}
    </div>
  {:else if !setup}
    <div class="loading-container">
      <div class="spinner"></div>
      <span class="loading-label">Checking VoxCtrl setup…</span>
    </div>
  {:else}
    <div class="modal-card">
      <header class="head">
        <span class="head-icon">{setup.is_complete ? "✅" : "🛠️"}</span>
        <div>
          <h2 class="title">{setup.is_complete ? "VoxCtrl is ready" : "Finish setting up VoxCtrl"}</h2>
          <p class="subtitle">
            {setup.is_complete
              ? "Press your shortcut anywhere to dictate."
              : "Global shortcuts stay inactive until the steps below are done."}
          </p>
        </div>
      </header>

      <ol class="steps">
        <!-- 1 · Keyboard access -->
        <li class="step" class:done={permissionState === "ok"}>
          <span class="step-icon">{icon(permissionState)}</span>
          <div class="step-body">
            <span class="step-title">Keyboard access for global shortcuts</span>
            <span class="step-detail">{setup.permissions.detail}</span>

            {#if permissionState !== "ok"}
              <div class="step-actions">
                {#if setup.permissions.can_relaunch}
                  <button class="btn-primary" onclick={handleRestart}>
                    Restart VoxCtrl to finish
                  </button>
                {:else if setup.permissions.pkexec_available}
                  <button class="btn-primary" onclick={handleSetup}>
                    Grant keyboard access
                  </button>
                {/if}
                <button class="btn-link" onclick={() => (showManual = !showManual)}>
                  {showManual ? "Hide manual commands" : "Set it up manually"}
                </button>
              </div>

              {#if setup.permissions.needs_relogin && !setup.permissions.can_relaunch}
                <p class="warn-note">
                  VoxCtrl could not apply the new permissions to this session by itself. Log out and
                  back in (or reboot) and they will take effect.
                </p>
              {/if}
            {:else if !setup.permissions.rule_is_current && setup.permissions.rule_exists}
              <p class="warn-note">
                This machine still has VoxCtrl's older permission rule. Re-running
                <button class="btn-link inline" onclick={handleSetup}>the setup</button>
                stops access from depending on a fresh login.
              </p>
            {/if}

            {#if showManual}
              <div class="manual">
                <pre>{setup.permissions.manual_commands}</pre>
                <button class="btn-link" onclick={copyManual}>
                  {copied ? "Copied" : "Copy commands"}
                </button>
              </div>
            {/if}
          </div>
        </li>

        <!-- 2 · Typing transcriptions into other windows -->
        <li class="step" class:done={injectionState === "ok"}>
          <span class="step-icon">{icon(injectionState)}</span>
          <div class="step-body">
            <span class="step-title">Typing text into other windows</span>
            <span class="step-detail">
              {#if setup.missing_injection_tool}
                <code>{setup.missing_injection_tool}</code> is missing, so transcriptions cannot be
                typed into the focused window. Running the setup above installs it.
              {:else}
                Ready — transcriptions can be typed straight into the focused window.
              {/if}
            </span>
          </div>
        </li>

        <!-- 3 · Speech model -->
        <li class="step" class:done={modelState === "ok"}>
          <span class="step-icon">{icon(modelState)}</span>
          <div class="step-body">
            <span class="step-title">Speech model</span>
            <span class="step-detail">
              {#if setup.model_ready}
                <code>{setup.model_size}</code> is downloaded and ready.
              {:else if setup.model_auto_downloads}
                Downloading <code>{setup.model_size}</code> in the background — no action needed.
              {:else}
                <code>{setup.model_size}</code> is not downloaded yet, so dictation produces no text.
              {/if}
            </span>

            {#if !setup.model_ready && !setup.model_auto_downloads}
              <div class="step-actions">
                <button class="btn-primary" onclick={handleDownloadModel} disabled={downloading}>
                  {downloading ? "Downloading…" : `Download ${setup.model_size}`}
                </button>
                <button class="btn-link" onclick={handleChooseModel}>Choose a different model</button>
              </div>
            {/if}
          </div>
        </li>
      </ol>

      {#if errorMsg}
        <div class="error-container">
          <span class="error-icon">❌</span>
          <span class="error-msg">{errorMsg}</span>
        </div>
      {/if}

      <div class="modal-actions">
        <button class="btn-secondary" onclick={handleClose}>
          {setup.is_complete ? "Close" : "Continue anyway"}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  @reference "tailwindcss";

  .diagnostic-window {
    @apply flex items-start justify-center w-screen h-screen bg-[var(--color-obsidian-950)] text-[var(--text)] overflow-y-auto p-5;
  }

  .modal-card {
    @apply w-full max-w-[520px] animate-[scaleUp_0.3s_cubic-bezier(0.175,0.885,0.32,1.2)_forwards];
  }

  .head {
    @apply flex items-start gap-3 mb-5;
  }

  .head-icon {
    @apply text-[32px] leading-none shrink-0;
  }

  .title {
    @apply text-xl font-extrabold text-white tracking-tight;
  }

  .subtitle {
    @apply text-[12.5px] text-[var(--color-obsidian-400)] mt-1;
  }

  .steps {
    @apply flex flex-col gap-2.5 list-none p-0 m-0;
  }

  .step {
    @apply flex items-start gap-3 p-3 rounded-lg bg-white/[0.03] border border-[var(--border)];
  }

  .step.done {
    @apply bg-transparent border-white/5;
  }

  .step-icon {
    @apply text-[16px] leading-6 shrink-0;
  }

  .step-body {
    @apply flex flex-col gap-1 min-w-0 flex-1;
  }

  .step-title {
    @apply text-[13.5px] font-bold text-white;
  }

  .step-detail {
    @apply text-[12.5px] leading-relaxed text-[var(--color-obsidian-300)];
  }

  .step-detail code {
    @apply bg-white/5 px-1.5 py-0.5 rounded font-mono text-[var(--color-accent-blue)];
  }

  .warn-note {
    @apply text-[12px] leading-relaxed text-amber-300/90 mt-1;
  }

  .step-actions {
    @apply flex flex-wrap items-center gap-2 mt-2;
  }

  .manual {
    @apply mt-2 rounded-md bg-black/40 border border-[var(--border)] p-2.5;
  }

  .manual pre {
    @apply text-[11px] leading-relaxed font-mono text-[var(--color-obsidian-200)] whitespace-pre-wrap break-all m-0 max-h-[180px] overflow-y-auto;
  }

  .error-container {
    @apply flex items-start gap-2 p-3 rounded bg-red-500/10 border border-red-500/20 text-left mt-4 text-[12px] text-red-400 w-full;
  }

  .error-icon {
    @apply shrink-0;
  }

  .error-msg {
    @apply leading-relaxed break-words;
  }

  .modal-actions {
    @apply flex justify-end mt-5;
  }

  .btn-primary {
    @apply inline-flex items-center justify-center py-2 px-3.5 rounded-md bg-[var(--color-accent-blue)] text-white font-bold text-[12.5px] shadow-[0_4px_14px_rgba(56,189,248,0.3)] transition-all duration-150 ease-out cursor-pointer;
  }

  .btn-primary:hover:not(:disabled) {
    @apply -translate-y-[1px] shadow-[0_6px_18px_rgba(56,189,248,0.45)] brightness-[1.05];
  }

  .btn-primary:disabled {
    @apply opacity-60 cursor-default shadow-none;
  }

  .btn-secondary {
    @apply inline-flex items-center justify-center py-2 px-4 rounded-md bg-[var(--color-obsidian-800)] text-[var(--color-obsidian-300)] border border-[var(--border)] font-semibold text-[12.5px] transition-all duration-150 ease-out cursor-pointer;
  }

  .btn-secondary:hover {
    @apply bg-[var(--color-obsidian-700)] text-white border-white/10;
  }

  .btn-link {
    @apply text-[12px] font-semibold text-[var(--color-accent-blue)] underline underline-offset-2 bg-transparent border-0 p-0 cursor-pointer;
  }

  .btn-link.inline {
    @apply text-[12px];
  }

  .loading-container {
    @apply flex flex-col items-center justify-center gap-3 h-full w-full;
  }

  .loading-label {
    @apply text-[13.5px] text-[var(--color-obsidian-200)] font-semibold text-center;
  }

  .hint {
    @apply text-[11.5px] text-[var(--color-obsidian-400)] max-w-[280px] text-center;
  }

  .spinner {
    @apply w-7 h-7 border-[2.5px] border-white/5 border-t-[var(--color-accent-blue)] rounded-full animate-spin;
  }

  @keyframes scaleUp {
    from { transform: scale(0.98); opacity: 0; }
    to { transform: scale(1); opacity: 1; }
  }
</style>
