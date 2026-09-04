import { invoke } from "@tauri-apps/api/core";
import { config } from "../../stores/config";
import { get } from "svelte/store";
import type { AppConfig } from "../../stores/config";
import type { GestureId } from "./wizard-data";
import { GESTURES, STEP_LABELS } from "./wizard-data";

/**
 * Cross-step state for the first-run wizard.
 *
 * Anything the app persists — engine, model, overlay, voice — is written
 * straight into the config store as the user picks it, so the wizard has no
 * "apply" step and a user who quits halfway keeps every choice they made. What
 * lives here is only the state that has nowhere else to be: which step is on
 * screen, the hotkey being recorded, and the log of anything that went wrong.
 */

export interface HotkeyBindingLike {
  id: string;
  label: string;
  keys: string[];
  gesture: string;
  target_id: string;
  target_ids: string[];
  tap_ms: number;
  hold_threshold_ms: number;
  disabled: boolean;
  openai_enabled: boolean;
  openai_model: string;
  openai_mode: string;
  openai_prompt: string;
  openai_system_prompt: string;
}

export interface OutputTargetLike {
  id: string;
  label: string;
  delivery: string;
  [key: string]: unknown;
}

/** Something that went wrong during setup and would keep the app from working
 *  properly once the wizard closes. Surfaced on the final screen. */
export interface WizardIssue {
  /** Stable key, so retrying a step replaces its entry instead of stacking. */
  id: string;
  /** Index into STEP_LABELS, for "Engine · …" attribution. */
  step: number;
  /** One line the user can act on. */
  title: string;
  /** Raw backend error, URLs and all — the part worth pasting into a bug report. */
  detail: string;
}

/** The binding id the wizard owns. Re-used on every save so re-visiting the
 *  hotkey step edits the same binding instead of piling up new ones. */
export const WIZARD_BINDING_ID = "default_hold";

/** The Voice Command Router target the wizard creates and binds to. */
export const COMMAND_TARGET_ID = "command";
export const COMMAND_TARGET_LABEL = "Command";

/**
 * Build the Command target the wizard binds its first hotkey to.
 *
 * Command delivery is a superset of plain injection: a transcription with no
 * "VoxCtrl <target>" phrase in it falls through to typing into the focused
 * window exactly as Inject would, and one with such a phrase is rerouted to the
 * named target instead. So this costs a first-time user nothing and means voice
 * commands work the day they add a second target, without re-binding anything.
 *
 * The shape is cloned from an existing target rather than written out field by
 * field: `OutputTarget` has three dozen delivery-specific fields, and a
 * hand-built literal here would silently rot the next time one is added.
 */
export function buildCommandTarget(template: OutputTargetLike): OutputTargetLike {
  return {
    ...template,
    id: COMMAND_TARGET_ID,
    label: COMMAND_TARGET_LABEL,
    delivery: "command",
    // Command targets route by keyword; the shell-command field belongs to
    // `exec` delivery and must stay empty or the editor flags it as invalid.
    command: null,
  };
}

class WizardState {
  /** Index into STEP_LABELS. */
  step = $state(0);
  /** Furthest step reached, so the tracker knows what is clickable. */
  visited = $state(0);
  /** Set during the cross-fade between steps. */
  leaving = $state(false);

  /** Chosen dictation gesture. Defaults to the same gesture a fresh install
   *  ships with, so "Continue" without touching anything is not a downgrade. */
  gesture = $state<GestureId>("hold");
  /** Captured key combination, or null until the user records one. */
  combo = $state<string[] | null>(null);
  /** True while the recorder is listening for keys. */
  recording = $state(false);
  /** Keys held down right now, shown live in the recorder. */
  held = $state<string[]>([]);
  /** Why the last capture was refused, if it was. */
  captureHint = $state<string | null>(null);

  /** Everything that failed along the way, newest wins per id. */
  issues = $state<WizardIssue[]>([]);

  /** Whether the user has explicitly chosen a transcription backend and a
   *  model size. The config always holds *some* value for both, so without
   *  these the engine step would let someone walk past it having decided
   *  nothing — and the model that then downloads is one they never picked.
   *  Kept here rather than in the step so going back and forth does not forget
   *  a choice already made. */
  engineChosen = $state(false);
  modelChosen = $state(false);

  get gestureInfo() {
    return GESTURES.find((g) => g.id === this.gesture) ?? GESTURES[2];
  }

  /** The combination to display: what is being held while recording, the
   *  captured combo otherwise. */
  get displayKeys(): string[] {
    if (this.recording) return this.held;
    return this.combo ?? [];
  }

  /** Record a failure for the final screen. Re-recording the same id replaces
   *  the previous entry, so a retry that fails twice is reported once. */
  recordIssue(issue: WizardIssue) {
    this.issues = [...this.issues.filter((i) => i.id !== issue.id), issue];
  }

