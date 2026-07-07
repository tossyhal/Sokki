import { invoke } from "@tauri-apps/api/core";
import type { SystemInfo } from "./types";

export function getSystemInfo(): Promise<SystemInfo> {
  return invoke<SystemInfo>("get_system_info");
}
