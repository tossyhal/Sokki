use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::audio::devices::{self, AudioDevice, AudioDevices};
use crate::audio::mixer::{MixerSink, SourcePeaks};
use crate::audio::pipeline::{CapturePipeline, CapturePipelineConfig};
use crate::audio::wav::StreamingWavWriter;
use crate::bootstrap::SOUNDCHECK_DIR;
use crate::db::Source;
use crate::error::{AppError, ALREADY_RECORDING, DEVICE_NOT_FOUND, IO_ERROR, SOUND_CHECK_BUSY};
use crate::recording::RecordingManager;

pub const SOUND_CHECK_LEVEL_EVENT: &str = "soundcheck://level";
const DEFAULT_DURATION_MS: u64 = 5_000;
const ERROR_POLL_INTERVAL_MS: u64 = 50;
const SILENCE_PEAK_DB: f32 = -120.0;
const SILENCE_WARNING_DB: f32 = -60.0;
const CLIPPING_WARNING_DB: f32 = -1.0;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SoundCheckRequest {
    pub source: Source,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundCheckResult {
    pub wav_path: String,
    pub duration_ms: u64,
    pub peak_mic_db: Option<f32>,
    pub peak_system_db: Option<f32>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundCheckLevelPayload {
    pub mic: f32,
    pub system: f32,
}

pub trait SoundCheckEventSink: Send + Sync {
    fn emit_level(&self, payload: SoundCheckLevelPayload);
}

pub struct TauriSoundCheckEventSink {
    app: tauri::AppHandle,
}

pub struct SoundCheckManager {
    busy: AtomicBool,
}

#[derive(Debug)]
struct BusyGuard<'a> {
    busy: &'a AtomicBool,
}

/// Receives mixed frames from the sound-check capture pipeline and streams
/// them into the test WAV. The first write failure is kept and aborts the run.
struct SoundCheckSink {
    io: Arc<Mutex<SoundCheckIo>>,
}

struct SoundCheckIo {
    writer: Option<StreamingWavWriter>,
    write_error: Option<AppError>,
}

impl MixerSink for SoundCheckSink {
    fn write_frame(&mut self, frame: &[f32]) {
        let mut io = self
            .io
            .lock()
            .expect("sound check io mutex should not be poisoned");
        if io.write_error.is_some() {
            return;
        }
        if let Some(writer) = io.writer.as_mut() {
            if let Err(error) = writer.write_frame(frame) {
                io.write_error = Some(error);
            }
        }
    }
}

impl SoundCheckManager {
    pub fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
        }
    }

    pub fn run(
        &self,
        recording_manager: &RecordingManager,
        data_dir: &Path,
        request: SoundCheckRequest,
        events: Arc<dyn SoundCheckEventSink>,
    ) -> Result<SoundCheckResult, AppError> {
        if recording_manager.recording_active() {
            return Err(AppError::new(
                ALREADY_RECORDING,
                "recording is already active",
            ));
        }
        let _guard = self.enter_busy()?;
        if request.source == Source::Import {
            return Err(AppError::new(
                IO_ERROR,
                "import source cannot be used for sound check",
            ));
        }
        let audio_devices = devices::list_audio_devices()?;
        validate_requested_devices(&request, &audio_devices)?;
        capture_sound_check(data_dir, request, events)
    }

    fn enter_busy(&self) -> Result<BusyGuard<'_>, AppError> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(AppError::new(
                SOUND_CHECK_BUSY,
                "sound check is already running",
            ));
        }
        Ok(BusyGuard { busy: &self.busy })
    }
}

