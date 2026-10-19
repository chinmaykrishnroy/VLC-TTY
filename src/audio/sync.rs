//! Bounded server-side video delay for the receiver's extra audio latency.
//! This estimates alignment; terminal/client presentation still needs external
//! measurement. It must never accumulate frames after seek/pause/media changes.
use crate::vlc::VideoFrame;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct VideoDelay {
    generation: u64,
    frames: VecDeque<(Instant, Arc<VideoFrame>)>,
    shown: Option<Arc<VideoFrame>>,
    last_serial: Option<u64>,
    bytes: usize,
}
impl VideoDelay {
    pub fn new() -> Self {
        Self {
            generation: 0,
            frames: VecDeque::with_capacity(12),
            shown: None,
            last_serial: None,
            bytes: 0,
        }
    }
    pub fn select(
        &mut self,
        frame: Option<Arc<VideoFrame>>,
        generation: u64,
        paused: bool,
        delay: Duration,
    ) -> Option<Arc<VideoFrame>> {
        self.at(frame, generation, paused, delay, Instant::now())
    }
    fn at(
        &mut self,
        frame: Option<Arc<VideoFrame>>,
        generation: u64,
        paused: bool,
        delay: Duration,
        now: Instant,
    ) -> Option<Arc<VideoFrame>> {
        if self.generation != generation || paused || frame.is_none() {
            self.frames.clear();
            self.shown = None;
            self.last_serial = None;
            self.bytes = 0;
            self.generation = generation;
        }
        if delay.is_zero() {
            self.frames.clear();
            self.bytes = 0;
            self.shown = frame.clone();
            self.last_serial = frame.as_ref().map(|f| f.serial);
        }
        if paused || delay.is_zero() {
            return frame;
        }
        if let Some(frame) = frame {
            if self.last_serial != Some(frame.serial) {
                self.last_serial = Some(frame.serial);
                self.bytes += frame.bgra.len();
                self.frames.push_back((now, frame));
            }
        }
        let cutoff = now
            .checked_sub(delay.min(Duration::from_millis(250)))
            .unwrap_or(now);
        while self.frames.front().is_some_and(|(at, _)| *at <= cutoff) {
            let (_, frame) = self.frames.pop_front().unwrap();
            self.bytes -= frame.bgra.len();
            self.shown = Some(frame);
        }
        // Retain at most 12 frames / 24 MiB, plus one shown frame. High-resolution
        // sources may shorten the delay instead of increasing memory indefinitely.
        while self.frames.len() > 12 || self.bytes > 24 * 1024 * 1024 {
            let (_, frame) = self.frames.pop_front().unwrap();
            self.bytes -= frame.bgra.len();
            self.shown = Some(frame);
        }
        self.shown.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(serial: u64) -> Option<Arc<VideoFrame>> {
        Some(Arc::new(VideoFrame {
            width: 1,
            height: 1,
            pitch: 4,
            bgra: vec![0; 4].into(),
            serial,
        }))
    }
    #[test]
    fn delay_is_bounded_and_generation_changes_never_replay_old_frames() {
        let mut delay = VideoDelay::new();
        let start = Instant::now();
        let duration = Duration::from_millis(100);
        assert!(delay.at(frame(1), 1, false, duration, start).is_none());
        assert_eq!(
            delay
                .at(frame(2), 1, false, duration, start + duration)
                .unwrap()
                .serial,
            1
        );
        assert!(delay
            .at(frame(3), 2, false, duration, start + duration)
            .is_none());
        assert_eq!(
            delay
                .at(frame(4), 2, true, duration, start + duration)
                .unwrap()
                .serial,
            4
        );
        for serial in 5..100 {
            delay.at(frame(serial), 2, false, duration, start + duration);
        }
        assert!(delay.frames.len() <= 12);
    }
}
