use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::audio::callback::{
    create_audio_callback, create_error_callback, create_monitor_callback, AudioCallbackState,
};
use crate::audio::device::{get_default_input_device, get_max_channels_input_config, get_max_channels_output_config};
use crate::audio::track::Track;
use crate::audio::writer::{generate_timestamp, FileWriter};
use crate::types::{RING_BUFFER_SECONDS, SAMPLE_RATE};

/// Audio engine manages audio I/O and recording
pub struct AudioEngine {
    /// Audio input device
    device: Device,

    /// Stream configuration
    config: StreamConfig,

    /// Number of input channels
    num_channels: usize,

    /// Audio tracks
    tracks: Arc<Vec<Track>>,

    /// Recording state flag
    recording: Arc<AtomicBool>,

    /// Active input audio stream
    input_stream: Option<Stream>,

    /// Active output audio stream (for monitoring)
    output_stream: Option<Stream>,

    /// File writer
    file_writer: Option<FileWriter>,

    /// Output directory for recordings
    output_dir: PathBuf,

    /// Monitor output channels (start, end) - 1-indexed
    /// If None, defaults to channels 1-2
    monitor_channels: Option<(u16, u16)>,
}

impl AudioEngine {
    /// Create a new audio engine with default device
    pub fn new(output_dir: PathBuf) -> Result<Self> {
        let device = get_default_input_device()?;
        let supported_config = get_max_channels_input_config(&device)?;

        let config = StreamConfig {
            channels: supported_config.channels(),
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(256), // Small buffer for low latency
        };

        let num_channels = config.channels as usize;

        // Create one track per input channel
        let mut tracks = Vec::new();
        for i in 0..num_channels {
            tracks.push(Track::new(i, i));
        }

        Ok(Self {
            device,
            config,
            num_channels,
            tracks: Arc::new(tracks),
            recording: Arc::new(AtomicBool::new(false)),
            input_stream: None,
            output_stream: None,
            file_writer: None,
            output_dir,
            monitor_channels: None,
        })
    }

    /// Set monitor output channels (1-indexed, e.g., 17-18 for aggregate devices)
    pub fn set_monitor_channels(&mut self, start: u16, end: u16) {
        self.monitor_channels = Some((start, end));
    }

    /// Create audio engine with specific device
    #[allow(dead_code)]
    pub fn with_device(device: Device, output_dir: PathBuf) -> Result<Self> {
        let supported_config = get_max_channels_input_config(&device)?;

        let config = StreamConfig {
            channels: supported_config.channels(),
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(256), // Small buffer for low latency
        };

        let num_channels = config.channels as usize;

        // Create one track per input channel
        let mut tracks = Vec::new();
        for i in 0..num_channels {
            tracks.push(Track::new(i, i));
        }

        Ok(Self {
            device,
            config,
            num_channels,
            tracks: Arc::new(tracks),
            recording: Arc::new(AtomicBool::new(false)),
            input_stream: None,
            output_stream: None,
            file_writer: None,
            output_dir,
            monitor_channels: None,
        })
    }

