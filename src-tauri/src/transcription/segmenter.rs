#[derive(Clone, Debug, PartialEq)]
pub struct RealtimeChunk {
    pub audio: Vec<f32>,
    pub chunk_start_ms: u64,
    pub valid_start_ms: u64,
    pub valid_end_ms: u64,
}

pub struct RealtimeSegmenter {
    sample_rate: u32,
    vad_threshold_db: i32,
    frame_samples: usize,
    preroll_samples: usize,
    silence_samples_to_commit: usize,
    max_speech_samples: usize,
    overlap_samples: usize,
    min_valid_samples: usize,
    cursor_samples: u64,
    pending: Vec<f32>,
    preroll: Vec<f32>,
    speech: Option<SpeechBuffer>,
}

struct SpeechBuffer {
    audio: Vec<f32>,
    chunk_start_sample: u64,
    valid_start_sample: u64,
    valid_end_sample: u64,
    trailing_silence_samples: usize,
}

const MAX_REALTIME_SPEECH_MS: u64 = 5_000;

impl RealtimeSegmenter {
    pub fn new(sample_rate: u32, vad_threshold_db: i32) -> Self {
        let frame_samples = samples_for_ms(sample_rate, 30).max(1);
        Self {
            sample_rate,
            vad_threshold_db,
            frame_samples,
            preroll_samples: samples_for_ms(sample_rate, 300),
            silence_samples_to_commit: samples_for_ms(sample_rate, 700),
            max_speech_samples: samples_for_ms(sample_rate, MAX_REALTIME_SPEECH_MS),
            overlap_samples: samples_for_ms(sample_rate, 600),
            min_valid_samples: samples_for_ms(sample_rate, 300),
            cursor_samples: 0,
            pending: Vec::new(),
            preroll: Vec::new(),
            speech: None,
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<RealtimeChunk> {
        self.pending.extend_from_slice(samples);
        let mut chunks = Vec::new();
        while self.pending.len() >= self.frame_samples {
            let frame: Vec<f32> = self.pending.drain(..self.frame_samples).collect();
            chunks.extend(self.process_frame(frame));
        }
        chunks
    }

    pub fn flush(&mut self) -> Option<RealtimeChunk> {
        if !self.pending.is_empty() {
            let frame = std::mem::take(&mut self.pending);
            let _ = self.process_frame(frame);
        }
        let speech = self.speech.take()?;
        self.finalize_speech(speech)
    }

    fn process_frame(&mut self, frame: Vec<f32>) -> Vec<RealtimeChunk> {
        let frame_start_sample = self.cursor_samples;
        self.cursor_samples += frame.len() as u64;
        let speech = is_speech(&frame, self.vad_threshold_db);

        match self.speech.take() {
            Some(mut active) => {
                active.audio.extend_from_slice(&frame);
                if speech {
                    active.valid_end_sample = self.cursor_samples;
                    active.trailing_silence_samples = 0;
                } else {
                    active.trailing_silence_samples += frame.len();
                }

                if active
                    .valid_end_sample
                    .saturating_sub(active.valid_start_sample)
                    >= self.max_speech_samples as u64
                {
                    let (chunk, next) = self.force_finalize(active);
                    self.speech = next;
                    chunk.into_iter().collect()
                } else if active.trailing_silence_samples >= self.silence_samples_to_commit {
                    self.finalize_speech(active).into_iter().collect()
                } else {
                    self.speech = Some(active);
                    Vec::new()
                }
            }
            None => {
                if speech {
                    let preroll_len = self.preroll.len();
                    let chunk_start_sample = frame_start_sample.saturating_sub(preroll_len as u64);
                    let mut audio = Vec::with_capacity(preroll_len + frame.len());
                    audio.extend_from_slice(&self.preroll);
                    audio.extend_from_slice(&frame);
                    self.speech = Some(SpeechBuffer {
                        audio,
                        chunk_start_sample,
                        valid_start_sample: frame_start_sample,
                        valid_end_sample: self.cursor_samples,
                        trailing_silence_samples: 0,
                    });
                    self.preroll.clear();
                } else {
                    self.preroll.extend_from_slice(&frame);
                    trim_front(&mut self.preroll, self.preroll_samples);
                }
                Vec::new()
            }
        }
    }

    fn force_finalize(
        &self,
        mut active: SpeechBuffer,
    ) -> (Option<RealtimeChunk>, Option<SpeechBuffer>) {
        let forced_valid_end = active.valid_start_sample + self.max_speech_samples as u64;
        let cut_offset = (forced_valid_end - active.chunk_start_sample) as usize;
        let chunk_audio = active.audio[..cut_offset.min(active.audio.len())].to_vec();
        let chunk = self.make_chunk(
            chunk_audio,
            active.chunk_start_sample,
            active.valid_start_sample,
            forced_valid_end,
        );

        let overlap_start = forced_valid_end.saturating_sub(self.overlap_samples as u64);
        let next_offset = (overlap_start - active.chunk_start_sample) as usize;
        let next_audio = active.audio[next_offset.min(active.audio.len())..].to_vec();
        let next = if active.valid_end_sample > forced_valid_end {
            active.audio = next_audio;
            active.chunk_start_sample = overlap_start;
            active.valid_start_sample = forced_valid_end;
            Some(active)
        } else {
            None
        };

        (chunk, next)
    }

    fn finalize_speech(&self, active: SpeechBuffer) -> Option<RealtimeChunk> {
        let valid_end = active.valid_end_sample;
        let audio_len =
            (valid_end.saturating_sub(active.chunk_start_sample) as usize).min(active.audio.len());
        self.make_chunk(
            active.audio[..audio_len].to_vec(),
            active.chunk_start_sample,
            active.valid_start_sample,
            valid_end,
        )
    }

    fn make_chunk(
        &self,
        audio: Vec<f32>,
        chunk_start_sample: u64,
        valid_start_sample: u64,
        valid_end_sample: u64,
    ) -> Option<RealtimeChunk> {
        if valid_end_sample.saturating_sub(valid_start_sample) < self.min_valid_samples as u64 {
            return None;
        }
        Some(RealtimeChunk {
            audio,
            chunk_start_ms: sample_to_ms(chunk_start_sample, self.sample_rate),
            valid_start_ms: sample_to_ms(valid_start_sample, self.sample_rate),
            valid_end_ms: sample_to_ms(valid_end_sample, self.sample_rate),
        })
    }
}

fn is_speech(samples: &[f32], vad_threshold_db: i32) -> bool {
    if samples.is_empty() {
        return false;
    }
    let energy = samples
        .iter()
        .map(|sample| (*sample as f64) * (*sample as f64))
        .sum::<f64>()
        / samples.len() as f64;
    let rms = energy.sqrt();
    let db = 20.0 * (rms + 1e-9).log10();
    db > vad_threshold_db as f64
}

fn samples_for_ms(sample_rate: u32, ms: u64) -> usize {
    (sample_rate as u64 * ms / 1_000) as usize
}

fn sample_to_ms(sample: u64, sample_rate: u32) -> u64 {
    sample * 1_000 / sample_rate as u64
}

fn trim_front(samples: &mut Vec<f32>, max_len: usize) {
    if samples.len() > max_len {
        let excess = samples.len() - max_len;
        samples.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_chunk_after_silence_threshold_with_preroll_excluded_from_valid_range() {
        let mut segmenter = RealtimeSegmenter::new(16_000, -40);

        assert!(segmenter.push(&silence_ms(300)).is_empty());
        assert!(segmenter.push(&tone_ms(900)).is_empty());
        let chunks = segmenter.push(&silence_ms(720));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_start_ms, 0);
        assert_eq!(chunks[0].valid_start_ms, 300);
        assert_eq!(chunks[0].valid_end_ms, 1_200);
        assert_eq!(chunks[0].audio.len(), samples_for_ms(1_200));
    }

    #[test]
    fn force_emits_at_5_seconds_and_overlaps_next_chunk_by_600ms() {
        let mut segmenter = RealtimeSegmenter::new(16_000, -40);

        let chunks = segmenter.push(&tone_ms(5_400));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_start_ms, 0);
        assert_eq!(chunks[0].valid_start_ms, 0);
        assert_eq!(chunks[0].valid_end_ms, 5_000);
        assert_eq!(chunks[0].audio.len(), samples_for_ms(5_000));

        let chunks = segmenter.push(&silence_ms(720));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_start_ms, 4_400);
        assert_eq!(chunks[0].valid_start_ms, 5_000);
        assert_eq!(chunks[0].valid_end_ms, 5_400);
    }

