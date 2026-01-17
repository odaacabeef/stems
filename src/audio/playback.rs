use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use anyhow::{bail, Result};
use hound::{WavReader, SampleFormat};
use crate::types::AtomicF32;

/// Represents a playback track for audio file playback
#[derive(Debug)]
pub struct PlaybackTrack {
    /// Audio samples loaded into memory (interleaved for stereo)
    pub samples: Vec<f32>,

    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,

    /// Sample rate of loaded audio (stored for validation, currently unused)
    #[allow(dead_code)]
    pub sample_rate: u32,

    /// Current playback position (frame index, not sample index)
    pub position: AtomicUsize,

    /// Whether this track is being monitored (heard in output)
    pub monitoring: AtomicBool,

    /// Whether this track is soloed
    pub solo: AtomicBool,

    /// Track level (0.0 - 1.0)
    pub level: AtomicF32,

    /// Track pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub pan: AtomicF32,

    /// Current peak level for metering (0.0 - 1.0)
    pub peak_level: AtomicF32,
}

impl PlaybackTrack {
    /// Load a WAV file from disk
    pub fn load_wav_file(filepath: &Path, target_sample_rate: u32) -> Result<Self> {
        let mut reader = WavReader::open(filepath)?;
        let spec = reader.spec();

        // Validate sample rate matches target
        if spec.sample_rate != target_sample_rate {
            bail!(
                "Sample rate mismatch: file '{}' is {}Hz, expected {}Hz",
                filepath.display(),
                spec.sample_rate,
                target_sample_rate
            );
        }

        // Validate channel count (mono or stereo only)
        if spec.channels != 1 && spec.channels != 2 {
            bail!(
                "Unsupported channel count: file '{}' has {} channels, expected 1 or 2",
                filepath.display(),
                spec.channels
            );
        }

        // Read all samples into memory
        let samples: Vec<f32> = match spec.sample_format {
            SampleFormat::Float => {
                // Read as float directly
                reader
                    .samples::<f32>()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| anyhow::anyhow!("Failed to read WAV samples: {}", e))?
            }
            SampleFormat::Int => {
                // Convert integer samples to float
                let bits = spec.bits_per_sample;
                if bits == 16 {
                    reader
                        .samples::<i16>()
                        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| anyhow::anyhow!("Failed to read WAV samples: {}", e))?
                } else if bits == 24 || bits == 32 {
                    reader
                        .samples::<i32>()
                        .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| anyhow::anyhow!("Failed to read WAV samples: {}", e))?
                } else {
                    bail!(
                        "Unsupported bit depth: file '{}' has {} bits per sample",
                        filepath.display(),
                        bits
                    );
                }
            }
        };

        Ok(Self {
            samples,
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            position: AtomicUsize::new(0),
            monitoring: AtomicBool::new(true), // Default to monitoring enabled
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            peak_level: AtomicF32::new(0.0),
        })
    }

    /// Get number of frames in the audio file
    pub fn num_frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// Get current playback position (frame index)
    pub fn get_position(&self) -> usize {
        self.position.load(Ordering::Relaxed)
    }

    /// Set playback position
    pub fn set_position(&self, pos: usize) {
        self.position.store(pos, Ordering::Relaxed);
    }

    /// Reset playback to beginning
    pub fn reset(&self) {
        self.position.store(0, Ordering::Relaxed);
    }

    /// Get monitoring status (audio-thread safe)
    pub fn is_monitoring(&self) -> bool {
        self.monitoring.load(Ordering::Relaxed)
    }

    /// Set monitoring status
    pub fn set_monitoring(&self, monitoring: bool) {
        self.monitoring.store(monitoring, Ordering::Relaxed);
    }

    /// Get solo status (audio-thread safe)
    pub fn is_solo(&self) -> bool {
        self.solo.load(Ordering::Relaxed)
    }

    /// Set solo status
    pub fn set_solo(&self, solo: bool) {
        self.solo.store(solo, Ordering::Relaxed);
    }

    /// Get level (audio-thread safe)
    pub fn get_level(&self) -> f32 {
        self.level.load(Ordering::Relaxed)
    }

    /// Set level (0.0 - 1.0)
    pub fn set_level(&self, level: f32) {
        let clamped = level.clamp(0.0, 1.0);
        self.level.store(clamped, Ordering::Relaxed);
    }

    /// Get pan (audio-thread safe)
    pub fn get_pan(&self) -> f32 {
        self.pan.load(Ordering::Relaxed)
    }

    /// Set pan (-1.0 to 1.0)
    pub fn set_pan(&self, pan: f32) {
        let clamped = pan.clamp(-1.0, 1.0);
        self.pan.store(clamped, Ordering::Relaxed);
    }

    /// Get peak level for metering (audio-thread safe)
    pub fn get_peak_level(&self) -> f32 {
        self.peak_level.load(Ordering::Relaxed)
    }

    /// Update peak level (called from audio thread)
    pub fn update_peak_level(&self, new_peak: f32) {
        self.peak_level.store(new_peak, Ordering::Relaxed);
    }

    /// Decay peak level (called from UI thread)
    pub fn decay_peak_level(&self, decay_rate: f32) {
        let current = self.get_peak_level();
        let new_peak = (current - decay_rate).max(0.0);
        self.peak_level.store(new_peak, Ordering::Relaxed);
    }

    /// Calculate stereo gain from pan position
    /// Returns (left_gain, right_gain)
    #[allow(dead_code)]
    pub fn calculate_pan_gains(&self) -> (f32, f32) {
        let pan = self.get_pan();

        // Equal power panning law
        let angle = (pan + 1.0) * std::f32::consts::PI / 4.0;
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        (left_gain, right_gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_track_num_frames() {
        let track = PlaybackTrack {
            samples: vec![0.0; 960], // 480 frames for stereo
            channels: 2,
            sample_rate: 48000,
            position: AtomicUsize::new(0),
            monitoring: AtomicBool::new(true),
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            peak_level: AtomicF32::new(0.0),
        };

        assert_eq!(track.num_frames(), 480);
    }

    #[test]
    fn test_level_clamping() {
        let track = PlaybackTrack {
            samples: vec![],
            channels: 1,
            sample_rate: 48000,
            position: AtomicUsize::new(0),
            monitoring: AtomicBool::new(true),
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            peak_level: AtomicF32::new(0.0),
        };

        track.set_level(1.5);
        assert_eq!(track.get_level(), 1.0);
        track.set_level(-0.5);
        assert_eq!(track.get_level(), 0.0);
    }

    #[test]
    fn test_pan_clamping() {
        let track = PlaybackTrack {
            samples: vec![],
            channels: 1,
            sample_rate: 48000,
            position: AtomicUsize::new(0),
            monitoring: AtomicBool::new(true),
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            peak_level: AtomicF32::new(0.0),
        };

        track.set_pan(2.0);
        assert_eq!(track.get_pan(), 1.0);
        track.set_pan(-2.0);
        assert_eq!(track.get_pan(), -1.0);
    }

    #[test]
    fn test_pan_gains() {
        let track = PlaybackTrack {
            samples: vec![],
            channels: 1,
            sample_rate: 48000,
            position: AtomicUsize::new(0),
            monitoring: AtomicBool::new(true),
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            peak_level: AtomicF32::new(0.0),
        };

        // Center pan
        track.set_pan(0.0);
        let (left, right) = track.calculate_pan_gains();
        assert!((left - 0.707).abs() < 0.01);
        assert!((right - 0.707).abs() < 0.01);

        // Full left
        track.set_pan(-1.0);
        let (left, right) = track.calculate_pan_gains();
        assert!((left - 1.0).abs() < 0.01);
        assert!(right.abs() < 0.01);

        // Full right
        track.set_pan(1.0);
        let (left, right) = track.calculate_pan_gains();
        assert!(left.abs() < 0.01);
        assert!((right - 1.0).abs() < 0.01);
    }
}
