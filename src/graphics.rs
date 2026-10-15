//! High-quality terminal video: Kitty, Sixel, iTerm2, ANSI fallback.
//!
//! Decode stays in LibVLC. Image conversion, scaling and terminal escape
//! encoding happen on a bounded worker thread. The UI keeps the last completed
//! image on screen until the next one is ready: no blank frames during encode.

use std::{
    str::FromStr,
    sync::{Arc, mpsc::{self, Receiver, SyncSender}},
    thread,
    time::{Duration, Instant},
};

use image::{DynamicImage, RgbImage};
use ratatui::{Frame, layout::{Rect, Size}};
use ratatui_image::{Image, Resize, picker::{Picker, ProtocolType}, protocol::Protocol};
use crate::vlc::VideoFrame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsChoice { Auto, Kitty, Sixel, Iterm2, Ansi }

impl FromStr for GraphicsChoice {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "kitty" => Ok(Self::Kitty),
            "sixel" => Ok(Self::Sixel),
            "iterm2" | "iterm" => Ok(Self::Iterm2),
            "ansi" | "blocks" | "halfblocks" => Ok(Self::Ansi),
            _ => Err(format!("invalid --graphics '{s}' (auto, kitty, sixel, iterm2, ansi)")),
        }
    }
}
impl GraphicsChoice {
    fn forced_protocol(self) -> Option<ProtocolType> {
        match self {
            Self::Kitty => Some(ProtocolType::Kitty),
            Self::Sixel => Some(ProtocolType::Sixel),
            Self::Iterm2 => Some(ProtocolType::Iterm2),
            _ => None,
        }
    }
}

struct EncodeRequest {
    frame: Arc<VideoFrame>,
    area: Size,
}
struct EncodeResult {
    serial: u64,
    area: Size,
    result: Result<Protocol, String>,
}

pub struct GraphicsRenderer {
    protocol_kind: ProtocolType,
    detected: ProtocolType,
    request_tx: SyncSender<EncodeRequest>,
    responses: Receiver<EncodeResult>,
    current: Option<Protocol>,
    current_serial: u64,
    area: Size,
    busy: bool,
    last_request_at: Instant,
    frame_interval: Duration,
    ansi_override: bool,
    error_note: Option<String>,
}

impl GraphicsRenderer {
    pub fn new(choice: GraphicsChoice, max_fps: u32) -> Self {
        // Do this after entering raw mode, before the first crossterm read.
        let mut picker = if choice == GraphicsChoice::Ansi { Picker::halfblocks() }
            else { Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()) };
        let detected = picker.protocol_type();
        if let Some(forced) = choice.forced_protocol() { picker.set_protocol_type(forced); }
        let protocol_kind = picker.protocol_type();
        let ansi_override = choice == GraphicsChoice::Ansi || protocol_kind == ProtocolType::Halfblocks;

        // Single inflight frame; latest-only selection, no runaway buffering.
        let (request_tx, requests) = mpsc::sync_channel::<EncodeRequest>(1);
        let (response_tx, responses) = mpsc::sync_channel::<EncodeResult>(1);
        thread::spawn(move || {
            while let Ok(req) = requests.recv() {
                let mut result = bgra_to_rgb(&req.frame)
                    .ok_or_else(|| "invalid BGRA frame or stride".to_string())
                    .and_then(|rgb| picker.new_protocol(
                        DynamicImage::ImageRgb8(rgb), req.area,
                        Resize::Scale(Some(image::imageops::FilterType::Triangle)),
                    ).map_err(|e| e.to_string()));
                if let Ok(ref mut proto) = result {
                    strip_clear_area(proto);
                }
                let result = EncodeResult { serial: req.frame.serial, area: req.area, result };
                if response_tx.send(result).is_err() { break; }
            }
        });
        Self {
            protocol_kind, detected, request_tx, responses,
            current: None, current_serial: 0, area: Size::default(),
            busy: false, last_request_at: Instant::now() - Duration::from_secs(1),
            frame_interval: Duration::from_millis(1000 / max_fps.clamp(1, 60) as u64),
            ansi_override, error_note: None,
        }
    }

    pub fn is_ansi(&self) -> bool { self.ansi_override }
    pub fn label(&self) -> String {
        if self.ansi_override { "ANSI half-block fallback".into() }
        else { format!("{:?} pixel graphics", self.protocol_kind) }
    }
    pub fn detected_label(&self) -> String { format!("{:?}", self.detected) }
    #[allow(dead_code)]
    pub fn warning(&self) -> Option<&str> { self.error_note.as_deref() }

    /// 'g' toggles between pixel graphics and ANSI without destroying state.
    pub fn toggle_ansi(&mut self) {
        if self.protocol_kind != ProtocolType::Halfblocks {
            self.ansi_override = !self.ansi_override;
            if !self.ansi_override { self.error_note = None; }
        }
    }

    pub fn prepare(&mut self, frame: Option<Arc<VideoFrame>>) {
        while let Ok(completed) = self.responses.try_recv() {
            self.busy = false;
            match completed.result {
                Ok(proto) => {
                    if completed.area == self.area {
                        self.current = Some(proto);
                        self.current_serial = completed.serial;
                    }
                }
                Err(e) => {
                    self.ansi_override = true;
                    self.error_note = Some(format!("Graphics encode failed: {e}"));
                }
            }
        }
        if self.ansi_override || self.busy || self.area.width == 0 || self.area.height == 0
            || self.last_request_at.elapsed() < self.frame_interval { return; }
        if let Some(frame) = frame {
            if frame.serial == self.current_serial { return; }
            if self.request_tx.try_send(EncodeRequest { frame, area: self.area }).is_ok() {
                self.busy = true;
                self.last_request_at = Instant::now();
            }
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        if self.ansi_override || area.width == 0 || area.height == 0 { return; }
        let size = Size::new(area.width, area.height);
        if size != self.area {
            self.area = size;
            // Old frame is retained until a new frame arrives for the new area.
            // Sizing check below ensures it won't be drawn if it exceeds the area.
        }
        if let Some(ref protocol) = self.current {
            let size = protocol.size();
            if size.width <= area.width && size.height <= area.height {
                let centered = Rect::new(
                    area.x + (area.width - size.width) / 2,
                    area.y + (area.height - size.height) / 2,
                    size.width, size.height,
                );
                f.render_widget(Image::new(protocol), centered);
            }
        }
    }
}

