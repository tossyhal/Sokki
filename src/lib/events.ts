import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  );
}

export function disposeEventListeners() {
  while (unlisteners.length > 0) {
    unlisteners.pop()?.();
  }
  initialized = false;
}
