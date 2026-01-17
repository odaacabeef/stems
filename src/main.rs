mod app;
mod audio;
mod config;
mod midi;
mod types;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::env;
use std::io;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::app::App;
use crate::config::Config;
use crate::midi::MidiCommand;
use crate::types::{RecordingState, SAMPLE_RATE};
use crate::ui::{handle_input, render_ui};

/// stems - multi-track audio recorder
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Terminal-based multi-track audio recorder with MIDI clock sync",
    long_about = "Terminal-based multi-track audio recorder with MIDI clock sync.\n\n\
                  Records individual tracks (one per input channel) and optionally \
                  the monitored stereo mix to a single file.\n\n\
                  Configuration is loaded from stems.yaml by default, or use --config \
                  to specify a different file."
)]
struct Args {
    /// List available audio and MIDI devices
    #[arg(short, long)]
    list_devices: bool,

    /// Path to configuration file
    #[arg(short, long, value_name = "PATH", default_value = "stems.yaml")]
    config: String,
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

/// Parse monitor channels string (e.g., "17-18") into (start, end) tuple
fn parse_monitor_channels(channels_str: &str) -> Result<(u16, u16)> {
    let parts: Vec<&str> = channels_str.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid monitor channels format '{}'. Expected format: START-END (e.g., '17-18')", channels_str);
    }

    let start = parts[0].parse::<u16>()
        .with_context(|| format!("Invalid start channel '{}'", parts[0]))?;
    let end = parts[1].parse::<u16>()
        .with_context(|| format!("Invalid end channel '{}'", parts[1]))?;

    if start < 1 {
        anyhow::bail!("Start channel must be >= 1, got {}", start);
    }

    if end < start {
        anyhow::bail!("End channel {} must be >= start channel {}", end, start);
    }

    if end - start + 1 != 2 {
        anyhow::bail!("Monitor channels must be exactly 2 channels (stereo), got {} channels", end - start + 1);
    }

    Ok((start, end))
}

/// Load configuration from file or use defaults
fn load_config(config_path: &str) -> Result<Config> {
    let path = Path::new(config_path);

    // If explicit config path provided and file doesn't exist, error
    if config_path != "stems.yaml" && !path.exists() {
        anyhow::bail!("Config file not found: {}", config_path);
    }

    // If default path and file doesn't exist, use defaults
    if config_path == "stems.yaml" && !path.exists() {
        return Ok(Config::default());
    }

    // Load and parse config file
    Config::from_file(path)
}

/// Apply track configuration from config file to audio engine tracks
fn apply_track_config(audio_engine: &audio::AudioEngine, config: &Config) -> Result<()> {
    let tracks = audio_engine.tracks();

    for (track_num, track_config) in &config.tracks {
        // Convert 1-based track number to 0-based index
        let track_index = track_num.saturating_sub(1);

        // Validate track exists
        if track_index >= tracks.len() {
            anyhow::bail!(
                "Track {} does not exist (device has {} channels)",
                track_num,
                tracks.len()
            );
        }

        let track = &tracks[track_index];

        // Apply configuration values
        if let Some(arm) = track_config.arm {
            track.set_armed(arm);
        }

        if let Some(monitor) = track_config.monitor {
            track.set_monitoring(monitor);
        }

        if let Some(solo) = track_config.solo {
            track.set_solo(solo);
        }

        if let Some(level) = track_config.level {
            track.set_level(level);
        }

        if let Some(pan) = track_config.pan {
            track.set_pan(pan);
        }
    }

    Ok(())
}

