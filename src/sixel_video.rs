//! Fit motion to the widget without sacrificing spatial detail under pressure.
//! Only palette training is sampled; every display pixel maps from the decoder.
//! Static artwork uses the original encoder.
use crate::vlc::VideoFrame;
use quantette::{
    color_map::{IndexedColorMap, NearestNeighborColorMap},
    deps::palette::Srgb,
    ImageRef, PaletteSize, Pipeline,
};
use ratatui::layout::Size;
use ratatui_image::{
    protocol::{sixel::Sixel, Protocol},
    FontSize,
};
use std::{fmt::Write, time::Duration};

pub struct PaletteBudget {
    pixels: usize,
    fast_frames: u32,
}
impl Default for PaletteBudget {
    fn default() -> Self {
        Self {
            pixels: 320 * 180,
            fast_frames: 0,
        }
    }
}
impl PaletteBudget {
    pub fn pixels(&self) -> usize {
        self.pixels
    }
    pub fn observe(&mut self, cost: Duration, interval: Duration) {
        if cost > interval.mul_f64(1.15) {
            self.pixels = (self.pixels * 3 / 4).max(160 * 90);
            self.fast_frames = 0;
        } else if cost < interval.mul_f64(0.65) {
            self.fast_frames += 1;
            if self.fast_frames >= 60 {
                self.pixels = (self.pixels * 5 / 4).min(640 * 360);
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
    // Stable stratified sampling avoids a regular stride completely missing
    // thin lines or periodic textures. It never becomes the displayed image.
    for y in 0..sample_height {
        let y0 = y * frame.height / sample_height;
        let y1 = (y + 1) * frame.height / sample_height;
        for x in 0..sample_width {
            let x0 = x * frame.width / sample_width;
            let x1 = (x + 1) * frame.width / sample_width;
            let seed = (x as u32).wrapping_mul(0x9e3779b9) ^ (y as u32).wrapping_mul(0x85ebca6b);
            let sx = x0 + seed as usize % (x1 - x0);
            let sy = y0 + (seed >> 16) as usize % (y1 - y0);
            let p = sy * frame.pitch + sx * 4;
            rgb.push(Srgb::new(
                frame.bgra[p + 2],
                frame.bgra[p + 1],
                frame.bgra[p],
            ));
        }
    }
    let image = ImageRef::new(sample_width as u32, sample_height as u32, &rgb)
        .map_err(|e| e.to_string())?;
    let palette = Pipeline::new()
        .palette_size(PaletteSize::MAX)
        .ditherer(None)
        .input_image(image)
        .output_srgb8_palette()
        .ok_or("Empty palette")?;
    let palette = palette.map_ref(|c| {
        Srgb::new(
            (((u32::from(c.red) * 100 + 127) / 255) * 255 / 100) as u8,
            (((u32::from(c.green) * 100 + 127) / 255) * 255 / 100) as u8,
            (((u32::from(c.blue) * 100 + 127) / 255) * 255 / 100) as u8,
        )
    });
    let mapping = NearestNeighborColorMap::new(palette.clone());
    // Cache colour matches, not positions: 4 MiB, discarded when this frame's
    // palette changes. Adjacent RGB values can share a match; fine edges cannot.
    let mut cache = vec![256u16; 128 * 128 * 128];
    // Upscaling cannot add source detail. Map each native source pixel once,
    // then expand its Sixel runs to the widget, retaining every decoded stroke.
    let (detail_width, detail_height) = if width >= frame.width && height >= frame.height {
        (frame.width, frame.height)
    } else {
        (width, height)
    };
    let mut indices = Vec::with_capacity(detail_width * detail_height);
    for y in 0..detail_height {
        let row = y * frame.height / detail_height * frame.pitch;
        for x in 0..detail_width {
            let p = row + x * frame.width / detail_width * 4;
            let offset = [[-1i16, 0], [0, 1]][y % 2][x % 2];
            let channel = |c: u8| (i16::from(c) + offset).clamp(0, 255) as u8;
            let color = Srgb::new(
                channel(frame.bgra[p + 2]),
                channel(frame.bgra[p + 1]),
                channel(frame.bgra[p]),
            );
            let key = ((usize::from(color.red) >> 1) << 14)
                | ((usize::from(color.green) >> 1) << 7)
                | (usize::from(color.blue) >> 1);
            let slot = &mut cache[key];
            if *slot == 256 {
                *slot = u16::from(mapping.palette_index(&color));
            }
            indices.push(*slot as u8);
        }
    }
    let data = encode_indices(
        &palette,
        &indices,
        detail_width,
        detail_height,
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
            (u32::from(c.red) * 100 + 127) / 255,
            (u32::from(c.green) * 100 + 127) / 255,
            (u32::from(c.blue) * 100 + 127) / 255
        );
    }
    // Group each six-row band by colour, emitting runs rather than pixels.
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
    fn palette_budget_does_not_destroy_native_widget_detail() {
        // Fine strokes must not turn into enlarged sample blocks when the
        // encoder reduces palette-building work under output pressure.
        let (width, height) = (96, 54);
        let mut bgra = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                bgra.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = VideoFrame {
            width,
            height,
            pitch: width * 4,
            bgra: bgra.into(),
            serial: 1,
        };
        let Protocol::Sixel(image) = encode(
            &frame,
            Size::new(96, 9),
            FontSize::new(1, 6),
            false,
            32 * 18,
        )
        .unwrap() else {
            panic!("Sixel expected")
        };
        let decoded = icy_sixel::SixelImage::decode(image.data.as_bytes()).unwrap();
        assert_eq!((decoded.width, decoded.height), (width, height));
        let correct = decoded
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(frame.bgra.as_chunks::<4>().0.iter())
            .filter(|(actual, source)| actual[0].abs_diff(source[0]) < 8)
            .count();
        assert!(
            correct * 100 >= width * height * 95,
            "Only {correct}/{} pixels retain fine detail after palette-budget reduction",
            width * height
        );
    }
    #[test]
    fn palette_training_budget_drops_quickly_and_recovers_slowly_with_bounds() {
        let mut budget = PaletteBudget::default();
        for _ in 0..100 {
            budget.observe(Duration::from_millis(200), Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 160 * 90);
        for _ in 0..59 {
            budget.observe(Duration::from_millis(1), Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 160 * 90);
        budget.observe(Duration::from_millis(1), Duration::from_millis(33));
        assert!(budget.pixels() > 160 * 90);
        for _ in 0..2000 {
            budget.observe(Duration::ZERO, Duration::from_millis(33));
        }
        assert_eq!(budget.pixels(), 640 * 360);
    }
    #[test]
    fn palette_sampling_does_not_alias_periodic_colour_strokes() {
        let colours = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255]];
        let frame = VideoFrame {
            width: 96,
            height: 54,
            pitch: 96 * 4,
            bgra: (0..54)
                .flat_map(|y| (0..96).flat_map(move |x| colours[(x + y) % 3]))
                .collect::<Vec<u8>>()
                .into(),
            serial: 1,
        };
        for (budget, scale) in [(16 * 9, 1), (32 * 18, 1), (96 * 54, 1), (16 * 9, 2)] {
            let Protocol::Sixel(out) = encode(
                &frame,
                Size::new(96 * scale, 9 * scale),
                FontSize::new(1, 6),
                false,
                budget,
            )
            .unwrap() else {
                panic!("Sixel expected")
            };
            let decoded = icy_sixel::SixelImage::decode(out.data.as_bytes()).unwrap();
            for (i, actual) in decoded.pixels.as_chunks::<4>().0.iter().enumerate() {
                let x = i % decoded.width * frame.width / decoded.width;
                let y = i / decoded.width * frame.height / decoded.height;
                let source = &frame.bgra[y * frame.pitch + x * 4..][..4];
                assert_eq!(
                    *actual,
                    [source[2], source[1], source[0], 255],
                    "Periodic colour lost at palette budget {budget}"
                );
            }
        }
    }
    #[test]
    fn indexed_pixels_preserve_coordinates_across_sixel_bands() {
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
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 0));
    }
    #[test]
    fn resize_contains_native_pixel_output_with_a_bounded_palette_sample() {
        let frame = VideoFrame {
            width: 16,
            height: 9,
            pitch: 64,
            bgra: [10, 60, 200, 255].repeat(16 * 9).into(),
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
