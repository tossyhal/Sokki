import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLimitEvent,
  RecordingLevelEvent,
  ModelDoneEvent,
  ModelErrorEvent,
  ModelProgressEvent,
  Segment,
  SessionStatusEvent,
  SoundCheckLevelEvent,
} from "./types";
import { useModelStore } from "../stores/useModelStore";
import { useRecordingStore } from "../stores/useRecordingStore";
import { useSessionStore } from "../stores/useSessionStore";

let initialized = false;
let initPromise: Promise<void> | null = null;
const unlisteners: UnlistenFn[] = [];

export async function initEventListeners() {
  if (initialized) {
    return;
  }
  if (initPromise) {
    return initPromise;
  }

  initPromise = registerEventListeners();
  return initPromise;
}

export function disposeEventListeners() {
  while (unlisteners.length > 0) {
    unlisteners.pop()?.();
  }
  initialized = false;
  initPromise = null;
}

async function registerEventListeners() {
  const registered: UnlistenFn[] = [];

  try {
    registered.push(
      await listen<SessionStatusEvent>("session://status", (event) => {
        useSessionStore
          .getState()
          .applyStatus(event.payload.sessionId, event.payload.status, event.payload.message);
      }),
    );
    registered.push(
      await listen<Segment>("transcript://segment", (event) => {
        useSessionStore.getState().applySegment(event.payload);
      }),
    );
    registered.push(
      await listen<RecordingLevelEvent>("recording://level", (event) => {
        useRecordingStore.getState().applyLevel(event.payload);
      }),
    );
    registered.push(
      await listen<RecordingElapsedEvent>("recording://elapsed", (event) => {
        useRecordingStore.getState().applyElapsed(event.payload);
      }),
    );
    registered.push(
      await listen<RecordingDropsEvent>("recording://drops", (event) => {
        useRecordingStore.getState().applyDrops(event.payload);
      }),
    );
    registered.push(
      await listen<RecordingLimitEvent>("recording://limit", (event) => {
        void useRecordingStore.getState().applyLimit(event.payload);
      }),
    );
    registered.push(
      await listen<SoundCheckLevelEvent>("soundcheck://level", (event) => {
        useRecordingStore.getState().applySoundCheckLevel(event.payload);
      }),
    );
    registered.push(
      await listen<ModelProgressEvent>("model://progress", (event) => {
        useModelStore.getState().applyProgress(event.payload);
      }),
    );
    registered.push(
      await listen<ModelDoneEvent>("model://done", (event) => {
        void useModelStore.getState().applyDone(event.payload.name);
      }),
    );
    registered.push(
      await listen<ModelErrorEvent>("model://error", (event) => {
        useModelStore.getState().applyError(event.payload);
      }),
    );
    unlisteners.push(...registered);
    initialized = true;
  } catch (error) {
    registered.forEach((unlisten) => unlisten());
    initialized = false;
    throw error;
  } finally {
    initPromise = null;
  }
}
