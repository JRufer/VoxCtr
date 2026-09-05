<script lang="ts">
  import appIcon from "../../assets/app_icon.png";
  import { config, saveConfig, configDirty, configLoaded } from "../../stores/config";
  import { status } from "../../stores/status";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";


  import GeneralTab from "./GeneralTab.svelte";
  import VisualTab from "./VisualTab.svelte";
  import EngineTab from "./EngineTab.svelte";
  import AudioTab from "./AudioTab.svelte";
  import HotkeysTab from "./HotkeysTab.svelte";
  import CommandsTab from "./CommandsTab.svelte";
  import TtsTab from "./TtsTab.svelte";
  import OpenAiTab from "./OpenAiTab.svelte";
  import FeaturesTab from "./FeaturesTab.svelte";
  import AboutTab from "./AboutTab.svelte";

  type Tab = "general" | "engine" | "hotkeys" | "commands" | "visual" | "audio" | "tts" | "features" | "openai" | "about";
  let activeTab = $state<Tab>("general");

  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: "general",  label: "General",  icon: "⚙️" },
    { id: "engine",   label: "Engine",   icon: "🧠" },
    { id: "hotkeys",  label: "Hotkeys",  icon: "⌨️" },
    { id: "commands", label: "Output Commands", icon: "🎯" },
    { id: "visual",   label: "Visual",   icon: "🎨" },
    { id: "audio",    label: "Audio",    icon: "🔊" },
    { id: "tts",      label: "TTS",      icon: "🗣️" },
    { id: "features", label: "Features", icon: "✨" },
    { id: "openai",   label: "OpenAI API", icon: "🤖" },
    { id: "about",    label: "About",    icon: "ℹ️" },
  ];

  async function handleSave() {
    await saveConfig($config);
  }

  async function toggleRecording() {
    await invoke("toggle_recording");
  }

  let lastTab: Tab = "general";
  let tabContentEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    const currentTab = activeTab;
    if (currentTab !== lastTab) {
      lastTab = currentTab;
      if (tabContentEl) {
        tabContentEl.scrollTop = 0;
      }
      if ($config?.audio?.dynamic_stream && $status.recording) {
        invoke("stop_recording").catch((e) => {
          console.error("Failed to stop recording on activeTab change:", e);
        });
      }
    }
  });

  onMount(() => {
    // The setup window sends the user here for the model step; land them on
    // the tab that actually has the download button.
    const tabListener = listen<string>("focus-settings-tab", (event) => {
      const requested = event.payload as Tab;
      if (tabs.some((t) => t.id === requested)) {
        activeTab = requested;
      }
    });

    let unsubscribeLoaded: () => void;
    unsubscribeLoaded = configLoaded.subscribe((loaded) => {
      if (loaded) {
        // Once config is loaded, retrieve values from config store
        const cfg = $config;
        if (cfg && cfg.engine && cfg.engine.whisper_cpp) {
          const modelSize = cfg.engine.whisper_cpp.model_size;
          if (cfg.engine.backend !== "moonshine") {
            invoke<boolean>("check_model_downloaded", {
              modelSize,
              modelDir: cfg.engine.whisper_cpp.model_dir || "",
            })
              .then(async (isDownloaded) => {
                if (!isDownloaded) {
                  activeTab = "engine";
                  // Only open and focus settings window if auto_show_settings is enabled
                  if (cfg?.ui?.auto_show_settings) {
                    try {
                      const { getCurrentWindow } = await import("@tauri-apps/api/window");
                      const currentWin = getCurrentWindow();
                      await currentWin.show();
                      await currentWin.setFocus();
                    } catch (winErr) {
                      console.error("Failed to programmatically show settings window on startup:", winErr);
                    }
                  }
                }
              })
              .catch((e) => {
                console.error("Failed to check model download status on startup:", e);
              });
          }
        }
        if (unsubscribeLoaded) {
          unsubscribeLoaded();
        } else {
          setTimeout(() => unsubscribeLoaded(), 0);
        }
      }
    });

    return () => {
      tabListener.then((unlisten) => unlisten()).catch(() => {});
    };
  });
</script>

