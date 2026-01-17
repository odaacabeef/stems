use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};
use std::sync::Arc;

use crate::app::App;
use crate::audio::{PlaybackTrack, Track};
use crate::app::Column;

/// Render the track list
pub fn render_track_list(
    frame: &mut Frame,
    area: Rect,
    tracks: &Arc<Vec<Track>>,
    selected_index: usize,
    selected_column: Column,
    edit_mode: bool,
) {
    // Rows
    let rows: Vec<Row> = tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_selected = i == selected_index;

            // Track name/number
            let track_name = format!("{:2}", track.id + 1);

            // Arm status
            let arm_status = if track.is_armed() {
                if track.is_recording() {
                    "[●]"
                } else {
                    "[R]"
                }
            } else {
                "[ ]"
            };

            // Monitor status
            let mon_status = if track.is_monitoring() {
                "[M]"
            } else {
                "[ ]"
            };

            // Solo status
            let solo_status = if track.is_solo() {
                "[S]"
            } else {
                "[ ]"
            };

            // Level
            let level_pct = (track.get_level() * 100.0) as u8;
            let level_str = format!("{:3}%", level_pct);

            // Pan
            let pan = track.get_pan();
            let pan_str = if pan < 0.0 {
                format!("L{:2}", (pan.abs() * 10.0) as u8)
            } else if pan > 0.0 {
                format!("R{:2}", (pan * 10.0) as u8)
            } else {
                " C ".to_string()
            };

            // Peak level for meter
            let peak = track.get_peak_level();
            let meter_str = create_meter_string(peak, 20);

            // Determine cell styles based on selection and edit mode
            let arm_color = if track.is_recording() {
                Color::Red
            } else if track.is_armed() {
                Color::Red
            } else {
                Color::Gray
            };

            // Helper to create cell style for selected cells
            let cell_style = |column: Column| {
                if is_selected && selected_column == column {
                    if edit_mode {
                        // Edit mode: cyan background with bold text
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        // Selected but not editing: dark gray background
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    }
                } else {
                    Style::default()
                }
            };

            Row::new(vec![
                Cell::from("  "), // Left padding
                Cell::from(track_name),
                Cell::from(arm_status).style(
                    if is_selected && selected_column == Column::Arm {
                        cell_style(Column::Arm).fg(arm_color)
                    } else {
                        Style::default().fg(arm_color)
                    }
                ),
                Cell::from(mon_status).style(
                    if is_selected && selected_column == Column::Monitor {
                        cell_style(Column::Monitor).fg(if track.is_monitoring() { Color::Green } else { Color::Gray })
                    } else {
                        Style::default().fg(if track.is_monitoring() { Color::Green } else { Color::Gray })
                    }
                ),
                Cell::from(solo_status).style(
                    if is_selected && selected_column == Column::Solo {
                        cell_style(Column::Solo).fg(if track.is_solo() { Color::Cyan } else { Color::Gray })
                    } else {
                        Style::default().fg(if track.is_solo() { Color::Cyan } else { Color::Gray })
                    }
                ),
                Cell::from(level_str).style(cell_style(Column::Level)),
                Cell::from(pan_str).style(cell_style(Column::Pan)),
                Cell::from(meter_str),
            ])
        })
        .collect();

    // Create table
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),  // Left padding
            Constraint::Length(3),  // Track
            Constraint::Length(3),  // Arm
            Constraint::Length(3),  // Monitor
            Constraint::Length(3),  // Solo
            Constraint::Length(4),  // Level
            Constraint::Length(3),  // Pan
            Constraint::Min(20),    // Meter
        ],
    )
    .column_spacing(1);

    frame.render_widget(table, area);
}

/// Create a simple text-based meter
fn create_meter_string(level: f32, width: usize) -> String {
    let level = level.clamp(0.0, 1.0);
    let filled = (level * width as f32) as usize;

    let mut meter = String::with_capacity(width);

    for i in 0..width {
        let rel_pos = i as f32 / width as f32;

        if i < filled {
            // Filled portion
            if rel_pos > 0.9 {
                meter.push('█'); // Peak (red zone)
            } else if rel_pos > 0.7 {
                meter.push('▓'); // Warning (yellow zone)
            } else {
                meter.push('▓'); // Normal (green zone)
            }
        } else {
            // Empty portion
            meter.push('░');
        }
    }

    meter
}

