use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::error::{AppError, WHISPER_ERROR, WHISPER_GPU_UNAVAILABLE};
use crate::settings::GpuMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveBackend {
    Gpu,
    Cpu,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperRuntimeInfo {
    pub compiled_gpu_support: bool,
    pub requested_backend: GpuMode,
    pub active_backend: ActiveBackend,
    pub gpu_error_message: Option<String>,
}

pub struct WhisperContextManager {
    state: Mutex<WhisperContextState>,
}

struct WhisperContextState {
    loaded_model: Option<LoadedWhisperContext>,
    active_backend: ActiveBackend,
    gpu_error_message: Option<String>,
}

struct LoadedWhisperContext {
    model_name: String,
    context: Arc<whisper_rs::WhisperContext>,
}

impl WhisperContextManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(WhisperContextState {
                loaded_model: None,
                active_backend: ActiveBackend::None,
                gpu_error_message: None,
            }),
        }
    }

    pub fn runtime_info(&self, requested_backend: GpuMode) -> WhisperRuntimeInfo {
        let state = self
            .state
            .lock()
            .expect("whisper context mutex should not be poisoned");
        WhisperRuntimeInfo {
            compiled_gpu_support: compiled_gpu_support(),
            requested_backend,
            active_backend: state.active_backend,
            gpu_error_message: state.gpu_error_message.clone(),
        }
    }

    pub fn ensure_loaded(
        &self,
        model_name: &str,
        model_path: &Path,
        gpu_mode: GpuMode,
    ) -> Result<ActiveBackend, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("whisper context mutex should not be poisoned");
        if state
            .loaded_model
            .as_ref()
            .is_some_and(|loaded| loaded.model_name == model_name)
            && is_cache_compatible(state.active_backend, gpu_mode)
        {
            return Ok(state.active_backend);
        }

        let plan = load_plan(gpu_mode);
        let mut gpu_error_message = None;
        for attempt in plan {
            match attempt {
                LoadAttempt::Gpu if !compiled_gpu_support() => {
                    let message = "GPU support is not compiled in this build".to_string();
                    if gpu_mode == GpuMode::ForceGpu {
                        return Err(AppError::new(WHISPER_GPU_UNAVAILABLE, message));
                    }
                    gpu_error_message = Some(message);
                }
                LoadAttempt::Gpu => match load_whisper_context(model_path, true) {
                    Ok(context) => {
                        state.loaded_model = Some(LoadedWhisperContext {
                            model_name: model_name.to_string(),
                            context: Arc::new(context),
                        });
                        state.active_backend = ActiveBackend::Gpu;
                        state.gpu_error_message = None;
                        return Ok(ActiveBackend::Gpu);
                    }
                    Err(error) if gpu_mode == GpuMode::ForceGpu => {
                        return Err(AppError::new(WHISPER_GPU_UNAVAILABLE, error));
                    }
                    Err(error) => {
                        gpu_error_message = Some(error);
                    }
                },
                LoadAttempt::Cpu => {
                    let context = load_whisper_context(model_path, false).map_err(|error| {
                        AppError::new(
                            WHISPER_ERROR,
                            format!("failed to load whisper model: {error}"),
                        )
                    })?;
                    state.loaded_model = Some(LoadedWhisperContext {
                        model_name: model_name.to_string(),
                        context: Arc::new(context),
                    });
                    state.active_backend = ActiveBackend::Cpu;
                    state.gpu_error_message = gpu_error_message;
                    return Ok(ActiveBackend::Cpu);
                }
            }
        }

        Err(AppError::new(WHISPER_ERROR, "failed to load whisper model"))
    }

    pub fn with_context<T>(
        &self,
        model_name: &str,
        read: impl FnOnce(&whisper_rs::WhisperContext) -> T,
    ) -> Option<T> {
        let state = self
            .state
            .lock()
            .expect("whisper context mutex should not be poisoned");
        let loaded = state.loaded_model.as_ref()?;
        if loaded.model_name == model_name {
            let context = Arc::clone(&loaded.context);
            drop(state);
            Some(read(&context))
        } else {
            None
        }
    }
}