/// Load playback tracks from config file
fn load_playback_tracks(config: &Config, sample_rate: u32) -> Result<Vec<audio::PlaybackTrack>> {
    let mut playback_tracks = Vec::new();

    for audio_config in &config.audio {
        let filepath = std::path::Path::new(&audio_config.file);

        // Load the WAV file
        let track = audio::PlaybackTrack::load_wav_file(filepath, sample_rate)?;

        // Apply configuration
        if let Some(monitor) = audio_config.monitor {
            track.set_monitoring(monitor);
        }
        if let Some(solo) = audio_config.solo {
            track.set_solo(solo);
        }
        if let Some(level) = audio_config.level {
            track.set_level(level);
        }
        if let Some(pan) = audio_config.pan {
            track.set_pan(pan);
        }

        playback_tracks.push(track);
    }

    Ok(playback_tracks)
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Handle --list-devices flag
    if args.list_devices {
        list_all_devices()?;
        return Ok(());
    }

    // Load configuration
    let config = load_config(&args.config)?;

    // Get output directory from current directory
    let output_dir = env::current_dir()?;

    // Create application with specific audio device if specified in config
    let mut app = if let Some(ref device_str) = config.devices.audio {
        let device_index = resolve_audio_device(device_str)?;
        let device = audio::device::get_device_by_index(device_index)?;

        let mut app = App::new(output_dir.clone())?;
        // Replace the audio engine with one using the specified device
        app.audio_engine = audio::AudioEngine::with_device(device, output_dir)?;
        app
    } else {
        App::new(output_dir)?
    };

    // Configure monitor output channels if specified in config
    if let Some(ref channels_str) = config.devices.monitorch {
        let (start, end) = parse_monitor_channels(channels_str)?;
        app.audio_engine.set_monitor_channels(start, end);
    }

    // Apply track configurations from config file
    apply_track_config(&app.audio_engine, &config)?;

    // Load playback tracks from config file
    let playback_tracks = load_playback_tracks(&config, SAMPLE_RATE)?;
    app.audio_engine.set_playback_tracks(playback_tracks);

    // Start audio stream
    if let Some(warning) = app.audio_engine.start_stream()? {
        app.show_warning(warning);
    }

    // Connect to MIDI device if specified in config
    let midi_rx = if let Some(ref device_str) = config.devices.midiin {
        let midi_index = resolve_midi_device(device_str)?;
        match app.midi_handler.connect(midi_index) {
            Ok(rx) => Some(rx),
            Err(e) => {
                app.show_error(format!("Failed to connect to MIDI device: {}", e));
                None
            }
        }
    } else {
        // Try default MIDI device (index 0) if available
        match midi::MidiHandler::list_ports() {
            Ok(ports) if !ports.is_empty() => match app.midi_handler.connect(0) {
                Ok(rx) => Some(rx),
                Err(_) => None,
            },
            _ => None,
        }
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
    println!("Configuration:");
    println!("  Create a stems.yaml file to configure devices and tracks");
    println!("  Use --config <path> to specify a different config file");
    println!();
    println!("Example stems.yaml:");
    println!("  devices:");
    println!("    audio: \"BlackHole 16ch + ES-9\"");
    println!("    monitorch: \"17-18\"");
    println!("    midiin: \"mc-source-b\"");
    println!();
    println!("  tracks:");
    println!("    1:");
    println!("      arm: false");
    println!("      monitor: true");
    println!("      level: 1.0");
    println!("      pan: 0.0");
    println!("    2:");
    println!("      monitor: true");
    println!("      level: 0.9");

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
fn handle_midi_command(
    app: &mut App,
    cmd: MidiCommand,
) -> Result<()> {
    match cmd {
        MidiCommand::Start => {
            app.recording_state = RecordingState::WaitingForClock;
            // Start playback if there are playback tracks
            if !app.audio_engine.playback_tracks().is_empty() {
                app.audio_engine.start_playback()?;
            }
        }

        MidiCommand::Stop => {
            // Stop playback immediately (non-blocking)
            if app.audio_engine.is_playing() {
                app.audio_engine.stop_playback()?;
            }

            // Update UI state immediately so user sees response
            app.recording_state = RecordingState::Stopped;
            app.recording_start_time = None;

            // Stop recording flag immediately (non-blocking)
            if app.audio_engine.is_recording() {
                app.audio_engine.stop_recording_async();
            }
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