    /// Initialize and start the audio stream
    /// Returns an optional warning message if there are non-critical issues
    pub fn start_stream(&mut self) -> Result<Option<String>> {
        if self.input_stream.is_some() {
            return Ok(None); // Already running
        }

        // Create ring buffer for audio recording (sized for all input channels)
        let buffer_samples = SAMPLE_RATE as usize * RING_BUFFER_SECONDS * self.num_channels;
        let (producer, consumer) = rtrb::RingBuffer::new(buffer_samples);

        // Use the same device for output monitoring (ensures single clock domain)
        // Query for maximum output channels to support aggregate devices
        let output_config = get_max_channels_output_config(&self.device)?;

        let output_sample_rate = output_config.sample_rate();
        let output_channels = output_config.channels();

        // Create ring buffer for live monitoring (always stereo internally)
        // Keep buffer small for low latency (~50ms)
        // Buffer size = (sample_rate * channels * duration_ms) / 1000
        let monitor_buffer_samples = (output_sample_rate as usize * 2 * 50) / 1000; // 50ms stereo buffer
        let (monitor_producer, monitor_consumer) = rtrb::RingBuffer::new(monitor_buffer_samples);

        // Create file writer
        let file_writer = FileWriter::new(
            consumer,
            self.output_dir.clone(),
            self.config.sample_rate,
        );
        self.file_writer = Some(file_writer);

        // Create audio callback state
        let callback_state = AudioCallbackState {
            tracks: self.tracks.clone(),
            recording: self.recording.clone(),
            producer,
            monitor_producer,
        };

        // Build input audio stream
        let audio_callback = create_audio_callback(callback_state, self.num_channels);
        let error_callback = create_error_callback();

        let input_stream = self
            .device
            .build_input_stream(&self.config, audio_callback, error_callback, None)
            .context("Failed to build audio input stream")?;

        // Build output audio stream for monitoring (using same device as input)
        // Create explicit stream config with all output channels
        let output_stream_config = StreamConfig {
            channels: output_channels,
            sample_rate: output_sample_rate,
            buffer_size: cpal::BufferSize::Fixed(256),
        };

        // Determine monitor channel routing (default to channels 1-2)
        let monitor_start = self.monitor_channels.map(|(s, _)| s).unwrap_or(1);
        let monitor_end = self.monitor_channels.map(|(_, e)| e).unwrap_or(2);

        let output_callback = create_monitor_callback(
            monitor_consumer,
            output_channels as usize,
            monitor_start as usize,
            monitor_end as usize,
        );
        let output_error_callback = create_error_callback();

        let output_stream = self
            .device
            .build_output_stream(
                &output_stream_config,
                output_callback,
                output_error_callback,
                None,
            )
            .context("Failed to build audio output stream")?;

        // Start both streams
        input_stream.play().context("Failed to play input stream")?;
        output_stream
            .play()
            .context("Failed to play output stream")?;

        // Store both streams
        self.input_stream = Some(input_stream);
        self.output_stream = Some(output_stream);

        // Check for sample rate mismatch (can cause audio glitches)
        let warning = if self.config.sample_rate != output_sample_rate {
            Some(format!(
                "Sample rate mismatch: input {}Hz, output {}Hz. May cause choppy audio.",
                self.config.sample_rate, output_sample_rate
            ))
        } else {
            None
        };

        Ok(warning)
    }

    /// Stop the audio stream
    pub fn stop_stream(&mut self) -> Result<()> {
        if let Some(stream) = self.input_stream.take() {
            stream.pause().context("Failed to pause input stream")?;
            drop(stream);
        }

        if let Some(stream) = self.output_stream.take() {
            stream.pause().context("Failed to pause output stream")?;
            drop(stream);
        }

        Ok(())
    }

    /// Start recording
    pub fn start_recording(&mut self) -> Result<String> {
        if self.recording.load(Ordering::Relaxed) {
            anyhow::bail!("Already recording");
        }

        // Generate timestamp for this recording session
        let timestamp = generate_timestamp();

        // Collect armed track IDs (use track.id, not vector index)
        let armed_track_ids: Vec<usize> = self
            .tracks
            .iter()
            .filter(|track| track.is_armed())
            .map(|track| track.id)
            .collect();

        // Start file writer with timestamp (only for armed tracks)
        if let Some(file_writer) = &mut self.file_writer {
            file_writer.start(timestamp.clone(), armed_track_ids)?;
        }

        // Set recording flag (audio callback will start writing to ring buffer)
        self.recording.store(true, Ordering::Relaxed);

        // Mark armed tracks as recording
        for track in self.tracks.iter() {
            if track.is_armed() {
                track.set_recording(true);
            }
        }

        Ok(timestamp)
    }

    /// Stop recording
    pub fn stop_recording(&mut self) -> Result<()> {
        if !self.recording.load(Ordering::Relaxed) {
            return Ok(()); // Not recording
        }

        // Clear recording flag
        self.recording.store(false, Ordering::Relaxed);

        // Stop file writer (this will drain the ring buffer and finalize files)
        if let Some(file_writer) = &mut self.file_writer {
            file_writer.stop()?;
        }

        // Clear recording status on tracks
        for track in self.tracks.iter() {
            track.set_recording(false);
        }

        Ok(())
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    /// Get reference to tracks
    pub fn tracks(&self) -> &Arc<Vec<Track>> {
        &self.tracks
    }

    /// Get device info
    #[allow(dead_code)]
    pub fn device_name(&self) -> String {
        self.device
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| "Unknown".to_string())
    }

    /// Get sample rate
    #[allow(dead_code)]
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        // Ensure recording is stopped and files are finalized
        let _ = self.stop_recording();
        let _ = self.stop_stream();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_audio_engine_creation() {
        let output_dir = env::current_dir().unwrap().join("test_recordings");

        // This test may fail on systems without audio devices
        if let Ok(engine) = AudioEngine::new(output_dir) {
            println!("Audio engine created successfully");
            println!("Device: {}", engine.device_name());
            println!("Channels: {}", engine.num_channels());
            println!("Sample rate: {}", engine.sample_rate());
            // Should have one track per input channel
            assert_eq!(engine.tracks().len(), engine.num_channels());
        }
    }
}
