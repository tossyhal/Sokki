use std::fs;
use std::path::Path;

pub const RECORDINGS_DIR: &str = "recordings";
pub const MODELS_DIR: &str = "models";
pub const SOUNDCHECK_DIR: &str = "soundcheck";

pub fn ensure_app_data_dirs(data_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(data_dir.join(RECORDINGS_DIR))?;
    fs::create_dir_all(data_dir.join(MODELS_DIR))?;
    fs::create_dir_all(data_dir.join(SOUNDCHECK_DIR))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn creates_required_app_data_directories() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let data_dir = std::env::temp_dir().join(format!("sokki-bootstrap-{unique}"));

        ensure_app_data_dirs(&data_dir).expect("app data directories should be created");

        assert!(data_dir.join(RECORDINGS_DIR).is_dir());
        assert!(data_dir.join(MODELS_DIR).is_dir());
        assert!(data_dir.join(SOUNDCHECK_DIR).is_dir());

        fs::remove_dir_all(data_dir).expect("temporary app data directory should be removable");
    }
}