  /** Drop a failure that has since been resolved (a retried download, a
   *  shortcut the desktop finally accepted). */
  clearIssue(id: string) {
    this.issues = this.issues.filter((i) => i.id !== id);
  }

  goTo(n: number) {
    if (n === this.step || this.leaving || n < 0 || n >= STEP_LABELS.length) return;
    this.leaving = true;
    this.recording = false;
    setTimeout(() => {
      this.step = n;
      this.visited = Math.max(this.visited, n);
      this.leaving = false;
    }, 200);
  }

  /**
   * Make sure the Command target exists, and hand back its id.
   *
   * Created here rather than shipped in `default_targets()` so that existing
   * installs are not given a target they never asked for; a first launch is
   * the one moment where adding one is expected.
   */
  async ensureCommandTarget(): Promise<string> {
    const targets = await invoke<OutputTargetLike[]>("get_targets");
    if (targets.some((t) => t.id === COMMAND_TARGET_ID && t.delivery === "command")) {
      return COMMAND_TARGET_ID;
    }
    const template = targets.find((t) => t.delivery === "inject") ?? targets[0];
    if (!template) {
      // No target to clone from at all: fall back to the built-in default
      // rather than writing a target with half its fields missing.
      return "default";
    }
    const next = [
      ...targets.filter((t) => t.id !== COMMAND_TARGET_ID),
      buildCommandTarget(template),
    ];
    await invoke("save_targets", { targets: next });
    return COMMAND_TARGET_ID;
  }

  /**
   * Persist the wizard's binding, replacing whatever the fresh-install
   * defaults were.
   *
   * A first launch is the one moment where overwriting the default bindings is
   * right: the user has just been asked, in as many words, how they want to
   * start talking, and leaving the shipped Super+Space alongside their answer
   * would give them a second shortcut they never asked for.
   */
  async saveBinding(): Promise<void> {
    if (!this.combo || this.combo.length === 0) return;
    const targetId = await this.ensureCommandTarget();
    const existing = await invoke<HotkeyBindingLike[]>("get_bindings").catch(() => []);
    const gest = this.gestureInfo;
    const binding: HotkeyBindingLike = {
      id: WIZARD_BINDING_ID,
      label: `Dictate (${gest.name})`,
      keys: [...this.combo],
      gesture: this.gesture,
      target_id: targetId,
      target_ids: [targetId],
      tap_ms: 300,
      hold_threshold_ms: 200,
      disabled: false,
      openai_enabled: false,
      openai_model: "",
      openai_mode: "custom",
      openai_prompt: "",
      openai_system_prompt: "",
    };
    // Anything the user added by hand in another window survives; only the
    // shipped defaults and a previous pass through this wizard are replaced.
    const kept = existing.filter(
      (b) => b.id !== WIZARD_BINDING_ID && b.id !== "default_toggle" && b.id !== "__tts_stop__",
    );
    await invoke("save_bindings", { bindings: [binding, ...kept] });
  }

  /** Load the binding a previous run of the wizard (or the defaults) left, so
   *  re-opening the hotkey step shows what is actually bound. */
  async loadBinding(): Promise<void> {
    try {
      const bindings = await invoke<HotkeyBindingLike[]>("get_bindings");
      const mine =
        bindings.find((b) => b.id === WIZARD_BINDING_ID) ??
        bindings.find((b) => b.id !== "__tts_stop__");
      if (mine && mine.keys.length > 0) {
        this.combo = [...mine.keys];
        if (GESTURES.some((g) => g.id === mine.gesture)) {
          this.gesture = mine.gesture as GestureId;
        }
      }
    } catch (e) {
      console.error("Wizard: failed to read existing bindings:", e);
    }
  }

  /** Reset every field. Only the tests need this; the wizard itself is built
   *  fresh with its window. */
  reset() {
    this.step = 0;
    this.visited = 0;
    this.leaving = false;
    this.gesture = "hold";
    this.combo = null;
    this.recording = false;
    this.held = [];
    this.captureHint = null;
    this.issues = [];
    this.engineChosen = false;
    this.modelChosen = false;
  }
}

export const wizard = new WizardState();

/**
 * Mutate the config store in place and let its auto-save persist the result.
 *
 * Svelte's store contract needs a `set` to notice a change, and the config
 * store's subscriber is what actually writes the file — so every wizard edit
 * goes through here rather than assigning into `$config` from a dozen places.
 */
export function patchConfig(mutate: (cfg: AppConfig) => void): void {
  // A fresh top-level object rather than the same reference mutated in place:
  // the store is read by this window and re-published to every other one after
  // a save, and handing round a single shared object makes it far too easy for
  // one of those paths to be looking at the very object another is editing.
  const cfg = { ...get(config) };
  mutate(cfg);
  config.set(cfg);
}
