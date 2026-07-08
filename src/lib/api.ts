import { invoke } from "@tauri-apps/api/core";
import type {
  AudioDevices,
  ExportSessionRequest,
  ImportFileResult,
  ImportFilesRequest,
  ModelInfo,
  RecordingState,
  RetranscribeSessionRequest,
  Segment,
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

export function getModels(): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("get_models");
}

export function downloadModel(name: string): Promise<void> {
  return invoke<void>("download_model", { name });
}

export function cancelDownload(name: string): Promise<void> {
  return invoke<void>("cancel_download", { name });
}

export function deleteModel(name: string): Promise<ModelInfo> {
  return invoke<ModelInfo>("delete_model", { name });
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

export function importFiles(request: ImportFilesRequest): Promise<ImportFileResult[]> {
  return invoke<ImportFileResult[]>("import_files", { request });
}

export function cancelTranscription(id: string): Promise<Session> {
  return invoke<Session>("cancel_transcription", { id });
}

export function getSessions(): Promise<Session[]> {
  return invoke<Session[]>("get_sessions");
}

export function getSession(id: string): Promise<Session> {
  return invoke<Session>("get_session", { id });
}

export function getSegments(sessionId: string): Promise<Segment[]> {
  return invoke<Segment[]>("get_segments", { sessionId });
}

export function retranscribeSession(request: RetranscribeSessionRequest): Promise<void> {
  return invoke<void>("retranscribe_session", { request });
}

export function exportSession(request: ExportSessionRequest): Promise<void> {
  return invoke<void>("export_session", { request });
}

export function renameSession(id: string, title: string): Promise<Session> {
  return invoke<Session>("rename_session", { id, title });
}

export function deleteSession(id: string): Promise<void> {
  return invoke<void>("delete_session", { id });
}
