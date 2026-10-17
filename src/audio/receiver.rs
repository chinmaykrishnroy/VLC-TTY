//! Local audio device output. Network work never runs on the CPAL callback.
use super::{
    protocol::{self, Message, MAX_SAMPLES},
    transport,
};
use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::{
    net::{Shutdown, SocketAddr, TcpListener},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POOL_BLOCKS: usize = 128;
const MAX_QUEUED_FRAMES: u64 = 36_000; // 750 ms at 48 kHz, plus the active block.
const JITTER_ALLOWANCE_US: i64 = 400_000;

struct Block {
    generation: u64,
    pts: i64,
    due: Instant,
    len: usize,
    data: [i16; MAX_SAMPLES],
}
#[derive(Default)]
struct Timeline {
    anchor: Option<(u64, i64, Instant)>,
}
impl Timeline {
    fn due(&mut self, generation: u64, pts: i64, lead_us: i64, arrived: Instant) -> Instant {
        if let Some((active, origin_pts, origin)) = self.anchor {
            let delta = pts.saturating_sub(origin_pts);
            let due = if delta >= 0 {
                origin.checked_add(Duration::from_micros(delta as u64))
            } else {
                origin.checked_sub(Duration::from_micros(delta.unsigned_abs()))
            };
            if let Some(due) = due.filter(|due| {
                active == generation
                    && arrived.saturating_duration_since(*due) < Duration::from_millis(150)
                    && due.saturating_duration_since(arrived) < Duration::from_secs(1)
            }) {
                return due;
            }
        }
        // Anchor once per playback generation, not once per arriving packet.
        // Contiguous PTS stays contiguous despite variable SSH delivery times.
        let due = arrived
            + Duration::from_micros(
                lead_us
                    .saturating_add(JITTER_ALLOWANCE_US)
                    .clamp(0, 1_000_000) as u64,
            );
        self.anchor = Some((generation, pts, due));
        due
    }
}
struct Buffer {
    free: ArrayQueue<Box<Block>>,
    ready: ArrayQueue<Box<Block>>,
    queued_frames: AtomicU64,
    current_frames: AtomicU64,
    generation: AtomicU64,
    paused: AtomicBool,
    failed: AtomicBool,
    underruns: AtomicU64,
    consumed_pts: AtomicU64,
    consumed_at_us: AtomicU64,
    consumed_generation: AtomicU64,
    clock_origin: Instant,
}
impl Buffer {
    fn new() -> Self {
        let free = ArrayQueue::new(POOL_BLOCKS);
        for _ in 0..POOL_BLOCKS {
            let _ = free.push(Box::new(Block {
                generation: 0,
                pts: 0,
                due: Instant::now(),
                len: 0,
                data: [0; MAX_SAMPLES],
            }));
        }
        Self {
            free,
            ready: ArrayQueue::new(POOL_BLOCKS),
            queued_frames: AtomicU64::new(0),
            current_frames: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            consumed_pts: AtomicU64::new(0),
            consumed_at_us: AtomicU64::new(0),
            consumed_generation: AtomicU64::new(0),
            clock_origin: Instant::now(),
        }
    }
    fn pop(&self) -> Option<Box<Block>> {
        let block = self.ready.pop()?;
        self.queued_frames
            .fetch_sub((block.len / 2) as u64, Ordering::Relaxed);
        Some(block)
    }
    fn push(&self, block: Box<Block>) {
        let frames = (block.len / 2) as u64;
        self.queued_frames.fetch_add(frames, Ordering::Relaxed);
        if let Err(block) = self.ready.push(block) {
            self.queued_frames.fetch_sub(frames, Ordering::Relaxed);
            let _ = self.free.push(block);
        }
    }
    fn clear(&self) {
        while let Some(block) = self.pop() {
            let _ = self.free.push(block);
        }
    }
}

struct Output {
    buffer: Arc<Buffer>,
    block: Option<Box<Block>>,
    offset: usize,
    phase: f64,
    rate: u32,
    channels: usize,
    waiting: bool,
    playing_generation: u64,
}
impl Output {
    fn frame(&mut self, now: Instant) -> Option<[f32; 2]> {
        self.waiting = false;
        if self.buffer.paused.load(Ordering::Acquire) {
            self.waiting = true;
            return None;
        }
        // Each iteration consumes a block from the fixed pool.
        for _ in 0..=POOL_BLOCKS {
            let generation = self.buffer.generation.load(Ordering::Acquire);
            if self
                .block
                .as_ref()
                .is_some_and(|b| b.generation != generation || self.offset + 1 >= b.len)
            {
                let block = self.block.take().unwrap();
                if block.generation == generation {
                    self.offset = self.offset.saturating_sub(block.len);
                } else {
                    self.offset = 0;
                    self.phase = 0.0;
                }
                let _ = self.buffer.free.push(block);
            }
            if self.block.is_none() {
                self.block = self.buffer.pop();
            }
            let block = self.block.as_ref()?;
            if block.generation != generation {
                continue;
            }
            let due = block.due
                + Duration::from_secs_f64(self.offset as f64 / 2.0 / protocol::RATE as f64);
            if self.playing_generation != generation && due > now + Duration::from_millis(2) {
                self.waiting = true;
                return None;
            }
            if self.playing_generation != generation
                && now.saturating_duration_since(due) > Duration::from_millis(150)
            {
                let _ = self.buffer.free.push(self.block.take().unwrap());
                self.offset = 0;
                continue;
            }
            let index = self.offset;
            // Once primed, the device clock consumes contiguous samples. A
            // slightly early callback must not insert wall-clock-gated silence.
            self.playing_generation = generation;
            let next = (index + 2).min(block.len - 2);
            let a = self.phase as f32;
            let result = [0, 1].map(|channel| {
                (block.data[index + channel] as f32 * (1.0 - a)
                    + block.data[next + channel] as f32 * a)
                    / 32768.0
            });
            self.buffer.consumed_at_us.store(
                now.saturating_duration_since(self.buffer.clock_origin)
                    .as_micros() as u64,
                Ordering::Relaxed,
            );
            self.buffer
                .consumed_generation
                .store(generation, Ordering::Relaxed);
            self.buffer.consumed_pts.store(
                block
                    .pts
                    .saturating_add((index / 2) as i64 * 1_000_000 / protocol::RATE as i64)
                    .max(0) as u64,
                Ordering::Relaxed,
            );
            self.phase += protocol::RATE as f64 / self.rate as f64;
            let advance = self.phase.floor() as usize;
            self.phase -= advance as f64;
            self.offset += advance * 2;
            return Some(result);
        }
        None
    }
    fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        device_latency: Duration,
    ) {
        let now = Instant::now() + device_latency.min(Duration::from_secs(1));
        let mut underrun = false;
        for (index, frame) in output.chunks_mut(self.channels).enumerate() {
            let sample = self.frame(now + Duration::from_secs_f64(index as f64 / self.rate as f64));
            underrun |= sample.is_none()
                && (!self.waiting
                    || self.buffer.consumed_generation.load(Ordering::Relaxed)
                        == self.buffer.generation.load(Ordering::Acquire))
                && !self.buffer.paused.load(Ordering::Acquire)
                && self.buffer.generation.load(Ordering::Acquire) != 0;
            let [left, right] = sample.unwrap_or([0.0; 2]);
            for (channel, value) in frame.iter_mut().enumerate() {
                *value = T::from_sample(if self.channels == 1 {
                    (left + right) * 0.5
                } else {
                    match channel {
                        0 => left,
                        1 => right,
                        _ => 0.0,
                    }
                });
            }
        }
        self.buffer.current_frames.store(
            self.block
                .as_ref()
                .map_or(0, |b| b.len.saturating_sub(self.offset) / 2) as u64,
            Ordering::Relaxed,
        );
        if underrun {
            self.buffer.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn device(buffer: Arc<Buffer>) -> Result<cpal::Stream> {
    let device = cpal::default_host()
        .default_output_device()
        .context("No local audio output device is available")?;
    let config = device
        .default_output_config()
        .context("Cannot configure local audio device")?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let output = Output {
        buffer: buffer.clone(),
        block: None,
        offset: 0,
        phase: 0.0,
        rate: config.sample_rate.0,
        channels: config.channels as usize,
        waiting: false,
        playing_generation: 0,
    };
    let error = move |_error| {
        buffer.failed.store(true, Ordering::Release);
    };
    macro_rules! build {
        ($ty:ty) => {{
            let mut output = output;
            device.build_output_stream(
                &config,
                move |data: &mut [$ty], info| {
                    let timestamp = info.timestamp();
                    output.fill(
                        data,
                        timestamp
                            .playback
                            .duration_since(&timestamp.callback)
                            .unwrap_or_default(),
                    );
                },
                error,
                None,
            )
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::U64 => build!(u64),
        _ => bail!("Unsupported local audio device sample format: {format}"),
    }?;
    stream.play()?;
    Ok(stream)
}

pub struct Receiver {
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
}
impl Receiver {
    pub fn start(address: SocketAddr, key: [u8; 32]) -> Result<Self> {
        transport::loopback(&address.to_string())?;
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("audio-receive".into())
            .spawn(move || {
                let buffer = Arc::new(Buffer::new());
                let stream = match device(buffer.clone()) {
                    Ok(stream) => {
                        let _ = tx.send(Ok(()));
                        stream
                    }
                    Err(error) => {
                        let _ = tx.send(Err(format!("{error:#}")));
                        return Err(error);
                    }
                };
                let result = serve(listener, &key, &s, &buffer);
                drop(stream);
                result
            })?;
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(Self {
                address,
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                bail!("Local audio receiver: {error}")
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                bail!("Audio device startup timed out: {error}")
            }
        }
    }
    pub fn finished(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| w.is_finished())
    }
    pub fn wait(mut self) -> Result<()> {
        self.worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| anyhow::anyhow!("Audio receiver panicked"))?
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    listener: TcpListener,
    key: &[u8; 32],
    stop: &Arc<AtomicBool>,
    buffer: &Arc<Buffer>,
) -> Result<()> {
    let mut connection = loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if buffer.failed.load(Ordering::Acquire) {
            bail!("Local audio device was disconnected or failed");
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                // Winsock inherits the listener's nonblocking mode. Framed
                // read_exact must use a blocking stream on every platform.
                stream.set_nonblocking(false)?;
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                if protocol::authenticate_server(&mut stream, key).is_ok() {
                    break stream;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(error) => return Err(error.into()),
        }
    };
    if protocol::read(&mut connection)? != Message::Format {
        bail!("Expected audio format handshake");
    }
    // No read timeout after framing starts: timeouts after a partial frame cannot
    // safely be retried. Shutdown below unblocks the reader on cancellation.
    connection.set_read_timeout(None)?;
    let mut feedback = connection.try_clone()?;
    let b = buffer.clone();
    let s = stop.clone();
    let done = Arc::new(AtomicBool::new(false));
    let d = done.clone();
    let monitor = thread::spawn(move || {
        while !s.load(Ordering::Acquire)
            && !d.load(Ordering::Acquire)
            && !b.failed.load(Ordering::Acquire)
        {
            if protocol::write(
                &mut feedback,
                &Message::Telemetry {
                    generation: b.generation.load(Ordering::Acquire),
                    queued_frames: (b.queued_frames.load(Ordering::Relaxed)
                        + b.current_frames.load(Ordering::Relaxed))
                        as u32,
                    underruns: b.underruns.load(Ordering::Relaxed),
                    pts: {
                        let at = b.consumed_at_us.load(Ordering::Relaxed) as i64;
                        let now = b.clock_origin.elapsed().as_micros() as i64;
                        if b.consumed_generation.load(Ordering::Relaxed)
                            == b.generation.load(Ordering::Acquire)
                            && b.consumed_pts.load(Ordering::Relaxed) > 0
                            && (now - at).abs() < 250_000
                            && !b.paused.load(Ordering::Acquire)
                        {
                            (b.consumed_pts.load(Ordering::Relaxed) as i64).saturating_add(now - at)
                        } else {
                            0
                        }
                    },
                },
            )
            .is_err()
            {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = feedback.shutdown(Shutdown::Both);
    });
    let mut timeline = Timeline::default();
    let result = (|| -> Result<()> {
        loop {
            match protocol::read(&mut connection)? {
                Message::State { generation, paused } => {
                    buffer.paused.store(paused, Ordering::Release);
                    buffer.generation.store(generation, Ordering::Release);
                    buffer.clear();
                }
                Message::Samples {
                    generation,
                    pts,
                    lead_us,
                    data,
                } => {
                    if generation != buffer.generation.load(Ordering::Acquire)
                        || buffer.paused.load(Ordering::Acquire)
                    {
                        continue;
                    }
                    // Bound retained audio to about 750 ms. Discard oldest work
                    // after a transport stall instead of building an audio backlog.
                    while buffer.queued_frames.load(Ordering::Relaxed) + (data.len() / 2) as u64
                        > MAX_QUEUED_FRAMES
                    {
                        if let Some(block) = buffer.pop() {
                            let _ = buffer.free.push(block);
                        }
                    }
                    let Some(mut block) = buffer.free.pop() else {
                        continue;
                    };
                    block.generation = generation;
                    block.pts = pts;
                    block.due = timeline.due(generation, pts, lead_us, Instant::now());
                    block.len = data.len();
                    block.data[..data.len()].copy_from_slice(&data);
                    buffer.push(block);
                }
                Message::End => return Ok(()),
                _ => bail!("Unexpected audio message"),
            }
        }
    })();
    done.store(true, Ordering::Release);
    let _ = connection.shutdown(Shutdown::Both);
    let _ = monitor.join();
    if buffer.failed.load(Ordering::Acquire) {
        bail!("Local audio device was disconnected or failed");
    }
    if stop.load(Ordering::Acquire) {
        Ok(())
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn early_device_callback_does_not_silence_buffered_pcm() {
        let buffer = Arc::new(Buffer::new());
        buffer.generation.store(1, Ordering::Release);
        let mut output = Output {
            buffer: buffer.clone(),
            block: None,
            offset: 0,
            phase: 0.0,
            rate: 48_000,
            channels: 2,
            waiting: false,
            playing_generation: 0,
        };
        let due = Instant::now();
        let mut block = buffer.free.pop().unwrap();
        block.generation = 1;
        block.due = due;
        block.pts = 1_000_000;
        block.len = MAX_SAMPLES;
        block.data.fill(1000);
        buffer.push(block);
        for index in 0..960 {
            // The second device callback is invoked 3 ms early. Hardware still
            // consumes these samples at 48 kHz; don't insert software silence.
            let offset = if index < 480 { index } else { index - 144 };
            assert!(
                output
                    .frame(due + Duration::from_secs_f64(offset as f64 / 48_000.0))
                    .is_some(),
                "Early device callback inserted silence at frame {index}"
            );
        }
    }
    #[test]
    fn timeline_reanchors_after_seek_or_large_network_stall() {
        let start = Instant::now();
        let mut timeline = Timeline::default();
        let first = timeline.due(1, 1_000_000, 0, start);
        assert_eq!(
            timeline.due(1, 1_020_000, 0, start + Duration::from_millis(28)),
            first + Duration::from_millis(20)
        );
        let seek = timeline.due(2, 5_000_000, 0, start + Duration::from_secs(1));
        assert_eq!(seek, start + Duration::from_millis(1400));
        let stall = timeline.due(2, 5_020_000, 0, start + Duration::from_secs(3));
        assert_eq!(stall, start + Duration::from_millis(3400));
    }
    #[test]
    fn packet_arrival_jitter_does_not_insert_silence_between_contiguous_samples() {
        let buffer = Arc::new(Buffer::new());
        buffer.generation.store(1, Ordering::Release);
        let mut output = Output {
            buffer: buffer.clone(),
            block: None,
            offset: 0,
            phase: 0.0,
            rate: 48_000,
            channels: 2,
            waiting: false,
            playing_generation: 0,
        };
        let mut timeline = Timeline::default();
        let arrived = Instant::now();
        let mut first_due = None;
        for (index, delay_ms) in [(0, 0), (1, 28)] {
            let mut block = buffer.free.pop().unwrap();
            block.generation = 1;
            block.pts = 1_000_000 + index * 20_000;
            block.due = timeline.due(1, block.pts, 0, arrived + Duration::from_millis(delay_ms));
            first_due.get_or_insert(block.due);
            block.len = MAX_SAMPLES;
            block.data.fill(1000);
            buffer.push(block);
        }
        for frame in 0..1920 {
            let now = first_due.unwrap() + Duration::from_secs_f64(frame as f64 / 48_000.0);
            assert!(
                output.frame(now).is_some(),
                "Inserted silence at frame {frame} despite buffered PCM"
            );
        }
    }
    #[test]
    fn device_callback_silences_underruns_and_rejects_stale_audio() {
        let buffer = Arc::new(Buffer::new());
        let mut output = Output {
            buffer: buffer.clone(),
            block: None,
            offset: 0,
            phase: 0.0,
            rate: 48_000,
            channels: 2,
            waiting: false,
            playing_generation: 0,
        };
        let mut data = [1.0f32; 8];
        output.fill(&mut data, Duration::ZERO);
        assert_eq!(data, [0.0; 8]);
        let mut block = buffer.free.pop().unwrap();
        block.len = 8;
        block.data[..8].fill(i16::MAX);
        block.generation = 1;
        block.due = Instant::now();
        buffer.push(block);
        buffer.generation.store(2, Ordering::Release);
        output.fill(&mut data, Duration::ZERO);
        assert_eq!(data, [0.0; 8]);
        assert_eq!(buffer.free.len(), POOL_BLOCKS);
    }
}
