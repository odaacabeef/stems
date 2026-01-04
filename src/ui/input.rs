use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

use crate::app::App;

/// Handle keyboard input
pub fn handle_input(app: &mut App) -> anyhow::Result<()> {
    // Poll for events with timeout
    if event::poll(Duration::from_millis(16))? {
        if let Event::Key(key) = event::read()? {
            handle_key_event(app, key);
        }
    }

    Ok(())
}

/// Handle a key event
fn handle_key_event(app: &mut App, key: KeyEvent) {
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.quit();
        }

        // Navigation (arrow keys and vim bindings)
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_down();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.move_left();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.move_right();
        }

        // Space - activate/edit cell
        KeyCode::Char(' ') => {
            app.activate();
        }

        // Arm all / Disarm all
        KeyCode::Char('R') => {
            // Check if any tracks are armed
            let any_armed = app.tracks().iter().any(|t| t.is_armed());

            if any_armed {
                app.disarm_all_tracks();
            } else {
                app.arm_all_tracks();
            }
        }

        // Toggle monitoring for all tracks
        KeyCode::Char('M') => {
            app.toggle_all_monitoring();
        }

        // Ctrl+C - quit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.quit();
        }

        // ? - toggle help
        KeyCode::Char('?') => {
            app.toggle_help();
        }

        _ => {}
    }
}
