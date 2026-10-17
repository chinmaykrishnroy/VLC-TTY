//! Callback ingestion: fixed pool, bounded queues, no sockets, locks or allocation.
use super::protocol::MAX_SAMPLES;
use crossbeam_queue::ArrayQueue;
use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, Ordering},
};

pub struct Packet {
    pub generation: u64,
    pub pts: i64,
    pub len: usize,
    pub data: [i16; MAX_SAMPLES],
}
pub struct Capture {
    pub free: ArrayQueue<Box<Packet>>,
    pub ready: ArrayQueue<Box<Packet>>,
    pub generation: AtomicU64,
    pub paused: AtomicBool,
    pub dropped: AtomicU64,
    pub last_pts: AtomicI64,
    pub draining: AtomicBool,
    pub volume: AtomicI32,
}
impl Capture {
    pub fn new() -> Self {
        let free = ArrayQueue::new(128);
        for _ in 0..128 {
            let _ = free.push(Box::new(Packet {
                generation: 0,
                pts: 0,
                len: 0,
                data: [0; MAX_SAMPLES],
            }));
        }
        Self {
            free,
            ready: ArrayQueue::new(128),
            generation: AtomicU64::new(1),
            paused: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            last_pts: AtomicI64::new(0),
            draining: AtomicBool::new(false),
            volume: AtomicI32::new(100),
        }
    }
    pub fn flush(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn state(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
        self.flush();
    }
    /// Gain is applied just before transmission, so queued decoder packets do
    /// not retain an old volume or force a rebuffer on every volume keypress.
    pub fn apply_volume(&self, samples: &mut [i16]) {
        let volume = self.volume.load(Ordering::Acquire).clamp(0, 125);
        if volume != 100 {
            for sample in samples {
                *sample = ((*sample as i32 * volume) / 100).clamp(i16::MIN as i32, i16::MAX as i32)
                    as i16;
            }
        }
    }
}
pub unsafe extern "C" fn play(opaque: *mut c_void, samples: *const c_void, count: u32, pts: i64) {
    if opaque.is_null() || samples.is_null() || count > 480_000 {
        return;
    }
    let capture = &*(opaque as *const Capture);
    capture.last_pts.store(
        pts.saturating_add(count as i64 * 1_000_000 / 48_000),
        Ordering::Relaxed,
    );
    capture.draining.store(false, Ordering::Release);
    let generation = capture.generation.load(Ordering::Acquire);
    let samples = std::slice::from_raw_parts(samples.cast::<i16>(), count as usize * 2);
    for (i, chunk) in samples.chunks(MAX_SAMPLES).enumerate() {
        let Some(mut packet) = capture.free.pop() else {
            capture.dropped.fetch_add(1, Ordering::Relaxed);
            capture.flush();
            return;
        };
        packet.generation = generation;
        packet.pts = pts.saturating_add((i * MAX_SAMPLES / 2) as i64 * 1_000_000 / 48_000);
        packet.len = chunk.len();
        packet.data[..chunk.len()].copy_from_slice(chunk);
        if let Err(packet) = capture.ready.push(packet) {
            let _ = capture.free.push(packet);
            capture.dropped.fetch_add(1, Ordering::Relaxed);
            capture.flush();
            return;
        }
    }
}
pub unsafe extern "C" fn pause(opaque: *mut c_void, _: i64) {
    (&*(opaque as *const Capture)).state(true);
}
pub unsafe extern "C" fn resume(opaque: *mut c_void, _: i64) {
    (&*(opaque as *const Capture)).state(false);
}
pub unsafe extern "C" fn flush(opaque: *mut c_void, _: i64) {
    (&*(opaque as *const Capture)).flush();
}
pub unsafe extern "C" fn drain(opaque: *mut c_void) {
    (&*(opaque as *const Capture))
        .draining
        .store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_volume_mutes_scales_and_clips_samples() {
        let mut capture = Capture::new();
        let samples = [i16::MAX, i16::MIN, 10_000, -10_000];
        for (volume, expected) in [
            (0, [0; 4]),
            (50, [16383, -16384, 5000, -5000]),
            (100, samples),
            (125, [i16::MAX, i16::MIN, 12500, -12500]),
        ] {
            capture.volume.store(volume, Ordering::Release);
            unsafe {
                play(
                    (&mut capture as *mut Capture).cast(),
                    samples.as_ptr().cast(),
                    2,
                    42,
                )
            };
            let mut packet = capture.ready.pop().unwrap();
            capture.apply_volume(&mut packet.data[..packet.len]);
            assert_eq!(&packet.data[..4], &expected);
            capture.free.push(packet).ok().unwrap();
        }
    }
    #[test]
    fn callback_pool_is_bounded_and_old_generations_are_identifiable() {
        let mut capture = Capture::new();
        let samples = [7i16; MAX_SAMPLES];
        for _ in 0..200 {
            unsafe {
                play(
                    (&mut capture as *mut Capture).cast(),
                    samples.as_ptr().cast(),
                    (MAX_SAMPLES / 2) as u32,
                    42,
                );
            }
        }
        assert_eq!(capture.ready.len(), 128);
        assert!(capture.dropped.load(Ordering::Relaxed) > 0);
        let packet = capture.ready.pop().unwrap();
        assert_eq!(packet.data, samples);
        assert!(packet.generation < capture.generation.load(Ordering::Acquire));
    }
}
