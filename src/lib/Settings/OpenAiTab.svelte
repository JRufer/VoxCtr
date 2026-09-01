<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  interface TestResult {
    success: boolean;
    message: string;
    models: string[];
  }

  let testing = $state(false);
  let testStatus = $state<{ success: boolean; message: string } | null>(null);
  let availableModels = $state<string[]>([]);

  let openaiModelOptions = $derived([
    ...availableModels.map(model => ({ value: model, label: model })),
    ...(cfg.openai.model && !availableModels.includes(cfg.openai.model)
      ? [{ value: cfg.openai.model, label: `${cfg.openai.model} (not found)` }]
      : [])
  ]);

  const openaiModeOptions = [
    { value: "clean", label: "Clean (grammar fix)" },
    { value: "formal", label: "Formal" },
    { value: "casual", label: "Casual" },
    { value: "bullet", label: "Bullet points" },
    { value: "concise", label: "Concise (summarize)" },
    { value: "custom", label: "Custom (editable)" }
  ];

  // Preset system prompts — selecting a built-in preset fills (and locks) the
  // System/User Prompt fields below. The "custom" preset leaves them editable.
  const presetSystemPrompts: Record<string, string> = {
    clean: "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
    formal: "Rewrite the user's text in formal professional language. Return only the result.",
    casual: "Rewrite the user's text in casual conversational language. Return only the result.",
    bullet: "Convert the user's text to a bullet-point list. Return only the list.",
    concise: "Summarize the user's text concisely in 1-2 sentences. Return only the summary.",
  };

  // Built-in presets are read-only; only the "custom" preset lets the user edit
  // the system and user prompts.
  let isCustom = $derived(cfg.openai.mode === "custom");

  function applyPreset() {
    const preset = presetSystemPrompts[cfg.openai.mode];
    if (preset !== undefined) {
      // Switching to a built-in preset overwrites the prompts with its fixed
      // values; the user prompt is always the plain "{text}" passthrough.
      cfg.openai.system_prompt = preset;
      cfg.openai.user_prompt = "{text}";
    }
    markDirty();
  }

  let userPromptValid = $derived(cfg.openai.user_prompt.includes("{text}"));

  // Warn if the chosen default model isn't among the models the server reported.
  // Calling a model the server doesn't have (e.g. an un-pulled Ollama model)
  // makes chat completions fail with a 404 during dictation.
  let modelMissing = $derived(
    availableModels.length > 0 &&
    !!cfg.openai.model &&
    !availableModels.includes(cfg.openai.model)
  );

  async function performTest() {
    testing = true;
    testStatus = null;
    try {
      const res = await invoke<TestResult>("test_openai", {
        endpoint: cfg.openai.endpoint,
        apiKey: cfg.openai.api_key,
        timeoutSecs: cfg.openai.timeout_secs,
      });
      testStatus = { success: res.success, message: res.message };
      if (res.success) {
        availableModels = res.models;
        // If our current model is empty but models are returned, auto-select the first one
        if (!cfg.openai.model && res.models.length > 0) {
          cfg.openai.model = res.models[0];
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
    Connect to any OpenAI-compatible API server — a local server or a hosted
    provider. The URL defaults to a local server; point it anywhere you like and
    supply an API key when the server requires one.
  </p>

  <div class="field-group">
    <h3>Connection</h3>
    <label class="field">
      <span>API URL</span>
      <input type="text" bind:value={cfg.openai.endpoint} onchange={markDirty} placeholder="http://localhost:11434" />
    </label>
    <label class="field">
      <span>API Key</span>
      <input type="password" bind:value={cfg.openai.api_key} onchange={markDirty} placeholder="Required for remote servers (optional for localhost)" autocomplete="off" />
    </label>
    <label class="field">
      <span>Model (Default)</span>
      {#if availableModels.length > 0}
        <CustomSelect bind:value={cfg.openai.model} options={openaiModelOptions} onchange={markDirty} />
      {:else}
        <input type="text" bind:value={cfg.openai.model} onchange={markDirty} placeholder="e.g. llama3.2:1b" />
      {/if}
    </label>
    {#if modelMissing}
      <span class="validation-error">
        ⚠️ The model <code>{cfg.openai.model}</code> isn't available on this server. Post-processing will fail with a 404 — pick a model from the list above or pull it on the server.
      </span>
    {/if}
    <label class="field">
      <span>Timeout (seconds)</span>
      <input type="number" min="1" max="60" bind:value={cfg.openai.timeout_secs} onchange={markDirty} />
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
    <h3>Default Prompts</h3>
    <p class="hint">
      The system prompt tells the model how to transform your speech. The user
      prompt is the message sent to the model — it must contain
      <code>{"{text}"}</code>, which is replaced with whatever you dictate.
      Built-in presets are read-only; choose <strong>Custom</strong> to edit the
      prompts yourself.
    </p>

    <label class="field">
      <span>Load Preset</span>
      <CustomSelect bind:value={cfg.openai.mode} options={openaiModeOptions} onchange={applyPreset} />
    </label>

    <div class="field col">
      <span>System Prompt</span>
      <textarea
        rows="3"
        bind:value={cfg.openai.system_prompt}
        onchange={markDirty}
        readonly={!isCustom}
        class={isCustom ? "" : "locked"}
        placeholder="Leave empty to send no system message"
      ></textarea>
    </div>

    <div class="field col">
      <span>User Prompt</span>
      <textarea
        rows="2"
        bind:value={cfg.openai.user_prompt}
        onchange={markDirty}
        readonly={!isCustom}
        class={isCustom ? (userPromptValid ? "" : "invalid") : "locked"}
        placeholder="{'{text}'}"
      ></textarea>
      {#if !isCustom}
        <span class="field-hint">Select the <strong>Custom</strong> preset to edit the system and user prompts.</span>
      {:else if !userPromptValid}
        <span class="validation-error">⚠️ The user prompt must contain the <code>{"{text}"}</code> placeholder.</span>
      {:else}
        <span class="field-hint">Use <code>{"{text}"}</code> where your dictated speech should be inserted.</span>
      {/if}
    </div>
  </div>
</section>

<style>
  @reference "../../app.css";

  .hint {
    @apply text-xs text-[var(--color-obsidian-300)] leading-relaxed mb-4 max-w-[520px];
  }
  .field.col {
    @apply flex-col items-start gap-1.5;
  }
  textarea {
    @apply w-full bg-[var(--bg)] border border-[var(--border)] rounded p-2 text-[var(--text)] text-[13px] font-mono resize-y;
  }
  textarea.invalid {
    @apply border-red-500 ring-2 ring-red-500/20;
  }
  textarea.locked {
    @apply opacity-60 cursor-not-allowed bg-black/20;
  }
  .validation-error {
    @apply text-xs font-semibold text-red-400;
  }
  .field-hint {
    @apply text-[11px] text-[var(--color-obsidian-300)];
  }
  code {
    @apply font-mono text-[var(--color-accent-blue)] bg-white/5 px-1 rounded;
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
