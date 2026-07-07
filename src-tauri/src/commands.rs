use serde::Serialize;
use tauri::Manager;

use crate::audio::devices::{self, AudioDevices};
use crate::bootstrap::MODELS_DIR;
use crate::db::{Db, Session};
use crate::error::{AppError, DB_ERROR, IO_ERROR};
use crate::recording::{
    RecordingManager, RecordingStateSnapshot, StartRecordingRequest, TauriRecordingEventSink,
};
use crate::settings::{GpuMode, Settings, SettingsPatch, SettingsStore};
use crate::sound_check::{
    SoundCheckManager, SoundCheckRequest, SoundCheckResult, TauriSoundCheckEventSink,
};
use crate::transcription::context::{ActiveBackend, WhisperContextManager};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub app_version: String,
    pub compiled_gpu_support: bool,
    pub requested_backend: GpuMode,
    pub active_backend: ActiveBackend,
    pub gpu_error_message: Option<String>,
    pub models_dir: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn get_system_info(
    app: tauri::AppHandle,
    settings_store: tauri::State<'_, SettingsStore>,
    whisper_context: tauri::State<'_, WhisperContextManager>,
) -> Result<SystemInfo, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    let models_dir = data_dir.join(MODELS_DIR);
    let settings = settings_store.load()?;
    let runtime = whisper_context.runtime_info(settings.gpu_mode);

    Ok(SystemInfo {
        app_version: app.package_info().version.to_string(),
        compiled_gpu_support: runtime.compiled_gpu_support,
        requested_backend: runtime.requested_backend,
        active_backend: runtime.active_backend,
        gpu_error_message: runtime.gpu_error_message,
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
    recording_manager.start(
        &db,
        &data_dir,
        request,
        std::sync::Arc::new(TauriRecordingEventSink::new(app)),
    )
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

#[tauri::command]
pub fn run_sound_check(
    app: tauri::AppHandle,
    recording_manager: tauri::State<'_, RecordingManager>,
    sound_check_manager: tauri::State<'_, SoundCheckManager>,
    request: SoundCheckRequest,
) -> Result<SoundCheckResult, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    sound_check_manager.run(
        &recording_manager,
        &data_dir,
        request,
        &TauriSoundCheckEventSink::new(app),
    )
}

#[tauri::command]
pub fn get_sessions(db: tauri::State<'_, Db>) -> Result<Vec<Session>, AppError> {
    db.list_sessions().map_err(db_error)
}

#[tauri::command]
pub fn get_session(db: tauri::State<'_, Db>, id: String) -> Result<Session, AppError> {
    get_session_impl(&db, &id)
}

#[tauri::command]
pub fn rename_session(
    db: tauri::State<'_, Db>,
    id: String,
    title: String,
) -> Result<Session, AppError> {
    let title = validate_session_title(&title)?;
    let changed = db.rename_session(&id, title).map_err(db_error)?;
    if !changed {
        return Err(session_not_found(&id));
    }
    get_session_impl(&db, &id)
}

#[tauri::command]
pub fn delete_session(db: tauri::State<'_, Db>, id: String) -> Result<(), AppError> {
    let changed = db.delete_session(&id).map_err(db_error)?;
    if changed {
        Ok(())
    } else {
        Err(session_not_found(&id))
    }
}

fn get_session_impl(db: &Db, id: &str) -> Result<Session, AppError> {
    db.get_session(id)
        .map_err(db_error)?
        .ok_or_else(|| session_not_found(id))
}

fn session_not_found(id: &str) -> AppError {
    AppError::new(DB_ERROR, format!("session not found: {id}"))
}

fn validate_session_title(title: &str) -> Result<&str, AppError> {
    let title = title.trim();
    if title.is_empty() {
        Err(AppError::new(DB_ERROR, "session title must not be empty"))
    } else {
        Ok(title)
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Language, SessionStatus, Source};
    use serde_json::json;

    #[test]
    fn system_info_serializes_with_camel_case_fields() {
        let info = SystemInfo {
            app_version: "0.0.0".to_string(),
            compiled_gpu_support: false,
            requested_backend: GpuMode::Auto,
            active_backend: ActiveBackend::None,
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

    #[test]
    fn get_session_impl_returns_db_error_when_missing() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");

        let error = get_session_impl(&db, "missing").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session not found: missing");
    }

    #[test]
    fn rename_session_rejects_empty_title_contract() {
        let error = validate_session_title("  ").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session title must not be empty");
    }

    #[test]
    fn session_serializes_for_library_cards() {
        let session = Session {
            id: "session-a".to_string(),
            title: "Session A".to_string(),
            created_at: 1_000,
            duration_ms: 12_000,
            audio_path: Some(
                "C:/Users/example/AppData/Roaming/com.sokki.app/recordings/a.wav".to_string(),
            ),
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status: SessionStatus::Interrupted,
            error_message: Some("recording interrupted by app shutdown".to_string()),
            drop_count: 3,
        };

        let value = serde_json::to_value(session).expect("Session should serialize");

        assert_eq!(value["createdAt"], json!(1_000));
        assert_eq!(value["durationMs"], json!(12_000));
        assert_eq!(value["status"], json!("interrupted"));
        assert_eq!(value["dropCount"], json!(3));
    }
}
