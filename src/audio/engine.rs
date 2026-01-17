use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::audio::callback::{
    create_audio_callback, create_error_callback, create_monitor_callback, AudioCallbackState,
};
use crate::audio::coreaudio_playback::{find_device_by_name, CoreAudioPlaybackStream};
use crate::audio::device::{get_default_input_device, get_max_channels_input_config, get_max_channels_output_config};
use crate::audio::mix_writer::MixWriter;
use crate::audio::playback::PlaybackTrack;
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

    /// CoreAudio playback stream (macOS - immediate stop control)
    coreaudio_playback_stream: Option<CoreAudioPlaybackStream>,

    /// File writer
    file_writer: Option<FileWriter>,

    /// Output directory for recordings
    output_dir: PathBuf,

    /// Monitor output channels (start, end) - 1-indexed
    /// If None, defaults to channels 1-2
    monitor_channels: Option<(u16, u16)>,

    /// Mix recording armed state
    mix_recording_armed: Arc<AtomicBool>,

    /// Mix recording file writer
    mix_writer: Option<MixWriter>,

    /// Mix recording is active
    mix_recording: Arc<AtomicBool>,

    /// Playback tracks for audio file playback
    playback_tracks: Arc<Vec<PlaybackTrack>>,

    /// Playback state flag (separate from recording)
    playing: Arc<AtomicBool>,
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
            coreaudio_playback_stream: None,
            file_writer: None,
            output_dir,
            monitor_channels: None,
            mix_recording_armed: Arc::new(AtomicBool::new(false)),
            mix_writer: None,
            mix_recording: Arc::new(AtomicBool::new(false)),
            playback_tracks: Arc::new(Vec::new()),
            playing: Arc::new(AtomicBool::new(false)),
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
            coreaudio_playback_stream: None,
            file_writer: None,
            output_dir,
            monitor_channels: None,
            mix_recording_armed: Arc::new(AtomicBool::new(false)),
            mix_writer: None,
            mix_recording: Arc::new(AtomicBool::new(false)),
            playback_tracks: Arc::new(Vec::new()),
            playing: Arc::new(AtomicBool::new(false)),
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
        // Keep buffer VERY small for low latency (~10ms)
        // Buffer size = (sample_rate * channels * duration_ms) / 1000
        let monitor_buffer_samples = (output_sample_rate as usize * 2 * 10) / 1000; // 10ms stereo buffer
        let (monitor_producer, monitor_consumer) = rtrb::RingBuffer::new(monitor_buffer_samples);

        // Create ring buffer for playback audio (separate stream for immediate stop control)
        let playback_buffer_samples = (output_sample_rate as usize * 2 * 10) / 1000; // 10ms stereo buffer
        let (playback_producer, playback_consumer) = rtrb::RingBuffer::new(playback_buffer_samples);

        // Create ring buffer for mix recording (stereo f32 samples)
        let mix_buffer_samples = SAMPLE_RATE as usize * RING_BUFFER_SECONDS * 2; // Stereo
        let (mix_recording_producer, mix_recording_consumer) = rtrb::RingBuffer::new(mix_buffer_samples);

        // Create file writer
        let file_writer = FileWriter::new(
            consumer,
            self.output_dir.clone(),
            self.config.sample_rate,
        );
        self.file_writer = Some(file_writer);

        // Create WAV writer for mix recording
        let mix_writer = MixWriter::new(
            mix_recording_consumer,
            self.output_dir.clone(),
            SAMPLE_RATE,
        );
        self.mix_writer = Some(mix_writer);

        // Create audio callback state
        let callback_state = AudioCallbackState {
            tracks: self.tracks.clone(),
            recording: self.recording.clone(),
            producer,
            monitor_producer,
            mix_recording_producer,
            mix_recording_armed: self.mix_recording_armed.clone(),
            playback_tracks: self.playback_tracks.clone(),
            playing: self.playing.clone(),
            playback_producer,
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
        // Use smallest possible buffer size for minimum latency
        let output_stream_config = StreamConfig {
            channels: output_channels,
            sample_rate: output_sample_rate,
            buffer_size: cpal::BufferSize::Fixed(64),
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

        // Create CoreAudio playback stream (macOS - provides immediate stop control)
        // Use very small buffer (64 frames) for minimal latency
        // Get the device name from cpal and find the corresponding CoreAudio device ID
        let device_name = self
            .device
            .description()
            .ok()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let device_id = find_device_by_name(&device_name);

        let coreaudio_stream = CoreAudioPlaybackStream::new(
            output_sample_rate as f64,
            64, // Very small buffer for immediate stop
            device_id,
            playback_consumer,
            output_channels as usize,
            monitor_start.saturating_sub(1) as usize, // Convert to 0-indexed
            monitor_end.saturating_sub(1) as usize,   // Convert to 0-indexed
        )
        .context("Failed to create CoreAudio playback stream")?;

        // Start all streams immediately (keep them running for zero-latency start/stop)
        input_stream.play().context("Failed to play input stream")?;
        output_stream
            .play()
            .context("Failed to play output stream")?;

        // Start CoreAudio playback stream immediately (will output silence until playing flag is set)
        let mut coreaudio_stream_started = coreaudio_stream;
        coreaudio_stream_started.start().context("Failed to start CoreAudio playback stream")?;

        // Store all streams
        self.input_stream = Some(input_stream);
        self.output_stream = Some(output_stream);
        self.coreaudio_playback_stream = Some(coreaudio_stream_started);

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

        // CoreAudio stream cleanup happens in Drop
        self.coreaudio_playback_stream = None;

        Ok(())
    }

    /// Start recording
    pub fn start_recording(&mut self) -> Result<String> {
        if self.recording.load(Ordering::Relaxed) {
            anyhow::bail!("Already recording");
        }

        // Wait for previous file writer threads to finish (if any)
        // This is where the blocking happens - better here than on Stop
        if let Some(file_writer) = &mut self.file_writer {
            file_writer.join()?;
        }

        if let Some(mix_writer) = &mut self.mix_writer {
            mix_writer.join()?;
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

        // Start mix writer if mix recording is armed
        if self.mix_recording_armed.load(Ordering::Relaxed) {
            if let Some(mix_writer) = &mut self.mix_writer {
                mix_writer.start(timestamp.clone())?;
                self.mix_recording.store(true, Ordering::Relaxed);
            }
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

    /// Stop recording immediately (non-blocking - signals writer threads to stop)
    pub fn stop_recording_async(&mut self) {
        if !self.recording.load(Ordering::Relaxed) {
            return; // Not recording
        }

        // Clear recording flag immediately (stops audio callback from writing more samples)
        self.recording.store(false, Ordering::Relaxed);

        // Clear mix recording flag
        self.mix_recording.store(false, Ordering::Relaxed);

        // Clear recording status on tracks
        for track in self.tracks.iter() {
            track.set_recording(false);
        }

        // Signal file writers to stop (non-blocking - just sets running flag to false)
        // Threads will drain buffers in background, we'll join them on next start
        if let Some(file_writer) = &mut self.file_writer {
            file_writer.stop_async();
        }

        if let Some(mix_writer) = &mut self.mix_writer {
            mix_writer.stop_async();
        }
    }

    /// Stop recording (blocking - drains buffers and finalizes files)
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

        // Stop mix writer if it was recording
        if self.mix_recording.load(Ordering::Relaxed) {
            if let Some(mix_writer) = &mut self.mix_writer {
                mix_writer.stop()?;
                self.mix_recording.store(false, Ordering::Relaxed);
            }
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

    /// Check if mix recording is armed
    pub fn is_mix_recording_armed(&self) -> bool {
        self.mix_recording_armed.load(Ordering::Relaxed)
    }

    /// Set mix recording armed state
    pub fn set_mix_recording_armed(&mut self, armed: bool) {
        self.mix_recording_armed.store(armed, Ordering::Relaxed);
    }

    /// Check if mix is currently recording
    pub fn is_mix_recording(&self) -> bool {
        self.mix_recording.load(Ordering::Relaxed)
    }

    /// Set playback tracks
    pub fn set_playback_tracks(&mut self, tracks: Vec<PlaybackTrack>) {
        self.playback_tracks = Arc::new(tracks);
    }

    /// Get reference to playback tracks
    pub fn playback_tracks(&self) -> &Arc<Vec<PlaybackTrack>> {
        &self.playback_tracks
    }

    /// Start playback
    pub fn start_playback(&mut self) -> Result<()> {
        // Reset all playback positions to 0
        for track in self.playback_tracks.iter() {
            track.reset();
        }

        // Set playing flag - audio callback will start mixing playback immediately
        self.playing.store(true, Ordering::Relaxed);

        Ok(())
    }

    /// Stop playback
    pub fn stop_playback(&mut self) -> Result<()> {
        // Clear playing flag immediately - audio callback stops mixing on next call (~1ms latency)
        self.playing.store(false, Ordering::Relaxed);

        // Drain the ring buffer to clear any queued audio (non-blocking, ring buffer is small)
        if let Some(ref mut stream) = self.coreaudio_playback_stream {
            stream.drain_buffer();
        }

        // Reset playback positions
        for track in self.playback_tracks.iter() {
            track.reset();
        }

        Ok(())
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
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
            println!("Channels: {}", engine.num_channels);
            println!("Sample rate: {}", engine.sample_rate());
            // Should have one track per input channel
            assert_eq!(engine.tracks().len(), engine.num_channels);
        }
    }
}
