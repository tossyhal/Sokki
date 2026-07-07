use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use serde::Serialize;

use crate::audio::buffer_pool::{AudioPacket, BufferPool};
use crate::audio::resample::{AudioResampler, OUTPUT_SAMPLE_RATE};
use crate::error::AppError;

pub const MIXER_TICK_MS: u64 = 20;
pub const MIXER_FRAME_SAMPLES: usize = 320;
pub const JITTER_MAX_MS: u64 = 200;
pub const JITTER_MAX_SAMPLES: usize = 3_200;
const LEVEL_TICKS: u64 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MixerSource {
    Mic,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLevels {
    pub mic: f32,
    pub system: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixedFrame {
    pub samples: Vec<f32>,
    pub levels: Option<SourceLevels>,
}

pub struct MixerCore {
    mic: JitterBuffer,
    system: JitterBuffer,
    mic_gain: f32,
    system_gain: f32,
    level_accumulator: LevelAccumulator,
    tick_count: u64,
}

impl MixerCore {
    pub fn new(mic_gain: f32, system_gain: f32) -> Self {
        Self {
            mic: JitterBuffer::new(JITTER_MAX_SAMPLES),
            system: JitterBuffer::new(JITTER_MAX_SAMPLES),
            mic_gain,
            system_gain,
            level_accumulator: LevelAccumulator::new(),
            tick_count: 0,
        }
    }

    pub fn push(&mut self, source: MixerSource, samples: &[f32]) {
        self.buffer_mut(source).push(samples);
    }

    pub fn buffered_samples(&self, source: MixerSource) -> usize {
        self.buffer(source).len()
    }

    pub fn has_buffered_samples(&self) -> bool {
        self.mic.len() > 0 || self.system.len() > 0
    }

    pub fn tick(&mut self) -> MixedFrame {
        let mic = self.mic.pop_or_silence();
        let system = self.system.pop_or_silence();
        let mut samples = Vec::with_capacity(MIXER_FRAME_SAMPLES);

        for (mic_sample, system_sample) in mic.iter().zip(&system) {
            samples.push(
                (mic_sample * self.mic_gain + system_sample * self.system_gain).clamp(-1.0, 1.0),
            );
        }

        self.level_accumulator.add(&mic, &system);
        self.tick_count += 1;
        let levels = if self.tick_count % LEVEL_TICKS == 0 {
            Some(self.level_accumulator.take())
        } else {
            None
        };

        MixedFrame { samples, levels }
    }

    fn buffer(&self, source: MixerSource) -> &JitterBuffer {
        match source {
            MixerSource::Mic => &self.mic,
            MixerSource::System => &self.system,
        }
    }

    fn buffer_mut(&mut self, source: MixerSource) -> &mut JitterBuffer {
        match source {
            MixerSource::Mic => &mut self.mic,
            MixerSource::System => &mut self.system,
        }
    }
}

struct JitterBuffer {
    samples: VecDeque<f32>,
    max_samples: usize,
}

impl JitterBuffer {
    fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    fn push(&mut self, samples: &[f32]) {
        self.samples.extend(samples.iter().copied());
        while self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }
    }

    fn pop_or_silence(&mut self) -> [f32; MIXER_FRAME_SAMPLES] {
        let mut output = [0.0; MIXER_FRAME_SAMPLES];
        for sample in &mut output {
            if let Some(next) = self.samples.pop_front() {
                *sample = next;
            } else {
                break;
            }
        }
        output
    }

    fn len(&self) -> usize {
        self.samples.len()
    }
}

struct LevelAccumulator {
    mic_square_sum: f32,
    system_square_sum: f32,
    sample_count: usize,
}

impl LevelAccumulator {
    fn new() -> Self {
        Self {
            mic_square_sum: 0.0,
            system_square_sum: 0.0,
            sample_count: 0,
        }
    }

    fn add(&mut self, mic: &[f32], system: &[f32]) {
        self.mic_square_sum += mic.iter().map(|sample| sample * sample).sum::<f32>();
        self.system_square_sum += system.iter().map(|sample| sample * sample).sum::<f32>();
        self.sample_count += mic.len();
    }

    fn take(&mut self) -> SourceLevels {
        let levels = if self.sample_count == 0 {
            SourceLevels {
                mic: 0.0,
                system: 0.0,
            }
        } else {
            SourceLevels {
                mic: (self.mic_square_sum / self.sample_count as f32).sqrt(),
                system: (self.system_square_sum / self.sample_count as f32).sqrt(),
            }
        };
        *self = Self::new();
        levels
    }
}

pub struct DropMonitor {
    last_drop_count: u64,
}

impl DropMonitor {
    pub fn new() -> Self {
        Self { last_drop_count: 0 }
    }

    pub fn poll(&mut self, drop_count: &AtomicU64) -> Option<u64> {
        let current = drop_count.load(Ordering::Relaxed);
        if current > self.last_drop_count {
            self.last_drop_count = current;
            Some(current)
        } else {
            None
        }
    }
}

impl Default for DropMonitor {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MixerInput {
    source: MixerSource,
    rx: Receiver<AudioPacket>,
    pool: BufferPool,
    drop_count: Arc<AtomicU64>,
    resampler: AudioResampler,
    drop_monitor: DropMonitor,
}

impl MixerInput {
    pub fn new(
        source: MixerSource,
        rx: Receiver<AudioPacket>,
        pool: BufferPool,
        drop_count: Arc<AtomicU64>,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, AppError> {
        Ok(Self {
            source,
            rx,
            pool,
            drop_count,
            resampler: AudioResampler::new(sample_rate, channels)?,
            drop_monitor: DropMonitor::new(),
        })
    }

    pub fn drain_into(&mut self, mixer: &mut MixerCore) -> Result<Option<u64>, AppError> {
        while let Ok(packet) = self.rx.try_recv() {
            let samples = self
                .resampler
                .process_interleaved(&packet.buf[..packet.len]);
            self.pool.return_buffer(packet.buf);
            let samples = samples?;
            mixer.push(self.source, &samples);
        }
        Ok(self.drop_monitor.poll(&self.drop_count))
    }

    pub fn finish_into(&mut self, mixer: &mut MixerCore) -> Result<(), AppError> {
        let samples = self.resampler.finish()?;
        mixer.push(self.source, &samples);
        Ok(())
    }
}

pub trait MixerSink: Send + 'static {
    fn write_frame(&mut self, frame: &[f32]);
}

pub struct MixerThread {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MixerThread {
    pub fn spawn<S>(
        mut core: MixerCore,
        mut inputs: Vec<MixerInput>,
        mut sink: S,
        mut on_level: impl FnMut(SourceLevels) + Send + 'static,
        mut on_drops: impl FnMut(u64) + Send + 'static,
    ) -> Self
    where
        S: MixerSink,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_paused = Arc::clone(&paused);
        let handle = thread::spawn(move || {
            let tick_duration = Duration::from_millis(MIXER_TICK_MS);
            let mut next_tick = Instant::now();
            while !thread_stop.load(Ordering::Relaxed) {
                if thread_paused.load(Ordering::Relaxed) {
                    thread::sleep(tick_duration);
                    next_tick = Instant::now();
                    continue;
                }

                for input in &mut inputs {
                    if let Ok(Some(drop_count)) = input.drain_into(&mut core) {
                        on_drops(drop_count);
                    }
                }

                let frame = core.tick();
                sink.write_frame(&frame.samples);
                if let Some(levels) = frame.levels {
                    on_level(levels);
                }

                next_tick += tick_duration;
                let now = Instant::now();
                if next_tick > now {
                    thread::sleep(next_tick - now);
                } else {
                    next_tick = now;
                }
            }

            for input in &mut inputs {
                let _ = input.finish_into(&mut core);
            }
            while core.has_buffered_samples() {
                let frame = core.tick();
                sink.write_frame(&frame.samples);
                if let Some(levels) = frame.levels {
                    on_level(levels);
                }
            }
        });

        Self {
            stop,
            paused,
            handle: Some(handle),
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn output_sample_rate() -> u32 {
    OUTPUT_SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::buffer_pool::{send_samples, BufferPool};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[test]
    fn tick_mixes_sources_with_gain_clamp_and_silence_fill() {
        let mut mixer = MixerCore::new(0.7, 0.7);
        mixer.push(MixerSource::Mic, &[1.0; MIXER_FRAME_SAMPLES]);
        mixer.push(MixerSource::System, &[1.0; MIXER_FRAME_SAMPLES]);

        let mixed = mixer.tick();
        assert!(mixed.samples.iter().all(|sample| *sample == 1.0));

        let silent = mixer.tick();
        assert!(silent.samples.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn single_source_is_passthrough_with_gain() {
        let mut mixer = MixerCore::new(0.5, 0.7);
        mixer.push(MixerSource::Mic, &[0.5; MIXER_FRAME_SAMPLES]);

        let mixed = mixer.tick();

        assert!(mixed.samples.iter().all(|sample| *sample == 0.25));
    }

    #[test]
    fn jitter_buffer_keeps_only_latest_200ms() {
        let mut mixer = MixerCore::new(1.0, 1.0);
        let samples = vec![0.25; JITTER_MAX_SAMPLES + MIXER_FRAME_SAMPLES];

        mixer.push(MixerSource::Mic, &samples);

        assert_eq!(mixer.buffered_samples(MixerSource::Mic), JITTER_MAX_SAMPLES);
    }

    #[test]
    fn emits_source_rms_every_100ms() {
        let mut mixer = MixerCore::new(1.0, 1.0);

        for _ in 0..4 {
            mixer.push(MixerSource::Mic, &[0.5; MIXER_FRAME_SAMPLES]);
            assert!(mixer.tick().levels.is_none());
        }

        mixer.push(MixerSource::Mic, &[0.5; MIXER_FRAME_SAMPLES]);
        let levels = mixer.tick().levels.expect("fifth tick should emit levels");

        assert!((levels.mic - 0.5).abs() < 0.0001);
        assert_eq!(levels.system, 0.0);
    }

    #[test]
    fn drop_monitor_reports_only_increases() {
        let drop_count = AtomicU64::new(0);
        let mut monitor = DropMonitor::new();

        assert_eq!(monitor.poll(&drop_count), None);
        drop_count.store(12, Ordering::Relaxed);
        assert_eq!(monitor.poll(&drop_count), Some(12));
        assert_eq!(monitor.poll(&drop_count), None);
    }

    #[test]
    fn drains_packets_into_jitter_and_returns_buffers_to_pool() {
        let pool = BufferPool::with_capacity(1);
        let (tx, rx) = crossbeam_channel::bounded(1);
        let drop_count = Arc::new(AtomicU64::new(0));
        send_samples(&[0.5; MIXER_FRAME_SAMPLES], &tx, &pool, &drop_count);

        let mut source =
            MixerInput::new(MixerSource::Mic, rx, pool.clone(), drop_count, 16_000, 1).unwrap();
        let mut mixer = MixerCore::new(1.0, 1.0);

        source.drain_into(&mut mixer).unwrap();

        assert_eq!(
            mixer.buffered_samples(MixerSource::Mic),
            MIXER_FRAME_SAMPLES
        );
        assert_eq!(pool.available_for_test(), 1);
    }

    #[test]
    fn finish_flushes_resampler_remainder_into_jitter() {
        let pool = BufferPool::with_capacity(1);
        let (_tx, rx) = crossbeam_channel::bounded(1);
        let drop_count = Arc::new(AtomicU64::new(0));
        let mut source =
            MixerInput::new(MixerSource::Mic, rx, pool, drop_count, 48_000, 1).unwrap();
        let mut mixer = MixerCore::new(1.0, 1.0);

        source.resampler.process_interleaved(&[0.5; 500]).unwrap();
        source.finish_into(&mut mixer).unwrap();

        assert!(mixer.buffered_samples(MixerSource::Mic) > 0);
    }

    #[test]
    fn returns_packet_buffer_when_resampling_fails() {
        let pool = BufferPool::with_capacity(1);
        let (tx, rx) = crossbeam_channel::bounded(1);
        let drop_count = Arc::new(AtomicU64::new(0));
        send_samples(&[0.5; 3], &tx, &pool, &drop_count);
        let mut source =
            MixerInput::new(MixerSource::Mic, rx, pool.clone(), drop_count, 16_000, 2).unwrap();
        let mut mixer = MixerCore::new(1.0, 1.0);

        let error = source.drain_into(&mut mixer).unwrap_err();

        assert_eq!(error.code, crate::error::DECODE_FAILED);
        assert_eq!(pool.available_for_test(), 1);
    }
}
