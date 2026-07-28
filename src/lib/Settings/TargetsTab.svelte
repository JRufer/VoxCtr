<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget, HotkeyBinding } from "./routing-types";
  import TargetEditorModal from "./TargetEditorModal.svelte";

  let targets = $state<OutputTarget[]>([]);
  let bindings = $state<HotkeyBinding[]>([]);

  // Modals state
  let editingTarget = $state<OutputTarget | null>(null);
  let isEditingTargetNew = $state(false);
  let originalTargetId = $state<string | null>(null);
  let confirmDeleteTargetId = $state<string | null>(null);

  onMount(async () => {
    targets = await invoke<OutputTarget[]>("get_targets");
    bindings = await invoke<HotkeyBinding[]>("get_bindings");
  });

  async function persistTargets() {
    try {
      await invoke("save_targets", { targets });
      console.log("Targets auto-saved successfully!");
    } catch (e) {
      console.error("Failed to save targets:", e);
    }
  }

  async function persistBindings() {
    try {
      await invoke("save_bindings", { bindings });
      console.log("Bindings auto-saved successfully!");
    } catch (e) {
      console.error("Failed to save bindings:", e);
    }
  }

  // --- CRUD Output Targets ---
  function addNewTarget() {
    isEditingTargetNew = true;
    editingTarget = {
      id: "new_target_" + Math.random().toString(36).substring(2, 6),
      label: "New Target",
      delivery: "inject",
      file_prefix: "- ",
      file_timestamp: true,
      file_mode: "append",
      http_method: "POST",
      http_json_template: { text: "{text}" },
      webhook_json_template: { text: "{text}" },
      mcp_tool: "speak_text",
      mcp_args: { text: "{text}" },
      chat_max_history: 20,
      chat_timeout_secs: 120,
      chat_reply_mode: "speak",
      send_on_release: false,
      append_newline: true,
      strip_newlines: false,
      processing: {
        apply_snippets: true,
      }
    };
  }

  function editTarget(tgt: OutputTarget) {
    isEditingTargetNew = false;
    originalTargetId = tgt.id;
    const clone = JSON.parse(JSON.stringify(tgt));
    if (!clone.processing) {
      clone.processing = {};
    }
    if (!clone.file_mode) {
      clone.file_mode = "append";
    }
    editingTarget = clone;
  }

  function cancelTargetModal() {
    editingTarget = null;
    originalTargetId = null;
  }

  async function saveTargetModal() {
    if (!editingTarget) return;

    if (isEditingTargetNew) {
      targets = [...targets, editingTarget];
    } else {
      // Update bindings that point to the old ID
      if (originalTargetId && originalTargetId !== editingTarget.id) {
        let bindingsChanged = false;
        bindings = bindings.map(b => {
          let updated = false;
          let newTargetIds = b.target_ids ? [...b.target_ids] : [b.target_id];
          for (let i = 0; i < newTargetIds.length; i++) {
            if (newTargetIds[i] === originalTargetId) {
              newTargetIds[i] = editingTarget!.id;
              updated = true;
            }
          }
          if (updated) {
            bindingsChanged = true;
            return { ...b, target_ids: newTargetIds, target_id: newTargetIds[0] };
          }
          return b;
        });

        if (bindingsChanged) {
          await persistBindings();
        }
      }

      targets = targets.map(t => t.id === originalTargetId ? editingTarget! : t);
    }
    editingTarget = null;
    originalTargetId = null;
    await persistTargets();
  }

  async function deleteTarget(id: string) {
    const usedBy = bindings.filter(b => {
      const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
      return ids.includes(id);
    }).map(b => b.label || b.id);
    if (usedBy.length > 0) {
      alert(`Cannot delete target. It is currently being used by hotkeys: ${usedBy.join(", ")}`);
      return;
    }
    targets = targets.filter(t => t.id !== id);
    await persistTargets();
  }
</script>

