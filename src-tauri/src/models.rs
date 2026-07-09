use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{
    AppError, CANCELED, DOWNLOAD_FAILED, IO_ERROR, MODEL_ALREADY_DOWNLOADING, MODEL_NOT_FOUND,
    VERIFY_FAILED,
};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MODEL_PROGRESS_EVENT: &str = "model://progress";
pub const MODEL_DONE_EVENT: &str = "model://done";
pub const MODEL_ERROR_EVENT: &str = "model://error";
const HF_TREE_API_URL: &str = "https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main";
const HF_RESOLVE_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

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
    #[serde(default)]
    pub corrupted: bool,
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

    pub fn remove(&mut self, name: &str) {
        self.models.remove(name);
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedModelMetadata {
    pub size_bytes: u64,
    pub sha256: String,
}

pub trait ModelDownloadClient {
    fn expected_metadata(
        &self,
        catalog: &ModelCatalogEntry,
    ) -> Result<Option<ExpectedModelMetadata>, AppError>;

    fn stream_model(
        &self,
        catalog: &ModelCatalogEntry,
        writer: &mut dyn Write,
    ) -> Result<u64, AppError>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgressPayload {
    pub name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDonePayload {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelErrorPayload {
    pub name: String,
    pub message: String,
}

pub trait ModelDownloadEventSink {
    fn emit_progress(&self, payload: ModelProgressPayload);
    fn emit_done(&self, payload: ModelDonePayload);
    fn emit_error(&self, payload: ModelErrorPayload);
}

pub struct TauriModelDownloadEventSink {
    app: tauri::AppHandle,
}

impl TauriModelDownloadEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl ModelDownloadEventSink for TauriModelDownloadEventSink {
    fn emit_progress(&self, payload: ModelProgressPayload) {
        use tauri::Emitter;
        let _ = self.app.emit(MODEL_PROGRESS_EVENT, payload);
    }

    fn emit_done(&self, payload: ModelDonePayload) {
        use tauri::Emitter;
        let _ = self.app.emit(MODEL_DONE_EVENT, payload);
    }

    fn emit_error(&self, payload: ModelErrorPayload) {
        use tauri::Emitter;
        let _ = self.app.emit(MODEL_ERROR_EVENT, payload);
    }
}

pub struct ReqwestModelDownloadClient {
    client: reqwest::blocking::Client,
}

#[derive(Clone, Default)]
pub struct ModelDownloadManager {
    active: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl ModelDownloadManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self, name: &str) -> Result<Arc<AtomicBool>, AppError> {
        let mut active = self
            .active
            .lock()
            .expect("model download mutex should not be poisoned");
        if active.contains_key(name) {
            return Err(AppError::new(
                MODEL_ALREADY_DOWNLOADING,
                format!("model is already downloading: {name}"),
            ));
        }
        let canceled = Arc::new(AtomicBool::new(false));
        active.insert(name.to_string(), Arc::clone(&canceled));
        Ok(canceled)
    }

    pub fn finish(&self, name: &str) {
        self.active
            .lock()
            .expect("model download mutex should not be poisoned")
            .remove(name);
    }

    pub fn cancel(&self, name: &str) -> Result<(), AppError> {
        let active = self
            .active
            .lock()
            .expect("model download mutex should not be poisoned");
        let Some(flag) = active.get(name) else {
            return Err(AppError::new(
                CANCELED,
                format!("model download is not active: {name}"),
            ));
        };
        flag.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl ReqwestModelDownloadClient {
    pub fn new() -> Result<Self, AppError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Sokki/0.0.0")
            .build()
            .map_err(download_error)?;
        Ok(Self { client })
    }
}

impl ModelDownloadClient for ReqwestModelDownloadClient {
    fn expected_metadata(
        &self,
        catalog: &ModelCatalogEntry,
    ) -> Result<Option<ExpectedModelMetadata>, AppError> {
        let entries: Vec<HfTreeEntry> = self
            .client
            .get(HF_TREE_API_URL)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(download_error)?
            .json()
            .map_err(download_error)?;
        Ok(entries
            .into_iter()
            .find(|entry| entry.path.as_deref() == Some(catalog.file_name))
            .and_then(|entry| {
                let lfs = entry.lfs?;
                let size_bytes = entry.size.or(lfs.size)?;
                Some(ExpectedModelMetadata {
                    size_bytes,
                    sha256: lfs.oid,
                })
            }))
    }

    fn stream_model(
        &self,
        catalog: &ModelCatalogEntry,
        writer: &mut dyn Write,
    ) -> Result<u64, AppError> {
        let url = format!("{HF_RESOLVE_BASE_URL}/{}", catalog.file_name);
        let mut response = self
            .client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(download_error)?;
        let bytes = std::io::copy(&mut response, writer).map_err(download_io_error)?;
        Ok(bytes)
    }
}

#[derive(Deserialize)]
struct HfTreeEntry {
    path: Option<String>,
    size: Option<u64>,
    lfs: Option<HfLfsEntry>,
}

#[derive(Deserialize)]
struct HfLfsEntry {
    oid: String,
    size: Option<u64>,
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

pub fn download_model_with_client(
    models_dir: &Path,
    name: &str,
    client: &dyn ModelDownloadClient,
    events: &dyn ModelDownloadEventSink,
) -> Result<(), AppError> {
    download_model_with_client_and_cancel(
        models_dir,
        name,
        client,
        events,
        Arc::new(AtomicBool::new(false)),
    )
}

pub fn download_model_with_client_and_cancel(
    models_dir: &Path,
    name: &str,
    client: &dyn ModelDownloadClient,
    events: &dyn ModelDownloadEventSink,
    canceled: Arc<AtomicBool>,
) -> Result<(), AppError> {
    match download_model_inner(models_dir, name, client, events, canceled) {
        Ok(()) => {
            events.emit_done(ModelDonePayload {
                name: name.to_string(),
            });
            Ok(())
        }
        Err(error) => {
            events.emit_error(ModelErrorPayload {
                name: name.to_string(),
                message: error.message.clone(),
            });
            Err(error)
        }
    }
}

fn download_model_inner(
    models_dir: &Path,
    name: &str,
    client: &dyn ModelDownloadClient,
    events: &dyn ModelDownloadEventSink,
    canceled: Arc<AtomicBool>,
) -> Result<(), AppError> {
    fs::create_dir_all(models_dir).map_err(io_error)?;
    let catalog = model_catalog()
        .iter()
        .find(|catalog| catalog.name == name)
        .ok_or_else(|| AppError::new(MODEL_NOT_FOUND, format!("unknown model: {name}")))?;
    let expected = client.expected_metadata(catalog).unwrap_or(None);
    let final_path = models_dir.join(catalog.file_name);
    let part_path = models_dir.join(format!("{}.part", catalog.file_name));
    if part_path.exists() {
        fs::remove_file(&part_path).map_err(io_error)?;
    }

    let file = File::create(&part_path).map_err(io_error)?;
    let mut writer = HashingProgressWriter::new(
        file,
        catalog.name,
        expected.as_ref().map(|metadata| metadata.size_bytes),
        events,
        canceled,
    );
    let streamed_bytes = match client.stream_model(catalog, &mut writer) {
        Ok(bytes) => bytes,
        Err(error) => {
            drop(writer);
            let _ = fs::remove_file(&part_path);
            return Err(error);
        }
    };
    let downloaded_bytes = writer.downloaded_bytes();
    if streamed_bytes != downloaded_bytes {
        drop(writer);
        let _ = fs::remove_file(&part_path);
        return Err(AppError::new(
            DOWNLOAD_FAILED,
            "download stream byte count did not match written bytes",
        ));
    }
    writer.emit_progress_now();
    let actual_sha256 = writer.finalize_hash();

    if let Some(expected) = expected.as_ref() {
        if expected.size_bytes != downloaded_bytes || !sha_eq(&expected.sha256, &actual_sha256) {
            let _ = fs::remove_file(&part_path);
            return Err(AppError::new(
                VERIFY_FAILED,
                "downloaded model failed SHA-256 or size verification",
            ));
        }
    }

    fs::rename(&part_path, &final_path).map_err(io_error)?;
    let mut manifest = load_manifest(models_dir)?;
    manifest.upsert(ModelManifestEntry {
        name: catalog.name.to_string(),
        file_name: catalog.file_name.to_string(),
        size_bytes: downloaded_bytes,
        sha256: Some(actual_sha256),
        verified: expected.is_some(),
        origin: ModelOrigin::App,
        corrupted: false,
    });
    save_manifest(models_dir, &manifest)
}

pub fn verify_model_with_client(
    models_dir: &Path,
    name: &str,
    client: &dyn ModelDownloadClient,
) -> Result<ModelInfo, AppError> {
    fs::create_dir_all(models_dir).map_err(io_error)?;
    let catalog = model_catalog()
        .iter()
        .find(|catalog| catalog.name == name)
        .ok_or_else(|| AppError::new(MODEL_NOT_FOUND, format!("unknown model: {name}")))?;
    let model_path = models_dir.join(catalog.file_name);
    if !model_path.is_file() {
        return Err(AppError::new(
            MODEL_NOT_FOUND,
            format!("model file not found: {}", catalog.file_name),
        ));
    }

    let expected = client.expected_metadata(catalog)?.ok_or_else(|| {
        AppError::new(
            VERIFY_FAILED,
            "expected model metadata was not available from Hugging Face",
        )
    })?;
    let actual_size = fs::metadata(&model_path).map_err(io_error)?.len();
    let actual_sha256 = hash_file(&model_path)?;
    let existing = load_manifest(models_dir)?;
    let origin = existing
        .get(name)
        .map(|entry| entry.origin)
        .unwrap_or(ModelOrigin::Manual);
    let verified = expected.size_bytes == actual_size && sha_eq(&expected.sha256, &actual_sha256);
    let entry = ModelManifestEntry {
        name: catalog.name.to_string(),
        file_name: catalog.file_name.to_string(),
        size_bytes: expected.size_bytes,
        sha256: Some(expected.sha256),
        verified,
        origin,
        corrupted: !verified,
    };

    let mut manifest = existing;
    manifest.upsert(entry);
    save_manifest(models_dir, &manifest)?;

    if !verified {
        return Err(AppError::new(
            VERIFY_FAILED,
            "local model failed SHA-256 or size verification",
        ));
    }

    model_info(models_dir, catalog, manifest.get(name))
}

pub fn delete_model(models_dir: &Path, name: &str) -> Result<ModelInfo, AppError> {
    let catalog = model_catalog()
        .iter()
        .find(|catalog| catalog.name == name)
        .ok_or_else(|| AppError::new(MODEL_NOT_FOUND, format!("unknown model: {name}")))?;
    let model_path = models_dir.join(catalog.file_name);
    let part_path = models_dir.join(format!("{}.part", catalog.file_name));
    remove_if_exists(&model_path)?;
    remove_if_exists(&part_path)?;
    let mut manifest = load_manifest(models_dir)?;
    manifest.remove(name);
    save_manifest(models_dir, &manifest)?;
    model_info(models_dir, catalog, manifest.get(name))
}

struct HashingProgressWriter<'a> {
    inner: File,
    hasher: Sha256,
    name: &'a str,
    total_bytes: Option<u64>,
    downloaded_bytes: u64,
    last_progress: Instant,
    events: &'a dyn ModelDownloadEventSink,
    canceled: Arc<AtomicBool>,
}

impl<'a> HashingProgressWriter<'a> {
    fn new(
        inner: File,
        name: &'a str,
        total_bytes: Option<u64>,
        events: &'a dyn ModelDownloadEventSink,
        canceled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            name,
            total_bytes,
            downloaded_bytes: 0,
            last_progress: Instant::now() - PROGRESS_INTERVAL,
            events,
            canceled,
        }
    }

    fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes
    }

    fn emit_progress_now(&mut self) {
        self.events.emit_progress(ModelProgressPayload {
            name: self.name.to_string(),
            downloaded_bytes: self.downloaded_bytes,
            total_bytes: self.total_bytes,
        });
        self.last_progress = Instant::now();
    }

    fn finalize_hash(mut self) -> String {
        let _ = self.inner.flush();
        hex_lower(self.hasher.finalize().as_slice())
    }
}

impl Write for HashingProgressWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.canceled.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "model download canceled",
            ));
        }
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.downloaded_bytes += written as u64;
        if self.last_progress.elapsed() >= PROGRESS_INTERVAL {
            self.emit_progress_now();
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
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
        (Some(entry), Some(size)) => entry.corrupted || size != entry.size_bytes,
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

fn download_io_error(error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::Interrupted {
        AppError::new(CANCELED, error.to_string())
    } else {
        AppError::new(DOWNLOAD_FAILED, format!("model download failed: {error}"))
    }
}

fn download_error(error: reqwest::Error) -> AppError {
    AppError::new(DOWNLOAD_FAILED, format!("model download failed: {error}"))
}

fn remove_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn hash_file(path: &Path) -> Result<String, AppError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(hasher.finalize().as_slice()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn sha_eq(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::sync::Mutex;
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
            corrupted: false,
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

    #[test]
    fn download_model_verifies_sha_renames_part_and_updates_manifest() {
        let models_dir = temp_dir("download-success");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            })),
            body: b"hello".to_vec(),
            stream_error: None,
        };
        let events = RecordingModelEvents::default();

        download_model_with_client(&models_dir, "tiny", &client, &events)
            .expect("download should verify");

        let model_path = models_dir.join("ggml-tiny.bin");
        assert_eq!(fs::read(&model_path).unwrap(), b"hello");
        assert!(!models_dir.join("ggml-tiny.bin.part").exists());
        let manifest = load_manifest(&models_dir).unwrap();
        let entry = manifest.get("tiny").expect("manifest entry should exist");
        assert_eq!(entry.size_bytes, 5);
        assert!(entry.verified);
        assert_eq!(entry.origin, ModelOrigin::App);
        assert!(!entry.corrupted);
        assert_eq!(
            entry.sha256.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
        assert_eq!(
            events.events.lock().unwrap().last(),
            Some(&ModelEvent::Done {
                name: "tiny".to_string()
            })
        );
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn download_model_keeps_api_failure_as_unverified_manifest_entry() {
        let models_dir = temp_dir("download-api-failure");
        let client = FakeModelDownloadClient {
            metadata: Err(AppError::new(
                crate::error::DOWNLOAD_FAILED,
                "hf api unavailable",
            )),
            body: b"hello".to_vec(),
            stream_error: None,
        };
        let events = RecordingModelEvents::default();

        download_model_with_client(&models_dir, "tiny", &client, &events)
            .expect("download should continue when metadata lookup fails");

        let manifest = load_manifest(&models_dir).unwrap();
        let entry = manifest.get("tiny").expect("manifest entry should exist");
        assert_eq!(entry.size_bytes, 5);
        assert!(!entry.verified);
        assert_eq!(entry.origin, ModelOrigin::App);
        assert!(!entry.corrupted);
        assert_eq!(
            entry.sha256.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn download_model_deletes_part_and_emits_error_on_verify_failure() {
        let models_dir = temp_dir("download-verify-failure");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            })),
            body: b"hello".to_vec(),
            stream_error: None,
        };
        let events = RecordingModelEvents::default();

        let error = download_model_with_client(&models_dir, "tiny", &client, &events).unwrap_err();

        assert_eq!(error.code, crate::error::VERIFY_FAILED);
        assert!(!models_dir.join("ggml-tiny.bin").exists());
        assert!(!models_dir.join("ggml-tiny.bin.part").exists());
        assert_eq!(load_manifest(&models_dir).unwrap().get("tiny"), None);
        assert!(events.events.lock().unwrap().iter().any(|event| matches!(
            event,
            ModelEvent::Error { name, message } if name == "tiny" && message.contains("SHA-256")
        )));
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn download_model_deletes_part_and_emits_error_when_stream_fails() {
        let models_dir = temp_dir("download-stream-failure");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            })),
            body: b"he".to_vec(),
            stream_error: Some(AppError::new(
                crate::error::DOWNLOAD_FAILED,
                "network disconnected",
            )),
        };
        let events = RecordingModelEvents::default();

        let error = download_model_with_client(&models_dir, "tiny", &client, &events).unwrap_err();

        assert_eq!(error.code, crate::error::DOWNLOAD_FAILED);
        assert!(!models_dir.join("ggml-tiny.bin").exists());
        assert!(!models_dir.join("ggml-tiny.bin.part").exists());
        assert!(events.events.lock().unwrap().iter().any(|event| matches!(
            event,
            ModelEvent::Error { name, message } if name == "tiny" && message.contains("network disconnected")
        )));
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn download_model_respects_cancel_flag_and_removes_part() {
        let models_dir = temp_dir("download-canceled");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            })),
            body: b"hello".to_vec(),
            stream_error: None,
        };
        let events = RecordingModelEvents::default();
        let canceled = Arc::new(AtomicBool::new(true));

        let error =
            download_model_with_client_and_cancel(&models_dir, "tiny", &client, &events, canceled)
                .unwrap_err();

        assert_eq!(error.code, CANCELED);
        assert!(!models_dir.join("ggml-tiny.bin").exists());
        assert!(!models_dir.join("ggml-tiny.bin.part").exists());
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn model_download_manager_rejects_duplicate_and_sets_cancel_flag() {
        let manager = ModelDownloadManager::new();

        let flag = manager.start("tiny").expect("first download should start");
        let duplicate = manager.start("tiny").unwrap_err();
        manager
            .cancel("tiny")
            .expect("active download should cancel");

        assert_eq!(duplicate.code, MODEL_ALREADY_DOWNLOADING);
        assert!(flag.load(Ordering::SeqCst));
        manager.finish("tiny");
        assert!(manager.start("tiny").is_ok());
    }

    #[test]
    fn delete_model_removes_file_part_and_manifest_entry() {
        let models_dir = temp_dir("delete-model");
        write_file(&models_dir.join("ggml-tiny.bin"), 5);
        write_file(&models_dir.join("ggml-tiny.bin.part"), 2);
        let mut manifest = ModelManifest::default();
        manifest.upsert(entry("tiny", "ggml-tiny.bin", 5, true, ModelOrigin::App));
        save_manifest(&models_dir, &manifest).expect("manifest should save");

        let info = delete_model(&models_dir, "tiny").expect("model should delete");
        let manifest = load_manifest(&models_dir).expect("manifest should load");

        assert!(!models_dir.join("ggml-tiny.bin").exists());
        assert!(!models_dir.join("ggml-tiny.bin.part").exists());
        assert_eq!(manifest.get("tiny"), None);
        assert!(!info.downloaded);
        assert!(!info.usable);
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn verify_model_marks_manual_file_verified_and_usable() {
        let models_dir = temp_dir("verify-manual-success");
        fs::write(models_dir.join("ggml-tiny.bin"), b"hello").expect("model file should write");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            })),
            body: Vec::new(),
            stream_error: None,
        };

        let info = verify_model_with_client(&models_dir, "tiny", &client)
            .expect("manual model should verify");
        let manifest = load_manifest(&models_dir).expect("manifest should load");
        let entry = manifest.get("tiny").expect("manifest entry should exist");

        assert!(info.downloaded);
        assert!(info.verified);
        assert!(info.usable);
        assert!(!info.corrupted);
        assert_eq!(info.origin, Some(ModelOrigin::Manual));
        assert!(entry.verified);
        assert!(!entry.corrupted);
        assert_eq!(entry.origin, ModelOrigin::Manual);
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn verify_model_marks_mismatch_corrupted_and_rejects() {
        let models_dir = temp_dir("verify-mismatch");
        fs::write(models_dir.join("ggml-tiny.bin"), b"hello").expect("model file should write");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            })),
            body: Vec::new(),
            stream_error: None,
        };

        let error = verify_model_with_client(&models_dir, "tiny", &client).unwrap_err();
        let manifest = load_manifest(&models_dir).expect("manifest should load");
        let entry = manifest.get("tiny").expect("manifest entry should exist");
        let models = get_model_inventory(&models_dir).expect("inventory should load");
        let info = find(&models, "tiny");

        assert_eq!(error.code, VERIFY_FAILED);
        assert!(!entry.verified);
        assert!(entry.corrupted);
        assert!(info.corrupted);
        assert!(!info.usable);
        let _ = fs::remove_dir_all(models_dir);
    }

    #[test]
    fn verify_model_rejects_missing_file() {
        let models_dir = temp_dir("verify-missing");
        let client = FakeModelDownloadClient {
            metadata: Ok(Some(ExpectedModelMetadata {
                size_bytes: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            })),
            body: Vec::new(),
            stream_error: None,
        };

        let error = verify_model_with_client(&models_dir, "tiny", &client).unwrap_err();

        assert_eq!(error.code, MODEL_NOT_FOUND);
        assert!(!models_dir.join(MANIFEST_FILE).exists());
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
            corrupted: false,
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

    struct FakeModelDownloadClient {
        metadata: Result<Option<ExpectedModelMetadata>, AppError>,
        body: Vec<u8>,
        stream_error: Option<AppError>,
    }

    impl ModelDownloadClient for FakeModelDownloadClient {
        fn expected_metadata(
            &self,
            _catalog: &ModelCatalogEntry,
        ) -> Result<Option<ExpectedModelMetadata>, AppError> {
            self.metadata.clone()
        }

        fn stream_model(
            &self,
            _catalog: &ModelCatalogEntry,
            writer: &mut dyn Write,
        ) -> Result<u64, AppError> {
            writer.write_all(&self.body).map_err(download_io_error)?;
            if let Some(error) = self.stream_error.clone() {
                return Err(error);
            }
            Ok(self.body.len() as u64)
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum ModelEvent {
        Progress {
            name: String,
            downloaded_bytes: u64,
            total_bytes: Option<u64>,
        },
        Done {
            name: String,
        },
        Error {
            name: String,
            message: String,
        },
    }

    #[derive(Default)]
    struct RecordingModelEvents {
        events: Mutex<Vec<ModelEvent>>,
    }

    impl ModelDownloadEventSink for RecordingModelEvents {
        fn emit_progress(&self, payload: ModelProgressPayload) {
            self.events.lock().unwrap().push(ModelEvent::Progress {
                name: payload.name,
                downloaded_bytes: payload.downloaded_bytes,
                total_bytes: payload.total_bytes,
            });
        }

        fn emit_done(&self, payload: ModelDonePayload) {
            self.events
                .lock()
                .unwrap()
                .push(ModelEvent::Done { name: payload.name });
        }

        fn emit_error(&self, payload: ModelErrorPayload) {
            self.events.lock().unwrap().push(ModelEvent::Error {
                name: payload.name,
                message: payload.message,
            });
        }
    }
}