/// Render the mix recording row below the track list
pub fn render_mix_recording_row(
    frame: &mut Frame,
    area: Rect,
    app: &App,
) {
    let is_armed = app.mix_recording_armed();
    let is_recording = app.mix_recording_is_recording();
    let is_selected = app.selected_on_mix_row;
    let is_arm_column_selected = is_selected && app.selected_column == Column::Arm;

    // Format arm status similar to track rows
    let arm_status = if is_recording {
        "[●]"
    } else if is_armed {
        "[R]"
    } else {
        "[ ]"
    };

    // Determine color based on state
    let arm_color = if is_recording || is_armed {
        Color::Red
    } else {
        Color::Gray
    };

    // Apply selection styling only to the arm checkbox when selected
    let arm_style = if is_arm_column_selected {
        Style::default()
            .bg(Color::DarkGray)
            .fg(arm_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(arm_color)
    };

    // Create the paragraph with arm status colored and selectable
    let line = Line::from(vec![
        ratatui::text::Span::raw("       "),
        ratatui::text::Span::styled(arm_status, arm_style),
        ratatui::text::Span::raw(" record monitored mix"),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Render the playback tracks list
pub fn render_playback_list(
    frame: &mut Frame,
    area: Rect,
    playback_tracks: &Arc<Vec<PlaybackTrack>>,
    selected_index: usize,
    selected_column: Column,
    edit_mode: bool,
    in_playback_section: bool,
) {
    // Rows
    let rows: Vec<Row> = playback_tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_selected = in_playback_section && i == selected_index;

            // Monitor status
            let mon_status = if track.is_monitoring() {
                "[M]"
            } else {
                "[ ]"
            };

            // Solo status
            let solo_status = if track.is_solo() {
                "[S]"
            } else {
                "[ ]"
            };

            // Level
            let level_pct = (track.get_level() * 100.0) as u8;
            let level_str = format!("{:3}%", level_pct);

            // Pan
            let pan = track.get_pan();
            let pan_str = if pan < 0.0 {
                format!("L{:2}", (pan.abs() * 10.0) as u8)
            } else if pan > 0.0 {
                format!("R{:2}", (pan * 10.0) as u8)
            } else {
                " C ".to_string()
            };

            // Peak level for meter
            let peak = track.get_peak_level();
            let meter_str = create_meter_string(peak, 20);

            // Helper to create cell style for selected cells
            let cell_style = |column: Column| {
                if is_selected && selected_column == column {
                    if edit_mode {
                        // Edit mode: cyan background with bold text
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        // Selected but not editing: dark gray background
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    }
                } else {
                    Style::default()
                }
            };

            // Track number (1-indexed)
            let track_number = format!("{:2}", i + 1);

            Row::new(vec![
                Cell::from("  "), // Left padding
                Cell::from(track_number),
                Cell::from("   "), // Skip arm column
                Cell::from(mon_status).style(
                    if is_selected && selected_column == Column::Monitor {
                        cell_style(Column::Monitor).fg(if track.is_monitoring() { Color::Green } else { Color::Gray })
                    } else {
                        Style::default().fg(if track.is_monitoring() { Color::Green } else { Color::Gray })
                    }
                ),
                Cell::from(solo_status).style(
                    if is_selected && selected_column == Column::Solo {
                        cell_style(Column::Solo).fg(if track.is_solo() { Color::Cyan } else { Color::Gray })
                    } else {
                        Style::default().fg(if track.is_solo() { Color::Cyan } else { Color::Gray })
                    }
                ),
                Cell::from(level_str).style(cell_style(Column::Level)),
                Cell::from(pan_str).style(cell_style(Column::Pan)),
                Cell::from(meter_str),
            ])
        })
        .collect();

    // Create table with same column widths as input tracks
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),  // Left padding
            Constraint::Length(3),  // Track (empty for playback)
            Constraint::Length(3),  // Arm (empty for playback)
            Constraint::Length(3),  // Monitor
            Constraint::Length(3),  // Solo
            Constraint::Length(4),  // Level
            Constraint::Length(3),  // Pan
            Constraint::Min(20),    // Meter + filename
        ],
    )
    .column_spacing(1);

    frame.render_widget(table, area);
}

