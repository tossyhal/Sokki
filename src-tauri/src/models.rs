use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, IO_ERROR};

pub const MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    App,
    Manual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCatalogEntry {
    pub name: &'static str,
    pub file_name: &'static str,
    pub estimated_size_bytes: u64,
    pub recommended: bool,
    pub description: &'static str,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifestEntry {
    pub name: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub verified: bool,
    pub origin: ModelOrigin,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    models: BTreeMap<String, ModelManifestEntry>,
}

impl ModelManifest {
    pub fn get(&self, name: &str) -> Option<&ModelManifestEntry> {
        self.models.get(name)
    }

    pub fn upsert(&mut self, entry: ModelManifestEntry) {
        self.models.insert(entry.name.clone(), entry);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub downloaded: bool,
    pub verified: bool,
    pub corrupted: bool,
    pub usable: bool,
    pub origin: Option<ModelOrigin>,
    pub recommended: bool,
    pub description: String,
}

pub fn model_catalog() -> &'static [ModelCatalogEntry] {
    &[
        ModelCatalogEntry {
            name: "tiny",
            file_name: "ggml-tiny.bin",
            estimated_size_bytes: 75 * 1024 * 1024,
            recommended: false,
            description: "動作確認用",
        },
        ModelCatalogEntry {
            name: "base",
            file_name: "ggml-base.bin",
            estimated_size_bytes: 142 * 1024 * 1024,
            recommended: false,
            description: "",
        },
        ModelCatalogEntry {
            name: "small",
            file_name: "ggml-small.bin",
            estimated_size_bytes: 488 * 1024 * 1024,
            recommended: false,
            description: "低スペックCPU向け",
        },
        ModelCatalogEntry {
            name: "medium-q5_0",
            file_name: "ggml-medium-q5_0.bin",
            estimated_size_bytes: 539 * 1024 * 1024,
            recommended: true,
            description: "バランス推奨",
        },
        ModelCatalogEntry {
            name: "medium",
            file_name: "ggml-medium.bin",
            estimated_size_bytes: 1_530 * 1024 * 1024,
            recommended: false,
            description: "高精度・重い",
        },
        ModelCatalogEntry {
            name: "large-v3-turbo",
            file_name: "ggml-large-v3-turbo.bin",
            estimated_size_bytes: 1_620 * 1024 * 1024,
            recommended: false,
            description: "高精度・高速(GPU推奨)",
        },
        ModelCatalogEntry {
            name: "large-v3",
            file_name: "ggml-large-v3.bin",
            estimated_size_bytes: 2_950 * 1024 * 1024,
            recommended: false,
            description: "最高精度(GPU推奨)",
        },
    ]
}

pub fn load_manifest(models_dir: &Path) -> Result<ModelManifest, AppError> {
    let path = models_dir.join(MANIFEST_FILE);
    if !path.exists() {
        return Ok(ModelManifest::default());
    }
    let content = fs::read_to_string(&path).map_err(io_error)?;
    serde_json::from_str(&content)
        .map_err(|error| AppError::new(IO_ERROR, format!("failed to parse manifest: {error}")))
}

pub fn save_manifest(models_dir: &Path, manifest: &ModelManifest) -> Result<(), AppError> {
    fs::create_dir_all(models_dir).map_err(io_error)?;
    let content = serde_json::to_string_pretty(manifest).map_err(|error| {
        AppError::new(
            IO_ERROR,
            format!("failed to serialize model manifest: {error}"),
        )
    })?;
    fs::write(models_dir.join(MANIFEST_FILE), format!("{content}\n")).map_err(io_error)
}

pub fn get_model_inventory(models_dir: &Path) -> Result<Vec<ModelInfo>, AppError> {
    let manifest = load_manifest(models_dir)?;
    model_catalog()
        .iter()
        .map(|catalog| model_info(models_dir, catalog, manifest.get(catalog.name)))
        .collect()
}

