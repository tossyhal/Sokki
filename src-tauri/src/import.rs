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
const BATCH_CHUNK_MAX_MS: u64 = 15_000;
const BATCH_FORCED_OVERLAP_MS: u64 = 1_000;
const VAD_FRAME_MS: u64 = 30;
const VAD_MIN_SILENCE_MS: u64 = 700;
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

#[derive(Clone, Debug, PartialEq)]
pub struct BatchAudioChunk {
    pub audio: Vec<f32>,
    pub chunk_start_ms: u64,
    pub valid_start_ms: u64,
    pub valid_end_ms: u64,
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

pub fn split_batch_audio_chunks(
    samples: &[f32],
    sample_rate: u32,
    vad_threshold_db: i32,
) -> Vec<BatchAudioChunk> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    let total_ms = sample_index_to_ms(samples.len(), sample_rate);
    let cut_candidates = offline_vad_cut_candidates(samples, sample_rate, vad_threshold_db);
    let mut chunks = Vec::new();
    let mut candidate_index = 0;
    let mut valid_start_ms = 0;
    let mut chunk_start_ms = 0;

    while valid_start_ms < total_ms {
        let remaining_ms = total_ms - valid_start_ms;
        let max_end_ms = valid_start_ms + remaining_ms.min(BATCH_CHUNK_MAX_MS);
        let (valid_end_ms, forced_cut) = if remaining_ms <= BATCH_CHUNK_MAX_MS {
            (total_ms, false)
        } else {
            while cut_candidates
                .get(candidate_index)
                .is_some_and(|candidate| *candidate <= valid_start_ms)
            {
                candidate_index += 1;
            }

            let mut chosen_candidate = None;
            let mut lookahead = candidate_index;
            while cut_candidates
                .get(lookahead)
                .is_some_and(|candidate| *candidate <= max_end_ms)
            {
                chosen_candidate = cut_candidates.get(lookahead).copied();
                lookahead += 1;
            }

            if let Some(candidate) = chosen_candidate {
                candidate_index = lookahead;
                (candidate, false)
            } else {
                (max_end_ms, true)
            }
        };

        if valid_end_ms <= valid_start_ms {
            break;
        }

        let start_sample = ms_to_sample_index(chunk_start_ms, sample_rate).min(samples.len());
        let end_sample = ms_to_sample_index(valid_end_ms, sample_rate).min(samples.len());
        chunks.push(BatchAudioChunk {
            audio: samples[start_sample..end_sample].to_vec(),
            chunk_start_ms,
            valid_start_ms,
            valid_end_ms,
        });

        valid_start_ms = valid_end_ms;
        chunk_start_ms = if forced_cut {
            valid_end_ms.saturating_sub(BATCH_FORCED_OVERLAP_MS)
        } else {
            valid_end_ms
        };
    }

    chunks
}

pub fn offline_vad_cut_candidates(
    samples: &[f32],
    sample_rate: u32,
    vad_threshold_db: i32,
) -> Vec<u64> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    let frame_samples = ms_to_sample_index(VAD_FRAME_MS, sample_rate).max(1);
    let mut candidates = Vec::new();
    let mut seen_speech = false;
    let mut silence_start_ms = None;
    let mut frame_start = 0;

    while frame_start < samples.len() {
        let frame_end = (frame_start + frame_samples).min(samples.len());
        let frame = &samples[frame_start..frame_end];
        let speech = db_from_rms(frame) > vad_threshold_db as f32;
        let start_ms = sample_index_to_ms(frame_start, sample_rate);

        if speech {
            if seen_speech {
                if let Some(start) = silence_start_ms.take() {
                    push_silence_candidate(&mut candidates, start, start_ms);
                }
            } else {
                silence_start_ms = None;
                seen_speech = true;
            }
        } else if seen_speech && silence_start_ms.is_none() {
            silence_start_ms = Some(start_ms);
        }

        frame_start = frame_end;
    }

    candidates
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

fn push_silence_candidate(candidates: &mut Vec<u64>, start_ms: u64, end_ms: u64) {
    if end_ms.saturating_sub(start_ms) >= VAD_MIN_SILENCE_MS {
        candidates.push(start_ms + (end_ms - start_ms) / 2);
    }
}

