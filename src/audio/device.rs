use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, Host, SupportedStreamConfig};

/// Audio device information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub max_input_channels: usize,
    pub sample_rate: u32,
}

/// Get the default audio host
pub fn get_host() -> Host {
    cpal::default_host()
}

/// Get the default input device
pub fn get_default_input_device() -> Result<Device> {
    let host = get_host();
    host.default_input_device()
        .context("No default input device available")
}

/// List all available input devices
#[allow(dead_code)]
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = get_host();
    let default_device = host.default_input_device();
    let default_name = default_device
        .as_ref()
        .and_then(|d| d.description().ok())
        .map(|desc| desc.name().to_string());

    let mut devices = Vec::new();

    for device in host.input_devices()? {
        let desc = device.description().ok();
        if desc.is_none() {
            continue;
        }
        let name = desc.unwrap().name().to_string();
        let is_default = Some(&name) == default_name.as_ref();

        // Get supported config
        let config = device.default_input_config().ok();
        let (max_channels, sample_rate) = if let Some(cfg) = config {
            (cfg.channels() as usize, cfg.sample_rate())
        } else {
            (0, 0)
        };

        devices.push(AudioDeviceInfo {
            name,
            is_default,
            max_input_channels: max_channels,
            sample_rate,
        });
    }

    Ok(devices)
}

/// Get the default input configuration for a device
pub fn get_default_input_config(device: &Device) -> Result<SupportedStreamConfig> {
    device
        .default_input_config()
        .context("Failed to get default input config")
}

/// Get device by name
#[allow(dead_code)]
pub fn get_device_by_name(name: &str) -> Result<Device> {
    let host = get_host();

    for device in host.input_devices()? {
        if let Ok(desc) = device.description() {
            if desc.name() == name {
                return Ok(device);
            }
        }
    }

    anyhow::bail!("Device '{}' not found", name)
}

/// Get device by index
pub fn get_device_by_index(index: usize) -> Result<Device> {
    let host = get_host();
    let devices: Vec<Device> = host.input_devices()?.collect();

    if index >= devices.len() {
        anyhow::bail!("Device index {} out of range (found {} devices)", index, devices.len());
    }

    Ok(devices[index].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_devices() {
        // This test may fail on systems without audio devices
        if let Ok(devices) = list_input_devices() {
            println!("Found {} input devices", devices.len());
            for device in devices {
                println!("  - {} ({}ch @ {}Hz) {}",
                    device.name,
                    device.max_input_channels,
                    device.sample_rate,
                    if device.is_default { "[DEFAULT]" } else { "" }
                );
            }
        }
    }

    #[test]
    fn test_get_default_device() {
        // This test may fail on systems without audio devices
        if let Ok(device) = get_default_input_device() {
            if let Ok(desc) = device.description() {
                println!("Default input device: {:?}", desc.name());
            }
        }
    }
}
