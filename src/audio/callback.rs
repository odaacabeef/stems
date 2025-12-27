use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use rtrb::Producer;
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
    num_input_channels: usize,
) {
    let num_frames = input_data.len() / num_input_channels;
    let is_recording = recording.load(Ordering::Relaxed);

    // Process each frame
    for frame_idx in 0..num_frames {
        let mut monitor_mix = 0.0f32;
        let mut track_count = 0;

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
            let _pan = track.get_pan();
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
            if track.is_monitoring() {
                monitor_mix += processed_sample;
                track_count += 1;
            }
        }

        // Send mixed output to monitor (stereo - same signal to both channels)
        if track_count > 0 {
            let mixed_sample = monitor_mix / track_count as f32;
            let _ = monitor_producer.push(mixed_sample); // Left
            let _ = monitor_producer.push(mixed_sample); // Right
        } else {
            // Silence if no tracks
            let _ = monitor_producer.push(0.0);
            let _ = monitor_producer.push(0.0);
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
            num_input_channels,
        );
    }
}

/// Create the monitor output callback closure
///
/// This reads from the monitor ring buffer and plays it through speakers
pub fn create_monitor_callback(
    mut consumer: rtrb::Consumer<f32>,
) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
    move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
        // Fill output buffer with samples from ring buffer
        for sample in data.iter_mut() {
            *sample = consumer.pop().unwrap_or(0.0);
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

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            1, // mono
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

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            1, // mono
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

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            1, // mono
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

        process_audio_input(
            &input_data,
            &tracks,
            &recording,
            &mut producer,
            &mut monitor_producer,
            2, // stereo
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
