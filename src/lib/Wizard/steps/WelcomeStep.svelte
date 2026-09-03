<script lang="ts">
  import appIcon from "../../../assets/app_icon.png";
  import { wizard } from "../wizard-state.svelte";

  const cards = [
    { glyph: "∇", n: "01", title: "Transcription engine", desc: "whisper.cpp or Moonshine, sized to your hardware." },
    { glyph: "⌁", n: "02", title: "Hotkey", desc: "A gesture and a key combo to start talking." },
    { glyph: "⌬", n: "03", title: "Overlay", desc: "A visual signal while the mic is live — or none." },
    { glyph: "◉", n: "04", title: "Live test", desc: "Dictate into a box and watch it land." },
    { glyph: "⊕", n: "05", title: "Voice output", desc: "Optional: let agents talk back through your speakers." },
  ];
</script>

<div class="welcome">
  <div class="intro">
    <div class="halo">
      <div class="ring"></div>
      <div class="ring inner"></div>
      <img src={appIcon} alt="VoxCtrl" />
    </div>
    <span class="vx-eyebrow">// welcome</span>
    <h1>Speak to <span>anything</span> on your machine.</h1>
    <p>
      A few minutes of setup. Pick a transcription engine, bind a hotkey, choose how VoxCtrl shows
      itself, and test it end to end. Everything runs on this device — nothing leaves it.
    </p>
    <div class="reassure">
      <span>↺</span> Nothing is permanent — every choice can be changed later in Settings.
    </div>
  </div>

  <div class="cards">
    {#each cards as c, i}
      <button class="vx-card step-card" onclick={() => wizard.goTo(i + 1)}>
        <div class="head">
          <span class="glyph">{c.glyph}</span>
          <span class="num">{c.n}</span>
        </div>
        <div class="title">{c.title}</div>
        <div class="desc">{c.desc}</div>
      </button>
    {/each}
  </div>
</div>

<style>
  .welcome {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1.1fr 1fr;
    gap: 48px;
    align-items: center;
  }

  .halo {
    position: relative;
    width: 96px;
    height: 96px;
    display: grid;
    place-items: center;
    margin-bottom: 22px;
  }

  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 1px solid rgba(34, 212, 239, 0.25);
    animation: vxPulse 2.4s infinite;
  }

  .ring.inner {
    inset: 12px;
    border-color: rgba(34, 212, 239, 0.15);
    animation-delay: 0.6s;
  }

  .halo img {
    width: 52px;
    height: 52px;
    border-radius: 12px;
    box-shadow: 0 0 16px rgba(34, 212, 239, 0.4);
  }

  h1 {
    margin: 12px 0 14px;
    font-size: clamp(30px, 4vw, 50px);
    line-height: 1.02;
    letter-spacing: -0.035em;
    font-weight: 600;
  }

  h1 span {
    color: var(--vx-gold-1);
  }

  .intro p {
    margin: 0;
    max-width: 560px;
    font-size: 15px;
    line-height: 1.55;
    color: var(--vx-txt-1);
  }

  .reassure {
    margin-top: 22px;
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 9px 14px;
    border: 1px solid rgba(34, 212, 239, 0.25);
    border-radius: 999px;
    background: rgba(34, 212, 239, 0.04);
    font-family: var(--vx-mono);
    font-size: 11.5px;
    color: var(--vx-cyan-2);
  }

  .reassure span {
    color: var(--vx-gold-1);
  }

  .cards {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .step-card {
    padding: 16px 18px;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.02), rgba(255, 255, 255, 0));
  }

  .step-card:hover {
    border-color: rgba(34, 212, 239, 0.4);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
  }

  .glyph {
    font-family: var(--vx-mono);
    font-size: 20px;
    color: var(--vx-cyan-1);
  }

  .num {
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-3);
  }

  .title {
    font-weight: 600;
    font-size: 14.5px;
    margin-bottom: 4px;
  }

  .desc {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--vx-txt-2);
  }

  @media (max-width: 1000px) {
    .welcome {
      grid-template-columns: 1fr;
      gap: 24px;
      align-content: start;
    }
  }
</style>
