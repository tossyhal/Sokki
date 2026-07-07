pub mod audio;
pub mod bootstrap;
pub mod commands;
pub mod db;
pub mod error;
pub mod recording;
pub mod recovery;
pub mod settings;
pub mod sound_check;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            bootstrap::ensure_app_data_dirs(&data_dir)?;
            let db = db::Db::open(data_dir.join("sokki.db"))?;
            let recovered = recovery::recover_interrupted_sessions(&db, &data_dir)?;
            if recovered > 0 {
                log::info!("Recovered {recovered} interrupted session(s)");
            }
            app.manage(db);
            app.manage(recording::RecordingManager::new());
            app.manage(settings::SettingsStore::at_data_dir(&data_dir));
            app.manage(sound_check::SoundCheckManager::new());
            log::info!("Sokki app data directory: {}", data_dir.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_system_info,
            commands::get_settings,
            commands::get_recording_state,
            commands::list_audio_devices,
            commands::pause_recording,
            commands::resume_recording,
            commands::run_sound_check,
            commands::start_recording,
            commands::stop_recording,
            commands::update_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
