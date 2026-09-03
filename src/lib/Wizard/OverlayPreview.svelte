<script lang="ts">
  /**
   * A small animated stand-in for an overlay style.
   *
   * Not the real overlay: that one is a separate always-on-top window driven by
   * live mic levels, and there is no microphone running while the wizard is on
   * screen. What this gives the user is the shape and colour of what they will
   * see — enough to choose between eight of them — without pretending to be a
   * live meter.
   */
  let { styleId, seed = 1 }: { styleId: string; seed?: number } = $props();

  /** Deterministic per-style bar profile, so a style always looks like itself. */
  const bars = $derived(
    Array.from({ length: 14 }, (_, i) => {
      const v = 0.3 + 0.7 * Math.abs(Math.sin(seed * 2.3 + i * 1.3));
      return {
        h: Math.round(v * 100),
        delay: ((i * 0.11 + seed * 0.07) % 1.1).toFixed(2),
        dur: (0.9 + ((i * 5) % 4) * 0.15).toFixed(2),
      };
    }),
  );

  const family = $derived(
    styleId === "pulse"
      ? "ring"
      : styleId === "vinyl"
        ? "needle"
        : styleId === "terminal"
          ? "terminal"
          : styleId === "waveform" || styleId === "blue_wave"
            ? "wave"
            : "bars",
  );

  const mono = $derived(styleId === "mono_bars");
</script>

<div class="preview" class:mono>
  {#if family === "bars"}
    <div class="bars">
      {#each bars.slice(0, styleId === "mono_bars" ? 5 : 14) as b}
        <div style:height="{b.h}%" style:animation-delay="{b.delay}s" style:animation-duration="{b.dur}s"></div>
      {/each}
    </div>
  {:else if family === "wave"}
    <svg viewBox="0 0 100 40" preserveAspectRatio="none" class="wave">
      <path
        d="M0 20 Q 8 6 16 20 T 32 20 T 48 20 T 64 20 T 80 20 T 96 20 T 112 20"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
      />
    </svg>
  {:else if family === "ring"}
    <div class="ring-wrap">
      <div class="ring"></div>
      <div class="ring r2"></div>
      <div class="ring-core"></div>
    </div>
  {:else if family === "needle"}
    <div class="dial">
      <div class="needle"></div>
      <div class="dial-arc"></div>
    </div>
  {:else}
    <div class="terminal">
      <span>&gt; listening</span>
      <div class="blocks">
        {#each bars.slice(0, 10) as b}
          <div style:animation-delay="{b.delay}s"></div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .preview {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--vx-cyan-0);
    background: radial-gradient(120% 120% at 50% 120%, rgba(34, 212, 239, 0.1), transparent 70%);
  }

  .preview.mono {
    color: #dfe6ee;
    background: none;
  }

  .bars {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 3px;
    width: 64%;
    height: 56%;
  }

  .bars > div {
    flex: 1;
    min-height: 10%;
    border-radius: 2px;
    background: currentColor;
    transform-origin: bottom;
    animation-name: vxBar;
    animation-iteration-count: infinite;
    animation-timing-function: ease-in-out;
    opacity: 0.9;
  }

  .wave {
    width: 76%;
    height: 46%;
    color: inherit;
    animation: waveShift 2.6s linear infinite;
  }

  @keyframes waveShift {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(-16%);
    }
  }

  .ring-wrap {
    position: relative;
    width: 46%;
    aspect-ratio: 1;
    display: grid;
    place-items: center;
  }

  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 1.5px solid currentColor;
    opacity: 0.4;
    animation: vxPulse 1.8s ease-in-out infinite;
  }

  .ring.r2 {
    inset: 22%;
    animation-delay: 0.5s;
  }

  .ring-core {
    width: 26%;
    aspect-ratio: 1;
    border-radius: 50%;
    background: currentColor;
    animation: vxPulse 1.8s ease-in-out infinite;
    animation-delay: 0.25s;
  }

  .dial {
    position: relative;
    width: 62%;
    aspect-ratio: 2 / 1;
    display: grid;
    place-items: end center;
  }

  .dial-arc {
    position: absolute;
    inset: 0;
    border-radius: 999px 999px 0 0;
    border: 1.5px solid currentColor;
    border-bottom: 0;
    opacity: 0.35;
  }

  .needle {
    position: absolute;
    bottom: 0;
    left: 50%;
    width: 2px;
    height: 82%;
    background: var(--vx-gold-1);
    transform-origin: bottom center;
    animation: needleSwing 2.2s ease-in-out infinite;
  }

  @keyframes needleSwing {
    0%,
    100% {
      transform: rotate(-38deg);
    }
    50% {
      transform: rotate(34deg);
    }
  }

  .terminal {
    width: 76%;
    font-family: var(--vx-mono);
    font-size: 9px;
    color: #7ab8ff;
    display: grid;
    gap: 5px;
  }

  .blocks {
    display: flex;
    gap: 2px;
    height: 10px;
  }

  .blocks > div {
    flex: 1;
    background: #7ab8ff;
    transform-origin: bottom;
    animation: vxBar 1.1s ease-in-out infinite;
  }
</style>
