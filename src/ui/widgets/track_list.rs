use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Cell, Row, Table},
    Frame,
};
use std::sync::Arc;

use crate::audio::Track;
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
                Color::Yellow
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
