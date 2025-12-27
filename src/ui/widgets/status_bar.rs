use ratatui::{
    layout::{Alignment, Rect},
    widgets::Paragraph,
    Frame,
};

use crate::types::RecordingState;

/// Render the status bar
pub fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    recording_state: RecordingState,
    tempo: Option<f64>,
    duration: &str,
) {
    // Simple format: "state: {stopped|recording}; bpm: {N}; time: {duration}"
    let state_text = match recording_state {
        RecordingState::Recording => "recording",
        RecordingState::WaitingForClock => "waiting",
        RecordingState::Stopped => "stopped",
    };

    let bpm_text = if let Some(bpm) = tempo {
        format!("{:.1}", bpm)
    } else {
        "-".to_string()
    };

    // Add 2 spaces of left padding
    let status_text = format!("  state: {}; bpm: {}; time: {}", state_text, bpm_text, duration);

    let status_widget = Paragraph::new(status_text).alignment(Alignment::Left);

    frame.render_widget(status_widget, area);
}
