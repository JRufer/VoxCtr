<script lang="ts">
  import { config } from "../../../stores/config";
  import { patchConfig } from "../wizard-state.svelte";
  import { OVERLAY_POSITIONS, OVERLAY_STYLES } from "../wizard-data";
  import OverlayPreview from "../OverlayPreview.svelte";

  const enabled = $derived($config.ui.show_overlay);
  const style = $derived($config.ui.overlay_style);
  const position = $derived($config.ui.overlay_position);

  const selected = $derived(
    OVERLAY_STYLES.find((o) => o.id === style) ?? OVERLAY_STYLES[0],
  );

  /** Where the preview card sits in the mock screen, as a percentage. */
  const previewTop = $derived(({ top: 16, center: 46, bottom: 76 } as Record<string, number>)[position] ?? 46);

  function setEnabled(on: boolean) {
    patchConfig((cfg) => {
      cfg.ui.show_overlay = on;
    });
  }

  function pickStyle(id: string) {
    patchConfig((cfg) => {
      cfg.ui.overlay_style = id;
      cfg.ui.show_overlay = true;
    });
  }

  function pickPosition(id: string) {
    patchConfig((cfg) => {
      cfg.ui.overlay_position = id;
    });
  }
</script>

<div class="overlay-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 03 · on-screen overlay</span>
      <h2 class="vx-title">Show a signal while listening?</h2>
      <p class="vx-lede">
        The overlay appears the moment your hotkey fires and shows live mic level plus the active
        target — so you know VoxCtrl is hearing you, and where the words are going. Turn it off for
        a silent setup.
      </p>
    </div>

    <div class="choice">
      <button class="vx-card mode" class:vx-on={enabled} onclick={() => setEnabled(true)}>
        <span class="mode-glyph on">⌬</span>
        <span>
          <span class="mode-title">Show overlay</span>
          <span class="mode-desc">Live level · active target</span>
        </span>
      </button>
      <button class="vx-card mode" class:off-on={!enabled} onclick={() => setEnabled(false)}>
        <span class="mode-glyph">⊘</span>
        <span>
          <span class="mode-title">No overlay</span>
          <span class="mode-desc">Silent · tray icon only</span>
        </span>
      </button>
    </div>
  </div>

  <div class="body" class:muted={!enabled}>
    <div class="styles">
      <div class="vx-label">style · {OVERLAY_STYLES.length} built in</div>
      <div class="style-grid">
        {#each OVERLAY_STYLES as o, i}
          <button class="vx-card style-card" class:vx-on={o.id === style} onclick={() => pickStyle(o.id)}>
            <div class="thumb">
              <!-- Only the chosen style gets the real recording here; see
                   OverlayPreview for why nine at once is not an option. -->
              <OverlayPreview seed={i + 1} styleId={o.id} showClip={o.id === style} />
            </div>
            <div class="style-meta">
              <div>
                <div class="style-name">{o.name}</div>
                <div class="style-sub">{o.meta}</div>
              </div>
              <span class="style-glyph" class:on={o.id === style}>{o.glyph}</span>
            </div>
          </button>
        {/each}
      </div>
    </div>

    <div class="position">
      <div class="vx-label">position · {position}</div>
      <div class="screen">
        <div class="grid-lines"></div>
        <div class="taskbar"></div>
        <div class="floating" style:top="{previewTop}%">
          <OverlayPreview seed={OVERLAY_STYLES.indexOf(selected) + 1} styleId={selected.id} showClip />
        </div>
      </div>
      <div class="pos-buttons">
        {#each OVERLAY_POSITIONS as p}
          <button class="pos" class:on={p.id === position} onclick={() => pickPosition(p.id)}>
            <span class="pos-glyph">{p.glyph}</span>{p.label}
          </button>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .overlay-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .head {
    display: flex;
    gap: 24px;
    align-items: flex-end;
    justify-content: space-between;
    flex: none;
    min-width: 0;
  }

  .copy {
    max-width: 720px;
    min-width: 0;
  }

  .choice {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    flex: none;
    width: 520px;
  }

  .mode {
    height: 66px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  /* "No overlay" reads as chosen without borrowing the cyan accent, which in
     this wizard always means "this is switched on". */
  .mode.off-on {
    border-color: var(--vx-line-2);
    background: var(--vx-bg-3);
    box-shadow: 0 0 0 1px var(--vx-line-2);
  }

  .mode-glyph {
    font-family: var(--vx-mono);
    font-size: 22px;
    color: var(--vx-txt-2);
  }

  .mode-glyph.on {
    color: var(--vx-cyan-1);
  }

  .mode-title {
    display: block;
    font-weight: 600;
    font-size: 14px;
  }

  .mode-desc {
    display: block;
    font-size: 12px;
    color: var(--vx-txt-2);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1.7fr 1fr;
    gap: 16px;
    transition: opacity 0.4s, filter 0.4s;
  }

  .body.muted {
    opacity: 0.18;
    filter: grayscale(1) blur(1px);
    pointer-events: none;
  }

  .styles,
  .position {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .style-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px;
    margin-top: 8px;
  }

  .style-card {
    overflow: hidden;
    padding: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    background: #05070a;
    overflow: hidden;
  }

  .style-meta {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
  }

  .style-name {
    font-weight: 600;
    font-size: 13px;
  }

  .style-sub {
    font-family: var(--vx-mono);
    font-size: 10.5px;
    color: var(--vx-txt-2);
  }

  .style-glyph {
    font-family: var(--vx-mono);
    font-size: 14px;
    color: var(--vx-txt-3);
    transition: color 0.3s;
  }

  .style-glyph.on {
    color: var(--vx-cyan-1);
  }

  .screen {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 12px;
    border: 1px solid var(--vx-line-2);
    background: linear-gradient(180deg, var(--vx-bg-2), var(--vx-bg-0));
    overflow: hidden;
    box-shadow: var(--vx-panel-shadow);
    margin-top: 8px;
  }

  .grid-lines {
    position: absolute;
    inset: 0;
    background-image: linear-gradient(to right, rgba(255, 255, 255, 0.03) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(255, 255, 255, 0.03) 1px, transparent 1px);
    background-size: 28px 28px;
  }

  .taskbar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 7%;
    background: rgba(0, 0, 0, 0.5);
    border-top: 1px solid var(--vx-line);
  }

  .floating {
    position: absolute;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 44%;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid rgba(34, 212, 239, 0.4);
    box-shadow: 0 0 24px rgba(34, 212, 239, 0.25);
    background: #05070a;
    transition: top 0.5s var(--vx-ease);
  }

  .pos-buttons {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    margin-top: 10px;
  }

  .pos {
    height: 46px;
    border-radius: 10px;
    border: 1px solid var(--vx-line);
    background: rgba(255, 255, 255, 0.02);
    color: var(--vx-txt-1);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.25s;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .pos:hover {
    border-color: var(--vx-line-2);
  }

  .pos.on {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.12);
    color: var(--vx-cyan-1);
  }

  .pos-glyph {
    font-family: var(--vx-mono);
  }

  @media (max-width: 1100px) {
    .head {
      flex-direction: column;
      align-items: stretch;
    }

    .choice {
      width: 100%;
    }

    .body {
      grid-template-columns: 1fr;
    }

    .style-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
