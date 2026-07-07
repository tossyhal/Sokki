pub mod audio;
pub mod bootstrap;
pub mod commands;
pub mod db;
pub mod error;
pub mod recording;
pub mod settings;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            bootstrap::ensure_app_data_dirs(&data_dir)?;
            app.manage(db::Db::open(data_dir.join("sokki.db"))?);
            app.manage(recording::RecordingManager::new());
            app.manage(settings::SettingsStore::at_data_dir(&data_dir));
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
            commands::start_recording,
            commands::stop_recording,
            commands::update_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
