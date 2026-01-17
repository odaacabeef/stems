use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use rtrb::Producer;
use crate::audio::playback::PlaybackTrack;
use crate::audio::track::Track;

/// Sample data sent to file writer
#[derive(Debug, Clone, Copy)]
pub struct RecordedSample {
    pub track_id: usize,
    pub sample: f32,
}

/// Audio callback state shared between setup and callback
pub struct AudioCallbackState {
    pub tracks: Arc<Vec<Track>>,
    pub recording: Arc<AtomicBool>,
    pub producer: Producer<RecordedSample>,
    pub monitor_producer: Producer<f32>,
    pub mix_recording_producer: Producer<f32>,
    pub mix_recording_armed: Arc<AtomicBool>,
    pub playback_tracks: Arc<Vec<PlaybackTrack>>,
    pub playing: Arc<AtomicBool>,
    pub playback_producer: Producer<f32>,  // Separate producer for playback audio
}

/// Process audio input in real-time
///
/// CRITICAL: This function runs in a real-time audio thread with strict constraints:
/// - NO memory allocations
/// - NO mutex locks (use atomics only)
/// - NO I/O operations
/// - NO blocking calls
/// - Processing must complete within buffer duration
pub fn process_audio_input(
    input_data: &[f32],
    tracks: &[Track],
    recording: &AtomicBool,
    producer: &mut Producer<RecordedSample>,
    monitor_producer: &mut Producer<f32>,
    mix_recording_producer: &mut Producer<f32>,
    mix_recording_armed: &AtomicBool,
    num_input_channels: usize,
    playback_tracks: &[PlaybackTrack],
    playing: &AtomicBool,
    playback_producer: &mut Producer<f32>,
) {
    let num_frames = input_data.len() / num_input_channels;
    let is_recording = recording.load(Ordering::Relaxed);
    let is_playing = playing.load(Ordering::Relaxed);

    // Check if any track has solo enabled (once per buffer for performance)
    let any_solo = tracks.iter().any(|t| t.is_solo());
    let any_playback_solo = playback_tracks.iter().any(|t| t.is_solo());
    let any_solo_overall = any_solo || any_playback_solo;

    // Track peak levels for playback tracks across the buffer
    // Use None to indicate track was not processed (not monitoring)
    let mut playback_peaks: Vec<Option<f32>> = vec![None; playback_tracks.len()];

    // Process each frame
    for frame_idx in 0..num_frames {
        let mut monitor_left = 0.0f32;
        let mut monitor_right = 0.0f32;

        // Process each track
        for track in tracks {
            // Get the input channel for this track
            let input_channel = track.input_channel;

            // Bounds check (safety)
            if input_channel >= num_input_channels {
                continue;
            }

            // De-interleave: get sample for this track's input channel
            let sample_idx = frame_idx * num_input_channels + input_channel;
            let input_sample = input_data[sample_idx];

            // Apply level control
            let level = track.get_level();
            let pan = track.get_pan();
            let processed_sample = input_sample * level;

            // Update peak meter (simple peak detection)
            let abs_sample = processed_sample.abs();
            let current_peak = track.get_peak_level();
            if abs_sample > current_peak {
                track.update_peak_level(abs_sample);
            }

            // If recording AND track is armed, push sample to ring buffer (non-blocking)
            if is_recording && track.is_armed() {
                let recorded_sample = RecordedSample {
                    track_id: track.id,
                    sample: processed_sample,
                };

                let _ = producer.push(recorded_sample);
            }

            // Mix into monitor output if monitoring is enabled
            // Solo logic: if any track is soloed, only monitor soloed tracks
            // Otherwise, monitor according to monitoring flag
            let should_monitor = if any_solo_overall {
                track.is_solo()
            } else {
                track.is_monitoring()
            };

            if should_monitor {
                // Apply panning: -1.0 = full left, 0.0 = center, +1.0 = full right
                // Constant power panning
                let pan_angle = (pan + 1.0) * 0.25 * std::f32::consts::PI; // Map -1..1 to 0..PI/2
                let left_gain = pan_angle.cos();
                let right_gain = pan_angle.sin();

                monitor_left += processed_sample * left_gain;
                monitor_right += processed_sample * right_gain;
            }
        }

        // Process playback tracks into separate playback stream
        let mut playback_left = 0.0f32;
        let mut playback_right = 0.0f32;

        if is_playing {
            for (track_idx, playback_track) in playback_tracks.iter().enumerate() {
                // Solo logic: if any solo enabled (input or playback), only monitor soloed tracks
                let should_monitor = if any_solo_overall {
                    playback_track.is_solo()
                } else {
                    playback_track.is_monitoring()
                };

                if !should_monitor {
                    continue;
                }

                // Get current playback position (frame index)
                let base_position = playback_track.get_position();
                let num_frames_total = playback_track.num_frames();

                // Skip if we've somehow gone past the end (shouldn't happen but be safe)
                if num_frames_total == 0 {
                    continue;
                }

                // Calculate position for this specific frame in the buffer
                let current_position = (base_position + frame_idx) % num_frames_total;

                // Read sample(s) from playback buffer
                let (left_sample, right_sample) = if playback_track.channels == 1 {
                    // Mono: duplicate to both channels
                    let sample = playback_track.samples[current_position];
                    (sample, sample)
                } else {
                    // Stereo: read both channels
                    let sample_idx = current_position * 2;
                    let left = playback_track.samples[sample_idx];
                    let right = playback_track.samples[sample_idx + 1];
                    (left, right)
                };

                // Apply level
                let level = playback_track.get_level();
                let left_sample = left_sample * level;
                let right_sample = right_sample * level;

                // Apply panning (equal power law)
                let pan = playback_track.get_pan();
                let pan_angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
                let left_gain = pan_angle.cos();
                let right_gain = pan_angle.sin();

                let panned_left = left_sample * left_gain;
                let panned_right = right_sample * right_gain;

                playback_left += panned_left;
                playback_right += panned_right;

                // Track peak level across buffer
                let peak = panned_left.abs().max(panned_right.abs());
                playback_peaks[track_idx] = Some(match playback_peaks[track_idx] {
                    Some(current_max) => current_max.max(peak),
                    None => peak,
                });
            }
        }

        // Send playback audio to separate playback stream
        let _ = playback_producer.push(playback_left);
        let _ = playback_producer.push(playback_right);

        // Combine input tracks and playback for monitor output
        let mixed_left = monitor_left + playback_left;
        let mixed_right = monitor_right + playback_right;

        // Send combined output to monitor (stereo)
        let _ = monitor_producer.push(mixed_left);
        let _ = monitor_producer.push(mixed_right);

        // If recording and mix recording is armed, send to mix recording buffer
        if is_recording && mix_recording_armed.load(Ordering::Relaxed) {
            let _ = mix_recording_producer.push(mixed_left);
            let _ = mix_recording_producer.push(mixed_right);
        }
    }

    // Increment playback positions after processing all frames (with looping)
    if is_playing {
        for playback_track in playback_tracks {
            let position = playback_track.get_position();
            let num_frames_total = playback_track.num_frames();

            if num_frames_total > 0 {
                let new_position = (position + num_frames) % num_frames_total;
                playback_track.set_position(new_position);
            }
        }
    }

    // Update peak meters for playback tracks with buffer maximum
    // Only update if track was actually processing audio (Some value)
    for (i, playback_track) in playback_tracks.iter().enumerate() {
        if let Some(peak) = playback_peaks[i] {
            let current_peak = playback_track.get_peak_level();
            if peak > current_peak {
                playback_track.update_peak_level(peak);
            }
        }
    }
}

