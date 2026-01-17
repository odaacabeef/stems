use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::{AudioEngine, Track};
use crate::midi::MidiHandler;
use crate::types::{MidiSyncStatus, RecordingState};

/// Message type for user notifications
#[derive(Debug, Clone)]
pub enum MessageType {
    Warning,
    Error,
}

/// User notification message
#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub msg_type: MessageType,
    pub timestamp: Instant,
}

/// Column in the track table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Arm,
    Monitor,
    Solo,
    Level,
    Pan,
}

impl Column {
    pub fn all() -> &'static [Column] {
        &[Column::Arm, Column::Monitor, Column::Solo, Column::Level, Column::Pan]
    }
}

/// Main application state
pub struct App {
    /// Audio engine
    pub audio_engine: AudioEngine,

    /// MIDI handler
    pub midi_handler: MidiHandler,

    /// Current recording state
    pub recording_state: RecordingState,

    /// Selected track index
    pub selected_track: usize,

    /// Selected column
    pub selected_column: Column,

    /// Whether the mix recording row is selected
    pub selected_on_mix_row: bool,

    /// Selected playback track index
    pub selected_playback_track: usize,

    /// Whether we're in the playback section
    pub in_playback_section: bool,

    /// Whether we're in edit mode
    pub edit_mode: bool,

    /// MIDI sync status
    pub midi_sync_status: MidiSyncStatus,

    /// Current tempo in BPM
    pub tempo: Option<f64>,

    /// Output directory for recordings
    #[allow(dead_code)]
    pub output_dir: PathBuf,

    /// Whether to exit the application
    pub should_quit: bool,

    /// Peak meter decay rate (per frame)
    pub meter_decay: f32,

    /// Current message to display (if any)
    pub message: Option<Message>,

    /// Message display duration
    pub message_duration: Duration,

    /// Whether to show help view
    pub show_help: bool,

    /// Recording start time
    pub recording_start_time: Option<Instant>,
}

impl App {
    /// Create a new application
    pub fn new(output_dir: PathBuf) -> anyhow::Result<Self> {
        let audio_engine = AudioEngine::new(output_dir.clone())?;
        let midi_handler = MidiHandler::new();

        Ok(Self {
            audio_engine,
            midi_handler,
            recording_state: RecordingState::Stopped,
            selected_track: 0,
            selected_column: Column::Arm,
            selected_on_mix_row: false,
            selected_playback_track: 0,
            in_playback_section: false,
            edit_mode: false,
            midi_sync_status: MidiSyncStatus::NoDevice,
            tempo: None,
            output_dir,
            should_quit: false,
            meter_decay: 0.01, // Decay 1% per frame
            message: None,
            message_duration: Duration::from_secs(3),
            show_help: false,
            recording_start_time: None,
        })
    }

    /// Get reference to tracks
    pub fn tracks(&self) -> &Arc<Vec<Track>> {
        self.audio_engine.tracks()
    }

    /// Get selected track
    pub fn selected_track(&self) -> &Track {
        &self.tracks()[self.selected_track]
    }

    /// Move selection up (previous track or edit value)
    pub fn move_up(&mut self) {
        if self.edit_mode {
            // Edit mode: modify value
            match self.selected_column {
                Column::Level => self.increase_level(),
                _ => {}
            }
        } else {
            // Navigate between sections: mix -> playback -> tracks (reverse order)
            if self.selected_on_mix_row {
                // Move from mix row to playback or tracks
                let num_playback = self.audio_engine.playback_tracks().len();
                if num_playback > 0 {
                    // Move to last playback track, go to Monitor column
                    self.selected_on_mix_row = false;
                    self.in_playback_section = true;
                    self.selected_playback_track = num_playback - 1;
                    self.selected_column = Column::Monitor;
                } else {
                    // No playback tracks, move to last input track
                    // Keep Arm column when going back to input tracks
                    self.selected_on_mix_row = false;
                    let num_tracks = self.tracks().len();
                    if num_tracks > 0 {
                        self.selected_track = num_tracks - 1;
                    }
                }
            } else if self.in_playback_section {
                // Move from playback section
                if self.selected_playback_track > 0 {
                    // Move to previous playback track
                    self.selected_playback_track -= 1;
                } else {
                    // Move to last input track
                    // Column stays the same (Monitor/Solo/Level/Pan all exist in input tracks)
                    self.in_playback_section = false;
                    let num_tracks = self.tracks().len();
                    if num_tracks > 0 {
                        self.selected_track = num_tracks - 1;
                    }
                }
            } else if self.selected_track > 0 {
                // Move to previous track
                self.selected_track -= 1;
            }
        }
    }

