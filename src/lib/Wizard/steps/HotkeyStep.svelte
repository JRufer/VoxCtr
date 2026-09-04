<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { wizard } from "../wizard-state.svelte";
  import {
    GESTURES,
    isModifiersOnly,
    keycapLabel,
    mapBrowserKeyToEvdev,
    type GestureId,
  } from "../wizard-data";

  let {
    registerGate,
    setBlocker,
  }: {
    registerGate: (step: number, gate: (() => Promise<boolean>) | null) => void;
    setBlocker: (step: number, reason: string | null) => void;
  } = $props();

  const STEP = 2;

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
    supported_gestures: GestureId[];
  };

  type KeysCheck = {
    accepted: boolean;
    enforced: boolean;
    accelerator: string | null;
    problem: string | null;
    message: string | null;
  };

  let status = $state<HotkeyStatus | null>(null);
  let registering = $state(false);
  let registerError = $state<string | null>(null);

  /** Only offer gestures the running shortcut backend can actually deliver.
   *  A desktop that only reports "this shortcut fired" cannot end a hold. */
  const gestures = $derived(
    status?.supported_gestures?.length
      ? GESTURES.filter((g) => status!.supported_gestures.includes(g.id))
      : GESTURES,
  );

  const hiddenGestures = $derived(GESTURES.length - gestures.length);

  const hotkeysActive = $derived(status?.is_active === true);

  /** True once a binding exists but the desktop has not accepted it. On
   *  backends that read the keyboard directly this never becomes true, because
   *  shortcuts are live the moment they are saved. */
  const needsRegistration = $derived(
    !!wizard.combo && wizard.combo.length > 0 && status !== null && !status.is_active,
  );

  const liveHint = $derived(
    wizard.recording && isModifiersOnly(wizard.held)
      ? "Keep holding and add a regular key — modifiers alone cannot be a shortcut."
      : null,
  );

  async function refreshStatus() {
    try {
      status = await invoke<HotkeyStatus>("check_hotkey_status");
      if (status.is_active) wizard.clearIssue("shortcut-register");
    } catch (e) {
      console.error("Wizard: failed to read hotkey status:", e);
    }
  }

  function startRecording() {
    wizard.recording = true;
    wizard.held = [];
    wizard.captureHint = null;
    void invoke("set_hotkeys_inhibited", { inhibited: true }).catch(() => {});
  }

  function stopRecording() {
    wizard.recording = false;
    wizard.held = [];
    void invoke("set_hotkeys_inhibited", { inhibited: false }).catch(() => {});
  }

  /**
   * Validate a captured combination against the same Rust rules the shortcut
   * registration uses, so the recorder and the desktop can never disagree
   * about what is bindable.
   */
  async function commitCapture(keys: string[]) {
    if (keys.length === 0) return;
    let check: KeysCheck;
    try {
      check = await invoke<KeysCheck>("check_hotkey_keys", { keys });
    } catch (e) {
      console.error("Wizard: hotkey validation failed:", e);
      // A failed validation call must not throw the user's capture away; the
      // save path checks again before anything is written.
      wizard.combo = [...keys];
      return;
    }
    if (check.accepted) {
      wizard.combo = [...keys];
      wizard.captureHint = null;
      // Written straight away rather than on "Continue": the desktop can only
      // be asked to register a shortcut that actually exists in bindings.toml,
      // and the registration panel below is the next thing the user touches.
      await persistBinding();
    } else {
      wizard.captureHint =
        check.message ?? "This key combination cannot be used as a shortcut. Try another.";
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (!wizard.recording) return;
    e.preventDefault();
    e.stopPropagation();
    const key = mapBrowserKeyToEvdev(e.key, e.code);
    if (e.key === "Escape" && wizard.held.length === 0) {
      stopRecording();
      return;
    }
    if (!wizard.held.includes(key)) wizard.held = [...wizard.held, key];
  }

  function onKeyUp(e: KeyboardEvent) {
    if (!wizard.recording) return;
    e.preventDefault();
    e.stopPropagation();
    const captured = [...wizard.held];
    stopRecording();
    void commitCapture(captured);
  }

  /** Save the binding and re-read how the desktop feels about it. */
  async function persistBinding() {
    try {
      await wizard.saveBinding();
    } catch (e) {
      console.error("Wizard: failed to save binding:", e);
      registerError = `Could not save the binding: ${e}`;
      wizard.recordIssue({
        id: "binding-save",
        step: STEP,
        title: "The hotkey binding could not be written — no shortcut will start dictation.",
        detail: `keys=${(wizard.combo ?? []).join("+")} gesture=${wizard.gesture}\n${e}`,
      });
      return;
    }
    wizard.clearIssue("binding-save");
    await refreshStatus();
  }

  function pickGesture(id: GestureId) {
    if (wizard.gesture === id) return;
    wizard.gesture = id;
    // A gesture change rewrites the binding, so the desktop has to be told
    // about it too — on the portal a different gesture can mean a different
    // registered trigger.
    if (wizard.combo && wizard.combo.length > 0) void persistBinding();
  }

  /** Ask the desktop to approve the shortcut — the portal dialog on Wayland,
   *  Mint's own shortcut registry on Cinnamon. */
  async function registerWithDesktop() {
    registering = true;
    registerError = null;
    try {
      status = await invoke<HotkeyStatus>("approve_shortcuts");
    } catch (e) {
      registerError = `${e}`;
      wizard.recordIssue({
        id: "shortcut-register",
        step: STEP,
        title: "Your desktop refused to register the global shortcut.",
        detail: `backend=${status?.backend ?? "unknown"} portal_refused=${status?.portal_refused ?? "?"}\nportal_error=${status?.portal_error ?? "(none)"}\n${e}`,
      });
      await refreshStatus();
    } finally {
      registering = false;
    }
  }

  async function openDesktopSettings() {
    try {
      await invoke("open_shortcut_settings");
    } catch (e) {
      registerError = `${e}`;
    }
  }

  // Continue is gated on a shortcut that will actually fire. A binding the
  // desktop has refused looks identical to a working one in the config file,
  // and the very next step asks the user to press it — so letting them past
  // here would send them into a test that cannot succeed.
  $effect(() => {
    let reason: string | null = null;
    if (wizard.recording) reason = "Press the keys you want to use.";
    else if (!wizard.combo || wizard.combo.length === 0) reason = "Record a key combination first.";
    else if (needsRegistration) reason = "Register the shortcut with your desktop to continue.";
    setBlocker(STEP, reason);
  });

  /** Keep the shipped gesture only while this machine can serve it. */
  $effect(() => {
    const allowed = gestures;
    if (allowed.length > 0 && !allowed.some((g) => g.id === wizard.gesture)) {
      wizard.gesture = allowed[0].id;
    }
  });

  onMount(() => {
    registerGate(STEP, async () => {
      await persistBinding();
      return status?.is_active !== false;
    });
    void wizard.loadBinding();
    void refreshStatus();
    // The backend watches the shortcut listener and announces when it starts
    // working, so approving the portal dialog unblocks this step on its own.
    const unlisten = listen<boolean>("setup-status-changed", () => void refreshStatus());
    // Belt and braces: some desktops accept a shortcut without the watcher
    // noticing until its next sweep.
    const poll = setInterval(() => {
      if (!wizard.recording) void refreshStatus();
    }, 3000);
    // The recorder listens at the window: a Super or Alt combination never
    // reaches a focused element intact, and the user should not have to keep
    // one focused while pressing four keys.
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      clearInterval(poll);
      void unlisten.then((off) => off()).catch(() => {});
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      if (wizard.recording) stopRecording();
      registerGate(STEP, null);
      setBlocker(STEP, null);
    };
  });

  const recorderState = $derived(
    wizard.recording
      ? {
          label: "listening for keys",
          hint: wizard.captureHint ?? liveHint ?? "Hold one or more modifiers, then press a key. Esc to cancel.",
          tone: "rec",
        }
      : wizard.combo && wizard.combo.length > 0
        ? {
            label: "binding captured",
            hint: wizard.captureHint ?? "Click anywhere in this area to record a different combination.",
            tone: "done",
          }
        : {
            label: "click to record",
            hint:
              wizard.captureHint ??
              "Needs at least one modifier (Ctrl, Alt, Shift or Super) plus a regular key — e.g. Super + Space.",
            tone: "idle",
          },
  );