    #[test]
    fn discards_chunks_shorter_than_300ms() {
        let mut segmenter = RealtimeSegmenter::new(16_000, -40);

        assert!(segmenter.push(&tone_ms(290)).is_empty());
        assert!(segmenter.push(&silence_ms(700)).is_empty());
    }

    #[test]
    fn flush_emits_current_speech_without_waiting_for_silence() {
        let mut segmenter = RealtimeSegmenter::new(16_000, -40);

        assert!(segmenter.push(&silence_ms(300)).is_empty());
        assert!(segmenter.push(&tone_ms(500)).is_empty());
        let chunk = segmenter.flush().expect("speech should flush");

        assert_eq!(chunk.chunk_start_ms, 0);
        assert_eq!(chunk.valid_start_ms, 300);
        assert_eq!(chunk.valid_end_ms, 800);
        assert_eq!(chunk.audio.len(), samples_for_ms(800));
    }

    fn tone_ms(ms: u64) -> Vec<f32> {
        vec![0.2; samples_for_ms(ms)]
    }

    fn silence_ms(ms: u64) -> Vec<f32> {
        vec![0.0; samples_for_ms(ms)]
    }

    fn samples_for_ms(ms: u64) -> usize {
        (16_000 * ms / 1_000) as usize
    }
}