    /// Move selection down (next track or edit value)
    pub fn move_down(&mut self) {
        if self.edit_mode {
            // Edit mode: modify value
            match self.selected_column {
                Column::Level => self.decrease_level(),
                _ => {}
            }
        } else {
            // Navigate between sections: tracks -> playback -> mix
            if self.selected_on_mix_row {
                // At mix row, can't go further down
            } else if self.in_playback_section {
                // Move to next playback track or to mix row
                let num_playback = self.audio_engine.playback_tracks().len();
                if self.selected_playback_track < num_playback.saturating_sub(1) {
                    self.selected_playback_track += 1;
                } else {
                    // At last playback track, move to mix row
                    self.in_playback_section = false;
                    self.selected_on_mix_row = true;
                    self.selected_column = Column::Arm;
                }
            } else {
                // Navigate to next track, playback section, or mix row
                let num_tracks = self.tracks().len();
                let num_playback = self.audio_engine.playback_tracks().len();

                if self.selected_track < num_tracks - 1 {
                    self.selected_track += 1;
                } else if num_playback > 0 {
                    // At last track, move to playback section
                    // Map column: Arm -> Monitor, others stay the same
                    if self.selected_column == Column::Arm {
                        self.selected_column = Column::Monitor;
                    }
                    self.in_playback_section = true;
                    self.selected_playback_track = 0;
                } else {
                    // No playback tracks, move directly to mix row
                    self.selected_on_mix_row = true;
                    self.selected_column = Column::Arm;
                }
            }
        }
    }

    /// Move selection left (previous column or edit value)
    pub fn move_left(&mut self) {
        if self.edit_mode {
            // Edit mode: modify value
            if self.selected_column == Column::Pan {
                self.pan_left();
            }
        } else {
            // Navigate to previous column based on current section
            if self.selected_on_mix_row {
                // Mix row only has Arm column, no left/right movement
            } else if self.in_playback_section {
                // Playback tracks: Monitor, Solo, Level, Pan
                match self.selected_column {
                    Column::Arm => {} // Should never be on Arm in playback
                    Column::Monitor => {} // Already at leftmost
                    Column::Solo => self.selected_column = Column::Monitor,
                    Column::Level => self.selected_column = Column::Solo,
                    Column::Pan => self.selected_column = Column::Level,
                }
            } else {
                // Input tracks: Arm, Monitor, Solo, Level, Pan
                let columns = Column::all();
                if let Some(idx) = columns.iter().position(|c| c == &self.selected_column) {
                    if idx > 0 {
                        self.selected_column = columns[idx - 1];
                    }
                }
            }
        }
    }

    /// Move selection right (next column or edit value)
    pub fn move_right(&mut self) {
        if self.edit_mode {
            // Edit mode: modify value
            if self.selected_column == Column::Pan {
                self.pan_right();
            }
        } else {
            // Navigate to next column based on current section
            if self.selected_on_mix_row {
                // Mix row only has Arm column, no left/right movement
            } else if self.in_playback_section {
                // Playback tracks: Monitor, Solo, Level, Pan
                match self.selected_column {
                    Column::Arm => {} // Should never be on Arm in playback
                    Column::Monitor => self.selected_column = Column::Solo,
                    Column::Solo => self.selected_column = Column::Level,
                    Column::Level => self.selected_column = Column::Pan,
                    Column::Pan => {} // Already at rightmost
                }
            } else {
                // Input tracks: Arm, Monitor, Solo, Level, Pan
                let columns = Column::all();
                if let Some(idx) = columns.iter().position(|c| c == &self.selected_column) {
                    if idx < columns.len() - 1 {
                        self.selected_column = columns[idx + 1];
                    }
                }
            }
        }
    }

