use std::sync::atomic::{AtomicU32, Ordering};

/// Default sample rate (Hz)
pub const SAMPLE_RATE: u32 = 48000;

/// Default buffer size (frames) - reserved for future use
#[allow(dead_code)]
pub const BUFFER_SIZE: u32 = 512;

/// Ring buffer size in seconds
pub const RING_BUFFER_SECONDS: usize = 5;

/// Atomic float wrapper for real-time audio thread safety
#[derive(Debug)]
pub struct AtomicF32 {
    storage: AtomicU32,
}

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self {
            storage: AtomicU32::new(value.to_bits()),
        }
    }

    pub fn load(&self, ordering: Ordering) -> f32 {
        f32::from_bits(self.storage.load(ordering))
    }

    pub fn store(&self, value: f32, ordering: Ordering) {
        self.storage.store(value.to_bits(), ordering);
    }
}

/// Commands sent between threads - reserved for future use
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AudioCommand {
    /// Start recording with timestamp
    StartRecording { timestamp: String },
    /// Stop recording and finalize files
    StopRecording,
    /// Set track level (0.0 - 1.0)
    SetTrackLevel { track_id: usize, level: f32 },
    /// Set track pan (-1.0 to 1.0)
    SetTrackPan { track_id: usize, pan: f32 },
    /// Arm or disarm a track
    ArmTrack { track_id: usize, armed: bool },
}

/// Recording state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Stopped,
    WaitingForClock,
    Recording,
}

/// MIDI sync status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiSyncStatus {
    NoDevice,
    NoClockDetected,
    Synced,
}
