<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, active = true } = $props();

  // Mirrors the analog VU needle constants in src-tauri/src/overlay.rs
  const VU_PIVOT_X = 144;
  const VU_PIVOT_Y = 70;
  const VU_RADIUS = 58;
  const VU_ANGLE_MIN = -0.95;
  const VU_ANGLE_MAX = 0.95;
  const DT = 0.016;

  let needlePath = $state(vuNeedlePath(VU_ANGLE_MIN));
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  function vuTargetAngle(level: number): number {
    return VU_ANGLE_MIN + Math.min(1, Math.max(0, level)) * (VU_ANGLE_MAX - VU_ANGLE_MIN);
  }

  function vuNeedlePath(angle: number): string {
    const x = VU_PIVOT_X + Math.sin(angle) * VU_RADIUS;
    const y = VU_PIVOT_Y - Math.cos(angle) * VU_RADIUS;
    return `M ${VU_PIVOT_X} ${VU_PIVOT_Y} L ${x.toFixed(1)} ${y.toFixed(1)}`;
  }

  // Mirrors spring() in src-tauri/src/overlay.rs
  function spring(state: { x: number; v: number }, target: number, dt: number) {
    const omega = 16.0;
    const zeta = 0.78;
    const a = omega * omega * (target - state.x) - 2.0 * zeta * omega * state.v;
    state.v += a * dt;
    state.x += state.v * dt;
    if (Math.abs(state.x - target) < 0.001 && Math.abs(state.v) < 0.02) {
      state.x = target;
      state.v = 0;
    }
  }

  onMount(() => {
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let phase = 0;
    const needle = { x: VU_ANGLE_MIN, v: 0 };

    function update() {
      phase += DT;
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;

      const ready = $status.audio_ready !== false;
      let target: number;
      if ($status.processing) {
        target = vuTargetAngle(0.3 + 0.25 * Math.abs(Math.sin(phase * 2.0)));
      } else if (recording && ready) {
        target = vuTargetAngle(currentVolume);
      } else {
        target = VU_ANGLE_MIN;
      }
      spring(needle, target, DT);
      needlePath = vuNeedlePath(needle.x);

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div class="vinyl" class:on={active}>
  <div class="header">
    <span class="vu-label">VU</span>
    <span class="subtitle">
      {#if $status.processing}
        ANALOG // PROCESSING
      {:else if recording && !isReady}
        ANALOG // WARMING UP
      {:else}
        ANALOG // INPUT LEVEL
      {/if}
    </span>
    <span class="spacer"></span>
    <span class="led" class:processing={$status.processing} class:standby={recording && !isReady}></span>
  </div>

  <div class="face">
    <div class="tick" style="left: 18px;"></div>
    <div class="tick" style="left: 60px;"></div>
    <div class="tick" style="left: 102px;"></div>
    <div class="tick" style="left: 144px;"></div>
    <div class="tick" style="left: 186px;"></div>
    <div class="tick" style="left: 228px;"></div>
    <div class="tick red" style="left: 270px;"></div>
    <div class="scale-label" style="left: 12px; top: 40px;">-20</div>
    <div class="scale-label" style="left: 122px; top: 40px;">0</div>
    <div class="scale-label red" style="left: 252px; top: 34px;">+3</div>

    <svg class="needle-svg" viewBox="0 0 288 72" preserveAspectRatio="none">
      <path class="needle" d={needlePath} />
    </svg>
    <div class="pivot"></div>
  </div>

  <div class="target">{targetLabel}</div>
</div>

<style>
  /* Vintage VU meter: fades in and settles slightly downward on load */
  .vinyl {
    width: 320px;
    height: 132px;
    background: linear-gradient(180deg, #f7ecd9 0%, #e7d6b8 100%);
    border: 1.5px solid rgba(120, 89, 53, 0.35);
    border-radius: 14px;
    box-shadow: 0 0 22px rgba(0, 0, 0, 0.35);
    position: relative;
    pointer-events: none;
    user-select: none;
    transform: translateY(16px);
    opacity: 0;
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.28s ease;
  }

  .vinyl.on {
    transform: translateY(0);
    opacity: 1;
  }

  .header {
    position: absolute;
    left: 16px;
    top: 12px;
    right: 16px;
    height: 14px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .vu-label {
    color: #5b4530;
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.17em;
  }

  .subtitle {
    color: rgba(91, 69, 48, 0.55);
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.19em;
  }

  .spacer {
    flex: 1;
  }

  .led {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #dc2626;
    box-shadow: 0 0 6px #dc2626;
    animation: vinyl-blink 1.2s ease-in-out infinite;
    flex-shrink: 0;
  }

  .led.standby {
    background: #f59e0b;
    box-shadow: 0 0 6px #f59e0b;
  }

  .led.processing {
    background: #38bdf8;
    box-shadow: 0 0 6px #38bdf8;
  }

  @keyframes vinyl-blink {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .face {
    position: absolute;
    left: 16px;
    top: 30px;
    width: 288px;
    height: 72px;
    background: rgba(255, 252, 244, 0.55);
    border: 1px solid rgba(120, 89, 53, 0.25);
    border-radius: 8px;
  }

  .tick {
    position: absolute;
    top: 14px;
    width: 1.5px;
    height: 22px;
    background: rgba(91, 69, 48, 0.4);
  }

  .tick.red {
    background: #dc2626;
    height: 30px;
  }

  .scale-label {
    position: absolute;
    color: rgba(91, 69, 48, 0.5);
    font-size: 7px;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .scale-label.red {
    color: #dc2626;
    font-weight: 800;
  }

  .needle-svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .needle {
    fill: none;
    stroke: #292524;
    stroke-width: 2px;
    vector-effect: non-scaling-stroke;
  }

  .pivot {
    position: absolute;
    left: 138px;
    top: 64px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #292524;
  }

  .target {
    position: absolute;
    left: 16px;
    bottom: 12px;
    width: 288px;
    color: rgba(91, 69, 48, 0.65);
    font-size: 9.5px;
    font-weight: 700;
    font-family: 'Outfit', 'Inter', sans-serif;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
