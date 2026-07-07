import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLevelEvent,
} from "./types";

let initialized = false;
const unlisteners: UnlistenFn[] = [];

export async function initEventListeners() {
  if (initialized) {
    return;
  }
  initialized = true;

  unlisteners.push(
    await listen("session://status", () => {
      // Store wiring is added with the session store.
    }),
    await listen<RecordingLevelEvent>("recording://level", () => {
      // Store wiring is added with the recording store.
    }),
    await listen<RecordingElapsedEvent>("recording://elapsed", () => {
      // Store wiring is added with the recording store.
    }),
    await listen<RecordingDropsEvent>("recording://drops", () => {
      // Toast wiring is added with the recording in-progress UI.
    }),
  );
}

export function disposeEventListeners() {
  while (unlisteners.length > 0) {
    unlisteners.pop()?.();
  }
  initialized = false;
}