    /// Jump to first track
    pub fn jump_to_first(&mut self) {
        if !self.edit_mode {
            self.selected_track = 0;
            self.selected_on_mix_row = false;
        }
    }

    /// Jump to last track (mix row)
    pub fn jump_to_last(&mut self) {
        if !self.edit_mode {
            // Jump to mix recording row
            self.selected_on_mix_row = true;
            // Set column to Arm for mix row
            self.selected_column = Column::Arm;
        }
    }

    /// Jump to leftmost column in current row
    pub fn jump_to_leftmost(&mut self) {
        if !self.edit_mode {
            if self.selected_on_mix_row {
                // Mix row only has Arm column
                self.selected_column = Column::Arm;
            } else if self.in_playback_section {
                // Playback tracks: leftmost is Monitor
                self.selected_column = Column::Monitor;
            } else {
                // Input tracks: leftmost is Arm
                self.selected_column = Column::Arm;
            }
        }
    }

    /// Jump to rightmost column in current row
    pub fn jump_to_rightmost(&mut self) {
        if !self.edit_mode {
            if self.selected_on_mix_row {
                // Mix row only has Arm column
                self.selected_column = Column::Arm;
            } else if self.in_playback_section {
                // Playback tracks: rightmost is Pan
                self.selected_column = Column::Pan;
            } else {
                // Input tracks: rightmost is Pan
                self.selected_column = Column::Pan;
            }
        }
    }

    /// Jump up 5 tracks
    pub fn jump_up_5(&mut self) {
        if !self.edit_mode {
            if self.selected_on_mix_row {
                // Jump from mix row to 5 tracks before the end
                self.selected_on_mix_row = false;
                let num_tracks = self.tracks().len();
                if num_tracks > 0 {
                    self.selected_track = num_tracks.saturating_sub(6);
                }
            } else {
                self.selected_track = self.selected_track.saturating_sub(5);
            }
        }
    }

    /// Jump down 5 tracks
    pub fn jump_down_5(&mut self) {
        if !self.edit_mode {
            if self.selected_on_mix_row {
                // Already at mix row
            } else {
                let num_tracks = self.tracks().len();
                if num_tracks > 0 {
                    let target = self.selected_track + 5;
                    if target >= num_tracks - 1 {
                        // Would go past last track, jump to mix row instead
                        self.selected_on_mix_row = true;
                        // Set column to Arm for mix row
                        self.selected_column = Column::Arm;
                    } else {
                        self.selected_track = target;
                    }
                }
            }
        }
    }

    /// Toggle edit mode or perform action
    pub fn activate(&mut self) {
        if self.edit_mode {
            // Exit edit mode
            self.edit_mode = false;
        } else if self.in_playback_section {
            // Handle activation on playback tracks
            let playback_tracks = self.audio_engine.playback_tracks();
            if self.selected_playback_track < playback_tracks.len() {
                let track = &playback_tracks[self.selected_playback_track];
                match self.selected_column {
                    Column::Monitor => {
                        // Toggle monitoring immediately
                        let current = track.is_monitoring();
                        track.set_monitoring(!current);
                    }
                    Column::Solo => {
                        // Toggle solo immediately
                        let current = track.is_solo();
                        track.set_solo(!current);
                    }
                    Column::Level | Column::Pan => {
                        // Enter edit mode for level and pan
                        self.edit_mode = true;
                    }
                    _ => {}
                }
            }
        } else if self.selected_on_mix_row {
            // Toggle mix recording armed state
            let current = self.audio_engine.is_mix_recording_armed();
            self.audio_engine.set_mix_recording_armed(!current);
        } else {
            // Enter edit mode or toggle arm/monitor (input tracks)
            match self.selected_column {
                Column::Arm => {
                    let track = self.selected_track();
                    // Can't change arm status while recording
                    if track.is_recording() {
                        self.show_error("Cannot change arm status while recording");
                    } else {
                        // Toggle arm immediately
                        let current = track.is_armed();
                        track.set_armed(!current);
                    }
                }
                Column::Monitor => {
                    // Toggle monitoring immediately
                    let track = self.selected_track();
                    let current = track.is_monitoring();
                    track.set_monitoring(!current);
                }
                Column::Solo => {
                    // Toggle solo immediately
                    let track = self.selected_track();
                    let current = track.is_solo();
                    track.set_solo(!current);
                }
                _ => {
                    // Enter edit mode
                    self.edit_mode = true;
                }
            }
        }
    }

