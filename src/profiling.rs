//! Opt-in server-side measurements. These do not measure client presentation.
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Latency(Mutex<VecDeque<u64>>);
impl Latency {
    pub fn observe(&self, duration: Duration) {
        let mut samples = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if samples.len() == 256 {
            samples.pop_front();
        }
        samples.push_back(duration.as_micros().min(u64::MAX as u128) as u64);
    }
    pub fn p95(&self) -> u64 {
        let mut samples: Vec<_> = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .copied()
            .collect();
        samples.sort_unstable();
        samples
            .get((samples.len() * 95).div_ceil(100).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Default)]
pub struct EncoderMetrics {
    pub requests: AtomicU64,
    pub pending_replaced: AtomicU64,
    pub encoded: AtomicU64,
    pub failures: AtomicU64,
    pub responses_replaced: AtomicU64,
    pub stale: AtomicU64,
    pub encode: Latency,
    pub queue_wait: Latency,
    pub request_to_backend: Latency,
    /// Sixel palette-training ceiling; this no longer reduces spatial detail.
    pub work_pixel_budget: AtomicU64,
}

pub struct CountingWriter<W> {
    inner: W,
    pub bytes: Arc<AtomicU64>,
    enabled: bool,
}
impl<W> CountingWriter<W> {
    pub fn new(inner: W, enabled: bool) -> Self {
        Self {
            inner,
            bytes: Arc::new(AtomicU64::new(0)),
            enabled,
        }
    }
}
impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        if self.enabled {
            self.bytes.fetch_add(written as u64, Ordering::Relaxed);
        }
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub struct Recorder {
    file: File,
    started: Instant,
    last_report: Instant,
    pub encoder: Arc<EncoderMetrics>,
    draw: Latency,
    draws: u64,
    unique_frames: u64,
    last_frame: Option<(u64, u64)>,
}
impl Recorder {
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: OpenOptions::new().write(true).create_new(true).open(path)?,
            started: Instant::now(),
            last_report: Instant::now() - Duration::from_secs(1),
            encoder: Arc::default(),
            draw: Latency::default(),
            draws: 0,
            unique_frames: 0,
            last_frame: None,
        })
    }
    pub fn record(
        &mut self,
        decode: crate::vlc::DecodeMetrics,
        bytes: u64,
        frame: Option<(u64, u64)>,
        draw_time: Duration,
        playback: &crate::vlc::PlaybackSnapshot,
        audio: crate::vlc::AudioMetrics,
    ) -> io::Result<()> {
        self.draws += 1;
        self.draw.observe(draw_time);
        if let Some(frame) = frame {
            if self.last_frame != Some(frame) {
                self.unique_frames += 1;
                self.last_frame = Some(frame);
            }
        }
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return Ok(());
        }
        let count = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        writeln!(self.file, "{{\"schema\":1,\"scope\":\"server only; not client presentation\",\"elapsed_ms\":{},\"vlc_callback_frames\":{},\"picture_allocations\":{},\"picture_allocated_bytes\":{},\"owned_frame_allocations\":{},\"copied_bytes\":{},\"copy_time_us\":{},\"encode_requests\":{},\"pending_replaced\":{},\"encoded_frames\":{},\"encode_failures\":{},\"responses_replaced\":{},\"stale_results\":{},\"encode_recent_p95_us\":{},\"queue_recent_p95_us\":{},\"draw_recent_p95_us\":{},\"request_to_backend_recent_p95_us\":{},\"work_pixel_budget\":{},\"backend_draws\":{},\"unique_frames_drawn\":{},\"backend_bytes\":{},\"playback_ms\":{},\"state\":{},\"remote_audio\":{},\"audio_generation\":{},\"audio_callback_drops\":{},\"audio_queued_frames\":{},\"audio_underrun_callbacks\":{},\"audio_estimated_receiver_pts\":{}}}",
            self.started.elapsed().as_millis(), decode.frames, decode.picture_allocations, decode.picture_bytes,
            decode.owned_frames, decode.copied_bytes, decode.copy_us, count(&self.encoder.requests), count(&self.encoder.pending_replaced),
            count(&self.encoder.encoded), count(&self.encoder.failures), count(&self.encoder.responses_replaced), count(&self.encoder.stale),
            self.encoder.encode.p95(), self.encoder.queue_wait.p95(), self.draw.p95(), self.encoder.request_to_backend.p95(), count(&self.encoder.work_pixel_budget), self.draws, self.unique_frames, bytes, playback.time_ms, playback.state, audio.remote, audio.generation, audio.callback_drops, audio.queued_frames, audio.underruns, audio.receiver_pts)?;
        self.file.flush()?;
        self.last_report = Instant::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_writes_count_only_accepted_bytes() {
        let mut buffer = [0u8; 3];
        let mut writer = CountingWriter::new(buffer.as_mut_slice(), true);
        assert_eq!(writer.write(b"12345").unwrap(), 3);
        assert_eq!(writer.bytes.load(Ordering::Relaxed), 3);
    }
    #[test]
    fn latency_window_is_bounded_and_uses_recent_samples() {
        let latency = Latency::default();
        for _ in 0..256 {
            latency.observe(Duration::from_micros(9000));
        }
        for _ in 0..256 {
            latency.observe(Duration::from_micros(10));
        }
        assert_eq!(latency.0.lock().unwrap().len(), 256);
        assert_eq!(latency.p95(), 10);
    }
    #[test]
    fn profiling_refuses_to_overwrite_existing_results() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("metrics.jsonl");
        std::fs::write(&path, "preserve").unwrap();
        assert!(Recorder::create(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "preserve");
    }
}
