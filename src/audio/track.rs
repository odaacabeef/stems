use std::sync::atomic::{AtomicBool, Ordering};
use crate::types::AtomicF32;

/// Represents a single audio track with real-time safe state
#[derive(Debug)]
pub struct Track {
    /// Track ID (0-based)
    pub id: usize,

    /// Track name (reserved for future display)
    #[allow(dead_code)]
    pub name: String,

    /// Whether this track is armed for recording
    pub armed: AtomicBool,

    /// Whether this track is being monitored (heard in output)
    pub monitoring: AtomicBool,

    /// Whether this track is soloed
    pub solo: AtomicBool,

    /// Track level (0.0 - 1.0)
    pub level: AtomicF32,

    /// Track pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub pan: AtomicF32,

    /// Input channel index that feeds this track
    pub input_channel: usize,

    /// Current peak level for metering (0.0 - 1.0)
    pub peak_level: AtomicF32,

    /// Whether this track is currently recording
    pub recording: AtomicBool,
}

impl Track {
    /// Create a new track
    pub fn new(id: usize, input_channel: usize) -> Self {
        Self {
            id,
            name: format!("Track {}", id + 1),
            armed: AtomicBool::new(false),
            monitoring: AtomicBool::new(false), // Monitoring disabled by default
            solo: AtomicBool::new(false),
            level: AtomicF32::new(1.0),
            pan: AtomicF32::new(0.0),
            input_channel,
            peak_level: AtomicF32::new(0.0),
            recording: AtomicBool::new(false),
        }
    }

    /// Get armed status (audio-thread safe)
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    /// Set armed status
    pub fn set_armed(&self, armed: bool) {
        self.armed.store(armed, Ordering::Relaxed);
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

    /// Get recording status (audio-thread safe)
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    /// Set recording status
    pub fn set_recording(&self, recording: bool) {
        self.recording.store(recording, Ordering::Relaxed);
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

impl Clone for Track {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            armed: AtomicBool::new(self.armed.load(Ordering::Relaxed)),
            monitoring: AtomicBool::new(self.monitoring.load(Ordering::Relaxed)),
            solo: AtomicBool::new(self.solo.load(Ordering::Relaxed)),
            level: AtomicF32::new(self.level.load(Ordering::Relaxed)),
            pan: AtomicF32::new(self.pan.load(Ordering::Relaxed)),
            input_channel: self.input_channel,
            peak_level: AtomicF32::new(self.peak_level.load(Ordering::Relaxed)),
            recording: AtomicBool::new(self.recording.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_creation() {
        let track = Track::new(0, 0);
        assert_eq!(track.id, 0);
        assert_eq!(track.name, "Track 1");
        assert!(!track.is_armed());
        assert_eq!(track.get_level(), 1.0);
        assert_eq!(track.get_pan(), 0.0);
    }

    #[test]
    fn test_level_clamping() {
        let track = Track::new(0, 0);
        track.set_level(1.5);
        assert_eq!(track.get_level(), 1.0);
        track.set_level(-0.5);
        assert_eq!(track.get_level(), 0.0);
    }

    #[test]
    fn test_pan_clamping() {
        let track = Track::new(0, 0);
        track.set_pan(2.0);
        assert_eq!(track.get_pan(), 1.0);
        track.set_pan(-2.0);
        assert_eq!(track.get_pan(), -1.0);
    }

    #[test]
    fn test_pan_gains() {
        let track = Track::new(0, 0);

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
