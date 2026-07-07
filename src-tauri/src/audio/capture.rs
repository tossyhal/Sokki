use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use crossbeam_channel::Sender;
use serde::Serialize;

use crate::audio::buffer_pool::{
    send_converted_samples, AudioPacket, BufferPool, DEFAULT_BUFFER_COUNT,
};
use crate::error::{AppError, DEVICE_LOST, DEVICE_NOT_FOUND};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMeta {
    pub sample_rate: u32,
    pub channels: u16,
}

pub type CaptureErrorCallback = Box<dyn Fn(AppError) + Send + Sync + 'static>;

pub trait AudioCapture: Send {
    fn start(
        &mut self,
        tx: Sender<AudioPacket>,
        pool: BufferPool,
        drop_count: Arc<AtomicU64>,
        err_cb: CaptureErrorCallback,
    ) -> Result<StreamMeta, AppError>;

    fn stop(&mut self);
}

pub struct CpalMicCapture {
    device_name: Option<String>,
    stream: Option<Stream>,
}

pub struct CpalLoopbackCapture {
    device_name: Option<String>,
    stream: Option<Stream>,
}

// Sokki targets Windows only; cpal marks Stream as non-Send across all platforms.
unsafe impl Send for CpalMicCapture {}
unsafe impl Send for CpalLoopbackCapture {}

impl CpalMicCapture {
    pub fn new(device_name: Option<String>) -> Self {
        Self {
            device_name,
            stream: None,
        }
    }
}

impl CpalLoopbackCapture {
    pub fn new(device_name: Option<String>) -> Self {
        Self {
            device_name,
            stream: None,
        }
    }
}

impl AudioCapture for CpalMicCapture {
    fn start(
        &mut self,
        tx: Sender<AudioPacket>,
        pool: BufferPool,
        drop_count: Arc<AtomicU64>,
        err_cb: CaptureErrorCallback,
    ) -> Result<StreamMeta, AppError> {
        let host = cpal::default_host();
        let device = input_device(&host, self.device_name.as_deref())?;
        let supported_config = device.default_input_config().map_err(|err| {
            AppError::new(
                DEVICE_NOT_FOUND,
                format!("failed to read input device config: {err}"),
            )
        })?;
        let sample_format = supported_config.sample_format();
        let config = supported_config.config();
        let meta = StreamMeta {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
        };
        let stream = build_input_stream_for_format(
            &device,
            &config,
            sample_format,
            tx,
            pool,
            drop_count,
            err_cb,
            "input",
        )?;

        stream.play().map_err(|err| {
            AppError::new(
                DEVICE_LOST,
                format!("failed to start input device stream: {err}"),
            )
        })?;
        self.stream = Some(stream);
        Ok(meta)
    }

    fn stop(&mut self) {
        self.stream.take();
    }
}

impl AudioCapture for CpalLoopbackCapture {
    fn start(
        &mut self,
        tx: Sender<AudioPacket>,
        pool: BufferPool,
        drop_count: Arc<AtomicU64>,
        err_cb: CaptureErrorCallback,
    ) -> Result<StreamMeta, AppError> {
        let host = cpal::default_host();
        let device = output_device(&host, self.device_name.as_deref())?;
        let supported_config = device.default_output_config().map_err(|err| {
            AppError::new(
                DEVICE_NOT_FOUND,
                format!("failed to read output device config: {err}"),
            )
        })?;
        let sample_format = supported_config.sample_format();
        let config = supported_config.config();
        let meta = StreamMeta {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
        };
        let stream = build_input_stream_for_format(
            &device,
            &config,
            sample_format,
            tx,
            pool,
            drop_count,
            err_cb,
            "loopback",
        )?;

        stream.play().map_err(|err| {
            AppError::new(
                DEVICE_LOST,
                format!("failed to start loopback stream: {err}"),
            )
        })?;
        self.stream = Some(stream);
        Ok(meta)
    }

    fn stop(&mut self) {
        self.stream.take();
    }
}

