import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLevelEvent,
} from "./types";
import { useRecordingStore } from "../stores/useRecordingStore";

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
    await listen<RecordingLevelEvent>("recording://level", (event) => {
      useRecordingStore.getState().applyLevel(event.payload);
    }),
    await listen<RecordingElapsedEvent>("recording://elapsed", (event) => {
      useRecordingStore.getState().applyElapsed(event.payload);
    }),
    await listen<RecordingDropsEvent>("recording://drops", (event) => {
      useRecordingStore.getState().applyDrops(event.payload);
    }),
  );
}

export function disposeEventListeners() {
  while (unlisteners.length > 0) {
    unlisteners.pop()?.();
  }
  initialized = false;
}
