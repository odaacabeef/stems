use std::time::{Duration, Instant};

/// MIDI clock pulses per quarter note
const MIDI_CLOCKS_PER_BEAT: u32 = 24;

/// State machine for MIDI clock synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockState {
    /// No MIDI clock activity
    Stopped,
    /// Received Start message, waiting for first clock
    WaitingForClock,
    /// Actively receiving clock pulses
    Running,
}

/// MIDI clock synchronization manager
#[derive(Debug)]
pub struct MidiClock {
    /// Current state
    state: ClockState,

    /// Clock pulse counter
    clock_count: u32,

    /// Last clock pulse time
    last_clock_time: Option<Instant>,

    /// Recent clock intervals for tempo calculation
    clock_intervals: Vec<Duration>,

    /// Maximum intervals to keep for averaging
    max_intervals: usize,

    /// Time since last clock (for timeout detection)
    last_activity: Instant,

    /// Timeout duration before considering clock lost
    timeout: Duration,
}

impl MidiClock {
    /// Create a new MIDI clock sync manager
    pub fn new() -> Self {
        Self {
            state: ClockState::Stopped,
            clock_count: 0,
            last_clock_time: None,
            clock_intervals: Vec::new(),
            max_intervals: 24, // Average over 1 beat
            last_activity: Instant::now(),
            timeout: Duration::from_secs(2),
        }
    }

    /// Handle MIDI Start message (0xFA)
    pub fn handle_start(&mut self) {
        self.state = ClockState::WaitingForClock;
        self.clock_count = 0;
        self.clock_intervals.clear();
        self.last_clock_time = None;
        self.last_activity = Instant::now();
    }

    /// Handle MIDI Stop message (0xFC)
    pub fn handle_stop(&mut self) {
        self.state = ClockState::Stopped;
        self.clock_count = 0;
        self.last_activity = Instant::now();
    }

    /// Handle MIDI Continue message (0xFB)
    pub fn handle_continue(&mut self) {
        self.state = ClockState::WaitingForClock;
        self.last_activity = Instant::now();
        // Note: clock_count is NOT reset on Continue
    }

    /// Handle MIDI Clock message (0xF8)
    /// Returns true if this is the first clock after Start
    pub fn handle_clock(&mut self) -> bool {
        let is_first_clock = self.state == ClockState::WaitingForClock;

        self.state = ClockState::Running;
        self.clock_count += 1;
        self.last_activity = Instant::now();

        // Calculate interval since last clock
        if let Some(last_time) = self.last_clock_time {
            let interval = self.last_activity.duration_since(last_time);

            // Store interval for tempo calculation
            self.clock_intervals.push(interval);
            if self.clock_intervals.len() > self.max_intervals {
                self.clock_intervals.remove(0);
            }
        }

        self.last_clock_time = Some(self.last_activity);

        is_first_clock
    }

    /// Get current clock state
    pub fn state(&self) -> ClockState {
        self.state
    }

    /// Get current clock count
    pub fn clock_count(&self) -> u32 {
        self.clock_count
    }

    /// Calculate current tempo in BPM from clock intervals
    pub fn calculate_tempo(&self) -> Option<f64> {
        if self.clock_intervals.is_empty() {
            return None;
        }

        // Calculate average interval between clocks
        let total_micros: u128 = self
            .clock_intervals
            .iter()
            .map(|d| d.as_micros())
            .sum();

        let avg_interval_micros = total_micros as f64 / self.clock_intervals.len() as f64;

        // Convert to BPM
        // 1 beat = 24 clocks
        // BPM = 60 seconds / (avg_interval * 24)
        let beat_duration_micros = avg_interval_micros * MIDI_CLOCKS_PER_BEAT as f64;
        let beat_duration_seconds = beat_duration_micros / 1_000_000.0;

        if beat_duration_seconds > 0.0 {
            Some(60.0 / beat_duration_seconds)
        } else {
            None
        }
    }

    /// Check if clock has timed out (no recent activity)
    pub fn is_timed_out(&self) -> bool {
        if self.state == ClockState::Stopped {
            return false;
        }

        self.last_activity.elapsed() > self.timeout
    }

    /// Get time since last activity
    #[allow(dead_code)]
    pub fn time_since_last_activity(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Reset the clock state
    pub fn reset(&mut self) {
        self.state = ClockState::Stopped;
        self.clock_count = 0;
        self.clock_intervals.clear();
        self.last_clock_time = None;
        self.last_activity = Instant::now();
    }
}

impl Default for MidiClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_clock_state_machine() {
        let mut clock = MidiClock::new();

        assert_eq!(clock.state(), ClockState::Stopped);

        clock.handle_start();
        assert_eq!(clock.state(), ClockState::WaitingForClock);

        let is_first = clock.handle_clock();
        assert!(is_first);
        assert_eq!(clock.state(), ClockState::Running);
        assert_eq!(clock.clock_count(), 1);

        let is_first = clock.handle_clock();
        assert!(!is_first);
        assert_eq!(clock.clock_count(), 2);

        clock.handle_stop();
        assert_eq!(clock.state(), ClockState::Stopped);
        assert_eq!(clock.clock_count(), 0);
    }

    #[test]
    fn test_tempo_calculation() {
        let mut clock = MidiClock::new();

        clock.handle_start();

        // Simulate 120 BPM: 500ms per beat = 20.833ms per clock
        // 120 BPM = 2 beats per second = 0.5 seconds per beat
        for _ in 0..48 {
            clock.handle_clock();
            thread::sleep(Duration::from_micros(20833));
        }

        if let Some(tempo) = clock.calculate_tempo() {
            println!("Calculated tempo: {:.1} BPM", tempo);
            // Allow some tolerance due to sleep inaccuracy
            assert!((tempo - 120.0).abs() < 10.0);
        }
    }

    #[test]
    fn test_continue_preserves_count() {
        let mut clock = MidiClock::new();

        clock.handle_start();
        clock.handle_clock();
        clock.handle_clock();
        assert_eq!(clock.clock_count(), 2);

        clock.handle_stop();
        assert_eq!(clock.clock_count(), 0);

        clock.handle_start();
        clock.handle_clock();
        assert_eq!(clock.clock_count(), 1);

        clock.handle_continue();
        clock.handle_clock();
        // Continue doesn't reset count
        assert_eq!(clock.clock_count(), 2);
    }
}
