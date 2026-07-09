use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use crate::error::{AppError, IO_ERROR};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevices {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

pub fn list_audio_devices() -> Result<AudioDevices, AppError> {
    let host = cpal::default_host();
    let default_input_name = host.default_input_device().and_then(device_name);
    let default_output_name = host.default_output_device().and_then(device_name);
    let input_names = device_names(host.input_devices().map_err(audio_error)?);
    let output_names = device_names(host.output_devices().map_err(audio_error)?);

    Ok(devices_from_names(
        input_names,
        output_names,
        default_input_name,
        default_output_name,
    ))
}

fn device_names(devices: impl Iterator<Item = cpal::Device>) -> Vec<String> {
    devices.filter_map(device_name).collect()
}

fn device_name(device: cpal::Device) -> Option<String> {
    device.name().ok().filter(|name| !name.trim().is_empty())
}

fn devices_from_names(
    input_names: Vec<String>,
    output_names: Vec<String>,
    default_input_name: Option<String>,
    default_output_name: Option<String>,
) -> AudioDevices {
    AudioDevices {
        inputs: input_names
            .into_iter()
            .map(|name| audio_device(name, default_input_name.as_deref()))
            .collect(),
        outputs: output_names
            .into_iter()
            .map(|name| audio_device(name, default_output_name.as_deref()))
            .collect(),
    }
}

fn audio_device(name: String, default_name: Option<&str>) -> AudioDevice {
    AudioDevice {
        id: name.clone(),
        is_default: default_name == Some(name.as_str()),
        name,
    }
}

fn audio_error(error: cpal::DevicesError) -> AppError {
    AppError::new(IO_ERROR, format!("failed to list audio devices: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn marks_matching_default_devices_and_serializes_as_camel_case() {
        let devices = devices_from_names(
            vec!["Built-in Microphone".to_string(), "USB Mic".to_string()],
            vec!["Speakers".to_string(), "HDMI Output".to_string()],
            Some("USB Mic".to_string()),
            Some("Speakers".to_string()),
        );

        let value = serde_json::to_value(devices).expect("devices should serialize");

        assert_eq!(
            value,
            json!({
                "inputs": [
                    { "id": "Built-in Microphone", "name": "Built-in Microphone", "isDefault": false },
                    { "id": "USB Mic", "name": "USB Mic", "isDefault": true }
                ],
                "outputs": [
                    { "id": "Speakers", "name": "Speakers", "isDefault": true },
                    { "id": "HDMI Output", "name": "HDMI Output", "isDefault": false }
                ]
            })
        );
    }
}
