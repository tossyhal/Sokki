use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::error::{AppError, DECODE_FAILED};

pub const OUTPUT_SAMPLE_RATE: u32 = 16_000;
const MAX_RELATIVE_RATIO: f64 = 2.0;

pub struct AudioResampler {
    input_sample_rate: u32,
    channels: usize,
    input_chunk_frames: usize,
    pending_mono: Vec<f32>,
    resampler: Option<SincFixedIn<f32>>,
}

impl AudioResampler {
    pub fn new(input_sample_rate: u32, channels: u16) -> Result<Self, AppError> {
        if input_sample_rate == 0 {
            return Err(invalid_audio("input sample rate must be greater than zero"));
        }
        if channels == 0 {
            return Err(invalid_audio("channel count must be greater than zero"));
        }

        let input_chunk_frames = ((input_sample_rate as usize) / 50).max(16);
        let resampler = if input_sample_rate == OUTPUT_SAMPLE_RATE {
            None
        } else {
            Some(
                SincFixedIn::<f32>::new(
                    OUTPUT_SAMPLE_RATE as f64 / input_sample_rate as f64,
                    MAX_RELATIVE_RATIO,
                    SincInterpolationParameters {
                        sinc_len: 128,
                        f_cutoff: 0.95,
                        interpolation: SincInterpolationType::Linear,
                        oversampling_factor: 128,
                        window: WindowFunction::BlackmanHarris2,
                    },
                    input_chunk_frames,
                    1,
                )
                .map_err(|err| {
                    AppError::new(DECODE_FAILED, format!("failed to create resampler: {err}"))
                })?,
            )
        };

        Ok(Self {
            input_sample_rate,
            channels: channels as usize,
            input_chunk_frames,
            pending_mono: Vec::with_capacity(input_chunk_frames),
            resampler,
        })
    }

    pub fn output_sample_rate(&self) -> u32 {
        OUTPUT_SAMPLE_RATE
    }

    pub fn process_interleaved(&mut self, samples: &[f32]) -> Result<Vec<f32>, AppError> {
        let mono = downmix_interleaved_to_mono(samples, self.channels)?;
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(mono);
        };

        let estimated_output = (mono.len() as u64 * OUTPUT_SAMPLE_RATE as u64
            / self.input_sample_rate as u64) as usize;
        let mut output = Vec::with_capacity(estimated_output.saturating_add(128));
        self.pending_mono.extend(mono);

        let ready_len = self.pending_mono.len() / self.input_chunk_frames * self.input_chunk_frames;
        for chunk in self.pending_mono[..ready_len].chunks(self.input_chunk_frames) {
            output.extend(process_mono_chunk(resampler, chunk, false)?);
        }
        self.pending_mono.drain(..ready_len);

        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<f32>, AppError> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(Vec::new());
        };
        if self.pending_mono.is_empty() {
            return Ok(Vec::new());
        }

        let output = process_mono_chunk(resampler, &self.pending_mono, true)?;
        self.pending_mono.clear();
        Ok(output)
    }
}

fn process_mono_chunk(
    resampler: &mut SincFixedIn<f32>,
    chunk: &[f32],
    is_final: bool,
) -> Result<Vec<f32>, AppError> {
    let wave = [chunk];
    let mut resampled = if is_final {
        resampler.process_partial(Some(&wave), None)
    } else {
        resampler.process(&wave, None)
    }
    .map_err(|err| AppError::new(DECODE_FAILED, format!("failed to resample audio: {err}")))?;

    Ok(resampled.pop().unwrap_or_default())
}

pub fn downmix_interleaved_to_mono(samples: &[f32], channels: usize) -> Result<Vec<f32>, AppError> {
    if channels == 0 {
        return Err(invalid_audio("channel count must be greater than zero"));
    }
    if samples.len() % channels != 0 {
        return Err(invalid_audio(
            "interleaved audio length must be divisible by channel count",
        ));
    }

    let mut mono = Vec::with_capacity(samples.len() / channels);
    for frame in samples.chunks_exact(channels) {
        let sum: f32 = frame.iter().sum();
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}

fn invalid_audio(message: impl Into<String>) -> AppError {
    AppError::new(DECODE_FAILED, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_interleaved_channels_to_mono() {
        let mono = downmix_interleaved_to_mono(&[0.5, -0.5, 1.0, 0.0], 2).unwrap();

        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn rejects_partial_interleaved_frame() {
        let error = downmix_interleaved_to_mono(&[0.0, 1.0, 2.0], 2).unwrap_err();

        assert_eq!(error.code, DECODE_FAILED);
        assert!(error.message.contains("divisible"));
    }

    #[test]
    fn resamples_48khz_sine_wave_to_16khz_mono() {
        let mut resampler = AudioResampler::new(48_000, 2).unwrap();
        let input = stereo_sine(440.0, 48_000, 0.5);

        let output = resampler.process_interleaved(&input).unwrap();

        assert_eq!(resampler.output_sample_rate(), 16_000);
        assert!((output.len() as i64 - 8_000).abs() <= 64);
        assert!(output.iter().any(|sample| sample.abs() > 0.5));
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn keeps_partial_input_between_streaming_calls() {
        let input = stereo_sine(440.0, 48_000, 0.5);
        let mut one_shot = AudioResampler::new(48_000, 2).unwrap();
        let mut one_shot_output = one_shot.process_interleaved(&input).unwrap();
        one_shot_output.extend(one_shot.finish().unwrap());

        let mut streaming = AudioResampler::new(48_000, 2).unwrap();
        let mut streaming_output = Vec::new();
        for chunk in input.chunks(4096) {
            streaming_output.extend(streaming.process_interleaved(chunk).unwrap());
        }
        streaming_output.extend(streaming.finish().unwrap());

        assert_eq!(streaming_output.len(), one_shot_output.len());
        assert!(streaming_output
            .iter()
            .zip(&one_shot_output)
            .all(|(left, right)| (left - right).abs() < 0.0001));
    }

    fn stereo_sine(freq_hz: f32, sample_rate: u32, seconds: f32) -> Vec<f32> {
        let frames = (sample_rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * freq_hz * frame as f32 / sample_rate as f32;
            let sample = phase.sin();
            samples.push(sample);
            samples.push(sample);
        }
        samples
    }
}
