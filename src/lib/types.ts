export type GpuMode = "auto" | "force_cpu" | "force_gpu";

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
