<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { recording, speaking, mcpRecording, status } from "../../stores/status";
  import { config } from "../../stores/config";
  import VoiceCard from "./VoiceCard.svelte";
  import Waveform from "./Waveform.svelte";
  import Pulse from "./Pulse.svelte";
  import BlueWave from "./BlueWave.svelte";
  import MonoBars from "./MonoBars.svelte";
  import Spectrum from "./Spectrum.svelte";
  import Terminal from "./Terminal.svelte";
  import Vinyl from "./Vinyl.svelte";

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  let visible = $state(true);
  let customOverlays = $state<CustomOverlay[]>([]);
  let activeCustomOverlay = $derived(
    customOverlays.find(o => o.name === $config.ui.overlay_style)
  );

  const triggerLabel = $derived($status.active_target_label || "Focused Window");
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  // Delay unmounting the visualizer when recording/speaking stops to allow CSS outro animation to finish
  let isRecordingOrSpeaking = $derived(
    ($recording && $config.ui.show_overlay) ||
    ($speaking && $config.tts.enabled && $config.tts.response_overlay) ||
    ($mcpRecording && $config.mcp.visual_feedback)
  );
  let renderOverlay = $state(false);
  let animateActive = $state(false);
  let timeoutId: any;
  let animateTimeoutId: any;

  $effect(() => {
    if (isRecordingOrSpeaking) {
      if (timeoutId) clearTimeout(timeoutId);
      renderOverlay = true;
      if (animateTimeoutId) clearTimeout(animateTimeoutId);
      animateTimeoutId = setTimeout(() => {
        animateActive = true;
      }, 25);
    } else {
      animateActive = false;
      timeoutId = setTimeout(() => {
        renderOverlay = false;
      }, 450);
    }
    return () => {
      if (timeoutId) clearTimeout(timeoutId);
      if (animateTimeoutId) clearTimeout(animateTimeoutId);
    };
  });

  let processedHtml = $derived.by(() => {
    if (!activeCustomOverlay) return "";
    return activeCustomOverlay.html
      .replace(/\{\{trigger\}\}/g, triggerLabel)
      .replace(/\{\{target\}\}/g, targetLabel);
  });

  let targetVolume = 0;
  let currentVolume = $state(0);
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;

  $effect(() => {
    // Whenever the overlay style changes, temporarily unmount the visualizer for 1 tick
    // to force the WebKitGTK transparent compositor to completely wipe and flush the old frame buffer
    const _style = $config.ui.overlay_style;
    visible = false;
    const timer = setTimeout(() => {
      visible = true;
    }, 25); // 25ms ensures a full repaint frame ticks in WebKitGTK
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const root = document.documentElement;
    root.style.setProperty("--voxctrl-audio-level", String(currentVolume));
    root.style.setProperty("--voxctrl-recording", $recording ? "1" : "0");
    root.style.setProperty("--voxctrl-processing", $status.processing ? "1" : "0");
    root.style.setProperty("--voxctrl-speaking", $speaking ? "1" : "0");
    root.style.setProperty("--voxctrl-mcp-recording", $mcpRecording ? "1" : "0");
    root.style.setProperty("--voxctrl-audio-ready", $status.audio_ready !== false ? "1" : "0");
  });

  onMount(() => {
    // Add transparent overlay class dynamically to html and body
    document.documentElement.classList.add("overlay-window");
    document.body.classList.add("overlay-window");

    // Force absolute transparency on HTML, body, and App containers to allow Tauri's transparent window to clip correctly
    document.documentElement.style.setProperty("background", "transparent", "important");
    document.body.style.setProperty("background", "transparent", "important");
    
    const appEl = document.getElementById("app");
    if (appEl) {
      appEl.style.setProperty("background", "transparent", "important");
    }

    // Fetch custom overlays from local sharing folder
    invoke<CustomOverlay[]>("get_custom_overlays")
      .then((res) => {
        customOverlays = res;
      })
      .catch((e) => {
        console.error("Failed to load custom overlays:", e);
      });

    // Listen to real-time audio levels from Rust backend
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
      window.dispatchEvent(new CustomEvent("voxctrl-audio-level", { detail: event.payload }));
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let time = 0;
    function updateAnimation() {
      // Smooth interpolation for visual reaction
      currentVolume += (targetVolume - currentVolume) * 0.42;
      targetVolume *= 0.82;

      // Dispatch high-performance window-level custom events
      if (activeCustomOverlay) {
        window.dispatchEvent(new CustomEvent("voxctrl-status", {
          detail: {
            recording: $recording,
            processing: $status.processing,
            speaking: $speaking,
            audio_ready: $status.audio_ready !== false,
            active_target_label: $status.active_target_label || "Focused Window",
            audio_level: currentVolume,
          }
        }));
      }

      animationFrameId = requestAnimationFrame(updateAnimation);
    }
    animationFrameId = requestAnimationFrame(updateAnimation);

    return () => {
      document.documentElement.classList.remove("overlay-window");
      document.body.classList.remove("overlay-window");
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });

  // Action to execute scripts dynamically in inserted html
  function executeScripts(node: HTMLElement) {
    const scripts = node.querySelectorAll("script");
    scripts.forEach((oldScript) => {
      const newScript = document.createElement("script");
      Array.from(oldScript.attributes).forEach((attr) => {
        newScript.setAttribute(attr.name, attr.value);
      });
      newScript.appendChild(document.createTextNode(oldScript.innerHTML));
      if (oldScript.parentNode) {
        oldScript.parentNode.replaceChild(newScript, oldScript);
      }
    });

    return {
      destroy() {
        window.dispatchEvent(new CustomEvent("voxctrl-cleanup"));
      }
    };
  }
</script>

<div class="overlay-root" data-recording={$recording} data-speaking={$speaking} data-processing={$status.processing}>
  {#if renderOverlay && visible}
    {#if $config.ui.overlay_style === "waveform"}
      <Waveform recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "pulse"}
      <Pulse recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "blue_wave"}
      <BlueWave recording={$recording} speaking={$speaking} active={animateActive} />
    {:else if $config.ui.overlay_style === "mono_bars"}
      <MonoBars recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "spectrum"}
      <Spectrum recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "terminal"}
      <Terminal recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "vinyl"}
      <Vinyl recording={$recording} active={animateActive} />
    {:else if activeCustomOverlay}
      {@html `<style>${activeCustomOverlay.css}</style>`}
      <div class="custom-overlay-content" class:active={animateActive} use:executeScripts>
        {@html processedHtml}
      </div>
    {:else if $config.ui.overlay_style !== "none"}
      <VoiceCard recording={$recording} speaking={$speaking} active={animateActive} />
    {/if}

    {#if $speaking}
      <div class="system-response-box speaking" class:on={animateActive}>
        <span class="mini-eq">
          {#each [0, 1, 2, 3, 4] as i}
            <span class="eq-bar" style="animation-delay: {i * 0.13}s"></span>
          {/each}
        </span>
        <span class="pill-text">
          <span class="pill-title">SYSTEM RESPONDING</span>
          <span class="pill-target">▸ {targetLabel}</span>
        </span>
      </div>
    {:else if $mcpRecording}
      <div class="system-response-box mcp" class:on={animateActive}>
        <span class="pulse-dot"></span>
        <span class="pill-text">
          <span class="pill-title">RECORDING</span>
          <span class="pill-target">▸ {targetLabel}</span>
        </span>
      </div>
    {/if}
  {/if}
</div>

<style>
  :global(html), :global(body), :global(#app) {
    background: transparent !important;
    background-color: transparent !important;
    box-shadow: none !important;
    border: none !important;
    outline: none !important;
    overflow: hidden !important;
    will-change: transform, opacity;
    backface-visibility: hidden;
  }

  :global(*:focus) {
    outline: none !important;
    box-shadow: none !important;
  }

  .overlay-root {
    width: 100%;
    height: 100%;
    background: transparent !important;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    user-select: none;
    overflow: hidden !important;
  }

  .system-response-box {
    position: absolute;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 46px;
    padding: 0 20px;
    border-radius: 23px;
    font-family: 'Outfit', 'Inter', system-ui, sans-serif;
    opacity: 0;
    transform: translateY(20px);
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.25),
      opacity 0.28s ease;
  }

  .system-response-box.on {
    opacity: 1;
    transform: translateY(0);
  }

  .system-response-box.speaking {
    background: rgba(4, 47, 36, 0.94);
    border: 1.2px solid rgba(16, 185, 129, 0.55);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5), 0 0 18px rgba(16, 185, 129, 0.25);
    color: #a7f3d0;
  }

  .system-response-box.mcp {
    background: rgba(76, 5, 25, 0.94);
    border: 1.2px solid rgba(244, 63, 94, 0.55);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5), 0 0 18px rgba(244, 63, 94, 0.25);
    color: #fecdd3;
  }

  .mini-eq {
    display: flex;
    align-items: center;
    gap: 2.5px;
    height: 22px;
  }

  .eq-bar {
    width: 3px;
    height: 8px;
    border-radius: 1.5px;
    background: #34d399;
    animation: eq-bounce 0.8s ease-in-out infinite alternate;
  }

  @keyframes eq-bounce {
    from { height: 7px; }
    to { height: 21px; }
  }

  .pill-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .pill-title {
    font-size: 10px;
    font-weight: 850;
    letter-spacing: 0.15em;
  }

  .pill-target {
    font-size: 8.5px;
    font-weight: 600;
    opacity: 0.55;
    max-width: 180px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pulse-dot {
    width: 10px;
    height: 10px;
    background-color: #f43f5e;
    border-radius: 50%;
    animation: flash 1.2s infinite ease-in-out;
  }

  @keyframes flash {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.15); }
  }
</style>
