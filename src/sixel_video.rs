//! Quantize motion at a bounded working resolution, then scale palette indices
//! to the exact widget pixel dimensions. Static artwork uses the original encoder.
use crate::vlc::VideoFrame;
use quantette::{deps::palette::Srgb, ImageRef, PaletteSize, Pipeline};
use ratatui::layout::Size;
use ratatui_image::{
    protocol::{sixel::Sixel, Protocol},
    FontSize,
};
use std::{fmt::Write, time::Duration};

pub struct VideoBudget {
    pixels: usize,
    fast_frames: u32,
}
impl Default for VideoBudget {
    fn default() -> Self {
        Self {
            pixels: 640 * 360,
            fast_frames: 0,
        }
    }
}
impl VideoBudget {
    pub fn pixels(&self) -> usize {
        self.pixels
    }
    pub fn observe(&mut self, cost: Duration, interval: Duration) {
        if cost > interval.mul_f64(1.15) {
            self.pixels = (self.pixels * 3 / 4).max(320 * 180);
            self.fast_frames = 0;
        } else if cost < interval.mul_f64(0.65) {
            self.fast_frames += 1;
            if self.fast_frames >= 60 {
                self.pixels = (self.pixels * 5 / 4).min(1280 * 720);
                self.fast_frames = 0;
            }
        } else {
            self.fast_frames = 0;
        }
    }
}

pub fn encode(
    frame: &VideoFrame,
    area: Size,
    font: FontSize,
    tmux: bool,
    pixels: usize,
) -> Result<Protocol, String> {
    let area = crate::graphics::bounded_area(area, font)?;
    if frame.width == 0
        || frame.height == 0
        || frame.pitch < frame.width.saturating_mul(4)
        || frame.bgra.len() < frame.pitch.saturating_mul(frame.height)
    {
        return Err("Invalid video frame".into());
    }
    let ratio = (f64::from(area.width) * f64::from(font.width) / frame.width as f64)
        .min(f64::from(area.height) * f64::from(font.height) / frame.height as f64);
    let fitted_height = (frame.height as f64 * ratio).floor().max(1.0) as usize;
    // A Sixel band occupies six physical rows, including its unused bits.
    let height = if fitted_height >= 6 {
        fitted_height / 6 * 6
    } else {
        fitted_height
    };
    let width = (height as f64 * frame.width as f64 / frame.height as f64)
        .floor()
        .max(1.0) as usize;
    let scale = (pixels.max(1) as f64 / (width * height) as f64)
        .sqrt()
        .min(1.0)
        .min(frame.width as f64 / width as f64)
        .min(frame.height as f64 / height as f64);
    let sample_width = (width as f64 * scale).floor().max(1.0) as usize;
    let sample_height = (height as f64 * scale).floor().max(1.0) as usize;
    let mut rgb = Vec::with_capacity(sample_width * sample_height);
    for y in 0..sample_height {
        let row = y * frame.height / sample_height * frame.pitch;
        for x in 0..sample_width {
            let p = row + x * frame.width / sample_width * 4;
            rgb.push(Srgb::new(
                frame.bgra[p + 2],
                frame.bgra[p + 1],
                frame.bgra[p],
            ));
        }
    }
    let image = ImageRef::new(sample_width as u32, sample_height as u32, &rgb)
        .map_err(|e| e.to_string())?;
    let indexed = Pipeline::new()
        .palette_size(PaletteSize::MAX)
        .ditherer(None)
        .input_image(image)
        .output_srgb8_indexed_image();
    let data = encode_indices(
        indexed.palette(),
        indexed.indices(),
        sample_width,
        sample_height,
        width,
        height,
    );
    let data = if tmux {
        format!("\x1bPtmux;{}\x1b\\", data.replace('\x1b', "\x1b\x1b"))
    } else {
        data
    };
    Ok(Protocol::Sixel(Sixel {
        data,
        size: Size::new(
            width.div_ceil(font.width as usize) as u16,
            height.div_ceil(font.height as usize) as u16,
        ),
        is_tmux: tmux,
    }))
}

