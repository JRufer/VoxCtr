<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import appIcon from "../../assets/voxctrl.gif";

  let version = $state("0.1.0");

  onMount(async () => {
    try {
      version = await getVersion();
    } catch (e) {
      console.error("Failed to fetch app version:", e);
    }
  });
</script>

<section>
  <h2>About VoxCtrl</h2>

  <div class="field-group about-card">
    <img src={appIcon} class="logo animated-logo" alt="VoxCtrl Logo" />
    <div>
      <div class="app-name">VoxCtrl</div>
      <div class="app-version">Version {version} — Rust + Tauri Edition</div>
      <p>
        Native, on-device voice-to-text for Linux and Windows.
        Uses <a href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>
        and <a href="https://github.com/usefulsensors/moonshine" target="_blank">Moonshine</a>
        for offline transcription and routes speech to any destination.
      </p>
    </div>
  </div>

  <div class="field-group">
    <h3>System</h3>
    <div class="kv"><span>Frontend</span><span>Svelte 5 + Tauri 2</span></div>
    <div class="kv"><span>Backend</span><span>Rust (Tokio async)</span></div>
    <div class="kv"><span>Inference</span><span>whisper.cpp & Moonshine</span></div>
    <div class="kv"><span>Config</span><span><code>~/.config/voxctrl/</code></span></div>
    <div class="kv"><span>Models</span><span><code>~/.local/share/voxctrl/models/</code></span></div>
    <div class="kv"><span>MCP socket</span><span><code>/tmp/voxctrl-mcp.sock</code></span></div>
  </div>

  <div class="field-group">
    <h3>Open Source Attributions</h3>
    <p class="credits-hint">This application is built possible by these outstanding open-source projects:</p>
    <div class="credits-list">
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/usefulsensors/moonshine" target="_blank">Useful Sensors Moonshine</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://tauri.app" target="_blank">Tauri Framework</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://svelte.dev" target="_blank">Svelte & Vite</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/rhasspy/piper" target="_blank">Piper TTS</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/kyutai-labs/pocket-tts" target="_blank">Pocket-TTS (Kyutai Labs)</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/huggingface/candle" target="_blank">Candle</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/trossora/whisper-rs" target="_blank">whisper-rs</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://rust-lang.org" target="_blank">Rust, Tokio & CPAL</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
    </div>
  </div>

  <div class="field-group">
    <h3>Links</h3>
    <a href="https://github.com/jrufer/voxctrl" target="_blank">GitHub Repository</a>
  </div>
</section>

<style>
  @reference "tailwindcss";

  .about-card {
    @apply flex-row! items-start gap-5;
  }
  .logo {
    @apply w-16 h-16 object-contain drop-shadow-[0_0_10px_rgba(56,189,248,0.6)] drop-shadow-[0_4px_24px_rgba(56,189,248,0.35)] rounded-xl;
  }
  .app-name {
    @apply text-2xl font-bold mb-1;
  }
  .app-version {
    @apply text-xs text-[var(--text-muted)] mb-2.5;
  }
  p {
    @apply text-[13px] text-[var(--text-muted)] leading-relaxed max-w-[400px];
  }
  a {
    @apply text-[var(--accent2)] text-[13px] no-underline;
  }
  a:hover {
    @apply underline;
  }
  .kv {
    @apply flex justify-between text-xs py-1 border-b border-[var(--border)];
  }
  .kv span:first-child {
    @apply text-[var(--text-muted)];
  }

  /* Open Source Attribution styling */
  .credits-hint {
    @apply text-xs text-[var(--text-muted)] mb-3;
  }
  .credits-list {
    @apply flex flex-col gap-2 mt-1;
  }
  .credit-item {
    @apply flex justify-between items-center py-1.5 border-b border-[var(--border)] last:border-none;
  }
  .credit-name-link {
    @apply text-[13px] font-normal text-white no-underline transition-colors duration-150 ease-out;
  }
  .credit-name-link:hover {
    @apply text-[var(--color-accent-blue)] underline;
  }
  .credit-license {
    @apply text-[10px] bg-[var(--color-accent-blue)]/8 text-[var(--color-accent-blue)] p-0.5 px-1.5 rounded border border-[var(--color-accent-blue)]/15 font-normal;
  }
</style>