fn capture_sound_check(
    data_dir: &Path,
    request: SoundCheckRequest,
    events: Arc<dyn SoundCheckEventSink>,
) -> Result<SoundCheckResult, AppError> {
    let duration_ms = request.duration_ms.unwrap_or(DEFAULT_DURATION_MS);
    let wav_path = sound_check_wav_path(data_dir);
    let writer = StreamingWavWriter::create(&wav_path)?;

    let io = Arc::new(Mutex::new(SoundCheckIo {
        writer: Some(writer),
        write_error: None,
    }));
    let peaks = Arc::new(Mutex::new(SourcePeaks::default()));
    let capture_error: Arc<Mutex<Option<AppError>>> = Arc::new(Mutex::new(None));

    let on_level = {
        let peaks = Arc::clone(&peaks);
        let events = Arc::clone(&events);
        move |levels: crate::audio::mixer::SourceLevels, frame_peaks: SourcePeaks| {
            let mut peaks = peaks
                .lock()
                .expect("sound check peaks mutex should not be poisoned");
            peaks.mic = peaks.mic.max(frame_peaks.mic);
            peaks.system = peaks.system.max(frame_peaks.system);
            drop(peaks);
            events.emit_level(SoundCheckLevelPayload {
                mic: levels.mic,
                system: levels.system,
            });
        }
    };
    let on_error = {
        let capture_error = Arc::clone(&capture_error);
        Arc::new(move |error: AppError| {
            capture_error
                .lock()
                .expect("sound check error mutex should not be poisoned")
                .get_or_insert(error);
        })
    };

    let pipeline = CapturePipeline::start(
        CapturePipelineConfig {
            source: request.source,
            mic_device: request.mic_device.clone(),
            loopback_device: request.loopback_device.clone(),
            // Measure true input peaks: no gain applied during sound check.
            mic_gain: 1.0,
            system_gain: 1.0,
        },
        Arc::new(AtomicU64::new(0)),
        SoundCheckSink {
            io: Arc::clone(&io),
        },
        on_level,
        on_error,
    );
    let pipeline = match pipeline {
        Ok(pipeline) => pipeline,
        Err(error) => {
            let _ = std::fs::remove_file(&wav_path);
            return Err(error);
        }
    };

    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    while Instant::now() < deadline {
        if capture_error
            .lock()
            .expect("sound check error mutex should not be poisoned")
            .is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(ERROR_POLL_INTERVAL_MS));
    }
    pipeline.stop();

    let mut io = io
        .lock()
        .expect("sound check io mutex should not be poisoned");
    let failure = capture_error
        .lock()
        .expect("sound check error mutex should not be poisoned")
        .take()
        .or_else(|| io.write_error.take());
    if let Some(error) = failure {
        drop(io);
        let _ = std::fs::remove_file(&wav_path);
        return Err(error);
    }

    let actual_duration_ms = io
        .writer
        .take()
        .ok_or_else(|| AppError::new(IO_ERROR, "sound check wav writer is already closed"))?
        .finalize()?;
    drop(io);

    let peaks = *peaks
        .lock()
        .expect("sound check peaks mutex should not be poisoned");
    let (peak_mic_db, peak_system_db, warnings) = analyze_peaks(request.source, peaks);

    Ok(SoundCheckResult {
        wav_path: wav_path.display().to_string(),
        duration_ms: actual_duration_ms,
        peak_mic_db,
        peak_system_db,
        warnings,
    })
}

/// Converts per-source sample peaks into dBFS values for the sources the
/// request captured, plus the spec §5.7 warnings (near-silent / clipping).
fn analyze_peaks(source: Source, peaks: SourcePeaks) -> (Option<f32>, Option<f32>, Vec<String>) {
    let mic_db = matches!(source, Source::Mic | Source::Mix).then(|| peak_db(peaks.mic));
    let system_db = matches!(source, Source::System | Source::Mix).then(|| peak_db(peaks.system));

    let mut warnings = Vec::new();
    if let Some(db) = mic_db {
        if db < SILENCE_WARNING_DB {
            warnings.push("マイクがほぼ無音です".to_string());
        } else if db > CLIPPING_WARNING_DB {
            warnings.push("マイク入力が大きすぎます".to_string());
        }
    }
    if let Some(db) = system_db {
        if db < SILENCE_WARNING_DB {
            warnings.push("システム音声がほぼ無音です".to_string());
        } else if db > CLIPPING_WARNING_DB {
            warnings.push("システム音声が大きすぎます".to_string());
        }
    }
    (mic_db, system_db, warnings)
}

fn peak_db(peak: f32) -> f32 {
    if peak <= 0.0 {
        SILENCE_PEAK_DB
    } else {
        (20.0 * peak.log10()).max(SILENCE_PEAK_DB)
    }
}

impl Default for SoundCheckManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TauriSoundCheckEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl SoundCheckEventSink for TauriSoundCheckEventSink {
    fn emit_level(&self, payload: SoundCheckLevelPayload) {
        let _ = self.app.emit(SOUND_CHECK_LEVEL_EVENT, payload);
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::SeqCst);
    }
}

