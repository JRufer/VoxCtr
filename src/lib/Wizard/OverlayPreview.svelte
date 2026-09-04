<script lang="ts">
  /**
   * Preview of one overlay style, for the wizard's overlay step.
   *
   * Plays a short recording of the real overlay when one is bundled at
   * `src/assets/overlays/{style id}.webm`, and falls back to a CSS animation
   * in its shape and colour when it is not. The fallback is not only for a
   * missing file: it is what stays on screen while the clip buffers, and what
   * the user sees if the webview cannot decode the clip at all.
   *
   * The clips are bundled rather than fetched from the web on purpose. VoxCtrl
   * runs entirely on-device and its CSP is `default-src 'self'`, so a remote
   * video element would be blocked outright — and a setup wizard that needs the
   * internet to show you a menu would be a poor first impression.
   *
   * `showClip` exists because a `<video>` here is not cheap. WebKitGTK builds a
   * whole GStreamer pipeline per element, and the bundled clips are AV1, which
   * it decodes in software; the overlay step used to mount nine of them at
   * once, which pinned the CPU and took the web process down with it — the
   * step hung and then went white. Callers now say which previews get a real
   * clip, and every other one renders the CSS fallback, which costs nothing.
   */
  let {
    styleId,
    seed = 1,
    showClip = true,
  }: { styleId: string; seed?: number; showClip?: boolean } = $props();

  // Resolved at build time, so a style with no clip simply has no entry and
  // renders its fallback — adding one is dropping a file in the folder.
  const CLIPS = import.meta.glob("../../assets/overlays/*.webm", {
    eager: true,
    query: "?url",
    import: "default",
  }) as Record<string, string>;

  /**
   * Whether this webview can decode the bundled clips at all.
   *
   * Asked once, before any element is built: a webview that answers "" for
   * these codecs would spend a pipeline per preview only to fail, so it is
   * better off going straight to the CSS fallback. `canPlayType` answers
   * "probably"/"maybe"/"" and only the empty string is a definite no.
   */
  const CAN_DECODE = (() => {
    if (typeof document === "undefined") return true;
    const probe = document.createElement("video");
    return (
      probe.canPlayType('video/webm; codecs="av01.0.05M.08"') !== "" ||
      probe.canPlayType("video/webm") !== ""
    );
  })();

  const clip = $derived.by(() => {
    if (!showClip || !CAN_DECODE) return null;
    const match = Object.entries(CLIPS).find(([path]) =>
      path.endsWith(`/${styleId}.webm`),
    );
    return match?.[1] ?? null;
  });

  /** Flipped once the clip has enough data to show a frame, so it fades in
   *  over the fallback instead of flashing a black box. */
  let playable = $state(false);

  // A different style means a different clip: hide the video again until the
  // new one is ready, or the previous frame lingers over the wrong preview.
  $effect(() => {
    void clip;
    playable = false;
  });

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

{#if clip}
  <!-- A silent UI animation, so there is nothing to caption. -->
  <!-- svelte-ignore a11y_media_has_caption -->
  <video
    class="clip"
    class:ready={playable}
    src={clip}
    autoplay
    muted
    loop
    playsinline
    preload="metadata"
    oncanplay={() => (playable = true)}
    onerror={() => (playable = false)}
  ></video>
{/if}

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

  .clip {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    opacity: 0;
    transition: opacity 0.35s ease;
    pointer-events: none;
  }

  .clip.ready {
    opacity: 1;
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
