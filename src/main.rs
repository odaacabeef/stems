mod app;
mod audio;
mod midi;
mod types;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::env;
use std::io;
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::app::App;
use crate::midi::MidiCommand;
use crate::types::RecordingState;
use crate::ui::{handle_input, render_ui};

/// stems - multi-track audio recorder
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// List available audio and MIDI devices
    #[arg(short, long)]
    list_devices: bool,

    /// Audio device index or name (use --list-devices to see available devices)
    #[arg(short, long)]
    audio_device: Option<String>,

    /// MIDI device index or name (use --list-devices to see available devices)
    #[arg(short, long)]
    midi_device: Option<String>,
}

/// Resolve audio device string (index or name) to device index
fn resolve_audio_device(device_str: &str) -> Result<usize> {
    // Try to parse as index first
    if let Ok(index) = device_str.parse::<usize>() {
        return Ok(index);
    }

    // Otherwise, search by name (case-insensitive substring match)
    let devices = audio::device::list_input_devices()?;
    let device_str_lower = device_str.to_lowercase();

    for (i, device) in devices.iter().enumerate() {
        if device.name.to_lowercase().contains(&device_str_lower) {
            return Ok(i);
        }
    }

    anyhow::bail!("Audio device '{}' not found", device_str)
}

/// Resolve MIDI device string (index or name) to device index
fn resolve_midi_device(device_str: &str) -> Result<usize> {
    // Try to parse as index first
    if let Ok(index) = device_str.parse::<usize>() {
        return Ok(index);
    }

    // Otherwise, search by name (case-insensitive substring match)
    let ports = midi::MidiHandler::list_ports()?;
    let device_str_lower = device_str.to_lowercase();

    for port in ports {
        if port.name.to_lowercase().contains(&device_str_lower) {
            return Ok(port.index);
        }
    }

    anyhow::bail!("MIDI device '{}' not found", device_str)
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Handle --list-devices flag
    if args.list_devices {
        list_all_devices()?;
        return Ok(());
    }

    // Get output directory from current directory
    let output_dir = env::current_dir()?;

    // Create application with specific audio device if specified
    let mut app = if let Some(device_str) = args.audio_device {
        let device_index = resolve_audio_device(&device_str)?;
        let device = audio::device::get_device_by_index(device_index)?;

        let mut app = App::new(output_dir.clone())?;
        // Replace the audio engine with one using the specified device
        app.audio_engine = audio::AudioEngine::with_device(device, output_dir)?;
        app
    } else {
        App::new(output_dir)?
    };

    // Start audio stream
    if let Some(warning) = app.audio_engine.start_stream()? {
        app.show_warning(warning);
    }

    // Try to connect to MIDI device

    let midi_index_result = if let Some(ref device_str) = args.midi_device {
        Some(resolve_midi_device(device_str))
    } else {
        None
    };

    let midi_rx = match midi::MidiHandler::list_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                None
            } else {
                // Determine MIDI device index
                let midi_index = if let Some(result) = midi_index_result {
                    match result {
                        Ok(index) => index,
                        Err(_) => 0, // Fall back to default
                    }
                } else {
                    0
                };

                match app.midi_handler.connect(midi_index) {
                    Ok(rx) => Some(rx),
                    Err(_) => None,
                }
            }
        }
        Err(_) => None,
    };

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run main loop
    let result = run_app(&mut terminal, &mut app, midi_rx);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// List all available audio and MIDI devices
fn list_all_devices() -> Result<()> {
    println!("stems - available devices");
    println!("=========================");
    println!();

    // List audio devices
    println!("Audio Input Devices:");
    match audio::device::list_input_devices() {
        Ok(devices) => {
            if devices.is_empty() {
                println!("  No audio input devices found");
            } else {
                for (i, device) in devices.iter().enumerate() {
                    let default_marker = if device.is_default { " [DEFAULT]" } else { "" };
                    println!(
                        "  [{}] {} - {}ch @ {}Hz{}",
                        i, device.name, device.max_input_channels, device.sample_rate, default_marker
                    );
                }
            }
        }
        Err(e) => {
            println!("  Error: {}", e);
        }
    }

    println!();

    // List MIDI devices
    println!("MIDI Input Devices:");
    match midi::MidiHandler::list_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                println!("  No MIDI input devices found");
            } else {
                for port in ports {
                    println!("  [{}] {}", port.index, port.name);
                }
            }
        }
        Err(e) => {
            println!("  Error: {}", e);
        }
    }

    println!();
    println!("Use --audio-device <index or name> to select an audio device");
    println!("Use --midi-device <index or name> to select a MIDI device");
    println!();
    println!("Examples:");
    println!("  stems --audio-device 0");
    println!("  stems --audio-device \"iPhone\"");
    println!("  stems --midi-device \"beefdown-sync\"");

    Ok(())
}

/// Main application loop
fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    midi_rx: Option<Receiver<MidiCommand>>,
) -> Result<()> {
    loop {
        // Handle MIDI commands
        if let Some(ref rx) = midi_rx {
            match rx.try_recv() {
                Ok(cmd) => {
                    handle_midi_command(app, cmd)?;
                }
                Err(TryRecvError::Empty) => {
                    // No MIDI command, continue
                }
                Err(TryRecvError::Disconnected) => {
                    // MIDI disconnected, continue without it
                }
            }
        }

        // Update MIDI sync status
        app.update_midi_status();

        // Update peak meters (decay)
        app.update_meters();

        // Update message display (auto-clear expired messages)
        app.update_message();

        // Render UI
        terminal.draw(|frame| render_ui(frame, app))?;

        // Handle input
        handle_input(app)?;

        // Check for quit
        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

/// Handle MIDI command from MIDI thread
fn handle_midi_command(app: &mut App, cmd: MidiCommand) -> Result<()> {
    match cmd {
        MidiCommand::Start => {
            app.recording_state = RecordingState::WaitingForClock;
        }

        MidiCommand::Stop => {
            if app.audio_engine.is_recording() {
                app.audio_engine.stop_recording()?;
            }
            app.recording_state = RecordingState::Stopped;
            app.recording_start_time = None;
        }

        MidiCommand::Clock => {
            // On first clock after start, begin recording
            if app.recording_state == RecordingState::WaitingForClock {
                app.audio_engine.start_recording()?;
                app.recording_state = RecordingState::Recording;
                app.recording_start_time = Some(std::time::Instant::now());
            }
        }

        MidiCommand::TempoUpdate(tempo) => {
            app.tempo = Some(tempo);
        }
    }

    Ok(())
}