<div class="settings-root">
  <!-- Unified Premium Sidebar -->
  <aside class="sidebar">
    <div class="brand">
      <img src={appIcon} class="brand-logo" alt="VoxCtrl Icon" />
      <div class="brand-text">
        <span class="brand-name">VoxCtrl</span>
        <span class="brand-tag">DEKTOP PANEL</span>
      </div>
    </div>

    <!-- Tab navigation -->
    <nav class="sidebar-nav">
      {#each tabs as tab}
        <button
          class="nav-btn"
          class:active={activeTab === tab.id}
          onclick={() => (activeTab = tab.id)}
        >
          <span class="nav-icon">{tab.icon}</span>
          <span class="nav-label">{tab.label}</span>
          {#if activeTab === tab.id}
            <div class="nav-active-pill"></div>
          {/if}
        </button>
      {/each}
    </nav>

    <!-- Recording Control & Status Center -->
    <div class="sidebar-footer">
      <div class="status-panel" class:recording={$status.recording} class:speaking={$status.speaking}>
        <div class="status-header">
          <div class="status-indicator">
            <span class="status-dot"></span>
            <span class="status-label">
              {$status.recording ? "Recording" : $status.speaking ? "Speaking" : "Idle"}
            </span>
          </div>
          <span class="word-count">{$status.word_count} words</span>
        </div>

        <!-- Custom springy record button -->
        <button
          class="btn-record"
          class:active={$status.recording}
          onclick={toggleRecording}
        >
          {#if $status.recording}
            <span class="pulse-ring"></span>
            <span class="btn-text">🛑 Stop</span>
          {:else}
            <span class="btn-text">🎙️ Record</span>
          {/if}
        </button>
      </div>
    </div>
  </aside>

  <!-- Main Content Area -->
  <main class="content-container">
    <div class="tab-content" bind:this={tabContentEl}>
      {#if activeTab === "general"}
        <GeneralTab bind:cfg={$config} />
      {:else if activeTab === "engine"}
        <EngineTab bind:cfg={$config} />
      {:else if activeTab === "hotkeys"}
        <HotkeysTab />
      {:else if activeTab === "commands"}
        <CommandsTab />
      {:else if activeTab === "visual"}
        <VisualTab bind:cfg={$config} />
      {:else if activeTab === "audio"}
        <AudioTab bind:cfg={$config} />
      {:else if activeTab === "tts"}
        <TtsTab bind:cfg={$config} />
      {:else if activeTab === "features"}
        <FeaturesTab bind:cfg={$config} />
      {:else if activeTab === "openai"}
        <OpenAiTab bind:cfg={$config} />
      {:else if activeTab === "about"}
        <AboutTab />
      {/if}
    </div>

    <!-- Floating Snappy Unsaved Changes Save Engine -->
    {#if $configDirty}
      <div class="floating-save-container">
        <span class="save-hint">Unsaved changes detected</span>
        <button class="btn-save" onclick={handleSave}>
          💾 Save Configuration
        </button>
      </div>
    {/if}
  </main>
</div>

<style>
  @reference "../../app.css";

  .settings-root {
    @apply flex h-screen w-screen bg-[var(--bg)] text-[var(--text)] overflow-hidden;
  }

  /* Redesigned Sidebar Base */
  /* Sized to its contents rather than to a fixed width. The longest label
     ("Output Commands") is 87px in Cantarell/Noto and 110px in DejaVu Sans at
     this size, and the app renders in whichever sans the system provides —
     neither Outfit nor Inter is bundled, and the CSP cannot fetch them. The old
     fixed 135px left 86px for the label, which clipped "Output Commands" by a
     hair on the narrow fonts and badly on the wide ones. `w-fit` fits whatever
     the machine actually renders; the floor keeps the old width as a minimum so
     nothing else in the panel gets narrower than it was. */
  .sidebar {
    @apply flex flex-col w-fit min-w-[135px] bg-[var(--color-obsidian-900)] border-r border-[var(--border)] shrink-0 px-1.5 py-5 z-10 shadow-[4px_0_24px_rgba(0,0,0,0.25)];
  }

  .brand {
    @apply flex items-center gap-2 px-1 py-1.5 pb-4 border-b border-white/[0.03] mb-4 whitespace-nowrap;
  }

  .brand-logo {
    @apply w-5 h-5 object-contain drop-shadow-[0_2px_8px_rgba(56,189,248,0.3)] animate-[floating_4s_ease-in-out_infinite] shrink-0;
  }

  @keyframes floating {
    0%, 100% { transform: translateY(0px) rotate(0deg); }
    50% { transform: translateY(-3px) rotate(3deg); }
  }

  .brand-text {
    @apply flex flex-col whitespace-nowrap overflow-hidden;
  }

  .brand-name {
    @apply text-[15px] font-[850] text-white tracking-[-0.5px] leading-[1.1] whitespace-nowrap;
  }

  .brand-tag {
    @apply text-[7px] font-bold text-[var(--color-accent-blue)] tracking-[0.12em] mt-0.5 whitespace-nowrap;
  }

  .sidebar-nav {
    @apply flex flex-col gap-1 flex-1 overflow-y-auto;
  }

  /* Nav Links with Snappy Elastic Springs */
  .nav-btn {
    @apply relative flex items-center gap-2 px-2 py-2 rounded-[var(--radius)] text-[var(--color-obsidian-300)] text-xs font-semibold text-left transition-all duration-150 ease-out whitespace-nowrap;
  }

  .nav-btn:hover {
    @apply text-white bg-white/[0.03] translate-x-[2px];
  }

  .nav-btn.active {
    @apply text-[var(--color-accent-blue)] bg-[var(--color-accent-blue)]/8 scale-[1.01] translate-x-[2px];
  }

  .nav-btn:active {
    @apply scale-[0.97] translate-x-0;
  }

  .nav-icon {
    @apply text-sm transition-transform duration-200 ease-out;
  }

  .nav-btn:hover .nav-icon {
    @apply scale-[1.18] rotate-[5deg];
  }

  .nav-active-pill {
    @apply absolute right-0 top-[15%] h-[70%] w-[3px] bg-[var(--color-accent-blue)] rounded-[99px_0_0_99px] shadow-[-2px_0_8px_var(--color-accent-blue)];
  }

  /* Record / Status Box Container */
  .sidebar-footer {
    @apply pt-4 border-t border-white/[0.03];
  }

  .status-panel {
    @apply bg-[var(--color-obsidian-950)] border border-[var(--border)] rounded-[var(--radius)] px-1.5 py-2 flex flex-col gap-2 transition-all duration-250 ease-out shadow-[inset_0_2px_6px_rgba(0,0,0,0.3)];
  }

  .status-panel.recording {
    @apply border-[var(--color-accent-blue)]/25 shadow-[0_4px_20px_rgba(56,189,248,0.1),_inset_0_2px_6px_rgba(0,0,0,0.3)];
  }

  .status-header {
    @apply flex flex-col gap-0.5 items-start whitespace-nowrap;
  }

  .status-indicator {
    @apply flex items-center gap-1 whitespace-nowrap;
  }

  .status-dot {
    @apply w-[5px] h-[5px] rounded-full bg-[var(--color-obsidian-300)] transition-all duration-150 ease-out;
  }

  .status-panel.recording .status-dot {
    @apply bg-[var(--color-accent-blue)] shadow-[0_0_8px_var(--color-accent-blue)] animate-[heartBeat_1.2s_infinite];
  }

  .status-panel.speaking .status-dot {
    @apply bg-[var(--color-accent-green)] shadow-[0_0_8px_var(--color-accent-green)];
  }

  @keyframes heartBeat {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.4); }
  }

  .status-label {
    @apply text-[10px] font-bold text-[var(--color-obsidian-300)] whitespace-nowrap;
  }

  .status-panel.recording .status-label {
    @apply text-[var(--color-accent-blue)];
  }

  .word-count {
    @apply text-[9px] text-white/30 font-semibold whitespace-nowrap;
  }

  /* Custom Spring Record CTA Button */
  .btn-record {
    @apply relative w-full px-1 py-1.5 rounded-[var(--radius)] bg-[var(--color-accent-blue)]/8 text-[var(--color-accent-blue)] text-[11px] font-bold text-center border border-[var(--color-accent-blue)]/20 shadow-[0_2px_6px_rgba(56,189,248,0.05)] transition-all duration-150 ease-out overflow-hidden whitespace-nowrap;
  }

  .btn-record:hover {
    @apply bg-[var(--color-accent-blue)] text-white border-[var(--color-accent-blue)] -translate-y-[1px] shadow-[0_4px_12px_rgba(56,189,248,0.25)];
  }

  .btn-record:active {
    @apply scale-[0.97] translate-y-0;
  }

  .btn-record.active {
    @apply bg-[var(--color-accent-blue)] text-white border-[var(--color-accent-blue)] shadow-[0_4px_16px_rgba(56,189,248,0.35)];
  }

  .pulse-ring {
    @apply absolute top-0 left-0 w-full h-full rounded-[var(--radius)] border-2 border-[var(--color-accent-blue)] animate-[ripple_1.6s_infinite_ease-out] opacity-0 pointer-events-none;
  }

  @keyframes ripple {
    0% { transform: scale(1); opacity: 0.5; }
    100% { transform: scale(1.15, 1.3); opacity: 0; }
  }

  .btn-text {
    @apply relative z-[2];
  }

  /* Content area */
  .content-container {
    @apply flex-1 flex flex-col bg-[var(--bg)] overflow-hidden relative z-[1];
  }

  /* Elevate container above sidebar when any tab displays a modal */
  .content-container:has(:global(.modal-backdrop)) {
    @apply z-[100];
  }

  .tab-content {
    @apply flex-1 overflow-y-auto p-[30px] z-[1];
  }

  /* Floating Snappy Save Change Banner */
  .floating-save-container {
    @apply absolute bottom-6 right-6 flex items-center gap-3.5 bg-[var(--color-obsidian-900)] border border-[var(--color-accent-blue)] p-2.5 px-4 rounded-[var(--radius)] shadow-[0_10px_30px_rgba(0,0,0,0.4),_0_0_12px_rgba(56,189,248,0.15)] z-[100] animate-[popUp_0.3s_cubic-bezier(0.175,0.885,0.32,1.275)_forwards];
  }

  @keyframes popUp {
    from { transform: translateY(20px) scale(0.9); opacity: 0; }
    to { transform: translateY(0) scale(1); opacity: 1; }
  }

  .save-hint {
    @apply text-xs font-semibold text-[var(--color-obsidian-300)];
  }

  .btn-save {
    @apply bg-[var(--color-accent-blue)] text-white px-3.5 py-1.5 rounded-md text-xs font-bold transition-all duration-150 ease-out shadow-[0_4px_10px_rgba(56,189,248,0.25)];
  }

  .btn-save:hover {
    @apply scale-[1.02] -translate-y-[1px] shadow-[0_6px_14px_rgba(56,189,248,0.35)] brightness-[1.05];
  }

  .btn-save:active {
    @apply scale-[0.97] translate-y-0;
  }
</style>
