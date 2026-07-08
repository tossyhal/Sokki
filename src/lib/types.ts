export type GpuMode = "auto" | "force_cpu" | "force_gpu";
export type Language = "ja" | "en" | "auto";
export type Source = "mic" | "system" | "mix" | "import";
export type ExportFormat = "txt" | "srt" | "md";
export type SessionStatus =
  | "recording"
  | "transcribing"
  | "done"
  | "error"
  | "interrupted";

export interface AppError {
  code: string;
  message: string;
}

export interface SystemInfo {
  appVersion: string;
  compiledGpuSupport: boolean;
  requestedBackend: GpuMode;
  activeBackend: "gpu" | "cpu" | "none";
  gpuErrorMessage: string | null;
  modelsDir: string;
  dataDir: string;
}

export interface Settings {
  defaultModel: string;
  language: Language;
  gpuMode: GpuMode;
  micDevice: string | null;
  loopbackDevice: string | null;
  micGain: number;
  systemGain: number;
  vadThresholdDb: number;
  onboardingDone: boolean;
  soundCheckRecommended: boolean;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface AudioDevices {
  inputs: AudioDevice[];
  outputs: AudioDevice[];
}

export interface Session {
  id: string;
  title: string;
  createdAt: number;
  durationMs: number;
  audioPath: string | null;
  source: Source;
  language: Language;
  model: string;
  status: SessionStatus;
  errorMessage: string | null;
  dropCount: number;
}

export interface Segment {
  id: number;
  sessionId: string;
  startMs: number;
  endMs: number;
  text: string;
  lang: string | null;
}

export interface StartRecordingRequest {
  source: Exclude<Source, "import">;
  language: Language;
  model: string;
  micDevice?: string | null;
  loopbackDevice?: string | null;
}

export interface RecordingState {
  active: boolean;
  sessionId: string | null;
  paused: boolean;
  elapsedMs: number;
}

export interface RecordingLevelEvent {
  mic: number;
  system: number;
}

export interface RecordingElapsedEvent {
  elapsedMs: number;
}

export interface RecordingDropsEvent {
  dropCount: number;
}

export interface SoundCheckRequest {
  source: Exclude<Source, "import">;
  micDevice?: string | null;
  loopbackDevice?: string | null;
  durationMs?: number | null;
}

export interface SoundCheckResult {
  wavPath: string;
  durationMs: number;
  peakMicDb: number;
  peakSystemDb: number;
  warnings: string[];
}

export interface SoundCheckLevelEvent {
  mic: number;
  system: number;
}

export interface SessionStatusEvent {
  sessionId: string;
  status: SessionStatus;
  message?: string | null;
}

export interface ImportFilesRequest {
  paths: string[];
  language: Language;
  model: string;
  forceUnknownDuration?: boolean | null;
}

export interface ImportFileResult {
  path: string;
  ok: boolean;
  sessionId: string | null;
  errorCode: string | null;
  errorMessage: string | null;
}

export interface RetranscribeSessionRequest {
  id: string;
  language: Language;
  model: string;
}

export interface ExportSessionRequest {
  sessionId: string;
  format: ExportFormat;
  path: string;
}
