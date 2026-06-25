<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  interface TestResult {
    success: boolean;
    message: string;
    models: string[];
  }

  let testing = $state(false);
  let testStatus = $state<{ success: boolean; message: string } | null>(null);
  let availableModels = $state<string[]>([]);

  let ollamaModelOptions = $derived([
    ...availableModels.map(model => ({ value: model, label: model })),
    ...(cfg.ollama.model && !availableModels.includes(cfg.ollama.model)
      ? [{ value: cfg.ollama.model, label: `${cfg.ollama.model} (not found)` }]
      : [])
  ]);

  const ollamaModeOptions = [
    { value: "clean", label: "Clean (grammar fix)" },
    { value: "formal", label: "Formal" },
    { value: "casual", label: "Casual" },
    { value: "bullet", label: "Bullet points" },
    { value: "concise", label: "Concise (summarize)" }
  ];

  async function performTest() {
    testing = true;
    testStatus = null;
    try {
      const res = await invoke<TestResult>("test_ollama", {
        endpoint: cfg.ollama.endpoint,
        apiKey: cfg.ollama.api_key,
        timeoutSecs: cfg.ollama.timeout_secs,
      });
      testStatus = { success: res.success, message: res.message };
      if (res.success) {
        availableModels = res.models;
        // If our current model is empty but models are returned, auto-select the first one
        if (!cfg.ollama.model && res.models.length > 0) {
          cfg.ollama.model = res.models[0];
          markDirty();
        }
      }
    } catch (e: any) {
      testStatus = { success: false, message: e.toString() };
    } finally {
      testing = false;
    }
  }

  onMount(() => {
    // Try to silently probe/load models on mount
    performTest();
  });
</script>

<section>
  <h2>OpenAI API LLM Post-Processing</h2>

  <p class="hint">
    Connect to any OpenAI-compatible API server — a local Ollama instance, LM
    Studio, or a hosted provider. The URL defaults to a local server; point it
    anywhere you like and supply an API key when the server requires one.
  </p>

  <div class="field-group">
    <h3>Connection</h3>
    <label class="field">
      <span>API URL</span>
      <input type="text" bind:value={cfg.ollama.endpoint} onchange={markDirty} placeholder="http://localhost:11434" />
    </label>
    <label class="field">
      <span>API Key</span>
      <input type="password" bind:value={cfg.ollama.api_key} onchange={markDirty} placeholder="Required for remote servers (optional for localhost)" autocomplete="off" />
    </label>
    <label class="field">
      <span>Model (Default)</span>
      {#if availableModels.length > 0}
        <CustomSelect bind:value={cfg.ollama.model} options={ollamaModelOptions} onchange={markDirty} />
      {:else}
        <input type="text" bind:value={cfg.ollama.model} onchange={markDirty} placeholder="e.g. llama3.2:1b" />
      {/if}
    </label>
    <label class="field">
      <span>Timeout (seconds)</span>
      <input type="number" min="1" max="60" bind:value={cfg.ollama.timeout_secs} onchange={markDirty} />
    </label>

    <div class="action-row">
      <button class="btn-test" onclick={performTest} disabled={testing}>
        {testing ? "⏳ Testing..." : "🔌 Test Connection"}
      </button>
      {#if testStatus}
        <span class="status-msg {testStatus.success ? 'success' : 'error'}">
          {testStatus.message}
        </span>
      {/if}
    </div>
  </div>

  <div class="field-group">
    <h3>Default Processing Mode</h3>
    <label class="field">
      <span>Mode</span>
      <CustomSelect bind:value={cfg.ollama.mode} options={ollamaModeOptions} onchange={markDirty} />
    </label>
  </div>
</section>

<style>
  @reference "tailwindcss";

  .hint {
    @apply text-xs text-[var(--color-obsidian-300)] leading-relaxed mb-4 max-w-[520px];
  }
  .field.col {
    @apply flex-col items-start gap-1.5;
  }
  textarea {
    @apply w-full bg-[var(--bg)] border border-[var(--border)] rounded p-2 text-[var(--text)] text-[13px] font-mono resize-y;
  }
  .action-row {
    @apply flex flex-col items-center gap-2 mt-3.5 pt-3.5 border-t border-[var(--border)];
  }
  .btn-test {
    @apply w-full bg-[var(--accent)] border-none text-white rounded-[var(--radius)] py-1.5 text-xs cursor-pointer font-bold transition-all duration-150 ease-out shadow-[0_2px_6px_rgba(56,189,248,0.15)];
  }
  .btn-test:hover:not(:disabled) {
    @apply brightness-110 -translate-y-0.5 shadow-[0_4px_12px_rgba(56,189,248,0.3)];
  }
  .btn-test:active:not(:disabled) {
    @apply translate-y-0;
  }
  .btn-test:disabled {
    @apply opacity-60 cursor-not-allowed;
  }
  .status-msg {
    @apply text-xs font-semibold text-center;
  }
  .status-msg.success {
    @apply text-emerald-400;
  }
  .status-msg.error {
    @apply text-red-400;
  }
</style>
