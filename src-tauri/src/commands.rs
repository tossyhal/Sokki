use serde::Serialize;
use tauri::Manager;

use crate::audio::devices::{self, AudioDevices};
use crate::bootstrap::MODELS_DIR;
use crate::db::{Db, Session};
use crate::error::{AppError, IO_ERROR};
use crate::recording::{RecordingManager, RecordingStateSnapshot, StartRecordingRequest};
use crate::settings::{Settings, SettingsPatch, SettingsStore};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub app_version: String,
    pub compiled_gpu_support: bool,
    pub requested_backend: String,
    pub active_backend: String,
    pub gpu_error_message: Option<String>,
    pub models_dir: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn get_system_info(app: tauri::AppHandle) -> Result<SystemInfo, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    let models_dir = data_dir.join(MODELS_DIR);

    Ok(SystemInfo {
        app_version: app.package_info().version.to_string(),
        compiled_gpu_support: cfg!(feature = "gpu-vulkan"),
        requested_backend: "auto".to_string(),
        active_backend: "none".to_string(),
        gpu_error_message: None,
        models_dir: models_dir.display().to_string(),
        data_dir: data_dir.display().to_string(),
    })
}

#[tauri::command]
pub fn get_settings(settings_store: tauri::State<'_, SettingsStore>) -> Result<Settings, AppError> {
    settings_store.load()
}

#[tauri::command]
pub fn update_settings(
    settings_store: tauri::State<'_, SettingsStore>,
    patch: SettingsPatch,
) -> Result<Settings, AppError> {
    settings_store.update(patch)
}

#[tauri::command]
pub fn list_audio_devices() -> Result<AudioDevices, AppError> {
    devices::list_audio_devices()
}

#[tauri::command]
pub fn start_recording(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    recording_manager: tauri::State<'_, RecordingManager>,
    request: StartRecordingRequest,
) -> Result<String, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    recording_manager.start(&db, &data_dir, request)
}

#[tauri::command]
pub fn pause_recording(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<(), AppError> {
    recording_manager.pause()
}

#[tauri::command]
pub fn resume_recording(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<(), AppError> {
    recording_manager.resume()
}

#[tauri::command]
pub fn stop_recording(
    db: tauri::State<'_, Db>,
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<Session, AppError> {
    recording_manager.stop(&db)
}

#[tauri::command]
pub fn get_recording_state(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<RecordingStateSnapshot, AppError> {
    recording_manager.state()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn system_info_serializes_with_camel_case_fields() {
        let info = SystemInfo {
            app_version: "0.0.0".to_string(),
            compiled_gpu_support: false,
            requested_backend: "auto".to_string(),
            active_backend: "none".to_string(),
            gpu_error_message: None,
            models_dir: "C:/Users/example/AppData/Roaming/com.sokki.app/models".to_string(),
            data_dir: "C:/Users/example/AppData/Roaming/com.sokki.app".to_string(),
        };

        let value = serde_json::to_value(info).expect("SystemInfo should serialize");

        assert_eq!(
            value,
            json!({
                "appVersion": "0.0.0",
                "compiledGpuSupport": false,
                "requestedBackend": "auto",
                "activeBackend": "none",
                "gpuErrorMessage": null,
                "modelsDir": "C:/Users/example/AppData/Roaming/com.sokki.app/models",
                "dataDir": "C:/Users/example/AppData/Roaming/com.sokki.app",
            })
        );
    }
}
