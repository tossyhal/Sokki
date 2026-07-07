import { invoke } from "@tauri-apps/api/core";
import type { AudioDevices, Settings, SystemInfo } from "./types";

export function getSystemInfo(): Promise<SystemInfo> {
  return invoke<SystemInfo>("get_system_info");
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function updateSettings(patch: Partial<Settings>): Promise<Settings> {
  return invoke<Settings>("update_settings", { patch });
}

export function listAudioDevices(): Promise<AudioDevices> {
  return invoke<AudioDevices>("list_audio_devices");
}