fn validate_requested_devices(
    request: &SoundCheckRequest,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    match request.source {
        Source::Mic => validate_input_device(request.mic_device.as_deref(), audio_devices),
        Source::System => validate_output_device(request.loopback_device.as_deref(), audio_devices),
        Source::Mix => {
            validate_input_device(request.mic_device.as_deref(), audio_devices)?;
            validate_output_device(request.loopback_device.as_deref(), audio_devices)
        }
        Source::Import => Ok(()),
    }
}

fn validate_input_device(
    requested: Option<&str>,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    validate_device("input", requested, &audio_devices.inputs)
}

fn validate_output_device(
    requested: Option<&str>,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    validate_device("output", requested, &audio_devices.outputs)
}

fn validate_device(
    kind: &str,
    requested: Option<&str>,
    devices: &[AudioDevice],
) -> Result<(), AppError> {
    if devices.is_empty() {
        return Err(AppError::new(
            DEVICE_NOT_FOUND,
            format!("no {kind} audio device is available"),
        ));
    }

    if let Some(requested) = requested {
        let found = devices.iter().any(|device| device.id == requested);
        if !found {
            return Err(AppError::new(
                DEVICE_NOT_FOUND,
                format!("{kind} audio device not found: {requested}"),
            ));
        }
    }

    Ok(())
}

fn sound_check_wav_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join(SOUNDCHECK_DIR)
        .join(format!("test-{}.wav", timestamp_ms()))
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::devices::{AudioDevice, AudioDevices};

    #[test]
    fn rejects_missing_requested_device() {
        let request = SoundCheckRequest {
            mic_device: Some("missing mic".to_string()),
            ..sample_request(Source::Mic)
        };

        let error = validate_requested_devices(&request, &sample_devices()).unwrap_err();

        assert_eq!(error.code, DEVICE_NOT_FOUND);
    }

    #[test]
    fn rejects_parallel_sound_checks() {
        let manager = SoundCheckManager::new();
        let _guard = manager.enter_busy().unwrap();

        let error = manager.enter_busy().unwrap_err();

        assert_eq!(error.code, SOUND_CHECK_BUSY);
    }

    #[test]
    fn analyze_reports_peaks_only_for_captured_sources() {
        let peaks = SourcePeaks {
            mic: 0.5,
            system: 0.5,
        };

        let (mic, system, warnings) = analyze_peaks(Source::Mic, peaks);
        assert!((mic.unwrap() - -6.02).abs() < 0.01);
        assert_eq!(system, None);
        assert!(warnings.is_empty());

        let (mic, system, _) = analyze_peaks(Source::System, peaks);
        assert_eq!(mic, None);
        assert!(system.is_some());
    }

    #[test]
    fn analyze_warns_on_silent_sources() {
        let (mic, system, warnings) = analyze_peaks(Source::Mix, SourcePeaks::default());

        assert_eq!(mic, Some(SILENCE_PEAK_DB));
        assert_eq!(system, Some(SILENCE_PEAK_DB));
        assert_eq!(
            warnings,
            vec![
                "マイクがほぼ無音です".to_string(),
                "システム音声がほぼ無音です".to_string(),
            ]
        );
    }

    #[test]
    fn analyze_warns_on_clipping_sources() {
        let peaks = SourcePeaks {
            mic: 1.0,
            system: 0.999,
        };

        let (mic, _, warnings) = analyze_peaks(Source::Mix, peaks);

        assert_eq!(mic, Some(0.0));
        assert_eq!(
            warnings,
            vec![
                "マイク入力が大きすぎます".to_string(),
                "システム音声が大きすぎます".to_string(),
            ]
        );
    }

    fn sample_request(source: Source) -> SoundCheckRequest {
        SoundCheckRequest {
            source,
            mic_device: None,
            loopback_device: None,
            duration_ms: Some(100),
        }
    }

    fn sample_devices() -> AudioDevices {
        AudioDevices {
            inputs: vec![AudioDevice {
                id: "mic-1".to_string(),
                name: "Microphone".to_string(),
                is_default: true,
            }],
            outputs: vec![AudioDevice {
                id: "speaker-1".to_string(),
                name: "Speakers".to_string(),
                is_default: true,
            }],
        }
    }
}
