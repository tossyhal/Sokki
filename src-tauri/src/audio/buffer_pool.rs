use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError};

pub const CHUNK: usize = 4096;
pub const DEFAULT_BUFFER_COUNT: usize = 64;

#[derive(Clone)]
pub struct BufferPool {
    free_rx: Receiver<Box<[f32; CHUNK]>>,
    free_tx: Sender<Box<[f32; CHUNK]>>,
}

pub struct AudioPacket {
    pub buf: Box<[f32; CHUNK]>,
    pub len: usize,
}

impl BufferPool {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUFFER_COUNT)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (free_tx, free_rx) = crossbeam_channel::bounded(capacity);
        for _ in 0..capacity {
            free_tx
                .send(Box::new([0.0; CHUNK]))
                .expect("new buffer pool channel should accept preallocated buffers");
        }
        Self { free_rx, free_tx }
    }

    pub fn return_buffer(&self, buf: Box<[f32; CHUNK]>) {
        let _ = self.free_tx.try_send(buf);
    }

    #[cfg(test)]
    fn available(&self) -> usize {
        self.free_rx.len()
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

pub fn send_samples(
    samples: &[f32],
    tx: &Sender<AudioPacket>,
    pool: &BufferPool,
    drop_count: &AtomicU64,
) {
    let mut offset = 0;
    while offset < samples.len() {
        let Ok(mut buf) = pool.free_rx.try_recv() else {
            drop_count.fetch_add((samples.len() - offset) as u64, Ordering::Relaxed);
            return;
        };

        let len = (samples.len() - offset).min(CHUNK);
        buf[..len].copy_from_slice(&samples[offset..offset + len]);
        offset += len;

        let packet = AudioPacket { buf, len };
        match tx.try_send(packet) {
            Ok(()) => {}
            Err(TrySendError::Full(packet)) | Err(TrySendError::Disconnected(packet)) => {
                drop_count.fetch_add(packet.len as u64, Ordering::Relaxed);
                pool.return_buffer(packet.buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn drops_remaining_frames_when_pool_is_exhausted() {
        let pool = BufferPool::with_capacity(1);
        let (tx, rx) = crossbeam_channel::bounded(2);
        let drop_count = AtomicU64::new(0);
        let samples = vec![0.25; CHUNK + 5];

        send_samples(&samples, &tx, &pool, &drop_count);

        let packet = rx.try_recv().expect("first packet should be sent");
        assert_eq!(packet.len, CHUNK);
        assert_eq!(packet.buf[0], 0.25);
        assert_eq!(packet.buf[CHUNK - 1], 0.25);
        assert!(rx.try_recv().is_err());
        assert_eq!(drop_count.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn counts_dropped_frames_and_returns_buffer_when_packet_channel_is_full() {
        let pool = BufferPool::with_capacity(1);
        let (tx, rx) = crossbeam_channel::bounded(0);
        let drop_count = AtomicU64::new(0);
        let samples = vec![0.5; 8];

        send_samples(&samples, &tx, &pool, &drop_count);

        assert!(rx.try_recv().is_err());
        assert_eq!(drop_count.load(Ordering::Relaxed), 8);
        assert_eq!(pool.available(), 1);
    }
}
