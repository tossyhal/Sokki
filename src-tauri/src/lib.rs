pub mod bootstrap;
pub mod error;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            bootstrap::ensure_app_data_dirs(&data_dir)?;
            log::info!("Sokki app data directory: {}", data_dir.display());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
