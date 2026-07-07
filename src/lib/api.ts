import { invoke } from "@tauri-apps/api/core";
import type {
  AudioDevices,
  RecordingState,
  Session,
  Settings,
  SoundCheckRequest,
  SoundCheckResult,
  StartRecordingRequest,
  SystemInfo,
} from "./types";

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

export function startRecording(request: StartRecordingRequest): Promise<string> {
  return invoke<string>("start_recording", { request });
}

export function pauseRecording(): Promise<void> {
  return invoke<void>("pause_recording");
}

export function resumeRecording(): Promise<void> {
  return invoke<void>("resume_recording");
}

export function stopRecording(): Promise<Session> {
  return invoke<Session>("stop_recording");
}

export function getRecordingState(): Promise<RecordingState> {
  return invoke<RecordingState>("get_recording_state");
}

export function runSoundCheck(request: SoundCheckRequest): Promise<SoundCheckResult> {
  return invoke<SoundCheckResult>("run_sound_check", { request });
}
