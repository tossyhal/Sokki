use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::audio::devices::{self, AudioDevice, AudioDevices};
use crate::audio::wav::{StreamingWavWriter, WAV_SAMPLE_RATE};
use crate::bootstrap::SOUNDCHECK_DIR;
use crate::db::Source;
use crate::error::{AppError, ALREADY_RECORDING, DEVICE_NOT_FOUND, IO_ERROR, SOUND_CHECK_BUSY};
use crate::recording::RecordingManager;

pub const SOUND_CHECK_LEVEL_EVENT: &str = "soundcheck://level";
const DEFAULT_DURATION_MS: u64 = 5_000;
const LEVEL_INTERVAL_MS: u64 = 100;
const SILENCE_PEAK_DB: f32 = -120.0;
const TEST_TONE_HZ: f32 = 880.0;
const TEST_TONE_AMPLITUDE: f32 = 0.2;

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
    pub peak_mic_db: f32,
    pub peak_system_db: f32,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundCheckLevelPayload {
    pub mic: f32,
    pub system: f32,
}

pub trait SoundCheckEventSink {
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
        events: &dyn SoundCheckEventSink,
    ) -> Result<SoundCheckResult, AppError> {
        if recording_manager.recording_active() {
            return Err(AppError::new(
                ALREADY_RECORDING,
                "recording is already active",
            ));
        }
        let _guard = self.enter_busy()?;
        let audio_devices = devices::list_audio_devices()?;
        self.run_with_devices(data_dir, request, &audio_devices, events)
    }

    fn run_with_devices(
        &self,
        data_dir: &Path,
        request: SoundCheckRequest,
        audio_devices: &AudioDevices,
        events: &dyn SoundCheckEventSink,
    ) -> Result<SoundCheckResult, AppError> {
        if request.source == Source::Import {
            return Err(AppError::new(
                IO_ERROR,
                "import source cannot be used for sound check",
            ));
        }
        validate_requested_devices(&request, audio_devices)?;

        let duration_ms = request.duration_ms.unwrap_or(DEFAULT_DURATION_MS);
        let wav_path = sound_check_wav_path(data_dir);
        let mut writer = StreamingWavWriter::create(&wav_path)?;
        let frame = sound_check_test_tone_frame();
        let ticks = duration_ms.div_ceil(LEVEL_INTERVAL_MS).max(1);
        let levels = level_payload_for_source(request.source);

        for _ in 0..ticks {
            writer.write_frame(&frame)?;
            events.emit_level(levels);
            thread::sleep(Duration::from_millis(LEVEL_INTERVAL_MS));
        }

        let actual_duration_ms = writer.finalize()?;
        let result = SoundCheckResult {
            wav_path: wav_path.display().to_string(),
            duration_ms: actual_duration_ms,
            peak_mic_db: peak_for_source(request.source, Source::Mic),
            peak_system_db: peak_for_source(request.source, Source::System),
            warnings: warnings_for_source(request.source),
        };
        Ok(result)
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

fn samples_per_level_tick() -> usize {
    (WAV_SAMPLE_RATE as u64 * LEVEL_INTERVAL_MS / 1_000) as usize
}

fn sound_check_test_tone_frame() -> Vec<f32> {
    let sample_count = samples_per_level_tick();
    let sample_rate = WAV_SAMPLE_RATE as f32;
    (0..sample_count)
        .map(|sample_index| {
            let phase =
                2.0 * std::f32::consts::PI * TEST_TONE_HZ * sample_index as f32 / sample_rate;
            phase.sin() * TEST_TONE_AMPLITUDE
        })
        .collect()
}

fn level_payload_for_source(source: Source) -> SoundCheckLevelPayload {
    match source {
        Source::Mic => SoundCheckLevelPayload {
            mic: 0.0,
            system: 0.0,
        },
        Source::System => SoundCheckLevelPayload {
            mic: 0.0,
            system: 0.0,
        },
        Source::Mix => SoundCheckLevelPayload {
            mic: 0.0,
            system: 0.0,
        },
        Source::Import => SoundCheckLevelPayload {
            mic: 0.0,
            system: 0.0,
        },
    }
}

fn peak_for_source(source: Source, target: Source) -> f32 {
    match (source, target) {
        (Source::Mic, Source::Mic)
        | (Source::System, Source::System)
        | (Source::Mix, Source::Mic)
        | (Source::Mix, Source::System) => SILENCE_PEAK_DB,
        _ => SILENCE_PEAK_DB,
    }
}

fn warnings_for_source(source: Source) -> Vec<String> {
    match source {
        Source::Mic => vec!["マイクがほぼ無音です".to_string()],
        Source::System => vec!["システム音声がほぼ無音です".to_string()],
        Source::Mix => vec![
            "マイクがほぼ無音です".to_string(),
            "システム音声がほぼ無音です".to_string(),
        ],
        Source::Import => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::devices::{AudioDevice, AudioDevices};
    use std::sync::Mutex;

    #[test]
    fn run_sound_check_writes_audible_wav_and_returns_absolute_path() {
        let manager = SoundCheckManager::new();
        let data_dir =
            temp_data_dir("run_sound_check_writes_audible_wav_and_returns_absolute_path");
        let events = TestSoundCheckEventSink::default();

        let result = manager
            .run_with_devices(
                &data_dir,
                sample_request(Source::Mic, 100),
                &sample_devices(),
                &events,
            )
            .unwrap();

        assert!(Path::new(&result.wav_path).is_absolute());
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.peak_mic_db, SILENCE_PEAK_DB);
        assert_eq!(events.levels.lock().unwrap().len(), 1);
        let mut reader = hound::WavReader::open(&result.wav_path).unwrap();
        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            samples.iter().any(|sample| *sample != 0),
            "sound check wav must contain audible samples for the playback gate"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn rejects_missing_requested_device() {
        let manager = SoundCheckManager::new();
        let data_dir = temp_data_dir("sound_check_rejects_missing_requested_device");
        let events = TestSoundCheckEventSink::default();
        let request = SoundCheckRequest {
            mic_device: Some("missing mic".to_string()),
            ..sample_request(Source::Mic, 100)
        };

        let error = manager
            .run_with_devices(&data_dir, request, &sample_devices(), &events)
            .unwrap_err();

        assert_eq!(error.code, DEVICE_NOT_FOUND);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn rejects_parallel_sound_checks() {
        let manager = SoundCheckManager::new();
        let _guard = manager.enter_busy().unwrap();

        let error = manager.enter_busy().unwrap_err();

        assert_eq!(error.code, SOUND_CHECK_BUSY);
    }

    fn sample_request(source: Source, duration_ms: u64) -> SoundCheckRequest {
        SoundCheckRequest {
            source,
            mic_device: None,
            loopback_device: None,
            duration_ms: Some(duration_ms),
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

    fn temp_data_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-sound-check-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[derive(Default)]
    struct TestSoundCheckEventSink {
        levels: Mutex<Vec<SoundCheckLevelPayload>>,
    }

    impl SoundCheckEventSink for TestSoundCheckEventSink {
        fn emit_level(&self, payload: SoundCheckLevelPayload) {
            self.levels.lock().unwrap().push(payload);
        }
    }
}
