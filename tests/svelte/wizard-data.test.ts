import { describe, test, expect } from "vitest";
import {
  GESTURES,
  MOONSHINE_MODELS,
  OVERLAY_POSITIONS,
  OVERLAY_STYLES,
  STEP_LABELS,
  STT_ENGINES,
  TTS_ENGINES,
  WHISPER_MODELS,
  accuracyBars,
  formatPercent,
  formatSize,
  isModifiersOnly,
  keycapLabel,
  mapBrowserKeyToEvdev,
  speedLabel,
  ttsSpeedLabel,
  waveBars,
} from "../../src/lib/Wizard/wizard-data";

describe("wizard formatting helpers", () => {
  test("formatSize switches to GB above a thousand megabytes", () => {
    expect(formatSize(75)).toBe("75 MB");
    expect(formatSize(466)).toBe("466 MB");
    expect(formatSize(1500)).toBe("1.5 GB");
    expect(formatSize(3100)).toBe("3.1 GB");
  });

  test("formatSize reports a zero-byte engine as having no download", () => {
    expect(formatSize(0)).toBe("none");
  });

  test("formatPercent rounds to whole percentages", () => {
    expect(formatPercent(0.58)).toBe("58%");
    expect(formatPercent(0.975)).toBe("98%");
  });

  test("speed scores map onto phrases the user can act on", () => {
    expect(speedLabel(0.99)).toBe("instant");
    expect(speedLabel(0.7)).toBe("< 1 s");
    expect(speedLabel(0.45)).toBe("1–3 s");
    expect(speedLabel(0.22)).toBe("3–8 s");
    expect(ttsSpeedLabel(0.99)).toBe("instant");
    expect(ttsSpeedLabel(0.32)).toBe("2–4 s");
  });
});

describe("wizard preview bars", () => {
  test("accuracyBars is deterministic, so a model always draws the same shape", () => {
    const a = accuracyBars(12, 0.8);
    const b = accuracyBars(12, 0.8);
    expect(a).toEqual(b);
    expect(a).toHaveLength(12);
  });

  test("bars past the accuracy mark are dimmed rather than dropped", () => {
    const bars = accuracyBars(10, 0.5);
    expect(bars.slice(0, 5).every((b) => b.o === 1)).toBe(true);
    expect(bars.slice(5).every((b) => b.o < 1)).toBe(true);
  });

  test("bar heights stay inside the box they are drawn in", () => {
    for (const bar of accuracyBars(12, 1)) {
      expect(bar.h).toBeGreaterThanOrEqual(8);
      expect(bar.h).toBeLessThanOrEqual(100);
    }
  });

  test("waveBars gives every bar its own offset so they do not pulse in lockstep", () => {
    const bars = waveBars(14);
    expect(bars).toHaveLength(14);
    expect(new Set(bars.map((b) => b.dl)).size).toBe(14);
  });
});

describe("key mapping", () => {
  test("modifiers map onto the left-hand evdev names bindings.toml uses", () => {
    expect(mapBrowserKeyToEvdev("Control", "ControlLeft")).toBe("KEY_LEFTCTRL");
    expect(mapBrowserKeyToEvdev("Alt", "AltLeft")).toBe("KEY_LEFTALT");
    expect(mapBrowserKeyToEvdev("Shift", "ShiftLeft")).toBe("KEY_LEFTSHIFT");
    expect(mapBrowserKeyToEvdev("Meta", "MetaLeft")).toBe("KEY_LEFTMETA");
  });

  test("letters, digits and named keys map the way the Settings recorder does", () => {
    expect(mapBrowserKeyToEvdev("v", "KeyV")).toBe("KEY_V");
    expect(mapBrowserKeyToEvdev("V", "KeyV")).toBe("KEY_V");
    expect(mapBrowserKeyToEvdev("4", "Digit4")).toBe("KEY_4");
    expect(mapBrowserKeyToEvdev(" ", "Space")).toBe("KEY_SPACE");
    expect(mapBrowserKeyToEvdev("F5", "F5")).toBe("KEY_F5");
    expect(mapBrowserKeyToEvdev("ArrowUp", "ArrowUp")).toBe("KEY_UP");
  });

  test("Escape uses the canonical KEY_ESC spelling, not KEY_ESCAPE", () => {
    // The evdev crate's debug name is KEY_ESC; KEY_ESCAPE is the legacy
    // spelling the config loader migrates away from.
    expect(mapBrowserKeyToEvdev("Escape", "Escape")).toBe("KEY_ESC");
  });

  test("keycapLabel turns evdev names back into something readable", () => {
    expect(keycapLabel("KEY_LEFTMETA")).toBe("Super");
    expect(keycapLabel("KEY_LEFTCTRL")).toBe("Ctrl");
    expect(keycapLabel("KEY_SPACE")).toBe("Space");
    expect(keycapLabel("KEY_V")).toBe("V");
    expect(keycapLabel("KEY_SOMETHING_ODD")).toBe("SOMETHING_ODD");
  });

  test("isModifiersOnly spots a combination that has no regular key", () => {
    expect(isModifiersOnly(["KEY_LEFTCTRL", "KEY_LEFTSHIFT"])).toBe(true);
    expect(isModifiersOnly(["KEY_LEFTCTRL", "KEY_V"])).toBe(false);
    expect(isModifiersOnly([])).toBe(false);
  });
});

describe("wizard tables", () => {
  test("the gesture ids are the ones the routing crate accepts", () => {
    expect(GESTURES.map((g) => g.id).sort()).toEqual(
      ["double_tap", "double_tap_hold", "hold", "toggle"].sort(),
    );
  });

  test("overlay style ids match the values Settings → Visual writes", () => {
    expect(OVERLAY_STYLES.map((o) => o.id)).toEqual([
      "blue_wave",
      "voice_card",
      "waveform",
      "pulse",
      "mono_bars",
      "spectrum",
      "terminal",
      "vinyl",
    ]);
    expect(OVERLAY_POSITIONS.map((p) => p.id)).toEqual(["top", "center", "bottom"]);
  });

  test("TTS engine ids match the TtsConfig engine union", () => {
    expect(TTS_ENGINES.map((t) => t.id).sort()).toEqual(
      ["breeze_tts_2", "espeak", "inflect_micro", "piper", "pocket_tts"].sort(),
    );
  });

  test("eSpeak is the only engine with nothing to download", () => {
    const free = TTS_ENGINES.filter((t) => t.mb === 0).map((t) => t.id);
    expect(free).toEqual(["espeak"]);
  });

  test("STT engines carry the backend values the config expects", () => {
    expect(STT_ENGINES.map((e) => e.id)).toEqual(["whisper-cpp", "moonshine"]);
    expect(STT_ENGINES[0].models).toBe(WHISPER_MODELS);
    expect(STT_ENGINES[1].models).toBe(MOONSHINE_MODELS);
  });

  test("model tables are ordered smallest to largest", () => {
    for (const models of [WHISPER_MODELS, MOONSHINE_MODELS]) {
      const sizes = models.map((m) => m.mb);
      expect([...sizes].sort((a, b) => a - b)).toEqual(sizes);
    }
  });

  test("there are seven steps and the tracker labels them all", () => {
    expect(STEP_LABELS).toHaveLength(7);
    expect(STEP_LABELS[0]).toBe("Welcome");
    expect(STEP_LABELS[STEP_LABELS.length - 1]).toBe("Done");
  });
});
