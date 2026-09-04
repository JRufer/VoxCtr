<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open as openExternal } from "@tauri-apps/plugin-shell";
  import appIcon from "../../assets/app_icon.png";
  import {
    formatBytes,
    progressPercent,
    type UpdateCheckPayload,
    type UpdateInfo,
    type UpdateProgress,
  } from "./update-types";

  // "checking" is the state on a cold open — the window can be raised from
  // Settings before any check has run, so it asks rather than showing nothing.
  type Phase = "checking" | "available" | "up-to-date" | "installing" | "installed" | "failed";

  let phase = $state<Phase>("checking");
  let info = $state<UpdateInfo | null>(null);
  let currentVersion = $state("");
  let progress = $state<UpdateProgress | null>(null);
  let errorMsg = $state<string | null>(null);
  let autoCheckDisabled = $state(false);

  let unlisten: UnlistenFn[] = [];

  const percent = $derived(progressPercent(progress));

  onMount(async () => {
    unlisten.push(
      await listen<UpdateProgress>("update-progress", (e) => {
        progress = e.payload;
      }),
    );
    unlisten.push(
      await listen<string>("update-installed", () => {
        phase = "installed";
      }),
    );
    unlisten.push(
      await listen<string>("update-failed", (e) => {
        phase = "failed";
        errorMsg = e.payload;
      }),
    );

    await load();
  });

  onDestroy(() => {
    unlisten.forEach((fn) => fn());
  });

  async function load() {
    try {
      // Whatever the launch check already found, so opening this window costs
      // no network round-trip.
      let payload = await invoke<UpdateCheckPayload>("get_pending_update");
      if (!payload.update) {
        payload = await invoke<UpdateCheckPayload>("check_for_update");
      }
      currentVersion = payload.current_version;
      info = payload.update;
      phase = payload.update ? "available" : "up-to-date";
    } catch (e) {
      phase = "failed";
      errorMsg = `${e}`;
    }
  }

  async function install() {
    if (!info?.can_self_update) return;
    phase = "installing";
    errorMsg = null;
    progress = { downloaded: 0, total: info.download_size };
    try {
      await invoke("install_update");
    } catch (e) {
      // The backend also emits `update-failed`; this covers the case where the
      // command itself never got that far.
      phase = "failed";
      errorMsg = `${e}`;
    }
  }

  async function notNow() {
    await invoke("dismiss_update");
  }

  async function skip() {
    if (!info) return;
    try {
      await invoke("skip_update_version", { version: info.version });
    } catch (e) {
      console.error("Could not record the skipped version:", e);
    }
    await notNow();
  }

  async function stopAutoChecking() {
    try {
      await invoke("set_update_auto_check", { enabled: false });
      autoCheckDisabled = true;
    } catch (e) {
      console.error("Could not turn off automatic update checks:", e);
    }
  }

  async function openReleasePage() {
    try {
      await openExternal(info?.release_url ?? "https://github.com/JRufer/VoxCtrl/releases/latest");
    } catch (e) {
      console.error("Could not open the release page:", e);
    }
  }
</script>

