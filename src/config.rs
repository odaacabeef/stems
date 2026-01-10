use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Top-level configuration structure
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub devices: DeviceConfig,

    #[serde(default)]
    pub tracks: HashMap<usize, TrackConfig>,
}

/// Device configuration
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct DeviceConfig {
    pub audio: Option<String>,
    pub monitorch: Option<String>,
    pub midiin: Option<String>,
}

/// Per-track configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct TrackConfig {
    #[serde(default)]
    pub arm: Option<bool>,

    #[serde(default)]
    pub monitor: Option<bool>,

    #[serde(default)]
    pub solo: Option<bool>,

    #[serde(default)]
    pub level: Option<f32>,

    #[serde(default)]
    pub pan: Option<f32>,
}

impl Config {
    /// Load configuration from YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML in: {}", path.display()))?;

        config.validate()?;
        Ok(config)
    }

    /// Validate configuration values
    fn validate(&self) -> Result<()> {
        // Validate monitor channels format if present
        if let Some(ref monitorch) = self.devices.monitorch {
            validate_monitor_channels(monitorch)?;
        }

        // Validate track configurations
        for (track_num, track_config) in &self.tracks {
            if *track_num < 1 {
                anyhow::bail!("Track number must be >= 1, got {}", track_num);
            }

            if let Some(level) = track_config.level {
                if !(0.0..=1.0).contains(&level) {
                    anyhow::bail!(
                        "Track {} level must be between 0.0 and 1.0, got {}",
                        track_num,
                        level
                    );
                }
            }

            if let Some(pan) = track_config.pan {
                if !(-1.0..=1.0).contains(&pan) {
                    anyhow::bail!(
                        "Track {} pan must be between -1.0 and 1.0, got {}",
                        track_num,
                        pan
                    );
                }
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            devices: DeviceConfig::default(),
            tracks: HashMap::new(),
        }
    }
}

/// Validate monitor channels format (START-END)
pub fn validate_monitor_channels(channels_str: &str) -> Result<(u16, u16)> {
    let parts: Vec<&str> = channels_str.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!(
            "Invalid monitor channels format '{}'. Expected format: START-END (e.g., '17-18')",
            channels_str
        );
    }

    let start = parts[0]
        .parse::<u16>()
        .with_context(|| format!("Invalid start channel '{}'", parts[0]))?;
    let end = parts[1]
        .parse::<u16>()
        .with_context(|| format!("Invalid end channel '{}'", parts[1]))?;

    if start < 1 {
        anyhow::bail!("Start channel must be >= 1, got {}", start);
    }

    if end < start {
        anyhow::bail!("End channel {} must be >= start channel {}", end, start);
    }

    if end - start + 1 != 2 {
        anyhow::bail!(
            "Monitor channels must be exactly 2 channels (stereo), got {} channels",
            end - start + 1
        );
    }

    Ok((start, end))
}
