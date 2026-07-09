use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

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
    stream: Option<CaptureStreamHandle>,
}

pub struct CpalLoopbackCapture {
    device_name: Option<String>,
    stream: Option<CaptureStreamHandle>,
}

struct CaptureStreamHandle {
    stop_tx: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl CaptureStreamHandle {
    fn stop(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

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
        let (handle, meta) = spawn_capture_stream(
            CaptureDeviceKind::Input,
            self.device_name.clone(),
            tx,
            pool,
            drop_count,
            err_cb,
        )?;
        self.stream = Some(handle);
        Ok(meta)
    }

    fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            stream.stop();
        }
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
        let (handle, meta) = spawn_capture_stream(
            CaptureDeviceKind::Loopback,
            self.device_name.clone(),
            tx,
            pool,
            drop_count,
            err_cb,
        )?;
        self.stream = Some(handle);
        Ok(meta)
    }

    fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            stream.stop();
        }
    }
}

#[derive(Clone, Copy)]
enum CaptureDeviceKind {
    Input,
    Loopback,
}

fn spawn_capture_stream(
    kind: CaptureDeviceKind,
    device_name: Option<String>,
    tx: Sender<AudioPacket>,
    pool: BufferPool,
    drop_count: Arc<AtomicU64>,
    err_cb: CaptureErrorCallback,
) -> Result<(CaptureStreamHandle, StreamMeta), AppError> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let thread = thread::spawn(move || {
        let result =
            start_stream_on_current_thread(kind, device_name, tx, pool, drop_count, err_cb);
        match result {
            Ok((stream, meta)) => {
                if ready_tx.send(Ok(meta)).is_ok() {
                    let _stream = stream;
                    let _ = stop_rx.recv();
                }
            }
            Err(error) => {
                let _ = ready_tx.send(Err(error));
            }
        }
    });

    match ready_rx.recv() {
        Ok(Ok(meta)) => Ok((
            CaptureStreamHandle {
                stop_tx: Some(stop_tx),
                thread: Some(thread),
            },
            meta,
        )),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = thread.join();
            Err(AppError::new(
                DEVICE_LOST,
                format!("capture stream thread failed before startup: {error}"),
            ))
        }
    }
}

fn start_stream_on_current_thread(
    kind: CaptureDeviceKind,
    device_name: Option<String>,
    tx: Sender<AudioPacket>,
    pool: BufferPool,
    drop_count: Arc<AtomicU64>,
    err_cb: CaptureErrorCallback,
) -> Result<(Stream, StreamMeta), AppError> {
    let host = cpal::default_host();
    let (device, sample_format, config, label) = match kind {
        CaptureDeviceKind::Input => {
            let device = input_device(&host, device_name.as_deref())?;
            let supported_config = device.default_input_config().map_err(|err| {
                AppError::new(
                    DEVICE_NOT_FOUND,
                    format!("failed to read input device config: {err}"),
                )
            })?;
            (
                device,
                supported_config.sample_format(),
                supported_config.config(),
                "input",
            )
        }
        CaptureDeviceKind::Loopback => {
            let device = output_device(&host, device_name.as_deref())?;
            let supported_config = device.default_output_config().map_err(|err| {
                AppError::new(
                    DEVICE_NOT_FOUND,
                    format!("failed to read output device config: {err}"),
                )
            })?;
            (
                device,
                supported_config.sample_format(),
                supported_config.config(),
                "loopback",
            )
        }
    };
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
        label,
    )?;

    stream.play().map_err(|err| {
        AppError::new(
            DEVICE_LOST,
            format!("failed to start {label} device stream: {err}"),
        )
    })?;
    Ok((stream, meta))
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
