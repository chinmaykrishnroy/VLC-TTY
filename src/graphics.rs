//! High-quality terminal video: Kitty, Sixel, iTerm2, ANSI fallback.
//!
//! Decode stays in LibVLC. Image conversion, scaling and terminal escape
//! encoding happen on a bounded worker thread. The UI keeps the last completed
//! image on screen until the next one is ready: no blank frames during encode.

use std::{
    str::FromStr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crate::profiling::EncoderMetrics;
use crate::{latest, vlc::VideoFrame};
use image::{DynamicImage, RgbImage, Rgba};
use ratatui::{
    layout::{Rect, Size},
    Frame,
};
use ratatui_image::{picker::ProtocolType, protocol::Protocol, FontSize, Image, Resize};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsChoice {
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Ansi,
}

impl FromStr for GraphicsChoice {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "kitty" => Ok(Self::Kitty),
            "sixel" => Ok(Self::Sixel),
            "iterm2" | "iterm" => Ok(Self::Iterm2),
            "ansi" | "blocks" | "halfblocks" => Ok(Self::Ansi),
            _ => Err(format!(
                "invalid --graphics '{s}' (auto, kitty, sixel, iterm2, ansi)"
            )),
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
    generation: u64,
    frame: Arc<VideoFrame>,
    area: Size,
    queued_at: Instant,
    motion: bool,
}
struct EncodeResult {
    generation: u64,
    serial: u64,
    area: Size,
    queued_at: Instant,
    result: Result<Protocol, String>,
}

pub struct GraphicsRenderer {
    protocol_kind: ProtocolType,
    detected: ProtocolType,
    request_tx: latest::Sender<EncodeRequest>,
    responses: latest::Receiver<EncodeResult>,
    current: Option<Protocol>,
    current_serial: u64,
    current_requested_at: Option<Instant>,
    area: Size,
    generation: u64,
    last_submitted: Option<(u64, u64, Size)>,
    last_request_at: Instant,
    frame_interval: Duration,
    ansi_override: bool,
    error_note: Option<String>,
    kitty_id: u32,
    is_tmux: bool,
    metrics: Option<Arc<EncoderMetrics>>,
    encode_latency_us: Arc<std::sync::atomic::AtomicU64>,
    draw_latency_us: Arc<std::sync::atomic::AtomicU64>,
    feedback_frame: Option<(u64, u64)>,
    drawn: Option<(u64, u64)>,
    startup_events: std::collections::VecDeque<ratatui::crossterm::event::Event>,
}

impl GraphicsRenderer {
    pub fn new(
        choice: GraphicsChoice,
        max_fps: u32,
        metrics: Option<Arc<EncoderMetrics>>,
        allow_compression: bool,
    ) -> Self {
        // Do this after entering raw mode, before the first crossterm read.
        let detection = crate::terminal_probe::detect(choice != GraphicsChoice::Ansi);
        let mut picker = detection.picker;
        let startup_events = detection.events;
        let detected = picker.protocol_type();
        if let Some(forced) = choice.forced_protocol() {
            picker.set_protocol_type(forced);
        }
        let protocol_kind = picker.protocol_type();
        let ansi_override =
            choice == GraphicsChoice::Ansi || protocol_kind == ProtocolType::Halfblocks;

        // Single inflight frame; latest-only selection, no runaway buffering.
        let (request_tx, requests) = latest::channel::<EncodeRequest>();
        let (response_tx, responses) = latest::channel::<EncodeResult>();
        let font_size = picker.font_size();
        let is_tmux = picker.tmux_detected();
        let kitty_id = 0x5600_0000 | (std::process::id() & 0x00ff_ffff);
        let compress = detection.kitty_compression && allow_compression;
        let worker_metrics = metrics.clone();
        let encode_latency_us = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_latency = encode_latency_us.clone();
        let draw_latency_us = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_draw_latency = draw_latency_us.clone();
        thread::spawn(move || {
            let mut budget = crate::sixel_video::VideoBudget::default();
            while let Some(req) = requests.recv() {
                let started = Instant::now();
                if let Some(metrics) = &worker_metrics {
                    metrics.queue_wait.observe(req.queued_at.elapsed());
                    if req.motion {
                        metrics
                            .work_pixel_budget
                            .store(budget.pixels() as u64, Ordering::Relaxed);
                    }
                }
                let result = if protocol_kind == ProtocolType::Sixel && req.motion {
                    crate::sixel_video::encode(
                        &req.frame,
                        req.area,
                        font_size,
                        is_tmux,
                        budget.pixels(),
                    )
                } else if protocol_kind == ProtocolType::Sixel {
                    encode_sixel_fast(&req.frame, req.area, font_size, is_tmux)
                } else {
                    encode_pixels(
                        &req.frame,
                        req.area,
                        font_size,
                        protocol_kind,
                        is_tmux,
                        kitty_id,
                        compress,
                    )
                };
                if req.motion {
                    budget.observe(
                        started.elapsed().max(Duration::from_micros(
                            worker_draw_latency.load(Ordering::Relaxed),
                        )),
                        Duration::from_millis(1000 / max_fps.clamp(1, 120) as u64),
                    );
                }
                worker_latency.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                if let Some(metrics) = &worker_metrics {
                    metrics.encode.observe(started.elapsed());
                    if result.is_ok() {
                        metrics.encoded.fetch_add(1, Ordering::Relaxed);
                    } else {
                        metrics.failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let result = EncodeResult {
                    generation: req.generation,
                    serial: req.frame.serial,
                    area: req.area,
                    queued_at: req.queued_at,
                    result,
                };
                match response_tx.replace(result) {
                    Ok(true) => {
                        if let Some(metrics) = &worker_metrics {
                            metrics.responses_replaced.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        });
        Self {
            protocol_kind,
            detected,
            request_tx,
            responses,
            current: None,
            current_serial: 0,
            current_requested_at: None,
            area: Size::default(),
            generation: 0,
            last_submitted: None,
            last_request_at: Instant::now() - Duration::from_secs(1),
            frame_interval: Duration::from_millis(1000 / max_fps.clamp(1, 120) as u64),
            ansi_override,
            error_note: None,
            kitty_id,
            is_tmux,
            metrics,
            encode_latency_us,
            draw_latency_us,
            feedback_frame: None,
            drawn: None,
            startup_events,
        }
    }

    pub fn encode_latency(&self) -> Duration {
        Duration::from_micros(self.encode_latency_us.load(Ordering::Relaxed))
    }

    pub fn take_startup_event(&mut self) -> Option<ratatui::crossterm::event::Event> {
        self.startup_events.pop_front()
    }

    /// Clear the cached frame on media changes, fullscreen transitions and resizes.
    /// In-flight worker results for the old rectangle are ignored after resize.
    pub fn invalidate(&mut self) {
        self.drawn = None;
        self.clear_owned_image();
        self.generation = self.generation.wrapping_add(1);
        self.last_submitted = None;
        self.request_tx.clear();
        self.current = None;
        self.current_serial = 0;
        self.current_requested_at = None;
        self.area = Size::default();
        self.last_request_at = Instant::now() - Duration::from_secs(1);
    }

    fn clear_owned_image(&self) {
        use std::io::{IsTerminal, Write};
        if self.protocol_kind == ProtocolType::Kitty && std::io::stdout().is_terminal() {
            let command = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", self.kitty_id);
            let command = if self.is_tmux {
                tmux_wrap(&command)
            } else {
                command
            };
            let _ = std::io::stdout().write_all(command.as_bytes());
        }
    }

    pub fn is_ansi(&self) -> bool {
        self.ansi_override
    }
    pub fn label(&self) -> String {
        if self.ansi_override {
            "ANSI half-block fallback".into()
        } else {
            format!("{:?} pixel graphics", self.protocol_kind)
        }
    }
    pub fn detected_label(&self) -> String {
        format!("{:?}", self.detected)
    }
    pub fn warning(&self) -> Option<&str> {
        self.error_note.as_deref()
    }

    /// 'g' toggles between pixel graphics and ANSI without destroying state.
    pub fn toggle_ansi(&mut self) {
        if self.protocol_kind != ProtocolType::Halfblocks {
            self.ansi_override = !self.ansi_override;
            if !self.ansi_override {
                self.error_note = None;
            }
        }
    }

    pub fn prepare(&mut self, frame: Option<Arc<VideoFrame>>, motion: bool) {
        if let Some(completed) = self.responses.try_take().filter(|completed| {
            let valid = completed.generation == self.generation && completed.area == self.area;
            if !valid {
                if let Some(metrics) = &self.metrics {
                    metrics.stale.fetch_add(1, Ordering::Relaxed);
                }
            }
            valid
        }) {
            match completed.result {
                Ok(proto) => {
                    if completed.area == self.area {
                        self.current = Some(proto);
                        self.current_serial = completed.serial;
                        self.current_requested_at = Some(completed.queued_at);
                    }
                }
                Err(e) => {
                    self.ansi_override = true;
                    self.error_note = Some(format!("Graphics encode failed: {e}"));
                }
            }
        }
        // Replace pending work with the newest frame; an encoder never builds a FIFO backlog.
        if self.ansi_override
            || self.area.width == 0
            || self.area.height == 0
            || self.last_request_at.elapsed() < self.frame_interval
        {
            return;
        }
        if let Some(frame) = frame {
            let key = (self.generation, frame.serial, self.area);
            if frame.serial == self.current_serial || self.last_submitted == Some(key) {
                return;
            }
            match self.request_tx.replace(EncodeRequest {
                generation: self.generation,
                frame,
                area: self.area,
                queued_at: Instant::now(),
                motion,
            }) {
                Ok(replaced) => {
                    if let Some(metrics) = &self.metrics {
                        metrics.requests.fetch_add(1, Ordering::Relaxed);
                        if replaced {
                            metrics.pending_replaced.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    self.last_submitted = Some(key);
                    self.last_request_at = Instant::now();
                }
                Err(_) => {
                    self.ansi_override = true;
                    self.error_note = Some("Graphics worker stopped; using ANSI".into());
                }
            }
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        if self.ansi_override || area.width == 0 || area.height == 0 {
            return;
        }
        let size = Size::new(area.width, area.height);
        if size != self.area {
            self.invalidate();
            self.area = size;
            // Old frame stays until a correctly sized new frame arrives, but
            // can't be drawn into a smaller area without clipping artifacts.
            self.current = None;
            self.current_serial = 0;
        }
        if let Some(ref protocol) = self.current {
            let size = protocol.size();
            let w = size.width.min(area.width);
            let h = size.height.min(area.height);
            if w > 0 && h > 0 {
                let centered = Rect::new(
                    area.x + (area.width.saturating_sub(w)) / 2,
                    area.y + (area.height.saturating_sub(h)) / 2,
                    w,
                    h,
                );
                f.render_widget(Image::new(protocol), centered);
                self.drawn = Some((self.generation, self.current_serial));
            }
        }
    }
    pub fn mark_ansi_drawn(&mut self, serial: u64) {
        self.drawn = Some((self.generation, serial));
    }
    pub fn take_drawn(&mut self) -> Option<(u64, u64)> {
        self.drawn.take()
    }
    pub fn observe_draw(&mut self, elapsed: Duration) {
        if self.drawn.is_some() && self.drawn != self.feedback_frame {
            if let (Some(metrics), Some(requested)) = (&self.metrics, self.current_requested_at) {
                metrics.request_to_backend.observe(requested.elapsed());
            }
            self.draw_latency_us
                .store(elapsed.as_micros() as u64, Ordering::Relaxed);
            self.feedback_frame = self.drawn;
        }
    }
}

impl Drop for GraphicsRenderer {
    fn drop(&mut self) {
        self.clear_owned_image();
    }
}

fn tmux_wrap(payload: &str) -> String {
    format!("\x1bPtmux;{}\x1b\\", payload.replace('\x1b', "\x1b\x1b"))
}

pub(crate) fn bounded_area(area: Size, font: FontSize) -> Result<Size, String> {
    if area.width == 0
        || area.height == 0
        || font.width == 0
        || font.height == 0
        || font.width > 256
        || font.height > 512
    {
        return Err("Invalid terminal area or cell dimensions".into());
    }
    let pixels = f64::from(area.width)
        * f64::from(font.width)
        * f64::from(area.height)
        * f64::from(font.height);
    let scale = (4_194_304.0 / pixels).sqrt().min(1.0);
    let max_cells = 4_194_304 / (u32::from(font.width) * u32::from(font.height));
    let height = ((f64::from(area.height) * scale).floor().max(1.0) as u32).min(max_cells);
    let width = ((f64::from(area.width) * scale).floor().max(1.0) as u32).min(max_cells / height);
    Ok(Size::new(width as u16, height as u16))
}

fn encode_pixels(
    frame: &VideoFrame,
    area: Size,
    font: FontSize,
    kind: ProtocolType,
    is_tmux: bool,
    kitty_id: u32,
    compress: bool,
) -> Result<Protocol, String> {
    let area = bounded_area(area, font)?;
    let image = DynamicImage::ImageRgb8(bgra_to_rgb(frame).ok_or("Invalid BGRA frame or stride")?);
    let resize = Resize::Scale(Some(image::imageops::FilterType::Nearest));
    let size = resize.size_for(&image, font, area);
    let image = resize.resize(&image, font, size, Some(Rgba([0, 0, 0, 255])));
    let result = match kind {
        // Reuse one owned ID rather than leaking a different terminal image for every frame.
        ProtocolType::Kitty => {
            ratatui_image::protocol::kitty::Kitty::new(image, size, kitty_id, is_tmux, compress)
                .map(Protocol::Kitty)
        }
        ProtocolType::Iterm2 => {
            ratatui_image::protocol::iterm2::Iterm2::new(image, size, is_tmux).map(Protocol::ITerm2)
        }
        ProtocolType::Halfblocks => {
            ratatui_image::protocol::halfblocks::Halfblocks::new(image, size)
                .map(Protocol::Halfblocks)
        }
        ProtocolType::Sixel => unreachable!("Sixel uses the direct encoder"),
    };
    result.map_err(|error| error.to_string())
}

/// Direct Sixel encoding with nearest-neighbor BGRA scaling and Wu quantization.
/// Disable diffusion dithering and omit the static-image ECH erase prefix to
/// reduce encoding work and avoid clearing the rectangle before each video frame.
fn encode_sixel_fast(
    frame: &VideoFrame,
    area: Size,
    font: FontSize,
    is_tmux: bool,
) -> Result<Protocol, String> {
    let area = bounded_area(area, font)?;
    if frame.width == 0 || frame.height == 0 || area.width == 0 || area.height == 0 {
        return Err("empty frame or area".to_string());
    }
    let font_w = if font.width == 0 {
        9.0
    } else {
        font.width as f64
    };
    let font_h = if font.height == 0 {
        18.0
    } else {
        font.height as f64
    };
    let max_w = (area.width as f64 * font_w).max(1.0);
    let max_h = (area.height as f64 * font_h).max(1.0);

    let wratio = max_w / frame.width as f64;
    let hratio = max_h / frame.height as f64;
    let ratio = wratio.min(hratio);
    let target_w = ((frame.width as f64 * ratio).round() as usize).max(1);
    let target_h = ((frame.height as f64 * ratio).round() as usize).max(1);

    let cell_w = ((target_w as f64 / font_w).ceil() as u16).min(area.width);
    let cell_h = ((target_h as f64 / font_h).ceil() as u16).min(area.height);

    let mut rgba = vec![0u8; target_w * target_h * 4];
    let fw = frame.width;
    let fh = frame.height;
    for y in 0..target_h {
        let src_y = (y * fh / target_h).min(fh - 1);
        let src_row = src_y * frame.pitch;
        let dst_row = y * target_w * 4;
        for x in 0..target_w {
            let src_x = (x * fw / target_w).min(fw - 1);
            let src_idx = src_row + src_x * 4;
            let dst_idx = dst_row + x * 4;
            if src_idx + 2 < frame.bgra.len() {
                rgba[dst_idx] = frame.bgra[src_idx + 2]; // R
                rgba[dst_idx + 1] = frame.bgra[src_idx + 1]; // G
                rgba[dst_idx + 2] = frame.bgra[src_idx]; // B
                rgba[dst_idx + 3] = 255; // A
            }
        }
    }

    let opts = icy_sixel::EncodeOptions {
        max_colors: 256,
        diffusion: 0.0,
        quantize_method: icy_sixel::QuantizeMethod::Wu,
    };
    let sixel_str = icy_sixel::sixel_encode(&rgba, target_w, target_h, &opts)
        .map_err(|e| format!("Sixel encode: {e}"))?;

    let final_data = if is_tmux {
        tmux_wrap(&sixel_str)
    } else {
        sixel_str
    };

    Ok(Protocol::Sixel(ratatui_image::protocol::sixel::Sixel {
        data: final_data,
        size: Size::new(cell_w, cell_h),
        is_tmux,
    }))
}

fn bgra_to_rgb(frame: &VideoFrame) -> Option<RgbImage> {
    let (w, h) = (frame.width, frame.height);
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return None;
    }
    if frame.pitch < w.checked_mul(4)? {
        return None;
    }
    if frame.bgra.len() < frame.pitch.checked_mul(h)? {
        return None;
    }
    let mut rgb = Vec::with_capacity(w.checked_mul(h)?.checked_mul(3)?);
    for row in frame.bgra.chunks_exact(frame.pitch).take(h) {
        for px in row[..w * 4].as_chunks::<4>().0 {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    RgbImage::from_raw(w as u32, h as u32, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol_output(proto: &Protocol) -> String {
        use ratatui::{buffer::Buffer, widgets::Widget};
        let size = proto.size();
        let mut buf = Buffer::empty(Rect::new(0, 0, size.width, size.height));
        Image::new(proto).render(buf.area, &mut buf);
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn compressed_kitty_preserves_pixels_and_reduces_compressible_payload() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use std::io::Read;
        let frame = VideoFrame {
            width: 32,
            height: 16,
            pitch: 32 * 4,
            bgra: [20, 60, 130, 255].repeat(32 * 16),
            serial: 1,
        };
        let mut outputs = Vec::new();
        for compress in [false, true] {
            let proto = encode_pixels(
                &frame,
                Size::new(8, 2),
                FontSize::new(8, 16),
                ProtocolType::Kitty,
                false,
                12345,
                compress,
            )
            .unwrap();
            let output = protocol_output(&proto);
            assert_eq!(output.contains("o=z"), compress);
            assert!(output.contains("t=d") && output.contains("i=12345"));
            let payload: String = output
                .split("\x1b_G")
                .skip(1)
                .map(|part| {
                    part.split_once(';')
                        .unwrap()
                        .1
                        .split("\x1b\\")
                        .next()
                        .unwrap()
                })
                .collect();
            let bytes = STANDARD.decode(payload).unwrap();
            let pixels = if compress {
                let mut decoded = Vec::new();
                flate2::read::ZlibDecoder::new(bytes.as_slice())
                    .read_to_end(&mut decoded)
                    .unwrap();
                decoded
            } else {
                bytes
            };
            assert_eq!(pixels, [130, 60, 20, 255].repeat(64 * 32));
            outputs.push(output.len());
        }
        assert!(
            outputs[1] < outputs[0] / 10,
            "compression should reduce flat artwork: {outputs:?}"
        );
    }

    #[test]
    #[ignore = "manual encoder measurement, not a real terminal FPS benchmark"]
    fn kitty_encoding_benchmark() {
        let mut frame = VideoFrame {
            width: 1280,
            height: 720,
            pitch: 1280 * 4,
            bgra: vec![0; 1280 * 720 * 4],
            serial: 1,
        };
        for noisy in [false, true] {
            let mut seed = 1234567u32;
            for (i, px) in frame.bgra.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let pixel = if noisy {
                    seed.to_le_bytes()
                } else {
                    [
                        (i % 1280 / 5) as u8,
                        (i / 1280 / 3) as u8,
                        (i / 1280 / 8) as u8,
                        255,
                    ]
                };
                px.copy_from_slice(&pixel);
            }
            for compress in [false, true] {
                let mut bytes = 0;
                let mut elapsed = Duration::ZERO;
                for _ in 0..8 {
                    let start = Instant::now();
                    let proto = encode_pixels(
                        &frame,
                        Size::new(100, 32),
                        FontSize::new(10, 20),
                        ProtocolType::Kitty,
                        false,
                        12345,
                        compress,
                    )
                    .unwrap();
                    elapsed += start.elapsed();
                    bytes += protocol_output(&proto).len();
                }
                println!("{{\"noisy\":{noisy},\"compressed\":{compress},\"mean_encode_us\":{},\"mean_protocol_bytes\":{}}}", elapsed.as_micros() / 8, bytes / 8);
            }
        }
    }
    fn test_sixel() -> Protocol {
        Protocol::Sixel(ratatui_image::protocol::sixel::Sixel {
            data: "\x1bPq#0~-\x1b\\".into(),
            size: Size::new(2, 1),
            is_tmux: false,
        })
    }

    #[test]
    fn invalidation_rejects_old_frame_even_at_same_dimensions() {
        let mut renderer = GraphicsRenderer::new(GraphicsChoice::Ansi, 60, None, true);
        let (sender, receiver) = latest::channel();
        renderer.responses = receiver;
        let size = Size::new(2, 1);
        renderer.area = size;
        let old_generation = renderer.generation;
        renderer.invalidate();
        renderer.area = size;
        assert!(sender
            .replace(EncodeResult {
                generation: old_generation,
                serial: 7,
                area: size,
                queued_at: Instant::now(),
                result: Ok(test_sixel())
            })
            .is_ok());
        renderer.prepare(None, true);
        assert!(
            renderer.current.is_none(),
            "A completed frame from before invalidation must not reappear"
        );
    }

    #[test]
    fn tmux_sixel_escapes_every_inner_escape() {
        let frame = VideoFrame {
            width: 2,
            height: 2,
            pitch: 8,
            bgra: vec![128; 16],
            serial: 1,
        };
        let Protocol::Sixel(proto) =
            encode_sixel_fast(&frame, Size::new(2, 1), FontSize::new(8, 16), true).unwrap()
        else {
            panic!("expected Sixel");
        };
        let payload = proto
            .data
            .strip_prefix("\x1bPtmux;")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap();
        assert!(payload.starts_with("\x1b\x1bP"));
        assert!(
            payload.ends_with("\x1b\x1b\\"),
            "Inner terminator must be escaped through tmux"
        );
    }
    #[test]
    fn choice_parsing() {
        assert_eq!(
            "kitty".parse::<GraphicsChoice>().unwrap(),
            GraphicsChoice::Kitty
        );
        assert_eq!(
            "blocks".parse::<GraphicsChoice>().unwrap(),
            GraphicsChoice::Ansi
        );
        assert!("not-valid".parse::<GraphicsChoice>().is_err());
    }
    #[test]
    fn stride_and_rgb_conversion() {
        let frame = VideoFrame {
            width: 2,
            height: 1,
            pitch: 12,
            bgra: vec![10, 20, 30, 255, 40, 50, 60, 255, 0, 0, 0, 0],
            serial: 1,
        };
        let img = bgra_to_rgb(&frame).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10]);
        assert_eq!(img.get_pixel(1, 0).0, [60, 50, 40]);
    }
    #[test]
    fn rejects_bad_stride() {
        let frame = VideoFrame {
            width: 3,
            height: 1,
            pitch: 4,
            bgra: vec![0; 4],
            serial: 1,
        };
        assert!(bgra_to_rgb(&frame).is_none());
    }
    #[test]
    fn terminal_pixel_budget_is_bounded() {
        for font in [FontSize::new(16, 32), FontSize::new(256, 512)] {
            for input in [
                Size::new(u16::MAX, u16::MAX),
                Size::new(u16::MAX, 1),
                Size::new(1, u16::MAX),
            ] {
                let area = bounded_area(input, font).unwrap();
                assert!(
                    u64::from(area.width)
                        * u64::from(area.height)
                        * u64::from(font.width)
                        * u64::from(font.height)
                        <= 4_194_304
                );
            }
        }
        assert!(bounded_area(Size::new(1, 1), FontSize::new(0, 16)).is_err());
    }

    #[test]
    fn kitty_frames_use_inline_data_and_reuse_the_owned_id() {
        use ratatui::{buffer::Buffer, widgets::Widget};
        let frame = VideoFrame {
            width: 2,
            height: 2,
            pitch: 8,
            bgra: vec![128; 16],
            serial: 1,
        };
        for _ in 0..2 {
            let proto = encode_pixels(
                &frame,
                Size::new(2, 1),
                FontSize::new(8, 16),
                ProtocolType::Kitty,
                false,
                12345,
                false,
            )
            .unwrap();
            let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
            Image::new(&proto).render(buf.area, &mut buf);
            let output: String = buf.content.iter().map(|cell| cell.symbol()).collect();
            assert!(
                output.contains("t=d"),
                "SSH requires inline transport: {output:?}"
            );
            assert!(
                output.contains("i=12345"),
                "Every frame must replace our owned image"
            );
        }
    }
    #[test]
    fn direct_fast_sixel_encoding() {
        let frame = VideoFrame {
            width: 4,
            height: 4,
            pitch: 16,
            bgra: vec![128; 64],
            serial: 1,
        };
        let res = encode_sixel_fast(&frame, Size::new(2, 1), FontSize::new(10, 20), false);
        assert!(res.is_ok());
        if let Ok(Protocol::Sixel(s)) = res {
            assert!(s.data.starts_with("\x1bP"));
            assert!(
                !s.data.contains("\x1b["),
                "Video encoding must not prefix a flashing erase sequence"
            );
            assert_eq!(s.size.width, 2);
            assert_eq!(s.size.height, 1);
        } else {
            panic!("expected Sixel protocol");
        }
    }
    #[test]
    fn fullscreen_sixel_dimensions() {
        let frame = VideoFrame {
            width: 1920,
            height: 1080,
            pitch: 1920 * 4,
            bgra: vec![128; 1920 * 1080 * 4],
            serial: 1,
        };
        // Simulated fullscreen: 180 cols x 45 rows, font 9x18
        let area = Size::new(180, 45);
        let font = FontSize::new(9, 18);
        let res = encode_sixel_fast(&frame, area, font, false);
        assert!(res.is_ok(), "encode failed: {:?}", res.err());
        let proto = res.unwrap();
        let size = proto.size();
        assert!(
            size.width <= area.width,
            "width {} > area {}",
            size.width,
            area.width
        );
        assert!(
            size.height <= area.height,
            "height {} > area {}",
            size.height,
            area.height
        );
    }
}
