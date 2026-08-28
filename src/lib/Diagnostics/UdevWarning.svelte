<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  type BoundShortcut = {
    binding_ids: string[];
    requested: string | null;
    trigger_description: string;
    bound: boolean;
  };

  type HotkeyStatus = {
    is_active: boolean;
    backend: string;
    is_private: boolean;
    portal_error: string | null;
    portal_refused: boolean;
    shortcuts: BoundShortcut[];
    session_type: string;
    devices_total: number;
    devices_readable: number;
    needs_attention: boolean;
    needs_manual_enable: boolean;
    manual_enable_hint: string | null;
    is_mint_desktop?: boolean;
    mint_shortcut_registered?: boolean;
    detail: string;
  };

  type SetupStatus = {
    hotkeys: HotkeyStatus;
    hotkeys_active: boolean;
    model_ready: boolean;
    model_size: string;
    model_auto_downloads: boolean;
    missing_injection_tool: string | null;
    pkexec_available: boolean;
    manual_package_commands: string;
    is_complete: boolean;
  };

  type StepState = "ok" | "busy" | "todo";

  let setup = $state<SetupStatus | null>(null);
  let installing = $state(false);
  let downloading = $state(false);
  let errorMsg = $state<string | null>(null);
  let showManual = $state(false);
  let copied = $state(false);
  let openingShortcutSettings = $state(false);
  let openShortcutSettingsError = $state<string | null>(null);
  let retryingShortcuts = $state(false);
  let retryShortcutsError = $state<string | null>(null);
  let registeringMint = $state(false);
  let registerMintError = $state<string | null>(null);

  // Polled rather than fetched once: the portal handshake finishes a moment
  // after launch, and the user may grant or change a shortcut in their
  // desktop's settings while this window is open. A stale screen is exactly the
  // confusion to avoid.
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
      errorMsg = err?.toString() ?? "Install failed";
      showManual = true;
    } finally {
      installing = false;
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

  async function openShortcutSettings() {
    openingShortcutSettings = true;
    openShortcutSettingsError = null;
    try {
      await invoke("open_shortcut_settings");
    } catch (err: any) {
      openShortcutSettingsError = err?.toString() ?? "Could not open shortcut settings.";
    } finally {
      openingShortcutSettings = false;
    }
  }

  async function handleApproveShortcuts() {
    retryingShortcuts = true;
    retryShortcutsError = null;
    try {
      await invoke("approve_shortcuts");
      await refresh();
    } catch (err: any) {
      retryShortcutsError = err?.toString() ?? "Failed to configure shortcuts for your desktop.";
      await refresh();
    } finally {
      retryingShortcuts = false;
    }
  }

  async function copyManual() {
    if (!setup) return;
    try {
      await navigator.clipboard.writeText(setup.manual_package_commands);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (err) {
      console.error("Clipboard write failed:", err);
    }
  }

  // Shortcuts count as done when something is actually delivering them. The
  // portal is the good case; the evdev fallback works but is worth a note.
  const hotkeyState = $derived<StepState>(
    !setup
      ? "busy"
      : setup.hotkeys.backend === "starting"
        ? "busy"
        : setup.hotkeys_active
          ? "ok"
          : "todo",
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
  {#if installing}
    <div class="loading-container">
      <div class="spinner"></div>
      <span class="loading-label">Installing the helper packages…</span>
      <p class="hint">You may be prompted for your administrator password.</p>
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
              : "Dictation is not ready yet — the steps below say why."}
          </p>
        </div>
      </header>

      <ol class="steps">
        <!-- 1 · How global shortcuts are delivered -->
        <li class="step" class:done={hotkeyState === "ok"}>
          <span class="step-icon">{icon(hotkeyState)}</span>
          <div class="step-body">
            <span class="step-title">Global shortcuts</span>
            <span class="step-detail">{setup.hotkeys.detail}</span>

            {#if setup.hotkeys.needs_manual_enable}
              <p class="warn-note">
                🔧 One more step on KDE: {setup.hotkeys.manual_enable_hint}
              </p>
              <div class="step-actions">
                <button
                  class="btn-primary"
                  onclick={openShortcutSettings}
                  disabled={openingShortcutSettings}
                >
                  {openingShortcutSettings ? "Opening…" : "Open Shortcut Settings"}
                </button>
              </div>
              {#if openShortcutSettingsError}
                <p class="warn-note">{openShortcutSettingsError}</p>
              {/if}
            {/if}
            {#if setup.hotkeys.backend === "portal" || setup.hotkeys.backend === "mint_dbus"}
              <p class="privacy-note">
                🔒 VoxCtrl asked your desktop to register its shortcuts and is told nothing
                else. It does not read your keyboard, cannot see what you type in other
                applications, and needed no special permissions to set up.
              </p>
            {/if}

            {#if setup.hotkeys_active && setup.hotkeys.shortcuts.length > 0}
              <ul class="shortcut-list">
                {#each setup.hotkeys.shortcuts as sc}
                  <li class:unbound={!sc.bound}>
                    {#if sc.bound}
                      <span class="shortcut-keys">
                        {sc.trigger_description || sc.requested || "chosen in your desktop settings"}
                      </span>
                    {:else}
                      <span class="shortcut-keys">not accepted by your desktop</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}

            {#if setup.hotkeys.backend === "evdev"}
              <p class="warn-note">
                Your desktop does not offer the global-shortcuts portal, so VoxCtrl is
                falling back to reading input devices — access this machine already had,
                which VoxCtrl neither requested nor configured. Shortcuts work, but in this
                mode every keystroke passes through VoxCtrl. Nothing is stored or sent
                anywhere; if you would rather it did not happen at all, a desktop with
                portal support (KDE Plasma, GNOME 48+, Hyprland) avoids it entirely.
              </p>
            {:else if setup.hotkeys.portal_refused}
              <p class="warn-note">
                Global shortcuts require approval from your system. If you closed or cancelled
                the desktop keybind prompt without approving, click below to show the prompt again.
              </p>
              <div class="step-actions">
                <button
                  class="btn-primary"
                  onclick={handleApproveShortcuts}
                  disabled={retryingShortcuts}
                >
                  {retryingShortcuts ? "Configuring…" : "Approve Keybinds"}
                </button>
              </div>
              {#if retryShortcutsError}
                <p class="warn-note">{retryShortcutsError}</p>
              {/if}
            {:else if setup.hotkeys.backend !== "starting"}
              <p class="warn-note">
                Global shortcuts require approval or registration from your system. VoxCtrl will not grant itself keyboard access to work around this.
              </p>
              <div class="step-actions">
                <button
                  class="btn-primary"
                  onclick={handleApproveShortcuts}
                  disabled={retryingShortcuts}
                >
                  {retryingShortcuts ? "Configuring…" : "Approve Shortcuts"}
                </button>
              </div>
              {#if retryShortcutsError}
                <p class="warn-note">{retryShortcutsError}</p>
              {/if}
            {/if}

            <!-- Rendered outside the branch chain: whatever the desktop said is
                 the most useful line on this screen for anyone diagnosing it,
                 and it applies to a refusal just as much as to a missing
                 portal. -->
            {#if setup.hotkeys.portal_error && setup.hotkeys.backend !== "portal" && setup.hotkeys.backend !== "evdev"}
              <span class="step-detail muted">Portal reported: {setup.hotkeys.portal_error}</span>
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
                typed into the focused window.
              {:else}
                Ready — transcriptions can be typed straight into the focused window.
              {/if}
            </span>

            {#if setup.missing_injection_tool}
              <div class="step-actions">
                {#if setup.pkexec_available && setup.manual_package_commands}
                  <button class="btn-primary" onclick={handleSetup}>Install it</button>
                {/if}
                {#if setup.manual_package_commands}
                  <button class="btn-link" onclick={() => (showManual = !showManual)}>
                    {showManual ? "Hide manual commands" : "Install it manually"}
                  </button>
                {/if}
              </div>

              {#if showManual}
                <div class="manual">
                  <pre>{setup.manual_package_commands}</pre>
                  <button class="btn-link" onclick={copyManual}>
                    {copied ? "Copied" : "Copy commands"}
                  </button>
                </div>
              {/if}
            {/if}
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

  .privacy-note {
    @apply text-[12px] leading-relaxed text-emerald-300/90 mt-1;
  }

  .muted {
    @apply text-[11px] text-[var(--color-obsidian-400)] mt-1;
  }

  .shortcut-list {
    @apply flex flex-wrap gap-1.5 list-none p-0 mt-2;
  }

  .shortcut-keys {
    @apply inline-block px-1.5 py-0.5 rounded bg-white/5 font-mono text-[11px] text-[var(--color-accent-blue)];
  }

  .shortcut-list li.unbound .shortcut-keys {
    @apply text-amber-300/90;
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