<main>
  <div class="card">
    <header>
      <img src={appIcon} class="logo" alt="" />
      <div>
        <h1>
          {#if phase === "up-to-date"}
            VoxCtrl is up to date
          {:else if phase === "installed"}
            Restarting into {info?.version}
          {:else if phase === "installing"}
            Updating to {info?.version}
          {:else if info}
            VoxCtrl {info.version} is available
          {:else}
            Checking for updates…
          {/if}
        </h1>
        {#if info && phase !== "up-to-date"}
          <p class="sub">You have {info.current_version} — {info.version} is the latest release.</p>
        {:else if phase === "up-to-date"}
          <p class="sub">Version {currentVersion} is the latest release.</p>
        {/if}
      </div>
    </header>

    {#if phase === "checking"}
      <div class="centered">
        <div class="spinner"></div>
        <p class="sub">Asking GitHub for the latest release…</p>
      </div>
    {:else if phase === "up-to-date"}
      <p class="body">Nothing to install. VoxCtrl checks again the next time it starts.</p>
    {:else if info}
      {#if info.notes}
        <section class="notes-block">
          <h2>What's new</h2>
          <pre class="notes">{info.notes}</pre>
        </section>
      {/if}

      {#if !info.can_self_update && info.unsupported_reason}
        <p class="warn">{info.unsupported_reason}</p>
      {/if}

      {#if phase === "installing"}
        <section class="progress-block">
          <div class="bar" class:indeterminate={percent === null}>
            <div class="fill" style={percent === null ? "" : `width: ${percent}%`}></div>
          </div>
          <p class="sub">
            {#if percent === null}
              Downloading {info.asset_name ?? "the update"}…
            {:else}
              Downloading — {percent}% of {formatBytes(progress?.total ?? info.download_size)}
            {/if}
          </p>
          <p class="hint">
            VoxCtrl keeps running until the download is verified. Nothing is replaced before then.
          </p>
        </section>
      {:else if phase === "installed"}
        <p class="body">
          The new version is installed. VoxCtrl is closing and will start again in a moment.
        </p>
      {:else if info.can_self_update}
        <p class="body">
          VoxCtrl will download {formatBytes(info.download_size)}, verify it against the checksum
          GitHub published, replace itself and restart. Your settings, models and voices are not
          touched.
        </p>
      {/if}

      {#if phase === "failed" && errorMsg}
        <p class="error">{errorMsg}</p>
        <p class="hint">Your current version is untouched and still working.</p>
      {/if}
    {:else if phase === "failed" && errorMsg}
      <p class="error">{errorMsg}</p>
    {/if}

    <footer>
      {#if phase === "installing" || phase === "installed"}
        <button class="btn-secondary" onclick={notNow} disabled={phase === "installing"}>
          Close
        </button>
      {:else if phase === "up-to-date"}
        <button class="btn-primary" onclick={notNow}>Close</button>
      {:else if info}
        <div class="left-actions">
          <button class="link" onclick={openReleasePage}>Full release notes</button>
          {#if !autoCheckDisabled}
            <button class="link muted" onclick={stopAutoChecking}>Stop checking automatically</button>
          {:else}
            <span class="hint">Automatic checks are off. Turn them back on in Settings → General.</span>
          {/if}
        </div>
        <div class="right-actions">
          <button class="btn-secondary" onclick={skip}>Skip this version</button>
          <button class="btn-secondary" onclick={notNow}>Not now</button>
          {#if info.can_self_update}
            <button class="btn-primary" onclick={install}>Update and restart</button>
          {:else}
            <button class="btn-primary" onclick={openReleasePage}>Open download page</button>
          {/if}
        </div>
      {:else}
        <button class="btn-secondary" onclick={notNow}>Close</button>
      {/if}
    </footer>
  </div>
</main>

<style>
  @reference "../../app.css";

  main {
    @apply flex items-start justify-center w-screen h-screen bg-[var(--color-obsidian-950)] text-[var(--text)] overflow-y-auto p-5;
  }
  .card {
    @apply w-full max-w-[520px] flex flex-col gap-4;
  }
  header {
    @apply flex items-start gap-3;
  }
  .logo {
    @apply w-11 h-11 rounded-xl shrink-0;
  }
  h1 {
    @apply text-lg font-extrabold text-white tracking-tight leading-tight;
  }
  h2 {
    @apply text-[11px] font-bold uppercase tracking-wider text-[var(--color-obsidian-400)] mb-1.5;
  }
  .sub {
    @apply text-[12.5px] text-[var(--color-obsidian-400)] mt-1;
  }
  .body {
    @apply text-[13px] leading-relaxed text-[var(--color-obsidian-200)];
  }
  .hint {
    @apply text-[11.5px] leading-relaxed text-[var(--color-obsidian-400)];
  }
  .notes-block {
    @apply rounded-lg bg-white/[0.03] border border-[var(--border)] p-3;
  }
  .notes {
    @apply text-[12px] leading-relaxed text-[var(--color-obsidian-200)] whitespace-pre-wrap break-words m-0 max-h-[240px] overflow-y-auto font-sans;
  }
  .warn {
    @apply text-[12.5px] leading-relaxed text-amber-300/90 rounded bg-amber-500/10 border border-amber-500/20 p-2.5;
  }
  .error {
    @apply text-[12.5px] leading-relaxed text-red-400 rounded bg-red-500/10 border border-red-500/20 p-2.5 break-words;
  }
  .progress-block {
    @apply flex flex-col gap-2;
  }
  .bar {
    @apply w-full h-2 rounded-full bg-white/5 overflow-hidden;
  }
  .fill {
    @apply h-full rounded-full bg-[var(--color-accent-blue)] transition-[width] duration-200 ease-out;
  }
  /* No total to measure against: sweep instead of claiming a percentage. */
  .bar.indeterminate .fill {
    @apply w-1/3 animate-pulse;
  }
  footer {
    @apply flex flex-wrap items-center justify-between gap-3 mt-1;
  }
  .left-actions {
    @apply flex flex-col items-start gap-1;
  }
  .right-actions {
    @apply flex items-center gap-2 ml-auto;
  }
  .btn-primary {
    @apply inline-flex items-center justify-center py-2 px-3.5 rounded-md bg-[var(--color-accent-blue)] text-white font-bold text-[12.5px] shadow-[0_4px_14px_rgba(56,189,248,0.3)] transition-all duration-150 ease-out cursor-pointer;
  }
  .btn-primary:hover {
    @apply -translate-y-[1px] shadow-[0_6px_18px_rgba(56,189,248,0.45)] brightness-[1.05];
  }
  .btn-secondary {
    @apply inline-flex items-center justify-center py-2 px-3.5 rounded-md bg-[var(--color-obsidian-800)] text-[var(--color-obsidian-300)] border border-[var(--border)] font-semibold text-[12.5px] transition-all duration-150 ease-out cursor-pointer;
  }
  .btn-secondary:hover {
    @apply bg-[var(--color-obsidian-700)] text-white border-white/10;
  }
  .btn-secondary:disabled,
  .btn-primary:disabled {
    @apply opacity-60 cursor-default shadow-none;
  }
  .link {
    @apply text-[12px] font-semibold text-[var(--color-accent-blue)] underline underline-offset-2 bg-transparent border-0 p-0 cursor-pointer text-left;
  }
  .link.muted {
    @apply text-[var(--color-obsidian-400)] font-normal no-underline;
  }
  .link.muted:hover {
    @apply underline text-[var(--color-obsidian-200)];
  }
  .centered {
    @apply flex flex-col items-center justify-center gap-3 py-8;
  }
  .spinner {
    @apply w-7 h-7 border-[2.5px] border-white/5 border-t-[var(--color-accent-blue)] rounded-full animate-spin;
  }
</style>