/// Create the audio callback closure
///
/// This returns a closure that will be called by cpal for each audio buffer
pub fn create_audio_callback(
    mut state: AudioCallbackState,
    num_input_channels: usize,
) -> impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static {
    move |data: &[f32], _info: &cpal::InputCallbackInfo| {
        process_audio_input(
            data,
            &state.tracks,
            &state.recording,
            &mut state.producer,
            &mut state.monitor_producer,
            &mut state.mix_recording_producer,
            &state.mix_recording_armed,
            num_input_channels,
            &state.playback_tracks,
            &state.playing,
            &mut state.playback_producer,
        );
    }
}

/// Create the monitor output callback closure
///
/// This reads from the monitor ring buffer and plays it through specific output channels
///
/// # Arguments
/// * `consumer` - Ring buffer consumer with stereo monitor audio
/// * `total_channels` - Total number of output channels in the device
/// * `monitor_start` - Start channel for monitoring (1-indexed, e.g., 17)
/// * `monitor_end` - End channel for monitoring (1-indexed, e.g., 18)
pub fn create_monitor_callback(
    mut consumer: rtrb::Consumer<f32>,
    total_channels: usize,
    monitor_start: usize,
    monitor_end: usize,
) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
    // Convert 1-indexed channels to 0-indexed
    let start_idx = monitor_start.saturating_sub(1);
    let end_idx = monitor_end.saturating_sub(1);

    move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
        // Calculate number of frames
        let num_frames = data.len() / total_channels;

        // Process each frame
        for frame_idx in 0..num_frames {
            let frame_start = frame_idx * total_channels;

            // Fill all channels in this frame with silence
            for ch in 0..total_channels {
                data[frame_start + ch] = 0.0;
            }

            // Pop stereo samples from ring buffer and place in monitor channels
            if start_idx < total_channels && end_idx < total_channels {
                let left_sample = consumer.pop().unwrap_or(0.0);
                let right_sample = consumer.pop().unwrap_or(0.0);

                data[frame_start + start_idx] = left_sample;
                data[frame_start + end_idx] = right_sample;
            }
        }
    }
}

