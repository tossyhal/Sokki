use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crossbeam_channel::bounded;

use crate::audio::buffer_pool::{BufferPool, DEFAULT_BUFFER_COUNT};
use crate::audio::capture::{AudioCapture, CpalLoopbackCapture, CpalMicCapture};
use crate::audio::mixer::{
    MixerCore, MixerInput, MixerSink, MixerSource, MixerThread, SourceLevels, SourcePeaks,
};
use crate::db::Source;
use crate::error::{AppError, IO_ERROR};

/// Handler invoked from capture/mixer threads on fatal stream errors
/// (DEVICE_LOST, WAV write failures). Implementations must not block:
/// they run on audio-adjacent threads and typically spawn a thread that
/// performs the auto-stop.
pub type SharedCaptureErrorHandler = Arc<dyn Fn(AppError) + Send + Sync + 'static>;

pub struct CapturePipelineConfig {
    pub source: Source,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub mic_gain: f32,
    pub system_gain: f32,
}

/// Owns the cpal capture streams and the mixer thread for one recording or
/// sound-check session. Audio flows: cpal callback → buffer pool packets →
/// per-source resampler/jitter buffer → 20ms mixer ticks → `MixerSink`.
pub struct CapturePipeline {
    captures: Vec<Box<dyn AudioCapture>>,
    mixer: Option<MixerThread>,
}

impl CapturePipeline {
    pub fn start<S>(
        config: CapturePipelineConfig,
        drop_count: Arc<AtomicU64>,
        sink: S,
        on_level: impl FnMut(SourceLevels, SourcePeaks) + Send + 'static,
        on_error: SharedCaptureErrorHandler,
    ) -> Result<Self, AppError>
    where
        S: MixerSink,
    {
        let mut captures: Vec<Box<dyn AudioCapture>> = Vec::new();
        let mut inputs: Vec<MixerInput> = Vec::new();

        let result = (|| {
            if matches!(config.source, Source::Mic | Source::Mix) {
                let capture = Box::new(CpalMicCapture::new(config.mic_device.clone()));
                start_capture_source(
                    capture,
                    MixerSource::Mic,
                    &drop_count,
                    &on_error,
                    &mut captures,
                    &mut inputs,
                )?;
            }
            if matches!(config.source, Source::System | Source::Mix) {
                let capture = Box::new(CpalLoopbackCapture::new(config.loopback_device.clone()));
                start_capture_source(
                    capture,
                    MixerSource::System,
                    &drop_count,
                    &on_error,
                    &mut captures,
                    &mut inputs,
                )?;
            }
            if captures.is_empty() {
                return Err(AppError::new(
                    IO_ERROR,
                    "capture pipeline requires a mic, system, or mix source",
                ));
            }
            Ok(())
        })();

        if let Err(error) = result {
            for capture in &mut captures {
                capture.stop();
            }
            return Err(error);
        }

        let mixer = MixerThread::spawn(
            MixerCore::new(config.mic_gain, config.system_gain),
            inputs,
            sink,
            on_level,
            |_| {},
        );

        Ok(Self {
            captures,
            mixer: Some(mixer),
        })
    }

    pub fn pause(&self) {
        if let Some(mixer) = &self.mixer {
            mixer.pause();
        }
    }

    pub fn resume(&self) {
        if let Some(mixer) = &self.mixer {
            mixer.resume();
        }
    }

    /// Stops the capture streams first, then joins the mixer thread, which
    /// drains and flushes any buffered audio into the sink before exiting.
    pub fn stop(mut self) {
        for capture in &mut self.captures {
            capture.stop();
        }
        if let Some(mixer) = self.mixer.take() {
            mixer.stop();
        }
    }
}

fn start_capture_source(
    mut capture: Box<dyn AudioCapture>,
    source: MixerSource,
    drop_count: &Arc<AtomicU64>,
    on_error: &SharedCaptureErrorHandler,
    captures: &mut Vec<Box<dyn AudioCapture>>,
    inputs: &mut Vec<MixerInput>,
) -> Result<(), AppError> {
    let pool = BufferPool::with_capacity(DEFAULT_BUFFER_COUNT);
    let (tx, rx) = bounded(DEFAULT_BUFFER_COUNT);
    let error_handler = Arc::clone(on_error);
    let meta = capture.start(
        tx,
        pool.clone(),
        Arc::clone(drop_count),
        Box::new(move |error| error_handler(error)),
    )?;
    captures.push(capture);
    inputs.push(MixerInput::new(
        source,
        rx,
        pool,
        Arc::clone(drop_count),
        meta.sample_rate,
        meta.channels,
    )?);
    Ok(())
}