fn encode_indices(
    palette: &[Srgb<u8>],
    indices: &[u8],
    sw: usize,
    sh: usize,
    width: usize,
    height: usize,
) -> String {
    let mut out = format!("\x1bP0;1;0q\"1;1;{width};{height}");
    for (i, c) in palette.iter().enumerate() {
        let _ = write!(
            out,
            "#{i};2;{};{};{}",
            u32::from(c.red) * 100 / 255,
            u32::from(c.green) * 100 / 255,
            u32::from(c.blue) * 100 / 255
        );
    }
    // Work in sample columns. A run expands directly into display columns,
    // avoiding a full-resolution RGBA allocation and palette lookup pass.
    let mut masks = vec![0u8; palette.len() * sw];
    for y0 in (0..height).step_by(6) {
        masks.fill(0);
        for y in y0..(y0 + 6).min(height) {
            let row = y * sh / height * sw;
            for x in 0..sw {
                masks[usize::from(indices[row + x]) * sw + x] |= 1 << (y - y0);
            }
        }
        for color in 0..palette.len() {
            let row = &masks[color * sw..(color + 1) * sw];
            let Some(last) = row.iter().rposition(|bits| *bits != 0) else {
                continue;
            };
            let _ = write!(out, "#{color}");
            let mut x = 0;
            while x <= last {
                let bits = row[x];
                let mut end = x + 1;
                while end <= last && row[end] == bits {
                    end += 1;
                }
                let count = (end * width).div_ceil(sw) - (x * width).div_ceil(sw);
                let ch = char::from(63 + bits);
                if count > 3 {
                    let _ = write!(out, "!{count}{ch}");
                } else {
                    for _ in 0..count {
                        out.push(ch);
                    }
                }
                x = end;
            }
            out.push('$');
        }
        out.push('-');
    }
    out.push_str("\x1b\\");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adaptive_detail_drops_quickly_and_recovers_slowly_with_bounds() {
        let mut budget = VideoBudget::default();
        for _ in 0..100 {
            budget.observe(Duration::from_millis(200), Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 320 * 180);
        for _ in 0..59 {
            budget.observe(Duration::from_millis(1), Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 320 * 180);
        budget.observe(Duration::from_millis(1), Duration::from_millis(33));
        assert!(budget.pixels() > 320 * 180);
        for _ in 0..2000 {
            budget.observe(Duration::ZERO, Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 1280 * 720);
    }
    #[test]
    fn sampled_palette_scales_to_widget_and_preserves_pixels_across_sixel_bands() {
        let palette = [Srgb::new(255, 0, 0), Srgb::new(0, 0, 255)];
        let indices = [0, 1, 1, 0];
        let data = encode_indices(&palette, &indices, 2, 2, 11, 13);
        let decoded = icy_sixel::SixelImage::decode(data.as_bytes()).unwrap();
        assert_eq!((decoded.width, decoded.height), (11, 18));
        for y in 0..13 {
            for x in 0..11 {
                let expected = if indices[y * 2 / 13 * 2 + x * 2 / 11] == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                assert_eq!(
                    &decoded.pixels[(y * 11 + x) * 4..(y * 11 + x + 1) * 4],
                    &expected
                );
            }
        }
        assert!(decoded.pixels[11 * 13 * 4..]
            .chunks_exact(4)
            .all(|p| p[3] == 0));
    }
    #[test]
    fn resize_changes_display_resolution_without_enlarging_the_working_image() {
        let frame = VideoFrame {
            width: 16,
            height: 9,
            pitch: 64,
            bgra: [10, 60, 200, 255].repeat(16 * 9),
            serial: 1,
        };
        let font = FontSize::new(10, 20);
        let mut sizes = Vec::new();
        for area in [Size::new(80, 24), Size::new(40, 12), Size::new(120, 40)] {
            let Protocol::Sixel(image) = encode(&frame, area, font, false, 640 * 360).unwrap()
            else {
                panic!("Sixel expected")
            };
            let decoded = icy_sixel::SixelImage::decode(image.data.as_bytes()).unwrap();
            assert!(decoded.width <= usize::from(area.width) * 10);
            assert!(decoded.height <= usize::from(area.height) * 20);
            assert!(image.size.width <= area.width && image.size.height <= area.height);
            sizes.push((decoded.width, decoded.height));
        }
        assert!(sizes[1].0 < sizes[0].0 && sizes[1].1 < sizes[0].1);
        assert!(sizes[2].0 > sizes[0].0 && sizes[2].1 > sizes[0].1);
    }
}
