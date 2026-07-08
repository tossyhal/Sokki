import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLevelEvent,
  SessionStatusEvent,
  SoundCheckLevelEvent,
} from "./types";
import { useRecordingStore } from "../stores/useRecordingStore";
import { useSessionStore } from "../stores/useSessionStore";

let initialized = false;
const unlisteners: UnlistenFn[] = [];

export async function initEventListeners() {
  if (initialized) {
    return;
  }
  initialized = true;

  unlisteners.push(
    await listen<SessionStatusEvent>("session://status", (event) => {
      useSessionStore
        .getState()
        .applyStatus(event.payload.sessionId, event.payload.status, event.payload.message);
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
    await listen<SoundCheckLevelEvent>("soundcheck://level", (event) => {
      useRecordingStore.getState().applySoundCheckLevel(event.payload);
    }),
  );
}

export function disposeEventListeners() {
  while (unlisteners.length > 0) {
    unlisteners.pop()?.();
  }
  initialized = false;
}
