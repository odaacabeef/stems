use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render the help view
pub fn render_help_view(frame: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(""),
        Line::from("  stems - multi-track audio recorder"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Navigation", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("    ↑↓ or k/j        Navigate between tracks"),
        Line::from("    ←→ or h/l        Navigate between columns (Arm/Monitor/Level/Pan)"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Editing", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("    Space            Toggle Arm/Monitor or enter edit mode for Level/Pan"),
        Line::from("    ↑↓ (Level)       Adjust volume in edit mode"),
        Line::from("    ←→ (Pan)         Adjust pan in edit mode"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Track Management", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("    R                Toggle arm for all tracks"),
        Line::from("    M                Toggle monitoring for all tracks"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Recording", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("    MIDI Start       Begin recording armed tracks"),
        Line::from("    MIDI Stop        Stop recording and save files"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Other", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("    ?                Toggle this help"),
        Line::from("    q or Ctrl+c      Quit"),
        Line::from(""),
        Line::from("  Press ? to close"),
    ];

    let paragraph = Paragraph::new(help_text).alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}
