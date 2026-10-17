use super::{
    capture::Capture,
    protocol::{self, Message},
};
use anyhow::{bail, Context, Result};
use std::{
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub fn loopback(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value
        .parse()
        .context("Audio endpoint must be a numeric loopback address and port")?;
    if !address.ip().is_loopback() {
        bail!("Audio endpoints must use loopback; use an SSH tunnel for remote connections");
    }
    Ok(address)
}
pub fn token(text: &str) -> Result<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        bail!("Audio session key must contain 64 hexadecimal characters");
    }
    let mut bytes = [0; 32];
    for (byte, chunk) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(chunk)?, 16)
            .context("Invalid audio session key")?;
    }
    Ok(bytes)
}

pub struct Transport {
    pub capture: Arc<Capture>,
    pub failed: Arc<AtomicBool>,
    pub queued_frames: Arc<AtomicU64>,
    pub underruns: Arc<AtomicU64>,
    pub presented_pts: Arc<AtomicI64>,
    stop: Arc<AtomicBool>,
    stream: TcpStream,
    workers: Vec<JoinHandle<()>>,
}
impl Transport {
    pub fn connect(
        address: SocketAddr,
        token: &[u8; 32],
        clock: unsafe extern "C" fn() -> i64,
    ) -> Result<Self> {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .context("Cannot reach local audio receiver through tunnel")?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        protocol::authenticate_client(&mut stream, token)?;
        protocol::write(&mut stream, &Message::Format)?;
        let capture = Arc::new(Capture::new());
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let queued_frames = Arc::new(AtomicU64::new(0));
        let underruns = Arc::new(AtomicU64::new(0));
        let presented_pts = Arc::new(AtomicI64::new(0));
        let mut output = stream.try_clone()?;
        let mut input = stream.try_clone()?;
        let mut transport = Self {
            capture: capture.clone(),
            failed: failed.clone(),
            queued_frames: queued_frames.clone(),
            underruns: underruns.clone(),
            presented_pts: presented_pts.clone(),
            stop: stop.clone(),
            stream,
            workers: Vec::new(),
        };
        let c = capture.clone();
        let s = stop.clone();
        let f = failed.clone();
        let writer = thread::Builder::new()
            .name("audio-send".into())
            .spawn(move || {
                let result = (|| -> std::io::Result<()> {
                    let mut state = (0, false);
                    while !s.load(Ordering::Acquire) {
                        let current = (
                            c.generation.load(Ordering::Acquire),
                            c.paused.load(Ordering::Acquire),
                        );
                        if state != current {
                            protocol::write(
                                &mut output,
                                &Message::State {
                                    generation: current.0,
                                    paused: current.1,
                                },
                            )?;
                            state = current;
                        }
                        if let Some(packet) = c.ready.pop() {
                            // LibVLC can deliver PCM well ahead of its PTS. Pace
                            // here, never in the native callback, so the client
                            // jitter buffer does not discard valid future audio.
                            while packet.generation == c.generation.load(Ordering::Acquire)
                                && !s.load(Ordering::Acquire)
                                && packet.pts.saturating_sub(unsafe { clock() }) > 80_000
                            {
                                thread::sleep(Duration::from_millis(2));
                            }
                            let result = if packet.generation == state.0
                                && packet.generation == c.generation.load(Ordering::Acquire)
                                && !state.1
                                && !s.load(Ordering::Acquire)
                            {
                                protocol::write(
                                    &mut output,
                                    &Message::Samples {
                                        generation: packet.generation,
                                        pts: packet.pts,
                                        lead_us: packet.pts.saturating_sub(unsafe { clock() }),
                                        data: packet.data[..packet.len].to_vec(),
                                    },
                                )
                            } else {
                                Ok(())
                            };
                            let _ = c.free.push(packet);
                            result?;
                        } else {
                            thread::sleep(Duration::from_millis(2));
                        }
                    }
                    let _ = protocol::write(&mut output, &Message::End);
                    Ok(())
                })();
                if result.is_err() && !s.load(Ordering::Acquire) {
                    f.store(true, Ordering::Release);
                }
            })?;
        transport.workers.push(writer);
        let s = stop.clone();
        let f = failed.clone();
        let q = queued_frames.clone();
        let u = underruns.clone();
        let pts = presented_pts.clone();
        let active_capture = capture.clone();
        let reader = thread::Builder::new()
            .name("audio-feedback".into())
            .spawn(move || {
                while !s.load(Ordering::Acquire) {
                    match protocol::read(&mut input) {
                        Ok(Message::Telemetry {
                            queued_frames,
                            underruns,
                            pts: presented,
                            generation,
                            ..
                        }) => {
                            q.store(queued_frames as u64, Ordering::Relaxed);
                            u.store(underruns, Ordering::Relaxed);
                            pts.store(
                                if generation == active_capture.generation.load(Ordering::Acquire) {
                                    presented
                                } else {
                                    0
                                },
                                Ordering::Relaxed,
                            );
                        }
                        _ => {
                            if !s.load(Ordering::Acquire) {
                                f.store(true, Ordering::Release);
                            }
                            break;
                        }
                    }
                }
            })?;
        transport.workers.push(reader);
        Ok(transport)
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Let the sender emit End before closing the socket. A normal quit must
        // not be reported as a truncated audio frame by the receiver.
        if !self.workers.is_empty() {
            let _ = self.workers.remove(0).join();
        }
        let _ = self.stream.shutdown(Shutdown::Both);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