    /// Arm all tracks (except those currently recording)
    pub fn arm_all_tracks(&mut self) {
        for track in self.tracks().iter() {
            if !track.is_recording() {
                track.set_armed(true);
            }
        }
    }

    /// Disarm all tracks (except those currently recording)
    pub fn disarm_all_tracks(&mut self) {
        for track in self.tracks().iter() {
            if !track.is_recording() {
                track.set_armed(false);
            }
        }
    }

    /// Toggle monitoring for all tracks
    pub fn toggle_all_monitoring(&mut self) {
        // Check if any track has monitoring enabled
        let any_monitoring = self.tracks().iter().any(|track| track.is_monitoring());

        // If any are monitoring, disable all; otherwise enable all
        for track in self.tracks().iter() {
            track.set_monitoring(!any_monitoring);
        }
    }

    /// Toggle solo for all tracks
    pub fn toggle_all_solo(&mut self) {
        // Check if any track has solo enabled
        let any_solo = self.tracks().iter().any(|track| track.is_solo());

        // If any are soloed, disable all; otherwise enable all
        for track in self.tracks().iter() {
            track.set_solo(!any_solo);
        }
    }

    /// Increase level of selected track
    fn increase_level(&mut self) {
        if self.in_playback_section {
            // Adjust playback track level
            let playback_tracks = self.audio_engine.playback_tracks();
            if self.selected_playback_track < playback_tracks.len() {
                let track = &playback_tracks[self.selected_playback_track];
                let current = track.get_level();
                track.set_level((current + 0.05).min(1.0));
            }
        } else {
            // Adjust input track level
            let track = self.selected_track();
            let current = track.get_level();
            track.set_level((current + 0.05).min(1.0));
        }
    }

    /// Decrease level of selected track
    fn decrease_level(&mut self) {
        if self.in_playback_section {
            // Adjust playback track level
            let playback_tracks = self.audio_engine.playback_tracks();
            if self.selected_playback_track < playback_tracks.len() {
                let track = &playback_tracks[self.selected_playback_track];
                let current = track.get_level();
                track.set_level((current - 0.05).max(0.0));
            }
        } else {
            // Adjust input track level
            let track = self.selected_track();
            let current = track.get_level();
            track.set_level((current - 0.05).max(0.0));
        }
    }

    /// Pan left
    fn pan_left(&mut self) {
        if self.in_playback_section {
            // Adjust playback track pan
            let playback_tracks = self.audio_engine.playback_tracks();
            if self.selected_playback_track < playback_tracks.len() {
                let track = &playback_tracks[self.selected_playback_track];
                let current = track.get_pan();
                // Round to nearest 0.1 to avoid floating point drift
                let new_pan = ((current - 0.1) * 10.0).round() / 10.0;
                track.set_pan(new_pan.max(-1.0));
            }
        } else {
            // Adjust input track pan
            let track = self.selected_track();
            let current = track.get_pan();
            // Round to nearest 0.1 to avoid floating point drift
            let new_pan = ((current - 0.1) * 10.0).round() / 10.0;
            track.set_pan(new_pan.max(-1.0));
        }
    }

