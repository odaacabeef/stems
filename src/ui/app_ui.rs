use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, MessageType};
use crate::ui::widgets::{render_help_view, render_status_bar, render_track_list, render_mix_recording_row, render_playback_list};

/// Render the main UI
pub fn render_ui(frame: &mut Frame, app: &App) {
    // If help is shown, render help view instead of normal UI
    if app.show_help {
        render_help_view(frame, frame.area());
        return;
    }

    // Check if we have a message to display
    let has_message = app.message.is_some();

    let chunks = if has_message {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Status bar
                Constraint::Length(1),  // Line break
                Constraint::Length(3),  // Message bar
                Constraint::Min(1),     // Track list
            ])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Status bar
                Constraint::Length(1),  // Line break
                Constraint::Min(1),     // Track list
            ])
            .split(frame.area())
    };

    // Render status bar
    let duration = app.recording_duration_str();
    render_status_bar(
        frame,
        chunks[0],
        app.recording_state,
        app.tempo,
        &duration,
    );

    // Render message bar if present
    if has_message {
        if let Some(ref msg) = app.message {
            let (color, prefix) = match msg.msg_type {
                MessageType::Warning => (Color::Yellow, "⚠ "),
                MessageType::Error => (Color::Red, "✖ "),
            };

            let text = format!("{}{}", prefix, msg.text);
            let message_widget = Paragraph::new(Line::from(text))
                .style(
                    Style::default()
                        .fg(color)
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::ALL));

            frame.render_widget(message_widget, chunks[2]);
        }

        // Split track list area vertically for track table, blank line, mix row, playback section, and remaining space
        let track_area = chunks[3];
        let num_tracks = app.tracks().len() as u16;
        let num_playback = app.audio_engine.playback_tracks().len() as u16;

        let mut constraints = vec![
            Constraint::Length(num_tracks), // Track table (exact size)
            Constraint::Length(1),          // Blank line
            Constraint::Length(1),          // Mix recording row
        ];

        // Add playback section if there are playback tracks
        if num_playback > 0 {
            constraints.push(Constraint::Length(1)); // Blank line
            constraints.push(Constraint::Length(num_playback)); // Playback tracks
        }

        constraints.push(Constraint::Min(0)); // Remaining empty space

        let track_area_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(track_area);

        // Render track list (skip chunk[2] which is line break)
        // If on mix row or playback section, pass invalid index so no track appears selected
        let selected_track_index = if app.selected_on_mix_row || app.in_playback_section {
            usize::MAX
        } else {
            app.selected_track
        };
        render_track_list(
            frame,
            track_area_chunks[0],
            app.tracks(),
            selected_track_index,
            app.selected_column,
            app.edit_mode,
        );

        // Render mix recording row (skip chunk[1] which is blank line)
        render_mix_recording_row(frame, track_area_chunks[2], app);

        // Render playback section if present
        if num_playback > 0 {
            // Render playback tracks (chunk[4] - chunk[3] is the blank line)
            render_playback_list(
                frame,
                track_area_chunks[4],
                app.audio_engine.playback_tracks(),
                app.selected_playback_track,
                app.selected_column,
                app.edit_mode,
                app.in_playback_section,
            );
        }
    } else {
        // Split track list area vertically for track table, blank line, mix row, playback section, and remaining space
        let track_area = chunks[2];
        let num_tracks = app.tracks().len() as u16;
        let num_playback = app.audio_engine.playback_tracks().len() as u16;

        let mut constraints = vec![
            Constraint::Length(num_tracks), // Track table (exact size)
            Constraint::Length(1),          // Blank line
            Constraint::Length(1),          // Mix recording row
        ];

        // Add playback section if there are playback tracks
        if num_playback > 0 {
            constraints.push(Constraint::Length(1)); // Blank line
            constraints.push(Constraint::Length(num_playback)); // Playback tracks
        }

        constraints.push(Constraint::Min(0)); // Remaining empty space

        let track_area_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(track_area);

        // Render track list (skip chunk[2] which is line break)
        // If on mix row or playback section, pass invalid index so no track appears selected
        let selected_track_index = if app.selected_on_mix_row || app.in_playback_section {
            usize::MAX
        } else {
            app.selected_track
        };
        render_track_list(
            frame,
            track_area_chunks[0],
            app.tracks(),
            selected_track_index,
            app.selected_column,
            app.edit_mode,
        );

        // Render mix recording row (skip chunk[1] which is blank line)
        render_mix_recording_row(frame, track_area_chunks[2], app);

        // Render playback section if present
        if num_playback > 0 {
            // Render playback tracks (chunk[4] - chunk[3] is the blank line)
            render_playback_list(
                frame,
                track_area_chunks[4],
                app.audio_engine.playback_tracks(),
                app.selected_playback_track,
                app.selected_column,
                app.edit_mode,
                app.in_playback_section,
            );
        }
    }
}
