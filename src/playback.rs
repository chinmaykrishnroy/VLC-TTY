//! Playback controls shared by frontends, independent of the terminal event loop.
use crate::vlc::{MediaTrack, PlaybackSnapshot, VlcPlayer};
use anyhow::Result;
use std::path::Path;

pub trait Playback {
    fn open(&mut self, source: &str) -> Result<()>;
    fn is_native_window(&self) -> bool;
    fn toggle_pause(&mut self);
    fn stop(&mut self);
    fn seek_relative(&mut self, delta_ms: i64);
    fn seek_to(&mut self, time_ms: i64);
    fn set_volume(&mut self, volume: i32);
    fn rate(&self) -> f32;
    fn set_rate(&mut self, rate: f32);
    fn snapshot(&self) -> PlaybackSnapshot;
    fn media_metadata(&self) -> crate::metadata::MediaMetadata {
        crate::metadata::MediaMetadata::default()
    }
    fn subtitle_tracks(&self) -> Vec<MediaTrack>;
    fn current_subtitle_id(&self) -> i32;
    fn select_subtitle(&mut self, id: i32) -> bool;
    fn audio_tracks(&self) -> Vec<MediaTrack>;
    fn current_audio_id(&self) -> i32;
    fn select_audio(&mut self, id: i32) -> bool;
    fn load_vlc_subtitle(&mut self, path: &Path) -> Result<()>;
    fn next_frame(&mut self);
    fn set_audio_delay(&mut self, delay_ms: i64) -> bool;
    fn set_subtitle_delay(&mut self, delay_ms: i64) -> bool;
}

impl Playback for VlcPlayer {
    fn media_metadata(&self) -> crate::metadata::MediaMetadata {
        VlcPlayer::media_metadata(self)
    }
    fn open(&mut self, source: &str) -> Result<()> {
        VlcPlayer::open(self, source)
    }
    fn is_native_window(&self) -> bool {
        VlcPlayer::is_native_window(self)
    }
    fn toggle_pause(&mut self) {
        VlcPlayer::toggle_pause(self);
    }
    fn stop(&mut self) {
        VlcPlayer::stop(self);
    }
    fn seek_relative(&mut self, delta_ms: i64) {
        VlcPlayer::seek_relative(self, delta_ms);
    }
    fn seek_to(&mut self, time_ms: i64) {
        VlcPlayer::seek_to(self, time_ms);
    }
    fn set_volume(&mut self, volume: i32) {
        VlcPlayer::set_volume(self, volume);
    }
    fn rate(&self) -> f32 {
        VlcPlayer::rate(self)
    }
    fn set_rate(&mut self, rate: f32) {
        VlcPlayer::set_rate(self, rate);
    }
    fn snapshot(&self) -> PlaybackSnapshot {
        VlcPlayer::snapshot(self)
    }
    fn subtitle_tracks(&self) -> Vec<MediaTrack> {
        VlcPlayer::subtitle_tracks(self)
    }
    fn current_subtitle_id(&self) -> i32 {
        VlcPlayer::current_subtitle_id(self)
    }
    fn select_subtitle(&mut self, id: i32) -> bool {
        VlcPlayer::select_subtitle(self, id)
    }
    fn audio_tracks(&self) -> Vec<MediaTrack> {
        VlcPlayer::audio_tracks(self)
    }
    fn current_audio_id(&self) -> i32 {
        VlcPlayer::current_audio_id(self)
    }
    fn select_audio(&mut self, id: i32) -> bool {
        VlcPlayer::select_audio(self, id)
    }
    fn load_vlc_subtitle(&mut self, path: &Path) -> Result<()> {
        VlcPlayer::load_vlc_subtitle(self, path)
    }
    fn next_frame(&mut self) {
        VlcPlayer::next_frame(self);
    }
    fn set_audio_delay(&mut self, ms: i64) -> bool {
        VlcPlayer::set_audio_delay(self, ms)
    }
    fn set_subtitle_delay(&mut self, ms: i64) -> bool {
        VlcPlayer::set_subtitle_delay(self, ms)
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub struct FakePlayback {
        pub opened: Vec<String>,
        pub volume: i32,
        pub state: i32,
        pub subtitle: i32,
        pub native: bool,
        pub paused: usize,
        pub frame_steps: usize,
        pub audio_delay: i64,
        pub subtitle_delay: i64,
        pub seeks: Vec<i64>,
        pub reject_open: bool,
        pub reject_tracks: bool,
        pub reject_delays: bool,
        pub metadata: crate::metadata::MediaMetadata,
    }
    impl Default for FakePlayback {
        fn default() -> Self {
            Self {
                opened: vec![],
                volume: 100,
                state: 3,
                subtitle: -1,
                native: false,
                paused: 0,
                frame_steps: 0,
                audio_delay: 0,
                subtitle_delay: 0,
                seeks: vec![],
                reject_open: false,
                reject_tracks: false,
                reject_delays: false,
                metadata: Default::default(),
            }
        }
    }
    impl Playback for FakePlayback {
        fn media_metadata(&self) -> crate::metadata::MediaMetadata {
            self.metadata.clone()
        }
        fn open(&mut self, source: &str) -> Result<()> {
            if self.reject_open {
                anyhow::bail!("test open failure");
            }
            self.opened.push(source.into());
            self.state = 3;
            Ok(())
        }
        fn is_native_window(&self) -> bool {
            self.native
        }
        fn toggle_pause(&mut self) {
            self.paused += 1;
            self.state = if self.state == 3 { 4 } else { 3 };
        }
        fn stop(&mut self) {
            self.state = 5;
        }
        fn seek_relative(&mut self, ms: i64) {
            self.seeks.push(ms);
        }
        fn seek_to(&mut self, ms: i64) {
            self.seeks.push(ms);
        }
        fn set_volume(&mut self, volume: i32) {
            self.volume = volume;
        }
        fn rate(&self) -> f32 {
            1.0
        }
        fn set_rate(&mut self, _: f32) {}
        fn snapshot(&self) -> PlaybackSnapshot {
            PlaybackSnapshot {
                time_ms: 0,
                length_ms: 8000,
                volume: self.volume,
                state: self.state,
                rate: 1.0,
                frames: 0,
            }
        }
        fn subtitle_tracks(&self) -> Vec<MediaTrack> {
            vec![]
        }
        fn current_subtitle_id(&self) -> i32 {
            self.subtitle
        }
        fn select_subtitle(&mut self, id: i32) -> bool {
            if self.reject_tracks {
                return false;
            }
            self.subtitle = id;
            true
        }
        fn audio_tracks(&self) -> Vec<MediaTrack> {
            vec![]
        }
        fn current_audio_id(&self) -> i32 {
            0
        }
        fn select_audio(&mut self, _: i32) -> bool {
            !self.reject_tracks
        }
        fn load_vlc_subtitle(&mut self, _: &Path) -> Result<()> {
            self.subtitle = 0;
            Ok(())
        }
        fn next_frame(&mut self) {
            self.frame_steps += 1;
            self.state = 4;
        }
        fn set_audio_delay(&mut self, ms: i64) -> bool {
            if self.reject_delays {
                return false;
            }
            self.audio_delay = ms;
            true
        }
        fn set_subtitle_delay(&mut self, ms: i64) -> bool {
            if self.reject_delays {
                return false;
            }
            self.subtitle_delay = ms;
            true
        }
    }
}
