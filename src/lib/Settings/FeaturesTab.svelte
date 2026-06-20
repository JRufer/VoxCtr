<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  // Snippets editing
  let snippetList = $state<{key: string, val: string}[]>(
    Object.entries(cfg.features.snippets).map(([k, v]) => ({ key: k, val: v as string }))
  );

  function syncSnippets() {
    const newSnippets: Record<string, string> = {};
    for (const {key, val} of snippetList) {
      if (key.trim()) {
        newSnippets[key.trim()] = val.trim();
      }
    }
    cfg.features.snippets = newSnippets;
    markDirty();
  }

  function addEmptySnippetRow() {
    snippetList = [...snippetList, { key: "", val: "" }];
  }

  function removeSnippetRow(index: number) {
    snippetList = snippetList.filter((_, i) => i !== index);
    syncSnippets();
  }

  let customVocabString = $derived(
    cfg.features.custom_vocabulary ? cfg.features.custom_vocabulary.join(", ") : ""
  );

  function onCustomVocabChange(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    cfg.features.custom_vocabulary = target.value
      .split(",")
      .map(w => w.trim())
      .filter(w => w.length > 0);
    markDirty();
  }

  // Reusable Svelte action to auto-resize textareas dynamically to fit their contents
  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
    // Initial calculation on mount or state update
    const timer = setTimeout(resize, 0);

    return {
      update() {
        resize();
      },
      destroy() {
        clearTimeout(timer);
        node.removeEventListener("input", resize);
      }
    };
  }
</script>

<section>
  <h2>Features & Post-Processing</h2>

  <div class="field-group">
    <h3>Text cleanup</h3>
    <label class="field">
      <span>Remove filler words (uh, um, hmm…)</span>
      <input type="checkbox" bind:checked={cfg.features.remove_fillers} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Spoken punctuation ("period" → ".")</span>
      <input type="checkbox" bind:checked={cfg.features.spoken_punctuation} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Auto-format lists ("first, second, third")</span>
      <input type="checkbox" bind:checked={cfg.features.auto_format_lists} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Custom Dictionary</h3>
    <p class="hint">Provide a comma-separated list of words (e.g. names or jargon like "Waylin, Rufer, Enola, Kenz") that are hard to spell. The transcription process will correct these in the final text.</p>
    <textarea 
      class="custom-vocab-input"
      placeholder="e.g. Waylin, Rufer, Enola, Kenz"
      value={customVocabString}
      oninput={onCustomVocabChange}
      use:autoResize
    ></textarea>
  </div>

  <div class="field-group">
    <div class="field-label-row">
      <div style="display: flex; flex-direction: column;">
        <h3 style="margin-bottom: 0;">Snippets</h3>
        <p class="hint" style="margin-top: 4px;">Type a trigger word → it expands to the replacement text.</p>
      </div>
      <button class="btn-add-inline" type="button" onclick={addEmptySnippetRow}>
        ＋ Add Snippet
      </button>
    </div>

    <div class="dynamic-list">
      {#each snippetList as snippet, idx}
        <div class="dynamic-list-row">
          <input 
            type="text" 
            placeholder="Trigger word" 
            bind:value={snippetList[idx].key} 
            oninput={syncSnippets} 
            style="flex: 0.4;"
          />
          <span style="color: var(--text-muted);">→</span>
          <input 
            type="text" 
            placeholder="Expansion text" 
            bind:value={snippetList[idx].val} 
            oninput={syncSnippets} 
            style="flex: 1;"
          />
          <button class="btn-remove-inline" type="button" onclick={() => removeSnippetRow(idx)}>✕</button>
        </div>
      {/each}
      {#if snippetList.length === 0}
        <div class="empty-state" style="padding: 20px; grid-column: 1 / -1;">
          <p>No snippets defined.</p>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  @reference "tailwindcss";

  .custom-vocab-input {
    @apply w-full min-h-[80px] bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] resize-y mt-2 outline-none box-border transition-all duration-200 ease-out;
  }

  .custom-vocab-input:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .custom-vocab-input::placeholder {
    @apply text-[var(--text-muted)] opacity-50;
  }
</style>
