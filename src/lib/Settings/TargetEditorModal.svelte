<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget } from "./routing-types";
  import CustomSelect from "./CustomSelect.svelte";

  let {
    editingTarget = $bindable(),
    isNew,
    existingTargets,
    isNested = false,
    onSave,
    onCancel
  }: {
    editingTarget: OutputTarget | null;
    isNew: boolean;
    existingTargets: OutputTarget[];
    isNested?: boolean;
    onSave: () => void;
    onCancel: () => void;
  } = $props();

  // Flat edit states to ensure absolute Svelte 5 reactivity for target processing overrides
  let editMcpArgsString = $state("");
  let editHttpTemplateString = $state("");
  let editWebhookTemplateString = $state("");

  // Chat target connection state
  interface ChatTestResult {
    success: boolean;
    message: string;
    models: string[];
  }
  let chatModels = $state<string[]>([]);
  let chatTesting = $state(false);
  let chatStatus = $state("");
  let chatStatusOk = $state(false);

  // Timestamp format preview for the file target. chrono decides what a
  // strftime pattern means, so the backend renders it and we show the result —
  // a preview when the pattern works, the reason when it does not.
  const DEFAULT_TIMESTAMP_FORMAT = "%Y-%m-%dT%H:%M:%SZ";
  let timestampPreview = $state("");
  let timestampError = $state<string | null>(null);

  $effect(() => {
    const fmt = editingTarget?.file_timestamp_format ?? "";
    if (!editingTarget || editingTarget.delivery !== "file" || !editingTarget.file_timestamp) {
      timestampError = null;
      timestampPreview = "";
      return;
    }

    let cancelled = false;
    invoke<string>("preview_timestamp_format", { format: fmt })
      .then(preview => {
        if (cancelled) return;
        timestampPreview = preview;
        timestampError = null;
      })
      .catch(e => {
        if (cancelled) return;
        timestampPreview = "";
        timestampError = `${e}`;
      });

    return () => {
      cancelled = true;
    };
  });

  // Derived validation for Terminal Command
  let commandError = $derived.by(() => {
    if (!editingTarget || editingTarget.delivery !== "exec") return null;
    const cmd = editingTarget.command || "";
    if (!cmd.includes("{text}")) {
      return "Command must contain '{text}' placeholder.";
    }
    return null;
  });

  // Derived JSON and placeholder validation for MCP custom arguments
  let mcpArgsError = $derived.by(() => {
    if (!editMcpArgsString.trim()) return null;
    try {
      JSON.parse(editMcpArgsString);
    } catch (e: any) {
      return "Invalid JSON format: " + e.message;
    }
    if (!editMcpArgsString.includes("{text}")) {
      return "Arguments must contain '{text}' placeholder.";
    }
    return null;
  });

  // Derived JSON and placeholder validation for HTTP custom template
  let httpTemplateError = $derived.by(() => {
    if (!editHttpTemplateString.trim()) return null;
    try {
      JSON.parse(editHttpTemplateString);
    } catch (e: any) {
      return "Invalid JSON format: " + e.message;
    }
    if (!editHttpTemplateString.includes("{text}")) {
      return "JSON template must contain '{text}' placeholder.";
    }
    return null;
  });

  // Derived JSON and placeholder validation for Webhook custom template
  let webhookTemplateError = $derived.by(() => {
    if (!editWebhookTemplateString.trim()) return null;
    try {
      JSON.parse(editWebhookTemplateString);
    } catch (e: any) {
      return "Invalid JSON format: " + e.message;
    }
    if (!editWebhookTemplateString.includes("{text}")) {
      return "JSON template must contain '{text}' placeholder.";
    }
    return null;
  });

  let chatUrlError = $derived.by(() => {
    if (!editingTarget || editingTarget.delivery !== "chat") return null;
    const url = (editingTarget.chat_url || "").trim();
    if (!url) return "A server base URL is required.";
    if (!/^https?:\/\//i.test(url)) return "URL must start with http:// or https://";
    return null;
  });

  let chatModelError = $derived.by(() => {
    if (!editingTarget || editingTarget.delivery !== "chat") return null;
    if (!(editingTarget.chat_model || "").trim()) return "A model name is required.";
    return null;
  });

  /// Probe the server and pull its model list, so the user picks rather than
  /// guessing the exact model id. Goes through Rust — the endpoint is a
  /// third-party server that need not send CORS headers to the webview.
  async function testChatConnection() {
    if (!editingTarget) return;
    chatTesting = true;
    try {
      const res = await invoke<ChatTestResult>("test_chat_target", { target: editingTarget });
      chatModels = res.models;
      chatStatus = res.message;
      chatStatusOk = res.success;
      if (res.success && !editingTarget.chat_model && res.models.length > 0) {
        editingTarget.chat_model = res.models[0];
      }
    } catch (e: any) {
      chatModels = [];
      chatStatus = String(e);
      chatStatusOk = false;
    } finally {
      chatTesting = false;
    }
  }

  async function resetChatConversation() {
    if (!editingTarget) return;
    try {
      const dropped = await invoke<number>("reset_chat_conversation", {
        targetId: editingTarget.id
      });
      chatStatus = `Conversation cleared (${dropped} message(s) discarded).`;
      chatStatusOk = true;
    } catch (e: any) {
      chatStatus = String(e);
      chatStatusOk = false;
    }
  }

  // Reusable Svelte action to auto-resize textareas dynamically to fit their contents
  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
    // Initial calculation on mount
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

  // Load flat states from editingTarget
  onMount(() => {
    if (editingTarget) {
      if (!editingTarget.processing) {
        editingTarget.processing = {};
      }
      if (!editingTarget.file_mode) {
        editingTarget.file_mode = "append";
      }
      // Targets saved before the timestamp format was configurable carry none.
      if (!editingTarget.file_timestamp_format) {
        editingTarget.file_timestamp_format = DEFAULT_TIMESTAMP_FORMAT;
      }
      // Targets saved before the Chat delivery type existed have no chat fields.
      editingTarget.chat_max_history ??= 20;
      editingTarget.chat_timeout_secs ??= 120;
      editingTarget.chat_reply_mode ||= "speak";
      editMcpArgsString = editingTarget.mcp_args ? JSON.stringify(editingTarget.mcp_args, null, 2) : '{\n  "text": "{text}"\n}';
      editHttpTemplateString = editingTarget.http_json_template ? JSON.stringify(editingTarget.http_json_template, null, 2) : '{\n  "text": "{text}"\n}';
      editWebhookTemplateString = editingTarget.webhook_json_template ? JSON.stringify(editingTarget.webhook_json_template, null, 2) : '{\n  "text": "{text}"\n}';
    }
  });

  function handleSave() {
    if (!editingTarget) return;
    if (editingTarget.id.trim() === "") {
      alert("Target ID cannot be empty.");
      return;
    }

    if (editingTarget.delivery === "file" && editingTarget.file_timestamp && timestampError) {
      alert("Validation Error: " + timestampError);
      return;
    }

    if (editingTarget.delivery === "exec") {
      if (commandError) {
        alert("Validation Error: " + commandError);
        return;
      }
    }

    if (editingTarget.delivery === "mcp") {
      if (mcpArgsError) {
        alert("Validation Error: " + mcpArgsError);
        return;
      }
      try {
        editingTarget.mcp_args = JSON.parse(editMcpArgsString);
      } catch (e) {
        alert("Invalid JSON format in MCP Custom Tool Arguments.");
        return;
      }
    }

    if (editingTarget.delivery === "http") {
      if (httpTemplateError) {
        alert("Validation Error: " + httpTemplateError);
        return;
      }
      try {
        editingTarget.http_json_template = JSON.parse(editHttpTemplateString);
      } catch (e) {
        alert("Invalid JSON format in HTTP Custom Post Body.");
        return;
      }
    }

    if (editingTarget.delivery === "webhook") {
      if (webhookTemplateError) {
        alert("Validation Error: " + webhookTemplateError);
        return;
      }
      try {
        editingTarget.webhook_json_template = JSON.parse(editWebhookTemplateString);
      } catch (e) {
        alert("Invalid JSON format in Webhook Custom Post Body.");
        return;
      }
    }

    if (editingTarget.delivery === "chat") {
      const err = chatUrlError || chatModelError;
      if (err) {
        alert("Validation Error: " + err);
        return;
      }
      // The number inputs hand back strings when typed into.
      editingTarget.chat_max_history = Number(editingTarget.chat_max_history) || 0;
      editingTarget.chat_timeout_secs = Number(editingTarget.chat_timeout_secs) || 120;
    }

    onSave();
  }
</script>

{#if editingTarget}
  <div class="modal-backdrop {isNested ? 'target-modal-backdrop' : ''}">
    <div class="modal glass animate-fade-in">
      <div class="modal-header">
        <h3>{isNew ? "Create Target" : "Edit Target"}</h3>
        <button class="close-btn" onclick={onCancel}>✕</button>
      </div>
      <div class="modal-body">
        {#if !isNested}
          <label class="field">
            <span>Target ID</span>
            <input
              type="text"
              bind:value={editingTarget.id}
              placeholder="e.g. obsidian_vault"
              disabled={!isNew}
            />
          </label>
        {/if}

        <label class="field">
          <span>Command Name</span>
          <input
            type="text"
            class="longer-display-label"
            bind:value={editingTarget.label}
            placeholder="e.g. Obsidian Notes"
          />
        </label>
        <p class="hint">
          Names this target in the app, and is what you say to send dictation here through a
          Voice Command Router target: &ldquo;VoxCtrl, <em>{editingTarget.label || "Obsidian Notes"}</em>,
          buy milk tomorrow&rdquo;. The Target ID works as a spoken name too, so pick something
          easy to say and easy for speech recognition to catch.
        </p>

        <label class="field col">
          <span class="field-title">Delivery System</span>
          <CustomSelect
            bind:value={editingTarget.delivery}
            options={[
              { value: "command", label: "Voice Command Router (VoxCtrl keyword)" },
              { value: "inject", label: "Inject Text Directly (Simulate keyboard)" },
              { value: "clipboard", label: "Save to Clipboard" },
              { value: "exec", label: "Execute Command" },
              { value: "file", label: "Write to File" },
              { value: "pipe", label: "FIFO Named Pipe" },
              { value: "socket", label: "TCP / Unix Socket" },
              { value: "dbus", label: "DBus Signal" },
              { value: "http", label: "HTTP Custom Client" },
              { value: "webhook", label: "Send Webhook Event" },
              { value: "mcp", label: "Call MCP Server Tool" },
              { value: "speak", label: "Speak Text Aloud (TTS)" },
              { value: "chat", label: "Chat with a Local LLM (Hermes / OpenAI-compatible)" }
            ]}
          />
        </label>

        <!-- Dynamic morphing options based on delivery type -->
        {#if editingTarget.delivery === "command"}
          <div class="morph-section mcp-container">
            <h5>Voice Command Router Settings</h5>
            <p class="hint">
              Types dictated text into your active application by default. If your dictation contains <code>VoxCtrl &lt;target_name&gt; &lt;text&gt;</code> (for example, <em>"VoxCtrl Notes Hello world"</em>), VoxCtrl dynamically reroutes the text to that target instead.
            </p>
          </div>
        {/if}
        {#if editingTarget.delivery === "exec"}
          <div class="morph-section mcp-container">
            <h5>Shell Executor Settings</h5>
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Terminal Command</span>
                <span class="field-tag">Shell command</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.command}
                placeholder="e.g. xdg-open {'{text}'}"
                class="full-width-input {commandError ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}"
              />
              <p class="hint">Use <code>{"{text}"}</code> inside the command string as a placeholder to substitute transcribed speech.</p>
              {#if commandError}
                <span class="validation-error-msg">
                  ⚠️ {commandError}
                </span>
              {/if}
            </div>
          </div>
        {/if}

        {#if editingTarget.delivery === "file"}
          <div class="morph-section">
            <h5>Local File Writer Settings</h5>
            <label class="field">
              <span>File Absolute Path</span>
              <input
                type="text"
                bind:value={editingTarget.file_path}
                placeholder="e.g. /home/user/notes.md"
              />
            </label>
            <label class="field">
              <span>Write Mode</span>
              <CustomSelect
                bind:value={editingTarget.file_mode}
                options={[
                  { value: "append", label: "Append (Add to end of file)" },
                  { value: "prepend", label: "Prepend (Add to beginning of file)" }
                ]}
              />
            </label>
            <label class="field">
              <span>Text Prefix</span>
              <input
                type="text"
                bind:value={editingTarget.file_prefix}
                placeholder="e.g. - "
              />
            </label>
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editingTarget.file_timestamp} />
              <span>Prepend date & time timestamp</span>
            </label>
            {#if editingTarget.file_timestamp}
              <label class="field">
                <span>Timestamp Format</span>
                <input
                  type="text"
                  bind:value={editingTarget.file_timestamp_format}
                  placeholder={DEFAULT_TIMESTAMP_FORMAT}
                  class={timestampError ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}
                />
              </label>
              <p class="hint">
                A <code>strftime</code> pattern, written in UTC. Common pieces:
                <code>%Y</code> year (2026), <code>%m</code> month (09),
                <code>%d</code> day (04), <code>%H</code> hour (17),
                <code>%M</code> minute, <code>%S</code> second,
                <code>%b</code> month name (Sep), <code>%a</code> weekday (Fri),
                <code>%p</code> AM/PM, <code>%Z</code> zone (UTC),
                <code>%%</code> a literal percent. Anything else is written as typed.
                Default: <code>{DEFAULT_TIMESTAMP_FORMAT}</code>
              </p>
              {#if timestampError}
                <span class="validation-error-msg">⚠️ {timestampError}</span>
              {:else if timestampPreview}
                <p class="hint">Each line starts with: <code>[{timestampPreview}]</code></p>
              {/if}
            {/if}
          </div>
        {/if}

        {#if editingTarget.delivery === "socket"}
          <div class="morph-section">
            <h5>Network Socket settings</h5>
            <label class="field">
              <span>Socket Host (TCP)</span>
              <input type="text" bind:value={editingTarget.socket_host} placeholder="localhost" />
            </label>
            <label class="field">
              <span>Socket Port (TCP)</span>
              <input type="number" bind:value={editingTarget.socket_port} placeholder="9000" />
            </label>
            <label class="field">
              <span>Unix Socket Path (Optional fallback)</span>
              <input type="text" bind:value={editingTarget.socket_unix} placeholder="/tmp/app.sock" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "pipe"}
          <div class="morph-section">
            <h5>FIFO Named Pipe settings</h5>
            <label class="field">
              <span>FIFO Pipe Path (Input)</span>
              <input type="text" bind:value={editingTarget.pipe_path} placeholder="e.g. /tmp/agent.in" />
            </label>
            <label class="field">
              <span>Response Pipe Path (Optional TTS Loopback)</span>
              <input type="text" bind:value={editingTarget.response_pipe} placeholder="e.g. /tmp/agent.out" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "dbus"}
          <div class="morph-section">
            <h5>Linux DBus emitter settings</h5>
            <label class="field">
              <span>Signal Method ID</span>
              <input type="text" bind:value={editingTarget.dbus_signal} placeholder="com.voxctrl.TextReady" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "http" || editingTarget.delivery === "webhook"}
          <div class="morph-section mcp-container">
            <h5>API Endpoint Client settings</h5>
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">REST endpoint URL</span>
              </div>
              {#if editingTarget.delivery === 'http'}
                <input
                  type="text"
                  bind:value={editingTarget.http_url}
                  placeholder="https://api.example.com/transcribe"
                  class="full-width-input"
                />
              {:else}
                <input
                  type="text"
                  bind:value={editingTarget.webhook_url}
                  placeholder="https://api.example.com/transcribe"
                  class="full-width-input"
                />
              {/if}
            </div>

            {#if editingTarget.delivery === "http"}
              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Method</span>
                </div>
              <CustomSelect
                bind:value={editingTarget.http_method}
                options={[
                  { value: "POST", label: "POST" },
                  { value: "GET", label: "GET" },
                  { value: "PUT", label: "PUT" }
                ]}
              />
              </div>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Custom Post Body</span>
                  <span class="field-tag">JSON Template</span>
                </div>
                <textarea
                  rows="4"
                  bind:value={editHttpTemplateString}
                  class={httpTemplateError ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}
                  placeholder={'{"text": "{text}"}'}
                  use:autoResize
                ></textarea>
                <p class="hint">Must be valid JSON. Use <code>{"{text}"}</code> to substitute transcribed speech.</p>
                {#if httpTemplateError}
                  <span class="validation-error-msg">
                    ⚠️ {httpTemplateError}
                  </span>
                {/if}
              </div>
            {:else}
              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Webhook Secret Token (HMAC)</span>
                </div>
                <input type="password" bind:value={editingTarget.webhook_secret} placeholder="Secret salt" class="full-width-input" />
              </div>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Custom Webhook Body</span>
                  <span class="field-tag">JSON Template</span>
                </div>
                <textarea
                  rows="4"
                  bind:value={editWebhookTemplateString}
                  class={webhookTemplateError ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}
                  placeholder={'{"text": "{text}"}'}
                  use:autoResize
                ></textarea>
                <p class="hint">Must be valid JSON. Use <code>{"{text}"}</code> to substitute transcribed speech.</p>
                {#if webhookTemplateError}
                  <span class="validation-error-msg">
                    ⚠️ {webhookTemplateError}
                  </span>
                {/if}
              </div>
            {/if}
          </div>
        {/if}

        {#if editingTarget.delivery === "mcp"}
          <div class="morph-section mcp-container">
            <h5>MCP Server Client settings</h5>
            
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Custom Socket/Pipe Path</span>
                <span class="field-tag">Optional override</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.mcp_path}
                placeholder="e.g. /tmp/voxctrl-mcp.sock"
                class="full-width-input"
              />
              <p class="hint">Leave empty to use defaults (<code>/tmp/voxctrl-mcp.sock</code> on Linux, <code>\\.\pipe\voxctrl-mcp</code> on Windows).</p>
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">MCP Tool Name</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.mcp_tool}
                placeholder="speak_text"
                class="full-width-input"
              />
            </div>

            <div class="field col mt-2">
              <div class="field-label-row">
                <span class="field-title">Custom Tool Arguments</span>
                <span class="field-tag">JSON Template</span>
              </div>
              <textarea
                rows="4"
                bind:value={editMcpArgsString}
                class={mcpArgsError ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}
                placeholder={'{"text": "{text}"}'}
                use:autoResize
              ></textarea>
              <p class="hint">Must be valid JSON. Use <code>{"{text}"}</code> to substitute transcribed speech.</p>
              {#if mcpArgsError}
                <span class="validation-error-msg">
                  ⚠️ {mcpArgsError}
                </span>
              {/if}
            </div>
          </div>
        {/if}

        {#if editingTarget.delivery === "chat"}
          <div class="morph-section mcp-container">
            <h5>Conversational LLM settings</h5>
            <p class="hint">
              Sends each dictation to an OpenAI-compatible
              <code>/v1/chat/completions</code> endpoint — Hermes, Ollama, llama.cpp —
              carrying the running conversation so the model keeps its context between turns.
            </p>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Server Base URL</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.chat_url}
                placeholder="http://localhost:8080"
                class="full-width-input {chatUrlError ? 'border-red-500! ring-2! ring-red-500/20!' : ''}"
              />
              <p class="hint">The <code>/v1</code> suffix is optional — it is added automatically.</p>
              {#if chatUrlError}
                <span class="validation-error-msg">⚠️ {chatUrlError}</span>
              {/if}
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Model</span>
                <button class="btn-action small" onclick={testChatConnection} disabled={chatTesting}>
                  {chatTesting ? "Connecting…" : "Test & fetch models"}
                </button>
              </div>
              {#if chatModels.length > 0}
                <CustomSelect
                  bind:value={editingTarget.chat_model}
                  options={chatModels.map((m) => ({ value: m, label: m }))}
                />
              {:else}
                <input
                  type="text"
                  bind:value={editingTarget.chat_model}
                  placeholder="e.g. hermes-4-14b"
                  class="full-width-input {chatModelError ? 'border-red-500! ring-2! ring-red-500/20!' : ''}"
                />
              {/if}
              <p class="hint">
                Press <strong>Test &amp; fetch models</strong> to confirm the server is reachable and
                pick from the ids it reports.
              </p>
              {#if chatModelError}
                <span class="validation-error-msg">⚠️ {chatModelError}</span>
              {/if}
              {#if chatStatus}
                <span class="chat-status {chatStatusOk ? 'ok' : 'bad'}">{chatStatus}</span>
              {/if}
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">API Key</span>
                <span class="field-tag">Optional</span>
              </div>
              <input
                type="password"
                bind:value={editingTarget.chat_api_key}
                placeholder="Usually not needed for a local server"
                class="full-width-input"
              />
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">System Prompt</span>
              </div>
              <textarea
                rows="3"
                bind:value={editingTarget.chat_system_prompt}
                placeholder="You are a concise voice assistant. Answer in one or two spoken sentences."
                use:autoResize
              ></textarea>
              <p class="hint">Sent ahead of every turn and never trimmed from the history window.</p>
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Reply Handling</span>
              </div>
              <CustomSelect
                bind:value={editingTarget.chat_reply_mode}
                options={[
                  { value: "speak", label: "Speak the reply aloud (TTS)" },
                  { value: "inject", label: "Type the reply into the focused window" },
                  { value: "clipboard", label: "Copy the reply to the clipboard" },
                  { value: "none", label: "Discard the reply (fire and forget)" }
                ]}
              />
              <p class="hint">Speaking the reply requires TTS to be enabled in the TTS tab.</p>
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Conversation Memory</span>
                <span class="field-tag">Messages</span>
              </div>
              <input
                type="number"
                min="0"
                bind:value={editingTarget.chat_max_history}
                placeholder="20"
                class="full-width-input"
              />
              <p class="hint">Most recent messages sent with each turn. <code>0</code> sends the whole conversation.</p>
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Request Timeout</span>
                <span class="field-tag">Seconds</span>
              </div>
              <input
                type="number"
                min="1"
                bind:value={editingTarget.chat_timeout_secs}
                placeholder="120"
                class="full-width-input"
              />
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Spoken Reset Phrase</span>
                <span class="field-tag">Optional</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.chat_reset_phrase}
                placeholder="e.g. new conversation"
                class="full-width-input"
              />
              <p class="hint">
                Saying exactly this clears the conversation instead of sending it to the model.
                Casing and punctuation are ignored.
              </p>
            </div>

            {#if !isNew}
              <div class="field col">
                <div class="chat-actions">
                  <button class="btn-action small" onclick={resetChatConversation}>
                    Reset conversation
                  </button>
                </div>
                <p class="hint">
                  Discards this target's stored conversation so the next dictation starts a new
                  thread. History is in-memory only and is never written to disk.
                </p>
              </div>
            {/if}
          </div>
        {/if}

        {#if editingTarget.delivery === "inject" || editingTarget.delivery === "command"}
          <div class="processing-toggles">
            <h5>Output</h5>
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editingTarget.strip_newlines} />
              <span>Strip newlines and carriage returns (Single-line mode)</span>
            </label>
          </div>
        {/if}
      </div>
      <div class="modal-footer">
        <button class="btn-action secondary" onclick={onCancel}>Cancel</button>
        <button class="btn-action primary" onclick={handleSave}>Done</button>
      </div>
    </div>
  </div>
{/if}

<style>
  @reference "../../app.css";

  .modal-backdrop {
    @apply fixed inset-0 bg-black/60 backdrop-blur-[4px] flex items-center justify-center z-[1000];
  }

  .target-modal-backdrop {
    @apply z-[1100]!;
  }

  .modal {
    @apply w-[90%] max-w-[500px] max-h-[85vh] rounded-[var(--radius)] border border-[var(--border)] flex flex-col shadow-[0_10px_30px_rgba(0,0,0,0.5)] bg-[#1e2436];
  }

  .modal-header {
    @apply flex justify-between items-center p-4 px-5 border-b border-[var(--border)];
  }

  .modal-header h3 {
    @apply m-0 text-[15px] font-bold;
  }

  .close-btn {
    @apply bg-transparent border-none text-[var(--text-muted)] text-base cursor-pointer;
  }
  .close-btn:hover {
    @apply text-[var(--text)];
  }

  .modal-body {
    @apply p-5 overflow-y-auto flex flex-col gap-3.5;
  }

  .modal-footer {
    @apply flex justify-end gap-2.5 p-4 px-5 border-t border-[var(--border)];
  }

  .morph-section {
    @apply bg-white/[0.02] border border-dashed border-[var(--border)] rounded-[var(--radius)] p-3 flex flex-col gap-2.5;
  }

  .morph-section h5, .processing-toggles h5 {
    @apply m-0 mb-1 text-[11px] font-bold uppercase text-[var(--accent2)];
  }

  .processing-toggles {
    @apply border-t border-[var(--border)] pt-3.5 flex flex-col gap-2.5;
  }

  .checkbox-field {
    @apply flex items-center gap-2 cursor-pointer text-xs text-[var(--text-muted)];
  }

  .checkbox-field input[type="checkbox"] {
    @apply cursor-pointer;
  }

  .checkbox-field:hover {
    @apply text-[var(--text)];
  }

  .btn-action {
    @apply bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs font-semibold cursor-pointer transition-all duration-150 ease-out;
  }
  .btn-action:hover {
    @apply bg-[var(--border)] border-[var(--text-muted)];
  }

  .btn-action.small {
    @apply p-1 px-2 text-[10px];
  }
  .btn-action:disabled {
    @apply opacity-50 cursor-not-allowed;
  }

  .chat-actions {
    @apply flex gap-2 items-center;
  }

  .chat-status {
    @apply block mt-1.5 text-[11px] leading-relaxed;
  }
  .chat-status.ok {
    @apply text-emerald-400;
  }
  .chat-status.bad {
    @apply text-red-400;
  }

  .btn-action.primary {
    @apply bg-[var(--accent)] text-white border-none;
  }
  .btn-action.primary:hover {
    @apply opacity-90;
  }

  .full-width-input {
    @apply w-full!;
  }

  textarea {
    @apply w-full bg-[var(--color-obsidian-950)] border border-[var(--border)] rounded-[var(--radius)] text-[var(--text)] p-2 px-3 text-[13px] font-mono resize-y outline-none shadow-[inset_0_2px_4px_rgba(0,0,0,0.2)] transition-all duration-150 ease-out;
  }
  textarea:hover {
    @apply border-white/15 bg-[var(--color-obsidian-900)];
  }
  textarea:focus {
    @apply border-[var(--accent2)] bg-[var(--color-obsidian-950)] shadow-[0_0_0_2px_rgba(56,189,248,0.15),_inset_0_2px_4px_rgba(0,0,0,0.2)];
  }

  .field.col {
    @apply flex-col items-start gap-1 w-full;
  }

  .mcp-container {
    @apply flex flex-col gap-4 py-1.5;
  }

  .field-label-row {
    @apply flex justify-between items-center w-full mb-0.5;
  }

  .field-title {
    @apply text-[13px] font-semibold text-[var(--text)];
  }

  .field-tag {
    @apply text-[9px] p-0.5 px-1.5 bg-[var(--accent2)]/8 text-[var(--accent2)] border border-[var(--accent2)]/25 rounded uppercase font-bold tracking-wide leading-none;
  }

  p.hint {
    @apply m-0 text-[11px] text-[var(--text-muted)] leading-relaxed;
  }

  p.hint code {
    @apply bg-[var(--color-obsidian-950)] text-[var(--color-accent-blue)] p-0.5 px-1 rounded font-mono text-[10px] border border-[var(--border)];
  }


  .validation-error-msg {
    @apply block mt-1 text-xs font-medium text-red-400 leading-normal;
  }

  .longer-display-label {
    @apply min-w-[255px]!;
  }

  .animate-fade-in {
    @apply animate-[fade-in_0.2s_cubic-bezier(0.16,1,0.3,1)];
  }

  @keyframes fade-in {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: none; }
  }
</style>
