use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, IO_ERROR};

pub const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsLanguage {
    Ja,
    En,
    Auto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMode {
    Auto,
    ForceCpu,
    ForceGpu,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_model: String,
    pub language: SettingsLanguage,
    pub gpu_mode: GpuMode,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub mic_gain: f32,
    pub system_gain: f32,
    pub vad_threshold_db: i32,
    pub onboarding_done: bool,
    pub sound_check_recommended: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_model: "medium-q5_0".to_string(),
            language: SettingsLanguage::Ja,
            gpu_mode: GpuMode::Auto,
            mic_device: None,
            loopback_device: None,
            mic_gain: 0.7,
            system_gain: 0.7,
            vad_threshold_db: -40,
            onboarding_done: false,
            sound_check_recommended: true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub default_model: Option<String>,
    pub language: Option<SettingsLanguage>,
    pub gpu_mode: Option<GpuMode>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub mic_device: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub loopback_device: Option<Option<String>>,
    pub mic_gain: Option<f32>,
    pub system_gain: Option<f32>,
    pub vad_threshold_db: Option<i32>,
    pub onboarding_done: Option<bool>,
    pub sound_check_recommended: Option<bool>,
}

impl SettingsPatch {
    fn apply_to(self, settings: &mut Settings) {
        if let Some(default_model) = self.default_model {
            settings.default_model = default_model;
        }
        if let Some(language) = self.language {
            settings.language = language;
        }
        if let Some(gpu_mode) = self.gpu_mode {
            settings.gpu_mode = gpu_mode;
        }
        if let Some(mic_device) = self.mic_device {
            settings.mic_device = mic_device;
        }
        if let Some(loopback_device) = self.loopback_device {
            settings.loopback_device = loopback_device;
        }
        if let Some(mic_gain) = self.mic_gain {
            settings.mic_gain = mic_gain;
        }
        if let Some(system_gain) = self.system_gain {
            settings.system_gain = system_gain;
        }
        if let Some(vad_threshold_db) = self.vad_threshold_db {
            settings.vad_threshold_db = vad_threshold_db;
        }
        if let Some(onboarding_done) = self.onboarding_done {
            settings.onboarding_done = onboarding_done;
        }
        if let Some(sound_check_recommended) = self.sound_check_recommended {
            settings.sound_check_recommended = sound_check_recommended;
        }
    }
}

fn deserialize_nullable_patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<SettingsStoreInner>,
}

struct SettingsStoreInner {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(SettingsStoreInner {
                path,
                lock: Mutex::new(()),
            }),
        }
    }

    pub fn at_data_dir(data_dir: &Path) -> Self {
        Self::new(data_dir.join(SETTINGS_FILE))
    }

    pub fn load(&self) -> Result<Settings, AppError> {
        let _guard = self
            .inner
            .lock
            .lock()
            .expect("settings mutex should not be poisoned");
        let settings = self.load_unlocked()?;
        self.save_unlocked(&settings)?;
        Ok(settings)
    }

    pub fn read(&self) -> Result<Settings, AppError> {
        let _guard = self
            .inner
            .lock
            .lock()
            .expect("settings mutex should not be poisoned");
        self.load_unlocked()
    }

    pub fn update(&self, patch: SettingsPatch) -> Result<Settings, AppError> {
        let _guard = self
            .inner
            .lock
            .lock()
            .expect("settings mutex should not be poisoned");
        let mut settings = self.load_unlocked()?;
        patch.apply_to(&mut settings);
        self.save_unlocked(&settings)?;
        Ok(settings)
    }

    fn load_unlocked(&self) -> Result<Settings, AppError> {
        if !self.inner.path.exists() {
            return Ok(Settings::default());
        }

        let content = fs::read_to_string(&self.inner.path)
            .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
        let patch: SettingsPatch = serde_json::from_str(&content)
            .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
        let mut settings = Settings::default();
        patch.apply_to(&mut settings);
        Ok(settings)
    }

    fn save_unlocked(&self, settings: &Settings) -> Result<(), AppError> {
        if let Some(parent) = self.inner.path.parent() {
            fs::create_dir_all(parent).map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
        }
        let content = serde_json::to_string_pretty(settings)
            .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
        fs::write(&self.inner.path, format!("{content}\n"))
            .map_err(|err| AppError::new(IO_ERROR, err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_creates_default_settings_file() {
        let path = temp_settings_path();
        let store = SettingsStore::new(path.clone());

        let settings = store.load().expect("settings should load");

        assert_eq!(settings, Settings::default());
        let saved: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&path).expect("settings file should be written"),
        )
        .expect("settings file should contain json");
        assert_eq!(saved["defaultModel"], "medium-q5_0");
        assert_eq!(saved["language"], "ja");

        cleanup(path);
    }

    #[test]
    fn load_completes_missing_keys_with_defaults() {
        let path = temp_settings_path();
        fs::create_dir_all(
            path.parent()
                .expect("temporary settings path should have parent"),
        )
        .expect("temporary settings directory should be creatable");
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "defaultModel": "small-q5_0",
                "onboardingDone": true
            }))
            .expect("partial settings should serialize"),
        )
        .expect("partial settings should write");
        let store = SettingsStore::new(path.clone());

        let settings = store.load().expect("settings should load");

        assert_eq!(settings.default_model, "small-q5_0");
        assert!(settings.onboarding_done);
        assert_eq!(settings.language, SettingsLanguage::Ja);
        assert_eq!(settings.gpu_mode, GpuMode::Auto);

        cleanup(path);
    }

    #[test]
    fn update_patches_and_persists_settings() {
        let path = temp_settings_path();
        let store = SettingsStore::new(path.clone());

        let updated = store
            .update(SettingsPatch {
                language: Some(SettingsLanguage::En),
                gpu_mode: Some(GpuMode::ForceCpu),
                mic_device: Some(Some("mic-1".to_string())),
                sound_check_recommended: Some(false),
                ..SettingsPatch::default()
            })
            .expect("settings should update");

        assert_eq!(updated.language, SettingsLanguage::En);
        assert_eq!(updated.gpu_mode, GpuMode::ForceCpu);
        assert_eq!(updated.mic_device.as_deref(), Some("mic-1"));
        assert!(!updated.sound_check_recommended);
        assert_eq!(
            store.load().expect("settings should reload"),
            updated,
            "updated settings should persist"
        );

        cleanup(path);
    }

    #[test]
    fn update_can_clear_nullable_devices() {
        let path = temp_settings_path();
        let store = SettingsStore::new(path.clone());
        store
            .update(SettingsPatch {
                mic_device: Some(Some("mic-1".to_string())),
                loopback_device: Some(Some("speaker-1".to_string())),
                ..SettingsPatch::default()
            })
            .expect("settings should update devices");

        let patch: SettingsPatch = serde_json::from_value(json!({
            "micDevice": null,
            "loopbackDevice": null
        }))
        .expect("nullable device patch should deserialize");
        let updated = store
            .update(patch)
            .expect("nullable devices should be cleared");

        assert_eq!(updated.mic_device, None);
        assert_eq!(updated.loopback_device, None);

        cleanup(path);
    }

    fn temp_settings_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("sokki-settings-{unique}"))
            .join(SETTINGS_FILE)
    }

    fn cleanup(path: PathBuf) {
        let dir = path
            .parent()
            .expect("temporary settings path should have parent")
            .to_path_buf();
        fs::remove_dir_all(dir).expect("temporary settings directory should be removable");
    }
}