/// Error callback for audio stream
pub fn create_error_callback() -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |_err| {
        // Silently handle audio stream errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_audio_processing_not_recording() {
        let tracks = vec![Track::new(0, 0)];
        tracks[0].set_armed(true);

        let recording = Arc::new(AtomicBool::new(false));
        let (mut producer, _consumer) = rtrb::RingBuffer::new(1024);
        let (mut monitor_producer, _monitor_consumer) = rtrb::RingBuffer::new(1024);

        let input_data = vec![0.5f32; 128]; // 128 samples, mono

        let (mut mix_recording_producer, _mix_recording_consumer) = rtrb::RingBuffer::new(1024);
        let mix_recording_armed = Arc::new(AtomicBool::new(false));

        let playback_tracks: Vec<PlaybackTrack> = vec![];
        let playing = Arc::new(AtomicBool::new(false));
        let (mut playback_producer, _playback_consumer) = rtrb::RingBuffer::new(1024);

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            &mut mix_recording_producer,
            &mix_recording_armed,
            1, // mono
            &playback_tracks,
            &playing,
            &mut playback_producer,
        );

        // Should not have written anything to recording buffer
        assert_eq!(producer.slots(), 1024);
    }

    #[test]
    fn test_audio_processing_armed_track() {
        let tracks = vec![Track::new(0, 0)];
        tracks[0].set_armed(true);
        tracks[0].set_level(0.5);

        let recording = Arc::new(AtomicBool::new(true));
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(1024);
        let (mut monitor_producer, _monitor_consumer) = rtrb::RingBuffer::new(1024);

        let input_data = vec![1.0f32; 16]; // 16 samples, mono

        let (mut mix_recording_producer, _mix_recording_consumer) = rtrb::RingBuffer::new(1024);
        let mix_recording_armed = Arc::new(AtomicBool::new(false));

        let playback_tracks: Vec<PlaybackTrack> = vec![];
        let playing = Arc::new(AtomicBool::new(false));
        let (mut playback_producer, _playback_consumer) = rtrb::RingBuffer::new(1024);

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            &mut mix_recording_producer,
            &mix_recording_armed,
            1, // mono
            &playback_tracks,
            &playing,
            &mut playback_producer,
        );

        // Should have written 16 samples
        assert_eq!(consumer.slots(), 16);

        // Check first sample
        if let Ok(sample) = consumer.pop() {
            assert_eq!(sample.track_id, 0);
            assert!((sample.sample - 0.5).abs() < 0.001); // 1.0 * 0.5 level
        }
    }

    #[test]
    fn test_peak_meter_update() {
        let tracks = vec![Track::new(0, 0)];
        tracks[0].set_armed(true);

        let recording = Arc::new(AtomicBool::new(true));
        let (mut producer, _consumer) = rtrb::RingBuffer::new(1024);
        let (mut monitor_producer, _monitor_consumer) = rtrb::RingBuffer::new(1024);

        let input_data = vec![0.8f32; 16]; // 16 samples at 0.8 amplitude

        let (mut mix_recording_producer, _mix_recording_consumer) = rtrb::RingBuffer::new(1024);
        let mix_recording_armed = Arc::new(AtomicBool::new(false));

        let playback_tracks: Vec<PlaybackTrack> = vec![];
        let playing = Arc::new(AtomicBool::new(false));
        let (mut playback_producer, _playback_consumer) = rtrb::RingBuffer::new(1024);

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            &mut mix_recording_producer,
            &mix_recording_armed,
            1, // mono
            &playback_tracks,
            &playing,
            &mut playback_producer,
        );

        // Peak should be updated to 0.8 (with level=1.0)
        let peak = tracks[0].get_peak_level();
        assert!((peak - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_multi_channel_input() {
        let tracks = vec![
            Track::new(0, 0), // Track 0 reads input channel 0
            Track::new(1, 1), // Track 1 reads input channel 1
        ];
        tracks[0].set_armed(true);
        tracks[1].set_armed(true);

        let recording = Arc::new(AtomicBool::new(true));
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(1024);
        let (mut monitor_producer, _monitor_consumer) = rtrb::RingBuffer::new(1024);

        // Stereo input: [L0, R0, L1, R1, L2, R2, L3, R3]
        // Left channel = 0.5, Right channel = 0.8
        let input_data = vec![0.5, 0.8, 0.5, 0.8, 0.5, 0.8, 0.5, 0.8];

        let (mut mix_recording_producer, _mix_recording_consumer) = rtrb::RingBuffer::new(1024);
        let mix_recording_armed = Arc::new(AtomicBool::new(false));

        let playback_tracks: Vec<PlaybackTrack> = vec![];
        let playing = Arc::new(AtomicBool::new(false));
        let (mut playback_producer, _playback_consumer) = rtrb::RingBuffer::new(1024);

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            &mut mix_recording_producer,
            &mix_recording_armed,
            2, // stereo
            &playback_tracks,
            &playing,
            &mut playback_producer,
        );

        // Should have 8 samples total (4 frames * 2 tracks)
        assert_eq!(consumer.slots(), 8);

        // Check that tracks got correct channel data
        let mut track0_samples = Vec::new();
        let mut track1_samples = Vec::new();

        while let Ok(sample) = consumer.pop() {
            if sample.track_id == 0 {
                track0_samples.push(sample.sample);
            } else {
                track1_samples.push(sample.sample);
            }
        }

        // Track 0 should have left channel (0.5)
        assert_eq!(track0_samples.len(), 4);
        for sample in track0_samples {
            assert!((sample - 0.5).abs() < 0.001);
        }

        // Track 1 should have right channel (0.8)
        assert_eq!(track1_samples.len(), 4);
        for sample in track1_samples {
            assert!((sample - 0.8).abs() < 0.001);
        }
    }
}
