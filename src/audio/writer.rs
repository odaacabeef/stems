use anyhow::{Context, Result};
use chrono::Local;
use hound::{WavSpec, WavWriter};
use rtrb::Consumer;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::callback::RecordedSample;

/// File writer that reads from ring buffer and writes to WAV files
pub struct FileWriter {
    consumer: Option<Consumer<RecordedSample>>,
    output_dir: PathBuf,
    sample_rate: u32,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<Result<Consumer<RecordedSample>>>>,
}

impl FileWriter {
    /// Create a new file writer
    pub fn new(
        consumer: Consumer<RecordedSample>,
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

    /// Start the file writer thread
    pub fn start(&mut self, timestamp: String, armed_track_ids: Vec<usize>) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            anyhow::bail!("File writer already running");
        }

        let consumer = self
            .consumer
            .take()
            .ok_or_else(|| anyhow::anyhow!("FileWriter already started"))?;

        self.running.store(true, Ordering::Relaxed);

        let output_dir = self.output_dir.clone();
        let sample_rate = self.sample_rate;
        let running = self.running.clone();

        let handle = thread::spawn(move || {
            run_file_writer(
                consumer,
                &output_dir,
                sample_rate,
                &running,
                &timestamp,
                armed_track_ids,
            )
        });

        self.thread_handle = Some(handle);

        Ok(())
    }

    /// Stop the file writer thread and wait for it to finish
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.thread_handle.take() {
            let consumer = handle
                .join()
                .map_err(|_| anyhow::anyhow!("File writer thread panicked"))??;

            // Restore the consumer so we can start recording again
            self.consumer = Some(consumer);
        }

        Ok(())
    }

    /// Check if the writer is running
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// File writer main loop
fn run_file_writer(
    mut consumer: Consumer<RecordedSample>,
    output_dir: &PathBuf,
    sample_rate: u32,
    running: &AtomicBool,
    timestamp: &str,
    armed_track_ids: Vec<usize>,
) -> Result<Consumer<RecordedSample>> {
    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .context("Failed to create output directory")?;

    // WAV specification: 32-bit float, mono, 48kHz (or configured rate)
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    // Create WAV writers only for armed tracks
    let mut writers: HashMap<usize, WavWriter<BufWriter<File>>> = HashMap::new();

    for &track_id in &armed_track_ids {
        let filename = format!("{:02}-{}.wav", track_id + 1, timestamp);
        let filepath = output_dir.join(&filename);

        let writer = WavWriter::create(&filepath, spec)
            .with_context(|| format!("Failed to create WAV file: {}", filepath.display()))?;

        writers.insert(track_id, writer);
    }

    // Track when to flush
    let mut last_flush = Instant::now();
    let flush_interval = Duration::from_secs(2);

    // Main write loop
    while running.load(Ordering::Relaxed) {
        // Read available samples from ring buffer
        let mut samples_written = 0;

        while let Ok(sample) = consumer.pop() {
            if let Some(writer) = writers.get_mut(&sample.track_id) {
                writer
                    .write_sample(sample.sample)
                    .with_context(|| format!("Failed to write sample for track {}", sample.track_id))?;
                samples_written += 1;
            }
        }

        // Periodically flush to disk for crash safety
        if last_flush.elapsed() > flush_interval {
            for writer in writers.values_mut() {
                writer.flush().context("Failed to flush WAV file")?;
            }
            last_flush = Instant::now();
        }

        // Sleep briefly if no samples were available
        if samples_written == 0 {
            thread::sleep(Duration::from_millis(1));
        }
    }

    // Drain any remaining samples
    while let Ok(sample) = consumer.pop() {
        if let Some(writer) = writers.get_mut(&sample.track_id) {
            let _ = writer.write_sample(sample.sample);
        }
    }

    // Finalize and close all writers
    for (track_id, writer) in writers.into_iter() {
        writer
            .finalize()
            .with_context(|| format!("Failed to finalize WAV file for track {}", track_id))?;
    }

    // Return the consumer so it can be reused
    Ok(consumer)
}

/// Generate a timestamp for file naming
pub fn generate_timestamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SAMPLE_RATE;

    #[test]
    fn test_timestamp_format() {
        let timestamp = generate_timestamp();
        assert_eq!(timestamp.len(), 15); // YYYYMMDD-HHMMSS
        assert!(timestamp.contains('-'));
    }

    #[test]
    fn test_file_writer_creation() {
        let (producer, consumer) = rtrb::RingBuffer::new(1024);
        let output_dir = PathBuf::from("./test_recordings");

        let writer = FileWriter::new(consumer, output_dir, SAMPLE_RATE);

        assert!(!writer.is_running());
        drop(producer); // Prevent unused variable warning
    }
}
