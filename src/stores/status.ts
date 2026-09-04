import { writable, derived } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  recording: boolean;
  processing: boolean;
  speaking: boolean;
  mcp_recording: boolean;
  audio_ready?: boolean;
  word_count: number;
  active_target_id?: string;
  active_target_label?: string;
}

export const status = writable<AppStatus>({
  recording: false,
  processing: false,
  speaking: false,
  mcp_recording: false,
  audio_ready: true,
  word_count: 0,
  active_target_id: "default",
  active_target_label: "Focused Window",
});

export const recording = derived(status, ($s) => $s.recording);
export const speaking = derived(status, ($s) => $s.speaking);
export const mcpRecording = derived(status, ($s) => $s.mcp_recording);
export const wordCount = derived(status, ($s) => $s.word_count);
export const activeTargetLabel = derived(status, ($s) => $s.active_target_label ?? "Focused Window");

// Listen to periodic status ticks from the Rust backend
let lastTickAt = 0;

listen<AppStatus>("status-tick", (event) => {
  lastTickAt = Date.now();
  status.set(event.payload);
});

// Initial fetch
invoke<AppStatus>("get_status").then(status.set).catch(console.error);

/**
 * Poll for status when the ticks stop arriving.
 *
 * The backend broadcasts `status-tick` every 150ms, so a gap this long means
 * the events are not reaching this window at all — which is survivable for a
 * status readout only if there is another way to get the answer. `invoke`
 * keeps working when the event channel does not, so it is the fallback.
 *
 * This costs nothing while events flow: the interval fires, sees a recent
 * tick, and does no IPC.
 */
const TICK_STALE_MS = 2000;
const POLL_INTERVAL_MS = 1000;

setInterval(() => {
  if (Date.now() - lastTickAt < TICK_STALE_MS) return;
  invoke<AppStatus>("get_status")
    .then(status.set)
    .catch(() => {
      // Nothing useful to do: the next poll tries again.
    });
}, POLL_INTERVAL_MS);
