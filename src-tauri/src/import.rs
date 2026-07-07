use std::fs;
use std::io;
use std::path::Path;

use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{
    AppError, AUDIO_TOO_LONG, DECODE_FAILED, DISK_FULL, DURATION_UNKNOWN, FILE_TOO_LARGE, IO_ERROR,
};

const MAX_IMPORT_FILE_SIZE_BYTES: u64 = 2_000_000_000;
const MAX_IMPORT_DURATION_MS: u64 = 3 * 60 * 60 * 1_000;
const WAV_BYTES_PER_SECOND: u64 = 32_000;
const ALLOWED_EXTENSIONS: &[&str] = &["wav", "mp3", "m4a", "aac", "flac", "ogg"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportValidationInput<'a> {
    pub path: &'a Path,
    pub file_size_bytes: u64,
    pub duration_ms: Option<u64>,
    pub force_unknown_duration: bool,
    pub available_space_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportValidationPlan {
    pub duration_ms: Option<u64>,
    pub estimated_wav_size_bytes: u64,
}

pub fn validate_import_file(
    path: impl AsRef<Path>,
    force_unknown_duration: bool,
    available_space_bytes: u64,
) -> Result<ImportValidationPlan, AppError> {
    let path = path.as_ref();
    let metadata = fs::metadata(path).map_err(io_error)?;
    let duration_ms = probe_audio_duration_ms(path)?;
    validate_import_limits(ImportValidationInput {
        path,
        file_size_bytes: metadata.len(),
        duration_ms,
        force_unknown_duration,
        available_space_bytes,
    })
}

pub fn validate_import_limits(
    input: ImportValidationInput<'_>,
) -> Result<ImportValidationPlan, AppError> {
    validate_extension(input.path)?;
    if input.file_size_bytes > MAX_IMPORT_FILE_SIZE_BYTES {
        return Err(AppError::new(
            FILE_TOO_LARGE,
            "import file is larger than 2GB",
        ));
    }

    if input
        .duration_ms
        .is_some_and(|duration| duration > MAX_IMPORT_DURATION_MS)
    {
        return Err(AppError::new(
            AUDIO_TOO_LONG,
            "import audio is longer than 3 hours",
        ));
    }
    if input.duration_ms.is_none() && !input.force_unknown_duration {
        return Err(AppError::new(
            DURATION_UNKNOWN,
            "audio duration could not be determined",
        ));
    }

    let duration_for_estimate = input.duration_ms.unwrap_or(MAX_IMPORT_DURATION_MS);
    let estimated_wav_size_bytes = estimate_wav_size_bytes(duration_for_estimate);
    if exceeds_available_space_budget(estimated_wav_size_bytes, input.available_space_bytes) {
        return Err(AppError::new(
            DISK_FULL,
            "estimated wav size exceeds available disk budget",
        ));
    }

    Ok(ImportValidationPlan {
        duration_ms: input.duration_ms,
        estimated_wav_size_bytes,
    })
}

pub fn probe_audio_duration_ms(path: impl AsRef<Path>) -> Result<Option<u64>, AppError> {
    let path = path.as_ref();
    let file = fs::File::open(path).map_err(io_error)?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(decode_error)?;
    let track = probed
        .format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AppError::new(DECODE_FAILED, "audio track not found"))?;

    let Some(sample_rate) = track.codec_params.sample_rate else {
        return Ok(None);
    };
    Ok(track
        .codec_params
        .n_frames
        .map(|frames| frames * 1_000 / sample_rate as u64))
}

fn validate_extension(path: &Path) -> Result<(), AppError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    if extension
        .as_deref()
        .is_some_and(|extension| ALLOWED_EXTENSIONS.contains(&extension))
    {
        Ok(())
    } else {
        Err(AppError::new(
            DECODE_FAILED,
            "unsupported import file extension",
        ))
    }
}

fn estimate_wav_size_bytes(duration_ms: u64) -> u64 {
    duration_ms.saturating_mul(WAV_BYTES_PER_SECOND) / 1_000
}

fn exceeds_available_space_budget(
    estimated_wav_size_bytes: u64,
    available_space_bytes: u64,
) -> bool {
    (estimated_wav_size_bytes as u128) * 10 > (available_space_bytes as u128) * 9
}

fn decode_error(error: SymphoniaError) -> AppError {
    AppError::new(DECODE_FAILED, format!("failed to probe audio: {error}"))
}

fn io_error(error: io::Error) -> AppError {
    AppError::new(IO_ERROR, format!("io error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_unsupported_extension_before_other_limits() {
        let error = validate_import_limits(input("movie.webm"))
            .expect_err("webm is outside v1 import scope");

        assert_eq!(error.code, DECODE_FAILED);
    }

    #[test]
    fn rejects_files_larger_than_two_gigabytes() {
        let error = validate_import_limits(ImportValidationInput {
            file_size_bytes: MAX_IMPORT_FILE_SIZE_BYTES + 1,
            ..input("audio.wav")
        })
        .unwrap_err();

        assert_eq!(error.code, FILE_TOO_LARGE);
    }

    #[test]
    fn rejects_audio_longer_than_three_hours() {
        let error = validate_import_limits(ImportValidationInput {
            duration_ms: Some(MAX_IMPORT_DURATION_MS + 1),
            ..input("audio.mp3")
        })
        .unwrap_err();

        assert_eq!(error.code, AUDIO_TOO_LONG);
    }

    #[test]
    fn rejects_unknown_duration_without_force_flag() {
        let error = validate_import_limits(ImportValidationInput {
            duration_ms: None,
            force_unknown_duration: false,
            ..input("audio.flac")
        })
        .unwrap_err();

        assert_eq!(error.code, DURATION_UNKNOWN);
    }

    #[test]
    fn permits_unknown_duration_with_three_hour_disk_estimate_when_forced() {
        let plan = validate_import_limits(ImportValidationInput {
            duration_ms: None,
            force_unknown_duration: true,
            available_space_bytes: 400 * 1024 * 1024,
            ..input("audio.ogg")
        })
        .unwrap();

        assert_eq!(plan.duration_ms, None);
        assert_eq!(
            plan.estimated_wav_size_bytes,
            estimate_wav_size_bytes(MAX_IMPORT_DURATION_MS)
        );
    }

    #[test]
    fn rejects_when_estimated_wav_exceeds_ninety_percent_of_free_space() {
        let estimated = estimate_wav_size_bytes(60_000);
        let error = validate_import_limits(ImportValidationInput {
            duration_ms: Some(60_000),
            available_space_bytes: estimated,
            ..input("audio.m4a")
        })
        .unwrap_err();

        assert_eq!(error.code, DISK_FULL);
    }

    #[test]
    fn probes_wav_duration_from_file_header() {
        let path = temp_path("probes_wav_duration_from_file_header.wav");
        write_mono_wav(&path, 16_000, 750);

        let duration_ms = probe_audio_duration_ms(&path).unwrap();

        assert_eq!(duration_ms, Some(750));
        let _ = fs::remove_file(path);
    }

    fn input(name: &str) -> ImportValidationInput<'_> {
        ImportValidationInput {
            path: Path::new(name),
            file_size_bytes: 1024,
            duration_ms: Some(60_000),
            force_unknown_duration: false,
            available_space_bytes: 10 * 1024 * 1024 * 1024,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sokki-import-{name}"))
    }

    fn write_mono_wav(path: &Path, sample_rate: u32, duration_ms: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        let frames = sample_rate as usize * duration_ms as usize / 1_000;
        for _ in 0..frames {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
    }
}
