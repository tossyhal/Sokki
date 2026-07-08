pub mod audio;
pub mod bootstrap;
pub mod commands;
pub mod db;
pub mod error;
pub mod import;
pub mod recording;
pub mod recovery;
pub mod settings;
pub mod sound_check;
pub mod transcription;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            bootstrap::ensure_app_data_dirs(&data_dir)?;
            let db_path = data_dir.join("sokki.db");
            let db = db::Db::open(&db_path)?;
            let recovered = recovery::recover_interrupted_sessions(&db, &data_dir)?;
            if recovered > 0 {
                log::info!("Recovered {recovered} interrupted session(s)");
            }
            let worker_db = Arc::new(db::Db::open(&db_path)?);
            let settings_store = settings::SettingsStore::at_data_dir(&data_dir);
            let initial_settings = settings_store.load()?;
            let whisper_context = Arc::new(transcription::context::WhisperContextManager::new());
            let tracker = Arc::new(transcription::jobs::JobTracker::new());
            let transcription_active = Arc::new(AtomicBool::new(false));
            let processor = Arc::new(transcription::inference::WhisperJobProcessor::new(
                Arc::clone(&worker_db),
                Arc::clone(&whisper_context),
                data_dir.join(bootstrap::MODELS_DIR),
                initial_settings.gpu_mode,
                Arc::clone(&transcription_active),
            ));
            let worker = transcription::worker::TranscribeWorkerHandle::start(
                worker_db,
                Arc::clone(&tracker),
                transcription_active,
                processor,
                Arc::new(transcription::worker::TauriTranscriptionEventSink::new(
                    app.handle().clone(),
                )),
            );
            app.manage(db);
            app.manage(recording::RecordingManager::new());
            app.manage(settings_store);
            app.manage(sound_check::SoundCheckManager::new());
            app.manage(whisper_context);
            app.manage(tracker);
            app.manage(transcription::worker::TranscribeWorkerState::new(worker));
            app.manage(import::ImportSessionIdGenerator::new());
            log::info!("Sokki app data directory: {}", data_dir.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cancel_transcription,
            commands::delete_session,
            commands::get_session,
            commands::get_segments,
            commands::get_sessions,
            commands::get_system_info,
            commands::import_files,
            commands::get_settings,
            commands::get_recording_state,
            commands::list_audio_devices,
            commands::pause_recording,
            commands::resume_recording,
            commands::rename_session,
            commands::run_sound_check,
            commands::start_recording,
            commands::stop_recording,
            commands::update_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
