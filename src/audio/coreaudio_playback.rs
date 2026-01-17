/// CoreAudio-based playback stream for macOS via FFI
///
/// This module uses CoreAudio directly via C/Objective-C FFI to achieve low-latency playback
/// with immediate stop capability. Unlike cpal, CoreAudio gives us direct control over buffer
/// sizes and allows immediate stream termination.

use anyhow::Result;
use std::ffi::c_void;

#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::c_void;

    // FFI bindings to our C/Objective-C CoreAudio wrapper
    #[repr(C)]
    pub struct CAPlaybackEngine {
        _private: [u8; 0],
    }

    pub type CAPlaybackCallback = extern "C" fn(
        user_data: *mut c_void,
        buffer: *mut f32,
        num_frames: u32,
        num_channels: u32,
    );

    extern "C" {
        pub fn ca_playback_create(
            sample_rate: f64,
            num_channels: u32,
            device_id: u32,
            callback: CAPlaybackCallback,
            user_data: *mut c_void,
        ) -> *mut CAPlaybackEngine;

        pub fn ca_playback_start(engine: *mut CAPlaybackEngine) -> bool;
        pub fn ca_playback_destroy(engine: *mut CAPlaybackEngine);
        pub fn ca_find_device_by_name(device_name: *const std::os::raw::c_char) -> u32;
    }
}

/// Find a CoreAudio device by name
///
/// Returns the AudioDeviceID if found, 0 otherwise
#[cfg(target_os = "macos")]
pub fn find_device_by_name(device_name: &str) -> u32 {
    use std::ffi::CString;

    let c_name = match CString::new(device_name) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    unsafe { ffi::ca_find_device_by_name(c_name.as_ptr()) }
}

#[cfg(not(target_os = "macos"))]
pub fn find_device_by_name(_device_name: &str) -> u32 {
    0
}

/// CoreAudio playback stream handle
#[cfg(target_os = "macos")]
pub struct CoreAudioPlaybackStream {
    engine: *mut ffi::CAPlaybackEngine,
    _consumer: Box<rtrb::Consumer<f32>>, // Keep consumer alive
    _callback_data: Box<CallbackData>,   // Keep callback data alive
}

#[cfg(target_os = "macos")]
struct CallbackData {
    consumer: *mut rtrb::Consumer<f32>,
    output_channels: usize,
    target_left: usize,
    target_right: usize,
}

#[cfg(target_os = "macos")]
extern "C" fn audio_callback(
    user_data: *mut c_void,
    buffer: *mut f32,
    num_frames: u32,
    num_channels: u32,
) {
    unsafe {
        let data = &mut *(user_data as *mut CallbackData);
        let consumer = &mut *data.consumer;

        // Clear all channels first
        let buffer_len = (num_frames * num_channels) as usize;
        for i in 0..buffer_len {
            *buffer.add(i) = 0.0;
        }

        // Fill target channels with stereo playback (interleaved format)
        if data.target_left < data.output_channels && data.target_right < data.output_channels {
            for frame_idx in 0..num_frames as usize {
                let left_sample = consumer.pop().unwrap_or(0.0);
                let right_sample = consumer.pop().unwrap_or(0.0);

                let base_idx = frame_idx * data.output_channels;
                *buffer.add(base_idx + data.target_left) = left_sample;
                *buffer.add(base_idx + data.target_right) = right_sample;
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl CoreAudioPlaybackStream {
    /// Create a new CoreAudio playback stream
    ///
    /// # Arguments
    /// * `sample_rate` - Sample rate in Hz (e.g., 48000)
    /// * `_buffer_frames` - Buffer size in frames (ignored, CoreAudio manages this)
    /// * `device_id` - CoreAudio AudioDeviceID (0 for default)
    /// * `consumer` - Ring buffer consumer to read playback audio from
    /// * `output_channels` - Total number of device output channels
    /// * `target_left` - Target left channel index (0-based)
    /// * `target_right` - Target right channel index (0-based)
    pub fn new(
        sample_rate: f64,
        _buffer_frames: u32,
        device_id: u32,
        consumer: rtrb::Consumer<f32>,
        output_channels: usize,
        target_left: usize,
        target_right: usize,
    ) -> Result<Self> {
        let mut consumer_box = Box::new(consumer);
        let consumer_ptr = &mut *consumer_box as *mut rtrb::Consumer<f32>;

        let mut callback_data = Box::new(CallbackData {
            consumer: consumer_ptr,
            output_channels,
            target_left,
            target_right,
        });

        let user_data = &mut *callback_data as *mut CallbackData as *mut c_void;

        let engine = unsafe {
            ffi::ca_playback_create(
                sample_rate,
                output_channels as u32,
                device_id,
                audio_callback,
                user_data,
            )
        };

        if engine.is_null() {
            anyhow::bail!("Failed to create CoreAudio playback engine");
        }

        Ok(Self {
            engine,
            _consumer: consumer_box,
            _callback_data: callback_data,
        })
    }

    /// Start the playback stream
    pub fn start(&mut self) -> Result<()> {
        let success = unsafe { ffi::ca_playback_start(self.engine) };
        if success {
            Ok(())
        } else {
            anyhow::bail!("Failed to start CoreAudio playback stream")
        }
    }

    /// Drain the ring buffer without stopping the audio unit (non-blocking)
    pub fn drain_buffer(&mut self) {
        unsafe {
            let consumer_ptr = self._callback_data.consumer;
            if let Some(consumer) = consumer_ptr.as_mut() {
                // Drain all samples from the buffer
                while consumer.pop().is_ok() {}
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for CoreAudioPlaybackStream {
    fn drop(&mut self) {
        unsafe {
            ffi::ca_playback_destroy(self.engine);
        }
    }
}

#[cfg(target_os = "macos")]
unsafe impl Send for CoreAudioPlaybackStream {}

// Stub implementation for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct CoreAudioPlaybackStream;

#[cfg(not(target_os = "macos"))]
impl CoreAudioPlaybackStream {
    pub fn new(
        _sample_rate: f64,
        _buffer_frames: u32,
        _device_id: u32,
        _consumer: rtrb::Consumer<f32>,
        _output_channels: usize,
        _target_left: usize,
        _target_right: usize,
    ) -> Result<Self> {
        anyhow::bail!("CoreAudio playback is only available on macOS")
    }

    pub fn start(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn drain_buffer(&mut self) {
        // No-op on non-macOS
    }
}
