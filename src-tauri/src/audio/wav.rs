use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{AppError, IO_ERROR};

pub const WAV_SAMPLE_RATE: u32 = 16_000;
pub const WAV_CHANNELS: u16 = 1;
pub const WAV_BITS_PER_SAMPLE: u16 = 16;
const WAV_BYTES_PER_SAMPLE: u64 = 2;

pub struct StreamingWavWriter {
    writer: hound::WavWriter<BufWriter<File>>,
    samples_written: u64,
}

impl StreamingWavWriter {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, AppError> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }

        let spec = hound::WavSpec {
            channels: WAV_CHANNELS,
            sample_rate: WAV_SAMPLE_RATE,
            bits_per_sample: WAV_BITS_PER_SAMPLE,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec).map_err(wav_error)?;
        Ok(Self {
            writer,
            samples_written: 0,
        })
    }

    pub fn write_frame(&mut self, samples: &[f32]) -> Result<(), AppError> {
        for sample in samples {
            self.writer
                .write_sample(f32_to_i16(*sample))
                .map_err(wav_error)?;
            self.samples_written += 1;
        }
        Ok(())
    }

    pub fn finalize(self) -> Result<u64, AppError> {
        let duration_ms = samples_to_duration_ms(self.samples_written);
        self.writer.finalize().map_err(wav_error)?;
        Ok(duration_ms)
    }
}

pub fn repair_wav(path: impl AsRef<Path>) -> Result<u64, AppError> {
    let path = path.as_ref();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(io_error)?;
    let file_size = file.metadata().map_err(io_error)?.len();
    if file_size < 44 {
        return Err(AppError::new(IO_ERROR, "invalid wav header"));
    }

    let mut header = [0u8; 12];
    file.read_exact(&mut header).map_err(io_error)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(AppError::new(IO_ERROR, "invalid wav header"));
    }

    if file_size > u32::MAX as u64 + 8 {
        return Err(AppError::new(IO_ERROR, "wav file is too large to repair"));
    }

    let data_size_offset = find_data_size_offset(&mut file, file_size)?;
    let data_start = data_size_offset + 4;
    let data_size = file_size
        .checked_sub(data_start)
        .ok_or_else(|| AppError::new(IO_ERROR, "invalid wav data chunk"))?;
    if data_size > u32::MAX as u64 {
        return Err(AppError::new(
            IO_ERROR,
            "wav data chunk is too large to repair",
        ));
    }

    file.seek(SeekFrom::Start(4)).map_err(io_error)?;
    file.write_all(&((file_size - 8) as u32).to_le_bytes())
        .map_err(io_error)?;
    file.seek(SeekFrom::Start(data_size_offset))
        .map_err(io_error)?;
    file.write_all(&(data_size as u32).to_le_bytes())
        .map_err(io_error)?;
    file.flush().map_err(io_error)?;

    Ok(samples_to_duration_ms(data_size / WAV_BYTES_PER_SAMPLE))
}

fn find_data_size_offset(file: &mut File, file_size: u64) -> Result<u64, AppError> {
    let mut offset = 12u64;
    while offset + 8 <= file_size {
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        let mut chunk_header = [0u8; 8];
        file.read_exact(&mut chunk_header).map_err(io_error)?;
        let chunk_size = u32::from_le_bytes(
            chunk_header[4..8]
                .try_into()
                .expect("slice length is checked"),
        ) as u64;
        if &chunk_header[0..4] == b"data" {
            return Ok(offset + 4);
        }
        offset = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(chunk_size))
            .and_then(|value| value.checked_add(chunk_size % 2))
            .ok_or_else(|| AppError::new(IO_ERROR, "invalid wav chunk size"))?;
    }

    Err(AppError::new(IO_ERROR, "wav data chunk not found"))
}

fn samples_to_duration_ms(samples: u64) -> u64 {
    samples * 1_000 / WAV_SAMPLE_RATE as u64
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    if sample < 0.0 {
        (sample * 32_768.0) as i16
    } else {
        (sample * 32_767.0) as i16
    }
}

fn wav_error(error: hound::Error) -> AppError {
    AppError::new(IO_ERROR, format!("wav error: {error}"))
}

fn io_error(error: std::io::Error) -> AppError {
    AppError::new(IO_ERROR, format!("io error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn writes_16khz_mono_16bit_pcm_wav() {
        let path = temp_wav_path("writes_16khz_mono_16bit_pcm_wav.wav");
        let mut writer = StreamingWavWriter::create(&path).unwrap();

        writer.write_frame(&[-1.2, -1.0, 0.0, 1.0, 1.2]).unwrap();
        let duration_ms = writer.finalize().unwrap();

        assert_eq!(duration_ms, 0);
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, WAV_SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, 16);

        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, vec![i16::MIN, i16::MIN, 0, i16::MAX, i16::MAX]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn repairs_interrupted_wav_header_and_returns_duration() {
        let path = temp_wav_path("repairs_interrupted_wav_header_and_returns_duration.wav");
        write_interrupted_wav(&path, &[0, i16::MAX, i16::MIN]);

        let duration_ms = repair_wav(&path).unwrap();

        assert_eq!(duration_ms, 0);
        let mut reader = hound::WavReader::open(&path).unwrap();
        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, vec![0, i16::MAX, i16::MIN]);
        let _ = fs::remove_file(path);
    }

    fn temp_wav_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sokki-{name}"))
    }

    fn write_interrupted_wav(path: &Path, samples: &[i16]) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&WAV_SAMPLE_RATE.to_le_bytes()).unwrap();
        file.write_all(&(WAV_SAMPLE_RATE * 2).to_le_bytes())
            .unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        for sample in samples {
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
}
