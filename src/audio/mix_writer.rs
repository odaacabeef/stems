use anyhow::{Context, Result};
use hound::{WavSpec, WavWriter};
use rtrb::Consumer;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Mix writer that reads stereo f32 samples from ring buffer and writes to WAV
pub struct MixWriter {
    consumer: Option<Consumer<f32>>,
    output_dir: PathBuf,
    sample_rate: u32,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<Result<Consumer<f32>>>>,
}

impl MixWriter {
    /// Create a new mix writer
    pub fn new(
        consumer: Consumer<f32>,
        output_dir: PathBuf,
        sample_rate: u32,
    ) -> Self {
        Self {
            consumer: Some(consumer),
            output_dir,
            sample_rate,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start the mix writer thread
    pub fn start(&mut self, timestamp: String) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            anyhow::bail!("Mix writer already running");
        }

        let consumer = self
            .consumer
            .take()
            .ok_or_else(|| anyhow::anyhow!("MixWriter already started"))?;

        self.running.store(true, Ordering::Relaxed);

        let output_dir = self.output_dir.clone();
        let sample_rate = self.sample_rate;
        let running = self.running.clone();

        let handle = thread::spawn(move || {
            run_mix_writer(
                consumer,
                &output_dir,
                sample_rate,
                &running,
                &timestamp,
            )
        });

        self.thread_handle = Some(handle);

        Ok(())
    }

    /// Signal the writer thread to stop (non-blocking - just sets flag)
    pub fn stop_async(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Wait for the writer thread to finish and restore consumer (blocking)
    pub fn join(&mut self) -> Result<()> {
        if let Some(handle) = self.thread_handle.take() {
            let consumer = handle
                .join()
                .map_err(|_| anyhow::anyhow!("Mix writer thread panicked"))??;

            // Restore the consumer so we can start recording again
            self.consumer = Some(consumer);
        }

        Ok(())
    }

    /// Stop the mix writer thread and wait for it to finish
    pub fn stop(&mut self) -> Result<()> {
        self.stop_async();
        self.join()
    }

    /// Check if the writer is running
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Mix writer main loop
fn run_mix_writer(
    mut consumer: Consumer<f32>,
    output_dir: &PathBuf,
    sample_rate: u32,
    running: &AtomicBool,
    timestamp: &str,
) -> Result<Consumer<f32>> {
    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .context("Failed to create output directory")?;

    // WAV specification: 32-bit float, stereo, 48kHz (or configured rate)
    let spec = WavSpec {
        channels: 2,          // Stereo
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    // Create WAV writer for mix
    let filename = format!("mix-{}.wav", timestamp);
    let filepath = output_dir.join(&filename);

    let mut writer = WavWriter::create(&filepath, spec)
        .with_context(|| format!("Failed to create WAV file: {}", filepath.display()))?;

    // Track when to flush
    let mut last_flush = Instant::now();
    let flush_interval = Duration::from_secs(2);

    // Main write loop
    while running.load(Ordering::Relaxed) {
        // Read available samples from ring buffer (interleaved stereo)
        let mut samples_written = 0;

        while let Ok(sample) = consumer.pop() {
            writer
                .write_sample(sample)
                .context("Failed to write sample to mix WAV file")?;
            samples_written += 1;
        }

        // Periodically flush to disk for crash safety
        if last_flush.elapsed() > flush_interval {
            writer.flush().context("Failed to flush mix WAV file")?;
            last_flush = Instant::now();
        }

        // Sleep briefly if no samples were available
        if samples_written == 0 {
            thread::sleep(Duration::from_millis(1));
        }
    }

    // Drain any remaining samples
    while let Ok(sample) = consumer.pop() {
        let _ = writer.write_sample(sample);
    }

    // Finalize and close writer
    writer
        .finalize()
        .context("Failed to finalize mix WAV file")?;

    // Return the consumer so it can be reused
    Ok(consumer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SAMPLE_RATE;

    #[test]
    fn test_mix_writer_creation() {
        let (producer, consumer) = rtrb::RingBuffer::new(1024);
        let output_dir = PathBuf::from("./test_recordings");

        let writer = MixWriter::new(consumer, output_dir, SAMPLE_RATE);

        assert!(!writer.is_running());
        drop(producer); // Prevent unused variable warning
    }
}