/// ratatui-image prepends an ECH clear_area sequence (`\x1b[...X`) meant for static transparent
/// images. In high-frequency video playback, erasing the cell rectangle to black before every
/// frame causes the terminal to flash black (blinking). Stripping this allows subsequent frames
/// to seamlessly overwrite previous pixels without clearing the area first.
fn strip_clear_area(proto: &mut Protocol) {
    match proto {
        Protocol::Sixel(ref mut sixel) => {
            if sixel.is_tmux {
                if let Some(pos) = sixel.data.find("\x1b\x1bP") {
                    let start = "\x1bPtmux;\x1b";
                    let payload = &sixel.data[pos..];
                    sixel.data = format!("{start}{payload}");
                }
            } else if let Some(pos) = sixel.data.find("\x1bP") {
                if pos > 0 {
                    sixel.data.drain(..pos);
                }
            }
        }
        Protocol::ITerm2(ref mut iterm2) => {
            if iterm2.is_tmux {
                if let Some(pos) = iterm2.data.find("\x1b\x1b]1337;") {
                    let start = "\x1bPtmux;\x1b";
                    let payload = &iterm2.data[pos..];
                    iterm2.data = format!("{start}{payload}");
                }
            } else if let Some(pos) = iterm2.data.find("\x1b]1337;") {
                if pos > 0 {
                    iterm2.data.drain(..pos);
                }
            }
        }
        _ => {}
    }
}

fn bgra_to_rgb(frame: &VideoFrame) -> Option<RgbImage> {
    let (w, h) = (frame.width, frame.height);
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 { return None; }
    if frame.pitch < w.checked_mul(4)? { return None; }
    if frame.bgra.len() < frame.pitch.checked_mul(h)? { return None; }
    let mut rgb = Vec::with_capacity(w.checked_mul(h)?.checked_mul(3)?);
    for row in frame.bgra.chunks_exact(frame.pitch).take(h) {
        for px in row[..w * 4].chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    RgbImage::from_raw(w as u32, h as u32, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::protocol::sixel::Sixel;

    #[test]
    fn choice_parsing() {
        assert_eq!("kitty".parse::<GraphicsChoice>().unwrap(), GraphicsChoice::Kitty);
        assert_eq!("blocks".parse::<GraphicsChoice>().unwrap(), GraphicsChoice::Ansi);
        assert!("not-valid".parse::<GraphicsChoice>().is_err());
    }
    #[test]
    fn stride_and_rgb_conversion() {
        let frame = VideoFrame { width: 2, height: 1, pitch: 12,
            bgra: vec![10,20,30,255,40,50,60,255,0,0,0,0], serial: 1 };
        let img = bgra_to_rgb(&frame).unwrap();
        assert_eq!(img.get_pixel(0,0).0, [30,20,10]);
        assert_eq!(img.get_pixel(1,0).0, [60,50,40]);
    }
    #[test]
    fn rejects_bad_stride() {
        let frame = VideoFrame { width: 3, height: 1, pitch: 4, bgra: vec![0;4], serial: 1 };
        assert!(bgra_to_rgb(&frame).is_none());
    }
    #[test]
    fn strips_clear_area_from_sixel() {
        let mut proto = Protocol::Sixel(Sixel {
            data: "\x1b[80X\x1b[1B\x1b[22A\x1bPq#0;2;0;0;0#1...".to_string(),
            size: Size::new(80, 22),
            is_tmux: false,
        });
        strip_clear_area(&mut proto);
        if let Protocol::Sixel(s) = proto {
            assert!(s.data.starts_with("\x1bPq"));
            assert!(!s.data.contains("\x1b[80X"));
        } else {
            panic!("expected Sixel protocol");
        }
    }
}
