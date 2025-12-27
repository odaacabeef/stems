use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, MessageType};
use crate::ui::widgets::{render_help_view, render_status_bar, render_track_list};

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
                Constraint::Length(1),  // Top padding
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
                Constraint::Length(1),  // Top padding
                Constraint::Length(1),  // Status bar
                Constraint::Length(1),  // Line break
                Constraint::Min(1),     // Track list
            ])
            .split(frame.area())
    };

    // Render status bar (skip chunk[0] which is top padding)
    let duration = app.recording_duration_str();
    render_status_bar(
        frame,
        chunks[1],
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

            frame.render_widget(message_widget, chunks[3]);
        }

        // Render track list (skip chunk[2] which is line break)
        render_track_list(
            frame,
            chunks[4],
            app.tracks(),
            app.selected_track,
            app.selected_column,
            app.edit_mode,
        );
    } else {
        // Render track list (skip chunk[2] which is line break)
        render_track_list(
            frame,
            chunks[3],
            app.tracks(),
            app.selected_track,
            app.selected_column,
            app.edit_mode,
        );
    }
}
