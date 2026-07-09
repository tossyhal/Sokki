use serde::Serialize;
use thiserror::Error;

pub const MODEL_NOT_FOUND: &str = "MODEL_NOT_FOUND";
pub const MODEL_CORRUPTED: &str = "MODEL_CORRUPTED";
pub const MODEL_UNVERIFIED: &str = "MODEL_UNVERIFIED";
pub const MODEL_ALREADY_DOWNLOADING: &str = "MODEL_ALREADY_DOWNLOADING";
pub const DOWNLOAD_FAILED: &str = "DOWNLOAD_FAILED";
pub const VERIFY_FAILED: &str = "VERIFY_FAILED";
pub const DEVICE_NOT_FOUND: &str = "DEVICE_NOT_FOUND";
pub const DEVICE_LOST: &str = "DEVICE_LOST";
pub const ALREADY_RECORDING: &str = "ALREADY_RECORDING";
pub const NOT_RECORDING: &str = "NOT_RECORDING";
pub const SOUND_CHECK_BUSY: &str = "SOUND_CHECK_BUSY";
pub const DECODE_FAILED: &str = "DECODE_FAILED";
pub const FILE_TOO_LARGE: &str = "FILE_TOO_LARGE";
pub const AUDIO_TOO_LONG: &str = "AUDIO_TOO_LONG";
pub const DURATION_UNKNOWN: &str = "DURATION_UNKNOWN";
pub const DISK_FULL: &str = "DISK_FULL";
pub const WHISPER_GPU_UNAVAILABLE: &str = "WHISPER_GPU_UNAVAILABLE";
pub const WHISPER_ERROR: &str = "WHISPER_ERROR";
pub const IO_ERROR: &str = "IO_ERROR";
pub const DB_ERROR: &str = "DB_ERROR";
pub const CANCELED: &str = "CANCELED";

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
#[error("{code}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_to_command_error_shape() {
        let error = AppError::new(MODEL_NOT_FOUND, "model is not downloaded");

        let value = serde_json::to_value(error).expect("AppError should serialize");

        assert_eq!(
            value,
            json!({
                "code": "MODEL_NOT_FOUND",
                "message": "model is not downloaded",
            })
        );
    }
}
