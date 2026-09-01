<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { config } from "../../stores/config";

  interface HistoryEntry {
    text: string;
    target_id: string;
    timestamp: string;
    inference_ms: number;
  }

  interface OutputTarget {
    id: string;
    label: string;
    delivery: string;
  }

  let entries = $state<HistoryEntry[]>([]);
  let targets = $state<OutputTarget[]>([]);
  let loading = $state(true);

  // Quick lookup map for target details
  let targetMap = $derived(new Map(targets.map(t => [t.id, t])));

  async function loadHistory() {
    try {
      const fetchedHistory = await invoke<HistoryEntry[]>("get_history");
      entries = fetchedHistory.slice(0, 100);
    } catch (err) {
      console.error("Failed to load history", err);
    }
  }

  onMount(() => {
    // Initial load (history + targets) runs concurrently.
    (async () => {
      try {
        const [fetchedHistory, fetchedTargets] = await Promise.all([
          invoke<HistoryEntry[]>("get_history"),
          invoke<OutputTarget[]>("get_targets")
        ]);
        entries = fetchedHistory.slice(0, 100);
        targets = fetchedTargets;
      } catch (err) {
        console.error("Failed to load history or targets", err);
      } finally {
        loading = false;
      }
    })();

    // Reactively refresh when the window is focused or when backend status
    // updates are dispatched. onMount must return a synchronous cleanup, so we
    // track the unlisten handles and invoke them once they resolve.
    let unlistenFocus: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;

    listen("tauri://focus", () => loadHistory()).then((u) => (unlistenFocus = u));
    listen("status-tick", () => loadHistory()).then((u) => (unlistenStatus = u));

    return () => {
      unlistenFocus?.();
      unlistenStatus?.();
    };
  });

  async function clearHistory() {
    await invoke("clear_history");
    entries = [];
  }

  function formatTime(iso: string) {
    try {
      const d = new Date(iso);
      if (isNaN(d.getTime())) return "Unknown Time";
      // Format as e.g. "May 20, 22:15:30"
      return d.toLocaleString(undefined, {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
    } catch {
      return "Unknown Time";
    }
  }

  function deliveryLabel(d: string) {
    const map: Record<string, string> = {
      inject: "Inject",
      clipboard: "Clipboard",
      exec: "Exec",
      pipe: "Pipe",
      socket: "Socket",
      file: "File",
      dbus: "DBus",
      http: "HTTP",
      webhook: "Webhook",
      mcp: "MCP",
    };
    return map[d] ?? d;
  }

  function getTargetNotes(targetId: string) {
    if (!targetId) return "";
    const ids = targetId.split(',').map(s => s.trim()).filter(s => s.length > 0);
    return ids.map(id => {
      const target = targetMap.get(id);
      if (!target) {
        return id === "default" ? "Focused Window (Inject)" : id;
      }
      return `${target.label} (${deliveryLabel(target.delivery)})`;
    }).join(" + ");
  }
</script>

<div class="history-root">
  <header class="toolbar">
    <div class="title-section">
      <span class="title-logo">📋</span>
      <div class="title-text">
        <h1>Transcript History</h1>
        <p class="subtitle">DICTATION LOGS FOR THIS SESSION</p>
      </div>
    </div>
    <button class="btn-clear interactive-spring" onclick={clearHistory}>
      🧹 Clear Log
    </button>
  </header>

  {#if !$config?.ui?.history_enabled}
    <div class="empty-state">
      <div class="empty-graphic">
        <span class="empty-emoji">🚫</span>
      </div>
      <h3 class="empty-title">History Disabled</h3>
      <p class="empty-subtitle">Transcript history is turned off. Enable it in <strong>Settings → General</strong> to start logging.</p>
    </div>
  {:else if loading}
    <div class="empty-state">
      <div class="spinner-container">
        <svg class="spinner-svg" viewBox="0 0 50 50">
          <circle class="path" cx="25" cy="25" r="20" fill="none" stroke-width="5"></circle>
        </svg>
      </div>
      <span class="loading-label">Retrieving transcript log...</span>
    </div>
  {:else if entries.length === 0}
    <div class="empty-state">
      <div class="empty-graphic">
        <span class="empty-emoji">📭</span>
      </div>
      <h3 class="empty-title">Log is Empty</h3>
      <p class="empty-subtitle">Your transcribed speech sessions will appear here.</p>
    </div>
  {:else}
    <div class="list">
      {#each entries as entry}
        <div class="entry card-spring">
          <div class="entry-bubble">
            <span class="bubble-decorator"></span>
            <p class="entry-text">{entry.text}</p>
            <button
              class="btn-copy"
              title="Copy to clipboard"
              onclick={() => navigator.clipboard.writeText(entry.text)}
            >⎘</button>
          </div>
          
          <div class="entry-meta">
            <div class="meta-row">
              <span class="badge badge-neutral">
                <span class="badge-icon">⏰</span>
                {formatTime(entry.timestamp)}
              </span>
              <span class="badge badge-orange">
                <span class="badge-icon">⚡</span>
                {entry.inference_ms}ms
              </span>
            </div>
            <span class="badge badge-blue">
              <span class="badge-icon">🎯</span>
              {getTargetNotes(entry.target_id)}
            </span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  @reference "../../app.css";

  .history-root {
    @apply flex flex-col h-screen w-screen bg-[var(--bg)] text-[var(--text)] overflow-hidden;
  }

  .toolbar {
    @apply flex items-center justify-between p-4 px-6 border-b border-[var(--border)] bg-[var(--color-obsidian-900)] shadow-[4px_0_24px_rgba(0,0,0,0.25)] z-10;
  }

  .title-section {
    @apply flex items-center gap-3.5;
  }

  .title-logo {
    @apply text-xl drop-shadow-[0_2px_8px_rgba(56,189,248,0.3)];
  }

  .title-text {
    @apply flex flex-col;
  }

  h1 {
    @apply text-[15px] font-[850] text-white tracking-[-0.5px] leading-[1.1];
  }

  .subtitle {
    @apply text-[7px] font-bold text-[var(--color-accent-blue)] tracking-[0.12em] mt-0.5;
  }

  .btn-clear {
    @apply bg-red-500/8 border border-red-500/20 text-red-400 p-1.5 px-3.5 rounded-[var(--radius)] text-xs font-bold transition-all duration-150 ease-out;
  }

  .btn-clear:hover {
    @apply bg-red-500 border-red-500 text-white shadow-[0_4px_12px_rgba(239,68,68,0.25)];
  }

  .btn-clear:active {
    @apply scale-[0.97];
  }

  .list {
    @apply flex-1 overflow-y-auto p-5 px-6 flex flex-col gap-2;
  }

  .entry {
    @apply bg-[var(--color-obsidian-800)]/40 border border-[var(--border)] rounded-[var(--radius)] p-3.5 px-4 flex flex-col gap-2.5 transition-all duration-200 ease-in-out animate-[cardSlide_0.25s_cubic-bezier(0.175,0.885,0.32,1.2)_forwards];
  }

  @keyframes cardSlide {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .entry:hover {
    @apply border-[var(--accent2)] shadow-[0_4px_20px_rgba(0,0,0,0.3)] -translate-y-[2px];
  }

  .entry-bubble {
    @apply relative pl-2.5 pr-7;
  }

  .bubble-decorator {
    @apply absolute left-0 top-1 bottom-1 w-[3px] bg-[var(--accent2)] rounded-full opacity-50;
  }

  .btn-copy {
    @apply absolute top-0 right-0 w-[22px] h-[22px] flex items-center justify-center text-xs bg-transparent border border-transparent rounded text-[var(--text-muted)] opacity-0 transition-all duration-150 ease-out cursor-pointer p-0;
  }

  .entry:hover .btn-copy {
    @apply opacity-100;
  }

  .btn-copy:hover {
    @apply text-[var(--accent2)] bg-[var(--accent2)]/8 border-[var(--accent2)]/20;
  }

  .btn-copy:active {
    @apply scale-[0.92];
  }

  .entry-text {
    @apply text-[13px] leading-relaxed text-[var(--text)] font-medium whitespace-pre-wrap;
  }

  .entry:hover .entry-text {
    @apply text-white;
  }

  .entry-meta {
    @apply flex flex-col gap-1.5;
  }

  .meta-row {
    @apply flex justify-between items-center;
  }

  .badge {
    @apply flex items-center gap-1 py-0.5 px-2 rounded-xl text-[10px] font-semibold tracking-wide border border-transparent;
  }

  .badge-icon {
    @apply text-[10px];
  }

  .badge-neutral {
    @apply bg-white/[0.03] text-[var(--text-muted)] border-[var(--border)];
  }

  .badge-blue {
    @apply bg-[var(--accent2)]/15 text-[var(--accent2)] border-[var(--accent2)]/30;
  }

  .badge-orange {
    @apply bg-purple-500/15 text-purple-300 border-purple-500/30;
  }

  .empty-state {
    @apply flex-1 flex flex-col items-center justify-center gap-3 text-[var(--text-muted)] p-10;
  }

  .spinner-container {
    @apply w-11 h-11 mb-2;
  }

  .spinner-svg {
    @apply w-full h-full animate-[rotate_1.8s_linear_infinite];
  }

  .spinner-svg .path {
    @apply stroke-[var(--color-accent-blue)] animate-[dash_1.5s_ease-in-out_infinite];
    stroke-linecap: round;
  }

  @keyframes rotate {
    100% { transform: rotate(360deg); }
  }

  @keyframes dash {
    0% { stroke-dasharray: 1, 150; stroke-dashoffset: 0; }
    50% { stroke-dasharray: 90, 150; stroke-dashoffset: -35; }
    100% { stroke-dasharray: 90, 150; stroke-dashoffset: -124; }
  }

  .loading-label {
    @apply text-xs font-semibold text-[var(--text-muted)] tracking-wider;
  }

  .empty-graphic {
    @apply w-16 h-16 bg-white/[0.03] border border-[var(--border)] rounded-full flex items-center justify-center mb-2;
  }

  .empty-emoji {
    @apply text-[28px];
  }

  .empty-title {
    @apply text-[15px] font-bold text-white tracking-wide;
  }

  .empty-subtitle {
    @apply text-xs text-[var(--text-muted)] text-center max-w-[260px] leading-relaxed;
  }

  .empty-subtitle strong {
    @apply text-[var(--accent2)] font-semibold;
  }
</style>
