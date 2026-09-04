<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import appIcon from "../../../assets/app_icon.png";
  import { config } from "../../../stores/config";
  import { wizard, type WizardIssue } from "../wizard-state.svelte";
  import { OVERLAY_STYLES, STEP_LABELS, TTS_ENGINES, keycapLabel } from "../wizard-data";

  let trayOpen = $state(false);
  let copied = $state(false);

  type SetupStatus = {
    hotkeys_active: boolean;
    model_ready: boolean;
    model_size: string;
    model_auto_downloads: boolean;
    missing_injection_tool: string | null;
    manual_package_commands: string;
    is_complete: boolean;
    hotkeys: { backend: string; portal_error: string | null; portal_refused: boolean };
  };

  let setup = $state<SetupStatus | null>(null);
  let rechecking = $state(false);

  /**
   * Ask the backend what is still broken, on top of whatever failed in front of
   * the user during the wizard.
   *
   * The two sources answer different questions. `wizard.issues` remembers the
   * download that failed twenty minutes ago and would otherwise scroll away;
   * `get_setup_status` catches what the wizard never touched — a missing
   * `wtype`, a shortcut listener that died after the hotkey step passed.
   */
  async function recheck() {
    rechecking = true;
    try {
      setup = await invoke<SetupStatus>("get_setup_status");
    } catch (e) {
      console.error("Wizard: could not read setup status:", e);
      setup = null;
    } finally {
      rechecking = false;
    }
  }

  /** Live problems derived from the backend's own view of the install. */
  const systemIssues = $derived.by<WizardIssue[]>(() => {
    if (!setup) return [];
    const out: WizardIssue[] = [];
    if (!setup.hotkeys_active) {
      out.push({
        id: "sys-hotkeys",
        step: 2,
        title: "No global shortcut is active — pressing your hotkey will do nothing.",
        detail:
          `backend=${setup.hotkeys.backend} portal_refused=${setup.hotkeys.portal_refused}\n` +
          `portal_error=${setup.hotkeys.portal_error ?? "(none)"}`,
      });
    }
    if (!setup.model_ready && !setup.model_auto_downloads) {
      out.push({
        id: "sys-model",
        step: 1,
        title: `Speech model "${setup.model_size}" is not on disk — dictation will record but produce no text.`,
        detail: `model_size=${setup.model_size} model_ready=false auto_downloads=${setup.model_auto_downloads}`,
      });
    }
    if (setup.missing_injection_tool) {
      out.push({
        id: "sys-inject",
        step: 4,
        title: `"${setup.missing_injection_tool}" is not installed — VoxCtrl cannot type text into other windows.`,
        detail:
          `missing_injection_tool=${setup.missing_injection_tool}\n` +
          `install with: ${setup.manual_package_commands || "(unknown package manager)"}`,
      });
    }
    return out;
  });

  /** Wizard-time failures first (they carry the raw backend error), then
   *  whatever the live check adds that is not already covered. */
  const issues = $derived.by<WizardIssue[]>(() => {
    const seenSteps = new Set(wizard.issues.map((i) => i.id));
    return [...wizard.issues, ...systemIssues.filter((i) => !seenSteps.has(i.id))];
  });

  const healthy = $derived(issues.length === 0);

  const combo = $derived((wizard.combo ?? []).map(keycapLabel).join(" + ") || "not set");

  const summary = $derived([
    {
      k: "engine",
      v:
        $config.engine.backend === "moonshine"
          ? `Moonshine · ${$config.engine.moonshine.model_size}`
          : `whisper.cpp · ${$config.engine.whisper_cpp.model_size}`,
    },
    { k: "hotkey", v: `${combo} · ${wizard.gestureInfo.name}` },
    {
      k: "overlay",
      v: $config.ui.show_overlay
        ? `${OVERLAY_STYLES.find((o) => o.id === $config.ui.overlay_style)?.name ?? $config.ui.overlay_style} · ${$config.ui.overlay_position}`
        : "off",
    },
    {
      k: "voice output",
      v: $config.tts.enabled
        ? (TTS_ENGINES.find((t) => t.id === $config.tts.engine)?.name ?? $config.tts.engine)
        : "off",
    },
  ]);

  /** One block of plain text holding every detail, for pasting into an issue. */
  const report = $derived(
    [
      "VoxCtrl setup report",
      `engine: ${$config.engine.backend} / ${$config.engine.backend === "moonshine" ? $config.engine.moonshine.model_size : $config.engine.whisper_cpp.model_size}`,
      `device: ${$config.engine.whisper_cpp.device}`,
      `hotkey: ${combo} (${wizard.gesture})`,
      `overlay: ${$config.ui.show_overlay ? `${$config.ui.overlay_style} / ${$config.ui.overlay_position}` : "off"}`,
      `tts: ${$config.tts.enabled ? $config.tts.engine : "off"}`,
      `shortcut backend: ${setup?.hotkeys.backend ?? "unknown"}`,
      "",
      ...issues.map(
        (i) => `[${STEP_LABELS[i.step] ?? "setup"}] ${i.title}\n${i.detail}\n`,
      ),
    ].join("\n"),
  );

  async function copyReport() {
    try {
      await navigator.clipboard.writeText(report);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (e) {
      console.error("Wizard: clipboard write failed:", e);
    }
  }

  async function openSettings() {
    try {
      await invoke("open_settings_tab", { tab: "general" });
    } catch (e) {
      console.error("Wizard: could not open Settings:", e);
    }
  }

  onMount(() => {
    void recheck();
  });
</script>

<div class="done-step">
  <div class="hero">
    <div class="tick" class:warn={!healthy}>{healthy ? "✓" : "!"}</div>
    <span class="vx-eyebrow">{healthy ? "// configured" : "// finished with problems"}</span>
    <h2>
      {#if healthy}
        VoxCtrl is <span>ready</span>.
      {:else}
        Setup is <span class="bad-word">incomplete</span>.
      {/if}
    </h2>
    <p>
      {#if healthy}
        This window can close. VoxCtrl keeps running quietly in your system tray and listens for
        your hotkey in any app.
      {:else}
        Your choices are saved, but {issues.length} problem{issues.length === 1 ? "" : "s"} will stop
        VoxCtrl working end to end. The details are on the right — fix them here or in Settings.
      {/if}
    </p>
    <div class="summary">
      {#each summary as s}
        <div>
          <div class="vx-label">{s.k}</div>
          <div class="summary-value">{s.v}</div>
        </div>
      {/each}
    </div>
  </div>

  <div class="panel">
    <div class="vx-label">system tray</div>
    <div class="panel-title">Find VoxCtrl in the tray</div>
    <div class="panel-desc">Click the icon to open Settings, pause listening, or quit. Try it:</div>
    <div class="mock-screen">
      {#if trayOpen}
        <div class="tray-menu">
          <div class="row hot"><span>Open Settings</span><span class="mono">↗</span></div>
          <div class="row">Pause listening</div>
          <div class="row"><span>Status</span><span class="mono good">● idle</span></div>
          <div class="sep"></div>
          <div class="row">Quit VoxCtrl</div>
        </div>
      {:else}
        <div class="click-me"><span>click me</span><span class="arrow">↘</span></div>
      {/if}
      <div class="mock-bar">
        <span class="sq"></span>
        <span class="ci"></span>
        <button class="tray-btn" class:on={trayOpen} onclick={() => (trayOpen = !trayOpen)}>
          <img src={appIcon} alt="Tray icon" />
        </button>
      </div>
    </div>
  </div>

  {#if healthy}
    <div class="panel">
      <div class="vx-label">where everything lives</div>
      <div class="panel-title">Every choice is editable</div>
      <div class="panel-desc">
        Nothing you picked here is locked in. Settings holds the same options plus the ones the
        wizard skipped — output targets, per-binding LLM cleanup, audio devices and the MCP server.
      </div>
      <ul class="where">
        <li><span class="glyph">⌁</span> Hotkeys <span class="tag">gestures & bindings</span></li>
        <li><span class="glyph">∇</span> Engine <span class="tag">model & device</span></li>
        <li><span class="glyph">⌬</span> Visual <span class="tag">overlay style & position</span></li>
        <li><span class="glyph">⊕</span> TTS <span class="tag">voice output</span></li>
        <li><span class="glyph">⊳</span> Output Commands <span class="tag">where text goes</span></li>
      </ul>
      <button class="vx-btn open-settings" onclick={openSettings}>Open Settings now</button>
    </div>
  {:else}
    <div class="panel problems">
      <div class="panel-head">
        <div>
          <div class="vx-label">problems · {issues.length}</div>
          <div class="panel-title">What still needs fixing</div>
        </div>
        <button class="vx-btn small" onclick={recheck} disabled={rechecking}>
          {#if rechecking}<span class="vx-spinner"></span>{/if} Re-check
        </button>
      </div>

      <div class="issue-list">
        {#each issues as issue (issue.id)}
          <div class="issue">
            <div class="issue-head">
              <span class="issue-step">{STEP_LABELS[issue.step] ?? "setup"}</span>
              <span class="issue-title">{issue.title}</span>
            </div>
            <pre class="issue-detail">{issue.detail}</pre>
          </div>
        {/each}
      </div>

      <div class="problem-actions">
        <button class="vx-btn small" onclick={copyReport}>
          {copied ? "✓ Copied" : "Copy diagnostics"}
        </button>
        <button class="vx-btn small" onclick={openSettings}>Open Settings</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .done-step {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 0.95fr 1fr 1fr;
    gap: 18px;
    align-items: stretch;
  }

  .hero {
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .tick {
    width: 76px;
    height: 76px;
    margin: 0 0 20px;
    border-radius: 50%;
    border: 2px solid var(--vx-gold-0);
    display: grid;
    place-items: center;
    color: var(--vx-gold-1);
    font-family: var(--vx-mono);
    font-size: 32px;
    box-shadow: 0 0 30px var(--vx-gold-glow);
    animation: vxRing 0.6s var(--vx-ease);
  }

  .tick.warn {
    border-color: var(--vx-bad);
    color: var(--vx-bad);
    box-shadow: 0 0 30px rgba(244, 99, 110, 0.3);
  }

  h2 {
    margin: 10px 0;
    font-size: clamp(28px, 3.2vw, 44px);
    letter-spacing: -0.035em;
    line-height: 1.02;
    font-weight: 600;
  }

  h2 span {
    color: var(--vx-gold-1);
  }

  h2 span.bad-word {
    color: var(--vx-bad);
  }

  .hero p {
    margin: 0 0 20px;
    font-size: 14.5px;
    line-height: 1.55;
    color: var(--vx-txt-1);
  }

  .summary {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px 14px;
    padding: 14px 16px;
    border-radius: 14px;
    border: 1px solid var(--vx-line);
    background: linear-gradient(180deg, rgba(34, 212, 239, 0.04), transparent);
  }

  .summary-value {
    font-size: 13px;
    font-weight: 600;
    color: var(--vx-txt-0);
    margin-top: 3px;
    word-break: break-word;
  }

  .panel {
    padding: 20px;
    border-radius: 18px;
    border: 1px solid var(--vx-line);
    background: var(--vx-bg-1);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .panel.problems {
    border-color: rgba(244, 99, 110, 0.4);
  }

  .panel-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .panel-title {
    font-weight: 600;
    font-size: 16px;
    margin: 6px 0 4px;
  }

  .panel-desc {
    font-size: 13px;
    color: var(--vx-txt-2);
    line-height: 1.5;
    margin-bottom: 16px;
  }

  .issue-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 10px 0 12px;
    display: grid;
    gap: 10px;
    align-content: start;
  }

  .issue {
    border: 1px solid var(--vx-line);
    border-radius: 12px;
    background: rgba(244, 99, 110, 0.04);
    padding: 11px 12px;
  }

  .issue-head {
    display: flex;
    gap: 9px;
    align-items: baseline;
  }

  .issue-step {
    flex: none;
    font-family: var(--vx-mono);
    font-size: 10px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--vx-bad);
  }

  .issue-title {
    font-size: 13px;
    line-height: 1.45;
    color: var(--vx-txt-0);
  }

  .issue-detail {
    margin: 8px 0 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.35);
    font-family: var(--vx-mono);
    font-size: 11px;
    line-height: 1.5;
    color: var(--vx-txt-2);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 150px;
    overflow: auto;
  }

  .problem-actions {
    display: flex;
    gap: 8px;
    margin-top: auto;
  }

  .mock-screen {
    position: relative;
    flex: 1;
    min-height: 210px;
    border-radius: 12px;
    border: 1px solid var(--vx-line-2);
    background: linear-gradient(180deg, var(--vx-bg-2), var(--vx-bg-0));
    overflow: hidden;
  }

  .mock-bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 40px;
    background: rgba(0, 0, 0, 0.55);
    border-top: 1px solid var(--vx-line);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: 0 14px;
    gap: 14px;
  }

  .sq,
  .ci {
    width: 14px;
    height: 14px;
    background: var(--vx-bg-4);
    border-radius: 3px;
  }

  .ci {
    border-radius: 50%;
  }

  .tray-btn {
    width: 44px;
    height: 36px;
    border-radius: 8px;
    border: 1px solid var(--vx-line);
    background: transparent;
    cursor: pointer;
    display: grid;
    place-items: center;
    transition: all 0.25s;
  }

  .tray-btn.on {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.1);
  }

  .tray-btn img {
    width: 18px;
    height: 18px;
    border-radius: 4px;
    box-shadow: 0 0 10px rgba(34, 212, 239, 0.5);
  }

  .tray-menu {
    position: absolute;
    right: 14px;
    bottom: 52px;
    width: 200px;
    padding: 6px;
    border-radius: 12px;
    border: 1px solid var(--vx-line-2);
    background: rgba(20, 24, 31, 0.95);
    box-shadow: var(--vx-panel-shadow);
    animation: vxPop 0.22s var(--vx-ease);
  }

  .row {
    padding: 10px 12px;
    border-radius: 8px;
    color: var(--vx-txt-1);
    font-size: 13px;
    display: flex;
    justify-content: space-between;
  }

  .row.hot {
    background: rgba(34, 212, 239, 0.1);
    color: var(--vx-cyan-1);
    font-weight: 600;
  }

  .mono {
    font-family: var(--vx-mono);
    font-size: 11px;
  }

  .good {
    color: var(--vx-good);
  }

  .sep {
    height: 1px;
    background: var(--vx-line);
    margin: 4px 8px;
  }

  .click-me {
    position: absolute;
    right: 46px;
    bottom: 48px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-cyan-1);
    animation: vxPulse 2s infinite;
  }

  .arrow {
    font-size: 16px;
    margin-right: -8px;
  }

  .where {
    list-style: none;
    padding: 0;
    margin: 0 0 16px;
    display: grid;
    gap: 2px;
  }

  .where li {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 10px;
    font-size: 12.5px;
    color: var(--vx-txt-1);
  }

  .where .glyph {
    font-family: var(--vx-mono);
    color: var(--vx-cyan-2);
    width: 16px;
  }

  .where .tag {
    margin-left: auto;
    font-family: var(--vx-mono);
    font-size: 10px;
    color: var(--vx-txt-3);
  }

  .open-settings {
    margin-top: auto;
    align-self: flex-start;
  }

  .vx-btn.small {
    height: 34px;
    padding: 0 12px;
    font-size: 12px;
  }

  @media (max-width: 1100px) {
    .done-step {
      grid-template-columns: 1fr;
      align-content: start;
    }
  }
</style>