#[allow(clippy::too_many_arguments)]
fn build_input_stream_for_format(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    tx: Sender<AudioPacket>,
    pool: BufferPool,
    drop_count: Arc<AtomicU64>,
    err_cb: CaptureErrorCallback,
    label: &'static str,
) -> Result<Stream, AppError> {
    match sample_format {
        SampleFormat::F32 => {
            build_input_stream::<f32>(device, config, tx, pool, drop_count, err_cb, label)
        }
        SampleFormat::I16 => {
            build_input_stream::<i16>(device, config, tx, pool, drop_count, err_cb, label)
        }
        SampleFormat::U16 => {
            build_input_stream::<u16>(device, config, tx, pool, drop_count, err_cb, label)
        }
        other => Err(AppError::new(
            DEVICE_NOT_FOUND,
            format!("unsupported {label} sample format: {other}"),
        )),
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender<AudioPacket>,
    pool: BufferPool,
    drop_count: Arc<AtomicU64>,
    err_cb: CaptureErrorCallback,
    label: &'static str,
) -> Result<Stream, AppError>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                send_converted_samples(data, &tx, &pool, &drop_count);
            },
            move |err| {
                err_cb(AppError::new(
                    DEVICE_LOST,
                    format!("{label} device error: {err}"),
                ));
            },
            None,
        )
        .map_err(|err| {
            AppError::new(
                DEVICE_LOST,
                format!("failed to build {label} stream: {err}"),
            )
        })
}

fn input_device(host: &cpal::Host, requested_name: Option<&str>) -> Result<cpal::Device, AppError> {
    if let Some(requested_name) = requested_name {
        let mut available = Vec::new();
        for device in host.input_devices().map_err(|err| {
            AppError::new(
                DEVICE_NOT_FOUND,
                format!("failed to list input devices: {err}"),
            )
        })? {
            if let Ok(name) = device.name() {
                if name == requested_name {
                    return Ok(device);
                }
                available.push(name);
            }
        }
        return Err(device_not_found(requested_name, &available));
    }

    host.default_input_device()
        .ok_or_else(|| AppError::new(DEVICE_NOT_FOUND, "default input device not found"))
}

fn output_device(
    host: &cpal::Host,
    requested_name: Option<&str>,
) -> Result<cpal::Device, AppError> {
    if let Some(requested_name) = requested_name {
        let mut available = Vec::new();
        for device in host.output_devices().map_err(|err| {
            AppError::new(
                DEVICE_NOT_FOUND,
                format!("failed to list output devices: {err}"),
            )
        })? {
            if let Ok(name) = device.name() {
                if name == requested_name {
                    return Ok(device);
                }
                available.push(name);
            }
        }
        return Err(output_device_not_found(requested_name, &available));
    }

    host.default_output_device()
        .ok_or_else(|| AppError::new(DEVICE_NOT_FOUND, "default output device not found"))
}

fn device_not_found(requested_name: &str, available: &[String]) -> AppError {
    let candidates = if available.is_empty() {
        "none".to_string()
    } else {
        available.join(", ")
    };
    AppError::new(
        DEVICE_NOT_FOUND,
        format!("input device '{requested_name}' not found. Available input devices: {candidates}"),
    )
}

fn output_device_not_found(requested_name: &str, available: &[String]) -> AppError {
    let candidates = if available.is_empty() {
        "none".to_string()
    } else {
        available.join(", ")
    };
    AppError::new(
        DEVICE_NOT_FOUND,
        format!(
            "output device '{requested_name}' not found. Available output devices: {candidates}"
        ),
    )
}

pub fn default_capture_pool() -> BufferPool {
    BufferPool::with_capacity(DEFAULT_BUFFER_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_meta_serializes_with_camel_case_fields() {
        let meta = StreamMeta {
            sample_rate: 48_000,
            channels: 2,
        };

        assert_eq!(
            serde_json::to_value(meta).expect("stream meta should serialize"),
            json!({ "sampleRate": 48000, "channels": 2 })
        );
    }

    #[test]
    fn device_not_found_error_lists_requested_name_and_candidates() {
        let error = device_not_found(
            "Missing Mic",
            &["Built-in Mic".to_string(), "USB Mic".to_string()],
        );

        assert_eq!(error.code, DEVICE_NOT_FOUND);
        assert!(error.message.contains("Missing Mic"));
        assert!(error.message.contains("Built-in Mic, USB Mic"));
    }

    #[test]
    fn output_device_not_found_error_lists_requested_name_and_candidates() {
        let error = output_device_not_found(
            "Missing Speakers",
            &["Speakers".to_string(), "HDMI Output".to_string()],
        );

        assert_eq!(error.code, DEVICE_NOT_FOUND);
        assert!(error.message.contains("Missing Speakers"));
        assert!(error.message.contains("Speakers, HDMI Output"));
    }
}