fn model_info(
    models_dir: &Path,
    catalog: &ModelCatalogEntry,
    manifest_entry: Option<&ModelManifestEntry>,
) -> Result<ModelInfo, AppError> {
    let file_path = models_dir.join(catalog.file_name);
    let file_size = match fs::metadata(&file_path) {
        Ok(metadata) if metadata.is_file() => Some(metadata.len()),
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    let downloaded = file_size.is_some();
    let corrupted = match (manifest_entry, file_size) {
        (Some(entry), Some(size)) => size != entry.size_bytes,
        _ => false,
    };
    let origin = manifest_entry
        .map(|entry| entry.origin)
        .or(file_size.map(|_| ModelOrigin::Manual));
    let verified = manifest_entry.is_some_and(|entry| entry.verified);
    let usable = downloaded
        && !corrupted
        && manifest_entry.is_some_and(|entry| entry.verified || entry.origin == ModelOrigin::App);
    let size_bytes = file_size
        .or_else(|| manifest_entry.map(|entry| entry.size_bytes))
        .unwrap_or(catalog.estimated_size_bytes);

    Ok(ModelInfo {
        name: catalog.name.to_string(),
        file_name: catalog.file_name.to_string(),
        size_bytes,
        downloaded,
        verified,
        corrupted,
        usable,
        origin,
        recommended: catalog.recommended,
        description: catalog.description.to_string(),
    })
}

fn io_error(error: std::io::Error) -> AppError {
    AppError::new(IO_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn catalog_contains_required_models_and_recommendation() {
        let catalog = model_catalog();

        assert_eq!(catalog.len(), 7);
        assert_eq!(catalog[0].name, "tiny");
        assert_eq!(catalog[3].name, "medium-q5_0");
        assert_eq!(catalog[3].file_name, "ggml-medium-q5_0.bin");
        assert!(catalog[3].recommended);
    }

    #[test]
    fn manifest_round_trips_origin_and_verification_metadata() {
        let models_dir = temp_dir("manifest-roundtrip");
        let mut manifest = ModelManifest::default();
        manifest.upsert(ModelManifestEntry {
            name: "medium-q5_0".to_string(),
            file_name: "ggml-medium-q5_0.bin".to_string(),
            size_bytes: 123,
            sha256: Some("abc".to_string()),
            verified: true,
            origin: ModelOrigin::App,
        });

        save_manifest(&models_dir, &manifest).expect("manifest should save");
        let loaded = load_manifest(&models_dir).expect("manifest should load");

        assert_eq!(
            loaded.get("medium-q5_0").map(|entry| entry.origin),
            Some(ModelOrigin::App)
        );
        assert_eq!(
            loaded
                .get("medium-q5_0")
                .and_then(|entry| entry.sha256.as_deref()),
            Some("abc")
        );
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn model_inventory_classifies_usable_corrupted_missing_and_manual_models() {
        let models_dir = temp_dir("inventory");
        write_file(&models_dir.join("ggml-base.bin"), 10);
        write_file(&models_dir.join("ggml-small.bin"), 9);
        write_file(&models_dir.join("ggml-medium-q5_0.bin"), 12);
        write_file(&models_dir.join("ggml-medium.bin"), 7);
        let mut manifest = ModelManifest::default();
        manifest.upsert(entry("base", "ggml-base.bin", 10, true, ModelOrigin::App));
        manifest.upsert(entry("small", "ggml-small.bin", 10, true, ModelOrigin::App));
        manifest.upsert(entry(
            "medium-q5_0",
            "ggml-medium-q5_0.bin",
            12,
            false,
            ModelOrigin::App,
        ));
        save_manifest(&models_dir, &manifest).expect("manifest should save");

        let models = get_model_inventory(&models_dir).expect("inventory should load");
        let tiny = find(&models, "tiny");
        let base = find(&models, "base");
        let small = find(&models, "small");
        let medium_q5 = find(&models, "medium-q5_0");
        let manual = find(&models, "medium");

        assert!(!tiny.downloaded);
        assert!(!tiny.usable);
        assert_eq!(tiny.origin, None);

        assert!(base.downloaded);
        assert!(base.verified);
        assert!(base.usable);
        assert!(!base.corrupted);
        assert_eq!(base.origin, Some(ModelOrigin::App));

        assert!(small.downloaded);
        assert!(small.corrupted);
        assert!(!small.usable);

        assert!(medium_q5.downloaded);
        assert!(!medium_q5.verified);
        assert!(medium_q5.usable);

        assert!(manual.downloaded);
        assert!(!manual.verified);
        assert!(!manual.usable);
        assert_eq!(manual.origin, Some(ModelOrigin::Manual));
        let _ = fs::remove_dir_all(models_dir);
    }

    fn entry(
        name: &str,
        file_name: &str,
        size_bytes: u64,
        verified: bool,
        origin: ModelOrigin,
    ) -> ModelManifestEntry {
        ModelManifestEntry {
            name: name.to_string(),
            file_name: file_name.to_string(),
            size_bytes,
            sha256: Some(format!("sha-{name}")),
            verified,
            origin,
        }
    }

    fn find<'a>(models: &'a [ModelInfo], name: &str) -> &'a ModelInfo {
        models
            .iter()
            .find(|model| model.name == name)
            .expect("model should exist")
    }

    fn write_file(path: &std::path::Path, len: usize) {
        fs::write(path, vec![b'x'; len]).expect("model file should write");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sokki-models-{name}-{unique}"));
        fs::create_dir_all(&path).expect("temp dir should create");
        path
    }
}
