use std::fs::File;
use std::io;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::resample::{AudioResampler, OUTPUT_SAMPLE_RATE};
use crate::error::{AppError, DECODE_FAILED, IO_ERROR};

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

pub fn decode_audio_file(path: impl AsRef<Path>) -> Result<DecodedAudio, AppError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(io_error)?;
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
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AppError::new(DECODE_FAILED, "audio track not found"))?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let input_sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| AppError::new(DECODE_FAILED, "audio sample rate is unknown"))?;
    let channels = codec_params
        .channels
        .ok_or_else(|| AppError::new(DECODE_FAILED, "audio channel layout is unknown"))?
        .count() as u16;
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(decode_error)?;
    let mut resampler = AudioResampler::new(input_sample_rate, channels)?;
    let mut output = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(decode_error(error)),
        };
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let mut buffer =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                buffer.copy_interleaved_ref(decoded);
                output.extend(resampler.process_interleaved(buffer.samples())?);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(decode_error(error)),
        }
    }
    output.extend(resampler.finish()?);

    if output.is_empty() {
        return Err(AppError::new(DECODE_FAILED, "decoded audio is empty"));
    }

    Ok(DecodedAudio {
        duration_ms: output.len() as u64 * 1_000 / OUTPUT_SAMPLE_RATE as u64,
        samples: output,
        sample_rate: OUTPUT_SAMPLE_RATE,
        channels: 1,
    })
}

fn decode_error(error: SymphoniaError) -> AppError {
    AppError::new(DECODE_FAILED, format!("failed to decode audio: {error}"))
}

fn io_error(error: io::Error) -> AppError {
    AppError::new(IO_ERROR, format!("io error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn decodes_wav_to_16khz_mono() {
        let path = temp_path("decodes_wav_to_16khz_mono.wav");
        write_stereo_wav(&path, 48_000, 500);

        let decoded = decode_audio_file(&path).unwrap();

        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.channels, 1);
        assert!((decoded.samples.len() as i64 - 8_000).abs() <= 128);
        assert!((decoded.duration_ms as i64 - 500).abs() <= 10);
        assert!(decoded.samples.iter().all(|sample| sample.is_finite()));
        assert!(decoded.samples.iter().any(|sample| sample.abs() > 0.1));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_files_without_audio_packets() {
        let path = temp_path("rejects_files_without_audio_packets.mp3");
        fs::write(&path, b"ID3\x04\0\0\0\0\0\0").unwrap();

        let error = decode_audio_file(&path).unwrap_err();

        assert_eq!(error.code, DECODE_FAILED);
        let _ = fs::remove_file(path);
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sokki-decode-{name}"))
    }

    fn write_stereo_wav(path: &Path, sample_rate: u32, duration_ms: u32) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        let frames = sample_rate as usize * duration_ms as usize / 1_000;
        for frame in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * frame as f32 / sample_rate as f32;
            let sample = (phase.sin() * i16::MAX as f32 * 0.5) as i16;
            writer.write_sample(sample).unwrap();
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }
}