<section class="targets-section">
  <div class="section-header">
    <div>
      <h2>Output Targets</h2>
      <p class="description">Define routing destinations where transcribed text is typed, copied, piped, or sent over network sockets.</p>
    </div>
  </div>

  <button class="btn-add-wide" onclick={addNewTarget}>
    ＋ Add New Output Target
  </button>

  <div class="bindings-list">
    {#each targets as t}
      <div class="binding-item glass">
        <div class="binding-content">
          <div class="binding-row2">
            <div class="binding-title">{t.label}</div>
            <span class="badge delivery">{t.delivery}</span>
          </div>
          {#if t.delivery === "exec"}
            <div class="binding-targets">Cmd: {t.command}</div>
          {/if}
          {#if t.delivery === "file"}
            <div class="binding-targets">File: {t.file_path}</div>
          {/if}
          {#if t.delivery === "socket"}
            <div class="binding-targets">Socket: {t.socket_host}:{t.socket_port}</div>
          {/if}
          {#if t.delivery === "pipe"}
            <div class="binding-targets">Pipe: {t.pipe_path} {#if t.response_pipe}→ {t.response_pipe}{/if}</div>
          {/if}
          {#if t.delivery === "http" || t.delivery === "webhook"}
            <div class="binding-targets">API: {t.http_url || t.webhook_url}</div>
          {/if}
          {#if t.delivery === "mcp"}
            <div class="binding-targets">MCP Tool: {t.mcp_tool || "speak_text"}</div>
          {/if}
          {#if t.delivery === "speak"}
            <div class="binding-targets">Speech: Plays transcribed text via TTS</div>
          {/if}
          {#if t.delivery === "chat"}
            <div class="binding-targets">
              Chat: {t.chat_model || "no model"} @ {t.chat_url || "no endpoint"} → {t.chat_reply_mode || "speak"}
            </div>
          {/if}
          <div class="binding-actions">
            <button class="btn-action small" onclick={() => editTarget(t)}>Edit</button>
            {#if confirmDeleteTargetId === t.id}
              <span class="confirm-label">Delete?</span>
              <button class="btn-action small danger" onclick={() => { deleteTarget(t.id); confirmDeleteTargetId = null; }}>Yes</button>
              <button class="btn-action small" onclick={() => confirmDeleteTargetId = null}>No</button>
            {:else}
              <button class="btn-action small danger" onclick={() => confirmDeleteTargetId = t.id}>Delete</button>
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="empty-state">
        <span class="empty-icon">🎯</span>
        <p>No output targets defined. Create one to route your transcription output!</p>
      </div>
    {/each}
  </div>
</section>

{#if editingTarget}
  <TargetEditorModal
    bind:editingTarget
    isNew={isEditingTargetNew}
    existingTargets={targets}
    isNested={false}
    onSave={saveTargetModal}
    onCancel={cancelTargetModal}
  />
{/if}

<style>
  @reference "tailwindcss";

  .targets-section {
    @apply flex flex-col gap-5 pb-10;
  }

  .section-header {
    @apply flex justify-between items-center gap-5 mb-2;
  }

  .description {
    @apply text-xs text-[var(--text-muted)] mt-1;
  }

  .bindings-list {
    @apply flex flex-col gap-2;
  }

  .binding-item {
    @apply flex border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 transition-all duration-200 ease-in-out bg-[#1a1f2e]/40;
  }
  .binding-item:hover {
    @apply border-[var(--accent2)] shadow-[0_4px_20px_rgba(0,0,0,0.3)];
  }

  .binding-content {
    @apply flex-1 flex flex-col gap-1 min-w-0;
  }

  .binding-row2 {
    @apply flex items-center justify-between gap-2;
  }

  .binding-title {
    @apply text-sm font-semibold text-[var(--text)];
  }

  .badge {
    @apply text-[10px] py-0.5 px-2 rounded-xl font-semibold uppercase;
  }

  .badge.delivery {
    @apply bg-cyan-500/15 text-cyan-300 border border-cyan-500/30;
  }

  .binding-targets {
    @apply text-xs text-[var(--text-muted)];
  }

  .binding-actions {
    @apply flex justify-end items-center gap-1.5 border-t border-white/[0.05] pt-2 mt-1;
  }

  .confirm-label {
    @apply text-[11px] text-[var(--text-muted)];
  }

  .btn-action {
    @apply bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs font-semibold cursor-pointer transition-all duration-150 ease-out;
  }
  .btn-action:hover {
    @apply bg-[var(--border)] border-[var(--text-muted)];
  }

  .btn-action.small {
    @apply p-1 px-2 text-[11px];
  }

  .btn-action.danger {
    @apply text-red-400 border-red-400/20;
  }
  .btn-action.danger:hover {
    @apply bg-red-400/10 border-red-400;
  }

  .btn-add-wide {
    @apply w-full bg-[var(--accent2)] text-white border-none rounded-[var(--radius)] py-1.5 text-xs font-bold cursor-pointer transition-all duration-150 ease-out mb-4 flex justify-center items-center gap-2 shadow-[0_2px_6px_rgba(56,189,248,0.15)];
  }
  .btn-add-wide:hover {
    @apply brightness-110 -translate-y-0.5 shadow-[0_4px_12px_rgba(56,189,248,0.35)];
  }
  .btn-add-wide:active {
    @apply translate-y-0;
  }

  .empty-state {
    @apply col-span-full flex flex-col items-center justify-center p-10 border-2 border-dashed border-[var(--border)] rounded-[var(--radius)] text-center bg-black/15;
  }

  .empty-icon {
    @apply text-[32px] mb-2;
  }

  .empty-state p {
    @apply text-xs text-[var(--text-muted)] m-0;
  }
</style>