impl Default for WhisperContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadAttempt {
    Gpu,
    Cpu,
}

fn load_plan(gpu_mode: GpuMode) -> &'static [LoadAttempt] {
    match gpu_mode {
        GpuMode::Auto => &[LoadAttempt::Gpu, LoadAttempt::Cpu],
        GpuMode::ForceCpu => &[LoadAttempt::Cpu],
        GpuMode::ForceGpu => &[LoadAttempt::Gpu],
    }
}

fn compiled_gpu_support() -> bool {
    cfg!(feature = "gpu-vulkan")
}

fn is_cache_compatible(active_backend: ActiveBackend, gpu_mode: GpuMode) -> bool {
    match gpu_mode {
        GpuMode::Auto => active_backend != ActiveBackend::None,
        GpuMode::ForceCpu => active_backend == ActiveBackend::Cpu,
        GpuMode::ForceGpu => active_backend == ActiveBackend::Gpu,
    }
}

fn load_whisper_context(
    model_path: &Path,
    use_gpu: bool,
) -> Result<whisper_rs::WhisperContext, String> {
    let mut params = whisper_rs::WhisperContextParameters::default();
    params.use_gpu(use_gpu);
    whisper_rs::WhisperContext::new_with_params(model_path.display().to_string(), params)
        .map_err(|error| error.to_string())
}

pub fn model_path(models_dir: &Path, file_name: &str) -> PathBuf {
    models_dir.join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_cpu_uses_cpu_only_load_plan() {
        assert_eq!(load_plan(GpuMode::ForceCpu), &[LoadAttempt::Cpu]);
    }

    #[test]
    fn auto_tries_gpu_then_cpu() {
        assert_eq!(
            load_plan(GpuMode::Auto),
            &[LoadAttempt::Gpu, LoadAttempt::Cpu]
        );
    }

    #[test]
    fn force_gpu_uses_gpu_only_load_plan() {
        assert_eq!(load_plan(GpuMode::ForceGpu), &[LoadAttempt::Gpu]);
    }

    #[test]
    fn runtime_info_starts_with_no_loaded_backend() {
        let manager = WhisperContextManager::new();

        let info = manager.runtime_info(GpuMode::Auto);

        assert_eq!(info.active_backend, ActiveBackend::None);
        assert_eq!(info.requested_backend, GpuMode::Auto);
        assert_eq!(info.compiled_gpu_support, cfg!(feature = "gpu-vulkan"));
    }

    #[test]
    fn loaded_cpu_context_is_not_compatible_with_force_gpu() {
        assert!(!is_cache_compatible(ActiveBackend::Cpu, GpuMode::ForceGpu));
    }

    #[test]
    fn loaded_gpu_context_is_not_compatible_with_force_cpu() {
        assert!(!is_cache_compatible(ActiveBackend::Gpu, GpuMode::ForceCpu));
    }

    #[test]
    fn auto_accepts_any_loaded_backend() {
        assert!(is_cache_compatible(ActiveBackend::Cpu, GpuMode::Auto));
        assert!(is_cache_compatible(ActiveBackend::Gpu, GpuMode::Auto));
        assert!(!is_cache_compatible(ActiveBackend::None, GpuMode::Auto));
    }

    #[test]
    fn force_gpu_without_compiled_support_returns_gpu_unavailable() {
        if compiled_gpu_support() {
            return;
        }
        let manager = WhisperContextManager::new();

        let error = manager
            .ensure_loaded(
                "medium-q5_0",
                Path::new("missing-model.bin"),
                GpuMode::ForceGpu,
            )
            .unwrap_err();

        assert_eq!(error.code, WHISPER_GPU_UNAVAILABLE);
        assert_eq!(
            manager.runtime_info(GpuMode::ForceGpu).active_backend,
            ActiveBackend::None
        );
    }
}
