<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget, HotkeyBinding } from "./routing-types";
  import TargetEditorModal from "./TargetEditorModal.svelte";
  import CustomSelect from "./CustomSelect.svelte";

  let targets = $state<OutputTarget[]>([]);
  let bindings = $state<HotkeyBinding[]>([]);
  let saving = $state(false);

  // Hotkey Modal state
  let editingBinding = $state<HotkeyBinding | null>(null);
  let isEditingBindingNew = $state(false);
  let recordingTarget = $state<"keys" | null>(null);
  let confirmDeleteBindingId = $state<string | null>(null);

  // Nested Target Modal state inside Hotkey Binding creation
  let editingTarget = $state<OutputTarget | null>(null);
  let isEditingTargetNew = $state(false);
  let targetIndexTriggeredNew = $state<number | null>(null);
  let activeDropdownIdx = $state<number | null>(null);

  // OpenAI LLM flat edit states
  let editOpenaiEnabled = $state(false);
  let editOpenaiModel = $state("");
  let editOpenaiMode = $state("custom");
  let editOpenaiPrompt = $state("");
  let editOpenaiSystemPrompt = $state("");

  // Helper to construct a canonical signature for a binding's key combination and gesture type
  function getBindingSignature(keys: string[], gesture: string): string {
    const sortedKeys = [...keys].sort().join(",");
    return `${gesture}:${sortedKeys}`;
  }

  // Keys alone, ignoring the gesture. double_tap and double_tap_hold on the
  // same keys are a supported pairing rather than a conflict, and on the portal
  // backend they are registered as a single system shortcut.
  function getTriggerSignature(keys: string[]): string {
    return [...keys].sort().join(",");
  }

  // Derived set of signatures that are shared by two or more bindings (regardless of disabled state)
  let conflictingSignatures = $derived.by(() => {
    const sigCounts = new Map<string, number>();
    for (const b of bindings) {
      if (b.keys && b.keys.length > 0) {
        const sig = getBindingSignature(b.keys, b.gesture);
        sigCounts.set(sig, (sigCounts.get(sig) || 0) + 1);
      }
    }
    const dups = new Set<string>();
    for (const [sig, count] of sigCounts.entries()) {
      if (count > 1) {
        dups.add(sig);
      }
    }
    return dups;
  });

  // Derived set of signatures that are shared by two or more active/enabled bindings
  let activeConflictingSignatures = $derived.by(() => {
    const activeSigCounts = new Map<string, number>();
    for (const b of bindings) {
      if (!b.disabled && b.keys && b.keys.length > 0) {
        const sig = getBindingSignature(b.keys, b.gesture);
        activeSigCounts.set(sig, (activeSigCounts.get(sig) || 0) + 1);
      }
    }
    const dups = new Set<string>();
    for (const [sig, count] of activeSigCounts.entries()) {
      if (count > 1) {
        dups.add(sig);
      }
    }
    return dups;
  });

  // Derived check: does the current edited binding in the modal conflict with another existing binding?
  // Excludes self-conflict: comparing a binding against the same binding ID never counts.
  // Also excludes the original keys of the binding being edited: re-using the same combination
  // that a binding already had is not a conflict with itself.
  let editingBindingConflict = $derived.by(() => {
    if (!editingBinding || !editingBinding.keys || editingBinding.keys.length === 0) return false;
    const editSig = getBindingSignature(editingBinding.keys, editingBinding.gesture);
    // Only flag as a conflict if another *different* binding (different ID) uses the same signature
    return bindings.some(b => b.id !== editingBinding!.id && getBindingSignature(b.keys, b.gesture) === editSig);
  });

  // Reusable Svelte action to auto-resize textareas dynamically to fit their contents
  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
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

  // How shortcuts are actually reaching the app. Shown in the UI because the
  // answer decides what VoxCtrl can see: on the portal it is told only that its
  // own shortcut fired, and the keys are chosen and owned by the desktop.
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
    detail: string;
    // KDE registers portal shortcuts disabled and gives no way to check that
    // over D-Bus, so this is a standing warning on KDE rather than a detected
    // fact about any one shortcut. See docs/hotkeys.md.
    needs_manual_enable: boolean;
    manual_enable_hint: string | null;
  };

  let hotkeyStatus = $state<HotkeyStatus | null>(null);
  let openingShortcutSettings = $state(false);
  let openShortcutSettingsError = $state<string | null>(null);
  let retryingShortcuts = $state(false);
  let retryShortcutsError = $state<string | null>(null);
  let backendBannerExpanded = $state(false);
  let manualEnableExpanded = $state(false);

  async function refreshHotkeyStatus() {
    try {
      hotkeyStatus = await invoke<HotkeyStatus>("check_hotkey_status");
    } catch (e) {
      console.error("Failed to read hotkey status:", e);
    }
  }

  async function openShortcutSettings() {
    openingShortcutSettings = true;
    openShortcutSettingsError = null;
    try {
      await invoke("open_shortcut_settings");
    } catch (e: any) {
      openShortcutSettingsError = e?.toString() ?? "Could not open shortcut settings.";
    } finally {
      openingShortcutSettings = false;
    }
  }

  async function handleRetryShortcuts() {
    retryingShortcuts = true;
    retryShortcutsError = null;
    try {
      await invoke("retry_portal_shortcuts");
      await refreshHotkeyStatus();
    } catch (e: any) {
      retryShortcutsError = e?.toString() ?? "Failed to request shortcut approval from desktop.";
      await refreshHotkeyStatus();
    } finally {
      retryingShortcuts = false;
    }
  }

  // The compositor may rename or reject a shortcut when bindings are saved, so
  // the panel is refreshed after every write rather than only on mount.
  let shortcutByBinding = $derived.by(() => {
    const map = new Map<string, BoundShortcut>();
    for (const s of hotkeyStatus?.shortcuts ?? []) {
      for (const id of s.binding_ids) map.set(id, s);
    }
    return map;
  });

  onMount(async () => {
    targets = await invoke<OutputTarget[]>("get_targets");
    bindings = await invoke<HotkeyBinding[]>("get_bindings");
    await refreshHotkeyStatus();
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
      await refreshHotkeyStatus();
    } catch (e) {
      console.error("Failed to save bindings:", e);
    }
  }

  function formatBindingTargets(b: HotkeyBinding) {
    const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
    return ids.map(id => {
      const t = targets.find(target => target.id === id);
      return t ? t.label : (id === "default" ? "Focused Window" : id);
    }).join(", ");
  }

  function getTargetLabel(id: string): string {
    const t = targets.find(target => target.id === id);
    return t ? `${t.label} (${t.delivery})` : (id === "default" ? "Focused Window" : id);
  }

  // --- CRUD Hotkey Bindings ---
  function addNewBinding() {
    if (targets.length === 0) {
      alert("Please create at least one Output Target before making a hotkey binding.");
      return;
    }
    isEditingBindingNew = true;
    keysCheck = null;
    originalBindingKeys = [];
    editOpenaiEnabled = false;
    editOpenaiModel = "";
    editOpenaiMode = "custom";
    editOpenaiPrompt = "";
    editOpenaiSystemPrompt = "";
    editingBinding = {
      id: "binding_" + Math.random().toString(36).substring(2, 6),
      label: "New Binding",
      keys: ["KEY_LEFTMETA", "KEY_SPACE"],
      gesture: "hold",
      target_id: targets[0].id,
      target_ids: [targets[0].id],
      tap_ms: 300,
      hold_threshold_ms: 200,
      disabled: false,
      openai_enabled: false,
      openai_model: "",
      openai_mode: "custom",
      openai_prompt: "",
      openai_system_prompt: "",
    };
  }

  function editBinding(b: HotkeyBinding) {
    isEditingBindingNew = false;
    keysCheck = null;
    // Track the keys the binding opened with so we can detect a no-op re-record
    // (user captures the exact same combo the binding already had) and avoid
    // showing a stale rejection from a different capture earlier in the session.
    originalBindingKeys = [...b.keys];
    const clone = JSON.parse(JSON.stringify(b));
    if (!clone.target_ids) {
      clone.target_ids = clone.target_id ? [clone.target_id] : [];
    }
    editOpenaiEnabled = clone.openai_enabled === true;
    editOpenaiModel = clone.openai_model || "";
    editOpenaiMode = clone.openai_mode || "custom";
    editOpenaiPrompt = clone.openai_prompt || "";
    editOpenaiSystemPrompt = clone.openai_system_prompt || "";
    editingBinding = clone;
  }

  async function toggleBindingDisabled(id: string) {
    bindings = bindings.map(b => b.id === id ? { ...b, disabled: !b.disabled } : b);
    await persistBindings();
  }

  function addBindingTarget() {
    if (!editingBinding) return;
    if (!editingBinding.target_ids) {
      editingBinding.target_ids = [editingBinding.target_id];
    }
    const available = targets.find(t => !editingBinding!.target_ids!.includes(t.id));
    const nextId = available ? available.id : targets[0].id;
    editingBinding.target_ids = [...editingBinding.target_ids, nextId];
  }

  function removeBindingTarget(index: number) {
    if (!editingBinding || !editingBinding.target_ids) return;
    editingBinding.target_ids = editingBinding.target_ids.filter((_, i) => i !== index);
  }

  async function saveBindingModal() {
    if (!editingBinding) return;
    if (editingBinding.id.trim() === "") {
      alert("Binding ID cannot be empty.");
      return;
    }
    if (editingBinding.keys.length === 0) {
      alert("Please capture at least one hotkey before saving.");
      return;
    }

    // Re-checked here, not just at capture time: a binding created by an older
    // VoxCtrl (or on a machine without the desktop portal) can be sitting in
    // the editor without ever passing through the recorder.
    //
    // Exception: if the keys haven't changed from what the binding opened with,
    // skip the structural check. The binding was already saved with these keys,
    // so rejecting them here would be a false error — especially when the user
    // is only editing the label or output target.
    const finalKeysSorted = [...editingBinding.keys].sort().join(",");
    const origKeysSorted = [...originalBindingKeys].sort().join(",");
    const keysUnchanged = !isEditingBindingNew && finalKeysSorted === origKeysSorted;

    if (!keysUnchanged) {
      try {
        const check = await invoke<KeysCheck>("check_hotkey_keys", {
          keys: editingBinding.keys,
        });
        if (!check.accepted) {
          keysCheck = check;
          alert(check.message ?? "This key combination cannot be used as a shortcut.");
          return;
        }
      } catch (e) {
        console.error("Failed to validate hotkey keys before saving:", e);
      }
    }
    if (editOpenaiEnabled) {
      if (editOpenaiMode === "custom") {
        if (!editOpenaiPrompt.includes("{text}")) {
          alert("LLM Configuration Error:\nYour custom prompt template MUST contain the '{text}' placeholder so the model knows where to insert the transcribed text.\n\nExample:\nwrite a haiku about {text}");
          return;
        }
      }
    }

    editingBinding.openai_enabled = editOpenaiEnabled;
    editingBinding.openai_model = editOpenaiModel;
    editingBinding.openai_mode = editOpenaiMode;
    editingBinding.openai_prompt = editOpenaiPrompt;
    editingBinding.openai_system_prompt = editOpenaiSystemPrompt;

    if (editingBinding.target_ids && editingBinding.target_ids.length > 0) {
      editingBinding.target_ids = editingBinding.target_ids.filter(id => id.trim() !== "");
      if (editingBinding.target_ids.length === 0) {
        alert("Please assign at least one Output Target.");
        return;
      }
      const uniqueIds = new Set(editingBinding.target_ids);
      if (uniqueIds.size !== editingBinding.target_ids.length) {
        alert("Duplicate targets detected.\nYou cannot assign the same Output Target multiple times to a single keybind.");
        return;
      }
      editingBinding.target_id = editingBinding.target_ids[0];
    } else {
      editingBinding.target_id = "";
      editingBinding.target_ids = [];
    }

    if (isEditingBindingNew) {
      if (bindings.some(b => b.id === editingBinding!.id)) {
        alert("Binding with this ID already exists.");
        return;
      }
      bindings = [...bindings, editingBinding];
    } else {
      bindings = bindings.map(b => b.id === editingBinding!.id ? editingBinding! : b);
    }
    editingBinding = null;
    recordingTarget = null;
    await persistBindings();
  }

  async function deleteBinding(id: string) {
    bindings = bindings.filter(b => b.id !== id);
    await persistBindings();
  }

  // --- Keyboard Event Capture / Recorder ---
  function mapBrowserKeyToEvdev(key: string, code: string): string {
    const codeUpper = code.toUpperCase();
    if (key === "Control") return "KEY_LEFTCTRL";
    if (key === "Alt") return "KEY_LEFTALT";
    if (key === "Shift") return "KEY_LEFTSHIFT";
    if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";

    if (codeUpper === "SPACE") return "KEY_SPACE";
    if (codeUpper === "ENTER") return "KEY_ENTER";
    if (codeUpper === "ESCAPE" || codeUpper === "ESC") return "KEY_ESC";
    if (codeUpper === "TAB") return "KEY_TAB";
    if (codeUpper === "BACKSPACE") return "KEY_BACKSPACE";
    if (codeUpper === "DELETE") return "KEY_DELETE";

    if (/^KEY[A-Z]$/.test(codeUpper)) {
      return `KEY_${codeUpper.slice(3)}`;
    }
    if (codeUpper.startsWith("KEY")) return codeUpper;
    if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
    if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
    if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;

    if (key.length === 1) return `KEY_${key.toUpperCase()}`;
    return `KEY_${codeUpper}`;
  }

  let currentlyPressedKeys = $state<string[]>([]);
  // The keys the binding had when the edit modal was opened. Used to detect
  // a no-op re-record so we don't show stale rejection state when the user
  // re-captures the exact same combo the binding already had.
  let originalBindingKeys = $state<string[]>([]);

  // Suppress all hotkey gestures while the recorder is active so the user
  // cannot accidentally trigger dictation while pressing keys for a new binding.
  $effect(() => {
    invoke("set_hotkeys_inhibited", { inhibited: recordingTarget === "keys" }).catch(
      (e: unknown) => console.error("Failed to set hotkeys inhibited:", e),
    );
  });

  // Result of validating the last captured combination. The rules live in Rust
  // (`voxctrl_hotkeys::accelerator`) and are reached over IPC, so the recorder
  // and the portal registration cannot disagree about what is bindable.
  type KeysCheck = {
    accepted: boolean;
    enforced: boolean;
    accelerator: string | null;
    problem: string | null;
    message: string | null;
  };
  let keysCheck = $state<KeysCheck | null>(null);

  // Presentational only — used for the live "keep going" hint while keys are
  // still held, and for the badge on saved bindings. The authoritative verdict
  // always comes from the backend, so drift here cannot let an invalid
  // combination through.
  const MODIFIER_KEYS = new Set([
    "KEY_LEFTCTRL", "KEY_RIGHTCTRL",
    "KEY_LEFTALT", "KEY_RIGHTALT",
    "KEY_LEFTSHIFT", "KEY_RIGHTSHIFT",
    "KEY_LEFTMETA", "KEY_RIGHTMETA",
  ]);

  function isModifiersOnly(keys: string[]): boolean {
    return keys.length > 0 && keys.every(k => MODIFIER_KEYS.has(k));
  }

  // Whether a bare-modifier binding is actually broken on this machine, as
  // opposed to merely fragile. Only the portal cannot deliver them.
  let portalEnforced = $derived(
    hotkeyStatus === null ||
    hotkeyStatus.backend === "portal" ||
    hotkeyStatus.backend === "starting",
  );

  // A shortcut needs a regular key. Say so while the user is still holding
  // modifiers, rather than letting them lift and get rejected.
  let liveCaptureHint = $derived.by(() => {
    if (recordingTarget !== "keys") return null;
    if (!isModifiersOnly(currentlyPressedKeys)) return null;
    if (!portalEnforced) return null;
    return "Keep holding and add a regular key — modifiers alone cannot be a shortcut.";
  });

  /// Validate a captured combination and commit it if the backend allows.
  async function commitCapture(keys: string[]): Promise<void> {
    if (!editingBinding || keys.length === 0) return;

    // If the user re-recorded the exact same combination the binding already
    // had, treat it as a no-op: clear any stale rejection from a previous
    // capture in this session and keep the current keys unchanged.
    const sortedNew = [...keys].sort().join(",");
    const sortedOrig = [...originalBindingKeys].sort().join(",");
    if (sortedNew === sortedOrig) {
      keysCheck = null;
      editingBinding.keys = [...keys];
      return;
    }

    let check: KeysCheck;
    try {
      check = await invoke<KeysCheck>("check_hotkey_keys", { keys });
    } catch (e) {
      console.error("Failed to validate hotkey keys:", e);
      // A validation call that fails must not silently discard the user's
      // capture; the save-time check still guards the real constraint.
      editingBinding.keys = [...keys];
      return;
    }
    keysCheck = check;
    if (check.accepted && editingBinding) {
      editingBinding.keys = [...keys];
    }
  }

  function handleRecordKeyDown(e: KeyboardEvent) {
    if (!recordingTarget || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    const evdevKey = mapBrowserKeyToEvdev(e.key, e.code);
    if (recordingTarget === "keys") {
      if (!currentlyPressedKeys.includes(evdevKey)) {
        currentlyPressedKeys = [...currentlyPressedKeys, evdevKey];
      }
      if (e.key === "Escape") {
        const captured = [...currentlyPressedKeys];
        currentlyPressedKeys = [];
        recordingTarget = null;
        void commitCapture(captured);
      }
    }
  }

  function handleRecordKeyUp(e: KeyboardEvent) {
    if (!recordingTarget || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    if (recordingTarget === "keys") {
      const captured = [...currentlyPressedKeys];
      currentlyPressedKeys = [];
      // Deliberately leaves recording mode even when the capture is refused.
      // Dropping straight back into "press your combination now" would hide the
      // shortcut the binding still has, and the user needs to see that their
      // working keybind survived the rejection.
      recordingTarget = null;
      void commitCapture(captured);
    }
  }

  function handleRecordKeyBlur() {
    if (!recordingTarget) return;
    if (recordingTarget === "keys" && currentlyPressedKeys.length > 0) {
      const captured = [...currentlyPressedKeys];
      currentlyPressedKeys = [];
      void commitCapture(captured);
    }
    recordingTarget = null;
  }

  // --- Nested Target Modal Handling ---
  function triggerNewTarget(idx: number) {
    targetIndexTriggeredNew = idx;
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

  function cancelTargetModal() {
    editingTarget = null;
    targetIndexTriggeredNew = null;
  }

  async function saveTargetModal() {
    if (!editingTarget) return;
    if (targets.some(t => t.id === editingTarget!.id)) {
      alert("Target with this ID already exists.");
      return;
    }

    targets = [...targets, editingTarget];

    if (targetIndexTriggeredNew !== null && editingBinding) {
      if (!editingBinding.target_ids) {
        editingBinding.target_ids = [];
      }
      editingBinding.target_ids[targetIndexTriggeredNew] = editingTarget.id;
    }

    editingTarget = null;
    targetIndexTriggeredNew = null;
    await persistTargets();
  }
</script>

<section class="hotkeys-section">
  <div class="section-header">
    <div>
      <h2>Hotkey Bindings</h2>
      <p class="description">Bind keyboard combinations to your output targets. Each hotkey supports customizable triggers (double-taps, holding keys, etc.)</p>
    </div>
  </div>

  {#if hotkeyStatus}
    <div
      class="backend-banner"
      class:private={hotkeyStatus.is_private}
      class:warn={!hotkeyStatus.is_private && hotkeyStatus.is_active}
      class:broken={!hotkeyStatus.is_active}
    >
      <button
        type="button"
        class="banner-toggle"
        onclick={() => backendBannerExpanded = !backendBannerExpanded}
        aria-expanded={backendBannerExpanded}
        aria-label="Toggle shortcut backend details"
      >
        <span class="backend-icon">
          {hotkeyStatus.is_private ? "🔒" : hotkeyStatus.is_active ? "⚠️" : "⛔"}
        </span>
        <strong class="banner-title">
          {#if hotkeyStatus.backend === "portal"}
            Your desktop is handling these shortcuts
          {:else if hotkeyStatus.backend === "windows_hook"}
            Global shortcuts are active
          {:else if hotkeyStatus.backend === "evdev"}
            Reading input devices directly
          {:else if hotkeyStatus.backend === "starting"}
            Starting up…
          {:else}
            Global shortcuts are not available
          {/if}
        </strong>
        <svg
          class="banner-chevron"
          class:expanded={backendBannerExpanded}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="6 9 12 15 18 9"></polyline>
        </svg>
      </button>
      {#if backendBannerExpanded}
        <div class="banner-content">
          <span class="backend-detail">{hotkeyStatus.detail}</span>
          {#if hotkeyStatus.backend === "portal"}
            <span class="backend-detail">
              Your desktop decides which keys VoxCtrl may claim, and may ask you to confirm them.
              If a shortcut below shows different keys than you chose, that is your desktop's
              choice and it wins.
            </span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if hotkeyStatus?.needs_manual_enable}
    <div class="manual-enable-banner">
      <button
        type="button"
        class="banner-toggle"
        onclick={() => manualEnableExpanded = !manualEnableExpanded}
        aria-expanded={manualEnableExpanded}
        aria-label="Toggle manual shortcut enable details"
      >
        <span class="backend-icon">🔧</span>
        <strong class="banner-title">One more step on KDE: enable these shortcuts yourself</strong>
        <svg
          class="banner-chevron"
          class:expanded={manualEnableExpanded}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="6 9 12 15 18 9"></polyline>
        </svg>
      </button>
      {#if manualEnableExpanded}
        <div class="banner-content">
          <span class="backend-detail">{hotkeyStatus.manual_enable_hint}</span>
          <div class="manual-enable-actions">
            <button
              class="btn-action primary"
              onclick={openShortcutSettings}
              disabled={openingShortcutSettings}
            >
              {openingShortcutSettings ? "Opening…" : "Open Shortcut Settings"}
            </button>
          </div>
          {#if openShortcutSettingsError}
            <span class="backend-detail error">{openShortcutSettingsError}</span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <button class="btn-add-wide" onclick={addNewBinding}>
    ＋ Add New Hotkey Binding
  </button>

  {#if conflictingSignatures.size > 0}
    <div class="conflict-alert-banner animate-fade-in">
      <span class="warning-icon">⚠️</span>
      <span class="alert-message">
        <strong>Conflict detected:</strong> Some key bindings share the same key combination and gesture type. Disabling one of the conflicting binds will fix the problem.
      </span>
    </div>
  {/if}

  <div class="bindings-list">
    {#each bindings as b}
      <div
        class="binding-item glass"
        class:disabled={b.disabled}
        class:has-conflict={b.keys && b.keys.length > 0 && conflictingSignatures.has(getBindingSignature(b.keys, b.gesture))}
        class:active-conflict={!b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture))}
      >
        {#if !b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture))}
          <span class="conflict-marker">CONFLICT</span>
        {/if}
        <div class="binding-content">
          <div class="binding-header-row">
            <div
              class="binding-title"
              class:has-conflict={!b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture))}
            >
              {b.label || b.id}
            </div>
            {#if b.openai_enabled}
              <span class="badge openai">LLM</span>
            {/if}
          </div>
          <div class="binding-row2">
            <div class="keys-display">
              {#each b.keys as k}
                <kbd>{k.replace("KEY_", "")}</kbd>
              {/each}
            </div>
            <span class="badge gesture">{b.gesture}</span>
            {#if portalEnforced && isModifiersOnly(b.keys)}
              <span
                class="badge unbound"
                title="Modifiers on their own cannot be registered with your desktop. Edit this binding and add a regular key."
              >
                needs a regular key
              </span>
            {/if}
            {#if hotkeyStatus?.backend === "portal" && !b.disabled && shortcutByBinding.get(b.id)}
              {@const sc = shortcutByBinding.get(b.id)!}
              {#if !sc.bound}
                <span class="badge unbound" title="Your desktop did not accept this shortcut.">
                  not bound by your desktop
                </span>
              {:else if sc.trigger_description}
                <span class="badge bound" title="The keys your desktop actually assigned.">
                  desktop: {sc.trigger_description}
                </span>
              {/if}
            {/if}
          </div>
          <div class="binding-targets">{formatBindingTargets(b)}</div>
          <div class="binding-actions">
            <button class="btn-action small" onclick={() => toggleBindingDisabled(b.id)}>
              {b.disabled ? "Enable" : "Disable"}
            </button>
            <button class="btn-action small" onclick={() => editBinding(b)}>Edit</button>
            {#if confirmDeleteBindingId === b.id}
              <span class="confirm-label">Delete?</span>
              <button class="btn-action small danger" onclick={() => { deleteBinding(b.id); confirmDeleteBindingId = null; }}>Yes</button>
              <button class="btn-action small" onclick={() => confirmDeleteBindingId = null}>No</button>
            {:else}
              <button class="btn-action small danger" onclick={() => confirmDeleteBindingId = b.id}>Delete</button>
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="empty-state">
        <span class="empty-icon">⌨</span>
        <p>No keyboard bindings defined. Create a binding to link a physical hotkey to a routing target!</p>
      </div>
    {/each}
  </div>
</section>

<!-- ========================================== -->
<!-- MODAL: Hotkey Binding Editor               -->
<!-- ========================================== -->
{#if editingBinding}
  <div class="modal-backdrop">
    <div class="modal glass animate-fade-in">
      <div class="modal-header">
        <h3>{isEditingBindingNew ? "Create Hotkey Binding" : "Edit Hotkey Binding"}</h3>
        <button class="close-btn" onclick={() => { editingBinding = null; recordingTarget = null; }}>✕</button>
      </div>
      <div class="modal-body">
        <label class="field">
          <span>Binding Identifier</span>
          <input
            type="text"
            bind:value={editingBinding.id}
            placeholder="e.g. dictation_shortcut"
            disabled={!isEditingBindingNew}
          />
        </label>

        <label class="field">
          <span>Display Label</span>
          <input
            type="text"
            class="longer-display-label"
            bind:value={editingBinding.label}
            placeholder="e.g. Global Speech to Text shortcut"
          />
        </label>

        <div class="field col">
          <div class="field-label-row">
            <span class="field-title">Assign to Output Targets</span>
            <button class="btn-add-inline" type="button" onclick={addBindingTarget}>
              ＋ Add Target
            </button>
          </div>

          <div class="target-selects-list">
            {#each editingBinding.target_ids || [] as tid, idx}
              <div class="target-select-row">
                <div class="custom-select-wrapper">
                  <button
                    type="button"
                    class="custom-select-trigger"
                    onclick={() => activeDropdownIdx = activeDropdownIdx === idx ? null : idx}
                  >
                    {getTargetLabel(tid)}
                  </button>
                  {#if activeDropdownIdx === idx}
                    <div class="custom-select-backdrop" role="presentation" onclick={() => activeDropdownIdx = null}></div>
                    <div class="custom-dropdown-menu">
                      {#each targets as t}
                        <button
                          type="button"
                          class="custom-dropdown-item"
                          disabled={editingBinding!.target_ids!.includes(t.id) && t.id !== tid}
                          onclick={(e) => {
                            e.preventDefault();
                            e.stopPropagation();
                            editingBinding!.target_ids![idx] = t.id;
                            activeDropdownIdx = null;
                          }}
                        >
                          {t.label} ({t.delivery})
                        </button>
                      {/each}
                      <button
                        type="button"
                        class="custom-dropdown-item create-new"
                        onclick={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          activeDropdownIdx = null;
                          triggerNewTarget(idx);
                        }}
                      >
                        <span class="plus-icon">＋</span> Create New Target
                      </button>
                    </div>
                  {/if}
                </div>
                {#if idx > 0}
                  <button
                    class="btn-remove-inline"
                    type="button"
                    onclick={() => removeBindingTarget(idx)}
                    title="Remove Target"
                  >
                    ✕
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        </div>

        <label class="field col">
          <span class="field-title">Input Gesture Style</span>
          <CustomSelect
            bind:value={editingBinding.gesture}
            options={[
              { value: "hold", label: "Hold keys to dictate (Release to transcribe)" },
              { value: "toggle", label: "Tap once to start recording, tap again to finish" },
              { value: "double_tap", label: "Double-tap hotkey to trigger recording" },
              { value: "double_tap_hold", label: "Double-tap & hold keys to dictate (Release to transcribe)" }
            ]}
          />
        </label>

        <!-- Timings dynamic displays -->
        {#if editingBinding.gesture === "hold" || editingBinding.gesture === "double_tap_hold"}
          <label class="field morph-section">
            <span>Hold Threshold (ms)</span>
            <input type="number" bind:value={editingBinding.hold_threshold_ms} placeholder="200" />
            <span class="hint">
              {editingBinding.gesture === "double_tap_hold"
                ? "How long the second tap must stay held before recording starts. Keeps a plain double-tap from being read as a double-tap-and-hold."
                : "Minimum duration to keep keys pressed to count as a 'hold'."}
            </span>
          </label>
        {/if}

        {#if editingBinding.gesture === "double_tap" || editingBinding.gesture === "double_tap_hold"}
          <label class="field morph-section">
            <span>Double-Tap Interval (ms)</span>
            <input type="number" bind:value={editingBinding.tap_ms} placeholder="300" />
            <span class="hint">
              Longest gap between releasing the first tap and pressing the second. Raise it if
              double-taps get missed; lower it if they fire when you did not mean them to.
            </span>
          </label>
        {/if}

        <!-- Premium Hotkey Recording Widget -->
        <div class="border-t border-white/5 pt-[14px] flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <h5 class="text-[11px] font-bold uppercase text-accent-blue tracking-[0.06em]">
              Hotkey Keybind Selection
            </h5>
            <div
              class={[
                "border-2 rounded-desktop p-4 text-center cursor-pointer outline-none transition-all duration-200 flex flex-col items-center justify-center min-h-[70px]",
                recordingTarget === "keys"
                  ? "border-solid border-[#f43f5e] bg-[rgba(244,63,94,0.05)] animate-border-pulse"
                  : (editingBindingConflict
                    ? "border-solid border-[#fbbf24] bg-[rgba(251,191,36,0.05)] hover:border-[#fbbf24]/80"
                    : "border-dashed border-white/5 bg-black/25 hover:border-accent-blue hover:bg-black/35 focus:border-accent-blue focus:bg-black/35")
              ].join(" ")}
              tabindex="0"
              role="button"
              aria-label="Base Hotkey recorder input"
              onclick={() => recordingTarget = "keys"}
              onfocus={() => recordingTarget = "keys"}
              onblur={() => handleRecordKeyBlur()}
              onkeydown={handleRecordKeyDown}
              onkeyup={handleRecordKeyUp}
            >
              {#if recordingTarget === "keys"}
                <div class="flex items-center gap-[10px]">
                  <span class="w-2 h-2 bg-accent-blue rounded-full animate-flash"></span>
                  <span class="text-[13px] font-semibold text-accent-blue">
                    {currentlyPressedKeys.length > 0
                      ? currentlyPressedKeys.join(" + ").replace(/KEY_/g, "")
                      : "Press your physical shortcut combination now..."}
                  </span>
                </div>
                {#if liveCaptureHint}
                  <span class="text-[11px] text-amber-300/90 mt-1.5">{liveCaptureHint}</span>
                {/if}
              {:else}
                <span class="text-[12px] text-obsidian-300 flex flex-col gap-1.5 items-center">
                  {#if editingBinding.keys.length > 0}
                    <div class="flex gap-1.5">
                      {#each editingBinding.keys as k}
                        <kbd class="px-1.5! py-0.5! text-[12px]! bg-accent-blue! text-black! border-none! font-extrabold! rounded!">{k.replace("KEY_", "")}</kbd>
                      {/each}
                    </div>
                    <span class="text-[10px] text-accent-blue opacity-80">(Click / Tab here to record)</span>
                  {:else}
                    ⚠️ Click/Focus here to press keys!
                  {/if}
                </span>
              {/if}
            </div>
          </div>

          {#if keysCheck && !keysCheck.accepted}
            <div class="keys-rejected" role="alert">
              <span class="shrink-0">🚫</span>
              <span>
                <strong>That combination was not accepted.</strong>
                {keysCheck.message}
              </span>
            </div>
          {:else if keysCheck && keysCheck.problem}
            <div class="keys-fragile" role="status">
              <span class="shrink-0">⚠️</span>
              <span>{keysCheck.message}</span>
            </div>
          {:else if keysCheck?.accelerator && portalEnforced}
            <span class="keys-accepted">
              Your desktop will register this as <code>{keysCheck.accelerator}</code>.
            </span>
          {/if}

          {#if editingBindingConflict}
            <span class="validation-error-msg" style="color: #fbbf24; font-weight: 500; margin-top: 4px;">
              ⚠️ Warning: This key combination and gesture type conflicts with another existing binding.
            </span>
          {/if}
        </div>

        <!-- LLM Post-Processing Settings -->
        <div class="processing-toggles border-t border-white/5 pt-[14px] mt-4">
          <h5>OpenAI API LLM Post-Processing</h5>
          <label class="checkbox-field">
            <input type="checkbox" bind:checked={editOpenaiEnabled} />
            <span>Enable LLM post-processing for this hotkey</span>
          </label>

          {#if editOpenaiEnabled}
            <div class="openai-target-settings pl-4 mt-2 ml-4">
              <label class="field">
                <span>Model Override (leave empty for global default)</span>
                <input
                  type="text"
                  bind:value={editOpenaiModel}
                  placeholder="e.g. llama3.2:1b"
                />
              </label>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">System Prompt Override</span>
                  <span class="field-tag">LLM Prompt</span>
                </div>
                <textarea
                  rows="3"
                  bind:value={editOpenaiSystemPrompt}
                  placeholder="Leave empty to use the default system prompt from Settings → OpenAI API"
                  use:autoResize
                ></textarea>
                <p class="hint">Overrides the default system prompt configured in Settings → OpenAI API. Leave empty to inherit it.</p>
              </div>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">User Prompt Template</span>
                  <span class="field-tag">LLM Prompt</span>
                </div>
                <textarea
                  rows="3"
                  bind:value={editOpenaiPrompt}
                  class={editOpenaiPrompt && !editOpenaiPrompt.includes("{text}") ? 'border-red-500! ring-2! ring-red-500/20! focus:border-red-500! focus:ring-red-500/20!' : ''}
                  placeholder="e.g. write a haiku about {'{text}'}"
                  use:autoResize
                ></textarea>
                {#if editOpenaiPrompt && !editOpenaiPrompt.includes("{text}")}
                  <span class="validation-error-msg">
                    ⚠️ Validation Error: Prompt template MUST contain the <code>{"{text}"}</code> placeholder.
                  </span>
                {:else}
                  <p class="hint">Prompt template MUST contain the <code>{"{text}"}</code> placeholder. Leave empty to inherit the default user prompt.</p>
                {/if}
              </div>
            </div>
          {/if}
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-action secondary" onclick={() => { editingBinding = null; recordingTarget = null; }}>Cancel</button>
        <button class="btn-action primary" onclick={saveBindingModal}>Done</button>
      </div>
    </div>
  </div>
{/if}

{#if editingTarget}
  <TargetEditorModal
    bind:editingTarget
    isNew={true}
    existingTargets={targets}
    isNested={true}
    onSave={saveTargetModal}
    onCancel={cancelTargetModal}
  />
{/if}

<style>
  @reference "../../app.css";

  .backend-banner {
    @apply flex flex-col rounded-[var(--radius)] p-2.5 px-3.5 mb-1 border bg-white/[0.03] border-[var(--border)] transition-colors duration-150;
  }

  .backend-banner.private {
    @apply bg-emerald-500/6 border-emerald-500/25;
  }

  .backend-banner.warn {
    @apply bg-amber-500/6 border-amber-500/25;
  }

  .backend-banner.broken {
    @apply bg-red-500/6 border-red-500/25;
  }

  .manual-enable-banner {
    @apply flex flex-col rounded-[var(--radius)] p-2.5 px-3.5 mb-1 border bg-amber-500/6 border-amber-500/25 transition-colors duration-150;
  }

  .banner-toggle {
    @apply flex items-center gap-2.5 w-full bg-transparent border-none p-0 text-left cursor-pointer select-none text-[var(--text)];
  }

  .banner-title {
    @apply flex-1 text-[12.5px] font-semibold leading-normal;
  }

  .banner-chevron {
    @apply w-4 h-4 text-[var(--text-muted)] shrink-0 transition-transform duration-200 ease-in-out;
  }

  .banner-chevron.expanded {
    @apply rotate-180 text-[var(--text)];
  }

  .banner-content {
    @apply flex flex-col gap-1 min-w-0 w-full text-[12.5px] mt-2 pt-2 border-t border-white/5 pl-6.5;
  }

  .backend-icon {
    @apply text-base leading-none shrink-0;
  }

  .backend-detail {
    @apply text-[var(--text-muted)] leading-relaxed;
  }

  .backend-detail.error {
    @apply text-red-400;
  }

  .manual-enable-actions {
    @apply mt-1;
  }

  .keys-rejected {
    @apply flex items-start gap-2 p-2.5 rounded-md bg-red-500/10 border border-red-500/25 text-[12px] leading-relaxed text-red-300;
  }

  .keys-fragile {
    @apply flex items-start gap-2 p-2.5 rounded-md bg-amber-500/10 border border-amber-500/25 text-[12px] leading-relaxed text-amber-200;
  }

  .keys-accepted {
    @apply text-[11.5px] text-emerald-300/90;
  }

  .keys-accepted code {
    @apply bg-white/5 px-1.5 py-0.5 rounded font-mono;
  }

  .hotkeys-section {
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

  .binding-item.disabled {
    @apply opacity-55 border-white/[0.05] bg-[repeating-linear-gradient(-45deg,transparent,transparent_10px,rgba(0,0,0,0.25)_10px,rgba(0,0,0,0.25)_20px)];
  }

  .binding-content {
    @apply flex-1 flex flex-col gap-1 min-w-0;
  }

  .binding-header-row {
    @apply flex items-center justify-between w-full;
  }

  .binding-title {
    @apply text-sm font-semibold text-[var(--text)];
  }

  .binding-row2 {
    @apply flex items-center justify-between gap-2;
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

  kbd {
    @apply bg-[var(--surface2)] border border-[var(--border)] rounded px-1.5 py-0.5 text-[11px] font-mono font-bold text-[var(--accent2)] shadow-[0_1px_0_rgba(0,0,0,0.4)];
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

  .btn-action {
    @apply bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs font-semibold cursor-pointer transition-all duration-150 ease-out;
  }
  .btn-action:hover {
    @apply bg-[var(--border)] border-[var(--text-muted)];
  }

  .btn-action.primary {
    @apply bg-[var(--accent)] text-white border-none;
  }
  .btn-action.primary:hover {
    @apply opacity-90;
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

  .badge {
    @apply text-[10px] py-0.5 px-2 rounded-xl font-semibold uppercase;
  }

  .badge.gesture {
    @apply bg-[var(--accent2)]/15 text-[var(--accent2)] border border-[var(--accent2)]/30;
  }

  .badge.openai {
    @apply bg-emerald-500/15 text-emerald-200 border border-emerald-500/30;
  }

  .modal-backdrop {
    @apply fixed inset-0 bg-black/60 backdrop-blur-[4px] flex items-center justify-center z-[1000];
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

  .processing-toggles h5 {
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

  .btn-add-inline {
    @apply bg-transparent border-none text-[var(--accent2)] text-[11px] font-bold cursor-pointer p-0.5 px-1.5 rounded transition-all duration-150 ease-out;
  }
  .btn-add-inline:hover {
    @apply bg-[var(--accent2)]/8 text-white;
  }

  .target-selects-list {
    @apply flex flex-col gap-2 w-full mt-1;
  }

  .target-select-row {
    @apply flex items-center gap-2 w-full;
  }


  .btn-remove-inline {
    @apply flex items-center justify-center box-border bg-red-500/8 border border-red-500/20 text-red-400 cursor-pointer text-xs font-bold px-3 py-1.5 rounded-[var(--radius)] transition-all duration-150 ease-out h-[34px];
  }
  .btn-remove-inline:hover {
    @apply bg-red-500 border-red-500 text-white shadow-[0_2px_8px_rgba(239,68,68,0.25)];
  }

  .longer-display-label {
    @apply min-w-[255px]!;
  }

  .openai-target-settings {
    @apply border-l-2 border-[var(--accent)] pl-3.5 ml-2.5 mt-2.5 mb-3.5 flex flex-col gap-2;
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

  .binding-item.has-conflict {
    @apply border-amber-500/45!;
  }

  .binding-item.active-conflict {
    @apply bg-amber-500/6 relative;
  }
  .binding-item.active-conflict:hover {
    @apply border-amber-500/70! shadow-[0_4px_20px_rgba(251,191,36,0.15)];
  }

  .binding-title.has-conflict {
    @apply pr-[75px];
  }

  .conflict-marker {
    @apply absolute top-2.5 right-3.5 bg-amber-400 text-neutral-900 text-[9px] font-extrabold p-0.5 px-1.5 rounded shadow-[0_2px_6px_rgba(251,191,36,0.25)];
  }

  .conflict-alert-banner {
    @apply flex items-center gap-2.5 bg-amber-500/6 border border-amber-500/25 rounded-[var(--radius)] p-2.5 px-3.5 mb-3 text-xs text-amber-400 leading-normal;
  }

  .warning-icon {
    @apply text-base shrink-0;
  }

  .animate-fade-in {
    @apply animate-[fade-in_0.2s_cubic-bezier(0.16,1,0.3,1)];
  }

  @keyframes fade-in {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: scale(1); }
  }

  .custom-select-wrapper {
    @apply relative flex-1 min-w-[190px];
  }

  .custom-select-trigger {
    @apply w-full text-left bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 pr-9 cursor-pointer transition-all duration-150 ease-out shadow-[0_4px_12px_rgba(0,0,0,0.15)] select-none text-[13px] font-semibold;
    background: var(--surface2) url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' fill='none' stroke='%239ca3af' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='m3 5 3 3 3-3'/></svg>") no-repeat right 12px center;
    background-size: 10px;
  }
  .custom-select-trigger:hover {
    @apply border-white/12 bg-[var(--color-obsidian-700)] -translate-y-[1px];
  }
  .custom-select-trigger:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(56,189,248,0.15)];
  }

  .custom-select-backdrop {
    @apply fixed inset-0 z-[199];
  }

  .custom-dropdown-menu {
    @apply absolute left-0 right-0 mt-1 max-h-60 overflow-y-auto bg-[#1e2436] border border-[var(--border)] rounded-[var(--radius)] shadow-[0_10px_30px_rgba(0,0,0,0.5)] z-[200] flex flex-col p-1 gap-0.5;
  }

  .custom-dropdown-item {
    @apply w-full text-left bg-transparent border-none rounded-[var(--radius)] p-2 px-3 text-[13px] text-[var(--text)] cursor-pointer transition-colors duration-150;
  }
  .custom-dropdown-item:hover {
    @apply bg-white/[0.04];
  }
  .custom-dropdown-item:disabled {
    @apply opacity-40 cursor-not-allowed;
  }
  .custom-dropdown-item:disabled:hover {
    @apply bg-transparent;
  }

  .custom-dropdown-item.create-new {
    @apply bg-emerald-950/80 border border-emerald-500/25 text-emerald-400 font-bold mt-1!;
  }
  .custom-dropdown-item.create-new:hover {
    @apply bg-emerald-900/90!;
  }
  .custom-dropdown-item.create-new .plus-icon {
    @apply text-emerald-300 font-extrabold mr-1;
  }
</style>