fn db_from_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -180.0;
    }
    let square_sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    let rms = (square_sum / samples.len() as f32).sqrt();
    20.0 * (rms + 1e-9).log10()
}

fn ms_to_sample_index(ms: u64, sample_rate: u32) -> usize {
    ((ms as u128 * sample_rate as u128) / 1_000) as usize
}

fn sample_index_to_ms(sample_index: usize, sample_rate: u32) -> u64 {
    sample_index as u64 * 1_000 / sample_rate as u64
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

    #[test]
    fn splits_long_audio_at_fifteen_seconds_with_one_second_overlap() {
        let samples = tone_ms(31_000);

        let chunks = split_batch_audio_chunks(&samples, 16_000, -40);

        assert_eq!(chunks.len(), 3);
        assert_chunk(&chunks[0], 0, 0, 15_000, 15_000);
        assert_chunk(&chunks[1], 14_000, 15_000, 30_000, 16_000);
        assert_chunk(&chunks[2], 29_000, 30_000, 31_000, 2_000);
    }

    #[test]
    fn prefers_farthest_vad_cut_candidate_within_fifteen_seconds() {
        let mut samples = Vec::new();
        samples.extend(tone_ms(9_000));
        samples.extend(silence_ms(1_000));
        samples.extend(tone_ms(10_000));

        let candidates = offline_vad_cut_candidates(&samples, 16_000, -40);
        let chunks = split_batch_audio_chunks(&samples, 16_000, -40);

        assert_eq!(candidates, vec![9_495]);
        assert_eq!(chunks.len(), 2);
        assert_chunk(&chunks[0], 0, 0, 9_495, 9_495);
        assert_chunk(&chunks[1], 9_495, 9_495, 20_000, 10_505);
    }

    #[test]
    fn keeps_short_audio_in_single_chunk_even_with_silence() {
        let mut samples = Vec::new();
        samples.extend(tone_ms(4_000));
        samples.extend(silence_ms(1_000));
        samples.extend(tone_ms(4_000));

        let chunks = split_batch_audio_chunks(&samples, 16_000, -40);

        assert_eq!(chunks.len(), 1);
        assert_chunk(&chunks[0], 0, 0, 9_000, 9_000);
    }

    #[test]
    fn ignores_leading_and_trailing_silence_as_cut_candidates() {
        let mut samples = Vec::new();
        samples.extend(silence_ms(1_000));
        samples.extend(tone_ms(20_000));
        samples.extend(silence_ms(1_000));

        let candidates = offline_vad_cut_candidates(&samples, 16_000, -40);
        let chunks = split_batch_audio_chunks(&samples, 16_000, -40);

        assert_eq!(candidates, Vec::<u64>::new());
        assert_eq!(chunks.len(), 2);
        assert_chunk(&chunks[0], 0, 0, 15_000, 15_000);
        assert_chunk(&chunks[1], 14_000, 15_000, 22_000, 8_000);
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

    fn tone_ms(duration_ms: u64) -> Vec<f32> {
        let frames = ms_to_sample_index(duration_ms, 16_000);
        (0..frames)
            .map(|frame| {
                let phase = 2.0 * std::f32::consts::PI * 440.0 * frame as f32 / 16_000.0;
                phase.sin() * 0.5
            })
            .collect()
    }

    fn silence_ms(duration_ms: u64) -> Vec<f32> {
        vec![0.0; ms_to_sample_index(duration_ms, 16_000)]
    }

    fn assert_chunk(
        chunk: &BatchAudioChunk,
        chunk_start_ms: u64,
        valid_start_ms: u64,
        valid_end_ms: u64,
        audio_duration_ms: u64,
    ) {
        assert_eq!(chunk.chunk_start_ms, chunk_start_ms);
        assert_eq!(chunk.valid_start_ms, valid_start_ms);
        assert_eq!(chunk.valid_end_ms, valid_end_ms);
        assert_eq!(
            sample_index_to_ms(chunk.audio.len(), 16_000),
            audio_duration_ms
        );
    }
}
