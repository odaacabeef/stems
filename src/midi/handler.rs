use anyhow::{Context, Result};
use midir::{Ignore, MidiInput, MidiInputConnection};
use parking_lot::Mutex;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::midi::clock::{ClockState, MidiClock};
use crate::types::MidiSyncStatus;

/// MIDI realtime message types
const MIDI_CLOCK: u8 = 0xF8;
const MIDI_START: u8 = 0xFA;
const MIDI_CONTINUE: u8 = 0xFB;
const MIDI_STOP: u8 = 0xFC;

/// Commands sent from MIDI handler to main application
#[derive(Debug, Clone)]
pub enum MidiCommand {
    /// Start recording
    Start,
    /// Stop recording
    Stop,
    /// MIDI clock pulse received
    Clock,
    /// Tempo updated (BPM)
    TempoUpdate(f64),
}

/// MIDI input port information
#[derive(Debug, Clone)]
pub struct MidiPortInfo {
    pub name: String,
    pub index: usize,
}

/// MIDI handler manages MIDI input and clock sync
pub struct MidiHandler {
    /// MIDI input connection
    connection: Option<MidiInputConnection<()>>,

    /// MIDI clock sync
    clock: Arc<Mutex<MidiClock>>,

    /// Command sender
    command_tx: Option<Sender<MidiCommand>>,
}

impl MidiHandler {
    /// Create a new MIDI handler
    pub fn new() -> Self {
        Self {
            connection: None,
            clock: Arc::new(Mutex::new(MidiClock::new())),
            command_tx: None,
        }
    }

    /// List available MIDI input ports
    pub fn list_ports() -> Result<Vec<MidiPortInfo>> {
        let midi_in = MidiInput::new("stems-query").context("Failed to create MIDI input")?;

        let ports = midi_in.ports();
        let mut port_infos = Vec::new();

        for (i, port) in ports.iter().enumerate() {
            let name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| format!("Unknown Port {}", i));

            port_infos.push(MidiPortInfo { name, index: i });
        }

        Ok(port_infos)
    }

    /// Connect to a MIDI input port
    pub fn connect(&mut self, port_index: usize) -> Result<Receiver<MidiCommand>> {
        // Create MIDI input
        let mut midi_in = MidiInput::new("stems").context("Failed to create MIDI input")?;

        // Only listen to realtime messages
        midi_in.ignore(Ignore::None);

        // Get available ports
        let ports = midi_in.ports();
        let port = ports
            .get(port_index)
            .context("MIDI port index out of range")?;

        // Create command channel
        let (tx, rx) = channel();
        self.command_tx = Some(tx.clone());

        // Clone for callback
        let clock = self.clock.clone();

        // Connect to port with callback
        let connection = midi_in
            .connect(
                port,
                "stems-input",
                move |_timestamp, message, _| {
                    handle_midi_message(message, &clock, &tx);
                },
                (),
            )
            .context("Failed to connect to MIDI port")?;

        self.connection = Some(connection);

        Ok(rx)
    }

    /// Disconnect from MIDI port
    pub fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close();
        }
        self.command_tx = None;
        self.clock.lock().reset();
    }

    /// Get current MIDI sync status
    pub fn sync_status(&self) -> MidiSyncStatus {
        if self.connection.is_none() {
            return MidiSyncStatus::NoDevice;
        }

        let clock = self.clock.lock();

        if clock.is_timed_out() {
            MidiSyncStatus::NoClockDetected
        } else if clock.state() == ClockState::Running {
            MidiSyncStatus::Synced
        } else {
            MidiSyncStatus::NoClockDetected
        }
    }

    /// Get current tempo in BPM
    pub fn tempo(&self) -> Option<f64> {
        self.clock.lock().calculate_tempo()
    }

    /// Get current clock state
    #[allow(dead_code)]
    pub fn clock_state(&self) -> ClockState {
        self.clock.lock().state()
    }

    /// Check if connected to a MIDI device
    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }
}

impl Drop for MidiHandler {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Handle incoming MIDI message
fn handle_midi_message(message: &[u8], clock: &Arc<Mutex<MidiClock>>, tx: &Sender<MidiCommand>) {
    if message.is_empty() {
        return;
    }

    let status = message[0];

    match status {
        MIDI_START => {
            let mut clock = clock.lock();
            clock.handle_start();
            let _ = tx.send(MidiCommand::Start);
        }

        MIDI_STOP => {
            let mut clock = clock.lock();
            clock.handle_stop();
            let _ = tx.send(MidiCommand::Stop);
        }

        MIDI_CONTINUE => {
            let mut clock = clock.lock();
            clock.handle_continue();
            // Treat Continue as Start for our purposes
            let _ = tx.send(MidiCommand::Start);
        }

        MIDI_CLOCK => {
            let mut clock = clock.lock();
            let _is_first_clock = clock.handle_clock();

            // Send clock command
            let _ = tx.send(MidiCommand::Clock);

            // Periodically send tempo updates (every 24 clocks = 1 beat)
            if clock.clock_count() % 24 == 0 {
                if let Some(tempo) = clock.calculate_tempo() {
                    let _ = tx.send(MidiCommand::TempoUpdate(tempo));
                }
            }
        }

        _ => {
            // Ignore other MIDI messages
        }
    }
}

/// Get port by name (case-insensitive substring match)
#[allow(dead_code)]
pub fn get_port_by_name(name: &str) -> Result<usize> {
    let ports = MidiHandler::list_ports()?;
    let name_lower = name.to_lowercase();

    for port in ports {
        if port.name.to_lowercase().contains(&name_lower) {
            return Ok(port.index);
        }
    }

    anyhow::bail!("MIDI port '{}' not found", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_midi_ports() {
        // This test may fail on systems without MIDI devices
        match MidiHandler::list_ports() {
            Ok(ports) => {
                println!("Found {} MIDI input ports:", ports.len());
                for port in ports {
                    println!("  [{}] {}", port.index, port.name);
                }
            }
            Err(e) => {
                println!("No MIDI ports available: {}", e);
            }
        }
    }

    #[test]
    fn test_midi_handler_creation() {
        let handler = MidiHandler::new();
        assert!(!handler.is_connected());
        assert_eq!(handler.clock_state(), ClockState::Stopped);
    }
}