</script>

<div class="hotkey-step">
  <div class="copy">
    <span class="vx-eyebrow">// 02 · first key binding</span>
    <h2 class="vx-title">How do you want to start talking?</h2>
    <p class="vx-lede">
      Pick a gesture, then record the keys. Taps keep your hands free while speaking; holds can't be
      left running by accident. Double-taps avoid clashes with shortcuts you already use.
    </p>
  </div>

  <div class="body">
    <div class="gestures">
      {#each gestures as g}
        {@const on = wizard.gesture === g.id}
        <button class="vx-card gesture" class:vx-on={on} onclick={() => (wizard.gesture = g.id)}>
          <div>
            <div class="g-name">{g.name}</div>
            <div class="g-desc">{g.desc}</div>
          </div>
          <div class="tracks">
            <div class="track-row">
              <span class="track-label">KEY</span>
              <div class="track">
                {#each g.key as [w, active]}
                  <div style:flex={w} class:on={active && on} class:lit={active}></div>
                {/each}
              </div>
            </div>
            <div class="track-row">
              <span class="track-label">MIC</span>
              <div class="track">
                {#each g.mic as [w, active]}
                  <div style:flex={w} class:on={active && on} class:lit={active}></div>
                {/each}
              </div>
            </div>
          </div>
        </button>
      {/each}
      {#if hiddenGestures > 0}
        <div class="hidden-note">
          {hiddenGestures} gesture{hiddenGestures === 1 ? "" : "s"} hidden — this desktop's shortcut
          backend only tells VoxCtrl that a shortcut fired, so it cannot time a hold.
        </div>
      {/if}
    </div>

    <div class="right">
      <button
        class="recorder"
        class:rec={recorderState.tone === "rec"}
        class:done={recorderState.tone === "done"}
        onclick={() => (wizard.recording ? stopRecording() : startRecording())}
      >
        {#if wizard.recording}
          <div class="scan"></div>
        {/if}
        <div class="rec-label">
          <span class="dot"></span>{recorderState.label}
        </div>
        <div class="keycaps">
          {#each wizard.displayKeys as key, i}
            <div class="vx-keycap" class:vx-hot={wizard.recording || recorderState.tone === "done"}>
              {keycapLabel(key)}
            </div>
            {#if i < wizard.displayKeys.length - 1}
              <span class="vx-plus">+</span>
            {/if}
          {/each}
        </div>
        <div class="rec-hint" class:bad={!!wizard.captureHint}>{recorderState.hint}</div>
      </button>

      {#if wizard.combo && wizard.combo.length > 0}
        <div class="sys" class:ok={hotkeysActive}>
          <span class="sys-glyph">{hotkeysActive ? "✓" : "⌬"}</span>
          <div class="sys-copy">
            <div class="sys-title">
              {hotkeysActive
                ? `Shortcuts are live (${status?.backend ?? "ready"})`
                : "Your desktop still needs to approve the shortcut"}
            </div>
            <div class="sys-desc">
              {#if hotkeysActive}
                The binding is registered system-wide. Continue to choose an overlay.
              {:else if status?.portal_error}
                {status.portal_error}
              {:else}
                Wayland desktops ask you to confirm global shortcuts. Click below and approve the
                dialog your desktop shows — you come straight back here.
              {/if}
            </div>
            {#if registerError}
              <div class="sys-err">{registerError}</div>
            {/if}
          </div>
          {#if !hotkeysActive}
            <div class="sys-actions">
              <button class="vx-btn" onclick={openDesktopSettings}>Desktop settings</button>
              <button class="vx-btn vx-primary" onclick={registerWithDesktop} disabled={registering}>
                {#if registering}<span class="vx-spinner"></span> Asking…{:else}Register →{/if}
              </button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .hotkey-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .copy {
    max-width: 820px;
    min-width: 0;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(300px, 1fr) 1.6fr;
    gap: 14px;
  }

  .gestures {
    display: grid;
    grid-auto-rows: min-content;
    gap: 10px;
    align-content: start;
  }

  .gesture {
    padding: 12px 16px;
    display: grid;
    grid-template-columns: 1fr 130px;
    gap: 14px;
    align-items: center;
  }

  .g-name {
    font-weight: 600;
    font-size: 14.5px;
    margin-bottom: 3px;
  }

  .g-desc {
    font-size: 12px;
    color: var(--vx-txt-2);
    line-height: 1.4;
  }

  .tracks {
    display: grid;
    gap: 5px;
  }

  .track-row {
    display: grid;
    grid-template-columns: 24px 1fr;
    gap: 6px;
    align-items: center;
  }

  .track-label {
    font-family: var(--vx-mono);
    font-size: 9.5px;
    color: var(--vx-txt-3);
    letter-spacing: 0.1em;
  }

  .track {
    display: flex;
    gap: 2px;
    height: 9px;
  }

  .track > div {
    border-radius: 2px;
    background: var(--vx-bg-3);
    transition: background 0.3s;
  }

  .track > div.lit {
    background: var(--vx-txt-2);
  }

  .track > div.on {
    background: var(--vx-cyan-0);
  }

  .hidden-note {
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px dashed var(--vx-line-2);
    font-size: 12px;
    line-height: 1.45;
    color: var(--vx-txt-2);
  }

  .right {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
  }

  .recorder {
    position: relative;
    overflow: hidden;
    flex: 1;
    min-height: 220px;
    border-radius: 18px;
    border: 1px dashed var(--vx-line-2);
    background: rgba(255, 255, 255, 0.015);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    cursor: pointer;
    font: inherit;
    color: inherit;
    transition: all 0.3s var(--vx-ease);
  }

  .recorder.rec {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.05);
    box-shadow: 0 0 0 1px rgba(34, 212, 239, 0.15), 0 0 40px rgba(34, 212, 239, 0.12);
  }

  .recorder.done {
    border-color: var(--vx-cyan-b);
    background: var(--vx-bg-1);
    box-shadow: 0 0 0 1px rgba(34, 212, 239, 0.1);
  }

  .scan {
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, transparent, rgba(34, 212, 239, 0.08), transparent);
    animation: vxScan 1.6s linear infinite;
    pointer-events: none;
  }

  .rec-label {
    font-family: var(--vx-mono);
    font-size: 11.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--vx-txt-2);
    display: flex;
    align-items: center;
    gap: 8px;
    transition: color 0.3s;
  }

  .recorder.rec .rec-label {
    color: var(--vx-cyan-1);
  }

  .recorder.done .rec-label {
    color: var(--vx-good);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 10px currentColor;
    animation: vxPulse 1.4s infinite;
  }

  .keycaps {
    display: flex;
    gap: 10px;
    align-items: center;
    min-height: 64px;
  }

  .rec-hint {
    font-size: 13px;
    color: var(--vx-txt-2);
    text-align: center;
    max-width: 520px;
    line-height: 1.5;
    padding: 0 20px;
  }

  .rec-hint.bad {
    color: var(--vx-warn);
  }

  .sys {
    flex: none;
    padding: 14px 18px;
    border-radius: 14px;
    border: 1px solid var(--vx-line);
    background: var(--vx-bg-1);
    display: flex;
    gap: 16px;
    align-items: center;
    animation: vxPop 0.3s var(--vx-ease);
  }

  .sys.ok {
    border-color: rgba(106, 212, 138, 0.4);
  }

  .sys-glyph {
    font-family: var(--vx-mono);
    font-size: 26px;
    color: var(--vx-gold-1);
  }

  .sys.ok .sys-glyph {
    color: var(--vx-good);
  }

  .sys-copy {
    flex: 1;
    min-width: 0;
  }

  .sys-title {
    font-weight: 600;
    font-size: 14px;
  }

  .sys-desc {
    font-size: 12.5px;
    color: var(--vx-txt-2);
    line-height: 1.45;
    margin-top: 2px;
  }

  .sys-err {
    margin-top: 6px;
    font-size: 12px;
    color: var(--vx-bad);
    line-height: 1.4;
  }

  .sys-actions {
    display: flex;
    gap: 8px;
    flex: none;
  }

  @media (max-width: 1100px) {
    .body {
      grid-template-columns: 1fr;
    }
  }
</style>