    /// Pan right
    fn pan_right(&mut self) {
        if self.in_playback_section {
            // Adjust playback track pan
            let playback_tracks = self.audio_engine.playback_tracks();
            if self.selected_playback_track < playback_tracks.len() {
                let track = &playback_tracks[self.selected_playback_track];
                let current = track.get_pan();
                // Round to nearest 0.1 to avoid floating point drift
                let new_pan = ((current + 0.1) * 10.0).round() / 10.0;
                track.set_pan(new_pan.min(1.0));
            }
        } else {
            // Adjust input track pan
            let track = self.selected_track();
            let current = track.get_pan();
            // Round to nearest 0.1 to avoid floating point drift
            let new_pan = ((current + 0.1) * 10.0).round() / 10.0;
            track.set_pan(new_pan.min(1.0));
        }
    }

    /// Update peak meters (decay)
    pub fn update_meters(&mut self) {
        for track in self.tracks().iter() {
            track.decay_peak_level(self.meter_decay);
        }
        // Decay playback track peak levels
        for track in self.audio_engine.playback_tracks().iter() {
            track.decay_peak_level(self.meter_decay);
        }
    }

    /// Clear message if it has expired
    pub fn update_message(&mut self) {
        if let Some(ref msg) = self.message {
            if msg.timestamp.elapsed() > self.message_duration {
                self.message = None;
            }
        }
    }

    /// Show a warning message
    pub fn show_warning(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            msg_type: MessageType::Warning,
            timestamp: Instant::now(),
        });
    }

    /// Show an error message
    pub fn show_error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            msg_type: MessageType::Error,
            timestamp: Instant::now(),
        });
    }

    /// Update MIDI sync status
    pub fn update_midi_status(&mut self) {
        self.midi_sync_status = self.midi_handler.sync_status();
        self.tempo = self.midi_handler.tempo();
    }

    /// Get recording state as string
    #[allow(dead_code)]
    pub fn recording_state_str(&self) -> &'static str {
        match self.recording_state {
            RecordingState::Stopped => "STOPPED",
            RecordingState::WaitingForClock => "WAITING",
            RecordingState::Recording => "RECORDING",
        }
    }

    /// Get MIDI sync status as string
    #[allow(dead_code)]
    pub fn midi_sync_str(&self) -> &'static str {
        match self.midi_sync_status {
            MidiSyncStatus::NoDevice => "NO DEVICE",
            MidiSyncStatus::NoClockDetected => "NO CLOCK",
            MidiSyncStatus::Synced => "SYNCED",
        }
    }

    /// Get tempo string
    #[allow(dead_code)]
    pub fn tempo_str(&self) -> String {
        if let Some(tempo) = self.tempo {
            format!("{:.1} BPM", tempo)
        } else {
            "--- BPM".to_string()
        }
    }

    /// Get recording duration string
    pub fn recording_duration_str(&self) -> String {
        if let Some(start_time) = self.recording_start_time {
            let duration = start_time.elapsed();
            let secs = duration.as_secs();
            let hours = secs / 3600;
            let minutes = (secs % 3600) / 60;
            let seconds = secs % 60;

            if hours > 0 {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            } else {
                format!("{:02}:{:02}", minutes, seconds)
            }
        } else {
            "-".to_string()
        }
    }

    /// Request quit
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Check if should quit
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Check if mix recording is armed
    pub fn mix_recording_armed(&self) -> bool {
        self.audio_engine.is_mix_recording_armed()
    }

    /// Check if mix is currently recording
    pub fn mix_recording_is_recording(&self) -> bool {
        self.audio_engine.is_mix_recording()
    }

    /// Toggle help view
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

}

impl Drop for App {
    fn drop(&mut self) {
        // Ensure clean shutdown
        let _ = self.audio_engine.stop_recording();
        let _ = self.audio_engine.stop_stream();
        self.midi_handler.disconnect();
    }
}
