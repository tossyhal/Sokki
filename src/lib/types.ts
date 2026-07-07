export type GpuMode = "auto" | "force_cpu" | "force_gpu";
export type Language = "ja" | "en" | "auto";

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
