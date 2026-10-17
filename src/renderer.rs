//! VLC-TTY — authentic desktop interface in TUI.
//! Implements VLC Desktop layout: top menu bar, full-width player/welcome canvas,
//! timeline slider, VLC controls toolbar with clean text buttons, and non-overlapping overlays.
use crate::{
    app::{App, HitZones, InputPurpose, MenuKind, Overlay, ViewMode},
    graphics::GraphicsRenderer,
    vlc::{PlaybackSnapshot, VideoFrame},
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::sync::OnceLock;

const VLC_CONE_PNG: &[u8] = include_bytes!("../assets/vlc_cone.png");
static VLC_CONE_IMAGE: OnceLock<Option<image::RgbaImage>> = OnceLock::new();

const ORANGE: Color = Color::Rgb(255, 120, 0); // VLC safety orange
const CYAN: Color = Color::Rgb(74, 203, 216);
const PALE: Color = Color::Rgb(225, 234, 241);
const DIM: Color = Color::Rgb(115, 131, 150);
const BG_DARK: Color = Color::Rgb(20, 23, 28); // VLC dark player background
const BAR_BG: Color = Color::Rgb(28, 32, 38); // Menu & controls bar background

#[inline]
fn pixel(frame: &VideoFrame, x: usize, y: usize) -> (u8, u8, u8) {
    if frame.width == 0 || frame.height == 0 {
        return (0, 0, 0);
    }
    let ix = x.min(frame.width - 1);
    let iy = y.min(frame.height - 1);
    let at = iy * frame.pitch + ix * 4;
    if at + 2 >= frame.bgra.len() {
        return (0, 0, 0);
    }
    (frame.bgra[at + 2], frame.bgra[at + 1], frame.bgra[at])
}

#[inline]
fn sample(img: &VideoFrame, x: usize, y: usize, w: usize, h: usize) -> (u8, u8, u8) {
    pixel(img, x * img.width / w.max(1), y * img.height / h.max(1))
}

fn fit_dimensions(img: &VideoFrame, area: Rect) -> (usize, usize) {
    let (w, h) = (area.width as usize, area.height as usize * 2);
    if w == 0 || h < 2 || img.width == 0 || img.height == 0 {
        return (0, 0);
    }
    let factor = (w as f64 / img.width as f64).min(h as f64 / img.height as f64);
    let width = ((img.width as f64 * factor).floor() as usize).max(1).min(w);
    let height = (((img.height as f64 * factor).floor() as usize) / 2 * 2)
        .max(2)
        .min(h);
    (width, height)
}

/// Direct ultra-fast ANSI half-block video renderer writing directly to the frame buffer.
/// Eliminates intermediate Paragraph and Span allocations for maximum framerate.
fn render_ansi_video(f: &mut Frame, img: &VideoFrame, area: Rect) {
    let (w, h) = fit_dimensions(img, area);
    if w == 0 || h == 0 {
        return;
    }
    let dest = Rect::new(
        area.x + (area.width.saturating_sub(w as u16)) / 2,
        area.y + (area.height.saturating_sub((h / 2) as u16)) / 2,
        w as u16,
        (h / 2) as u16,
    );
    let buf = f.buffer_mut();
    for row in 0..h / 2 {
        let y = dest.y + row as u16;
        if y >= dest.bottom() {
            break;
        }
        for col in 0..w {
            let x = dest.x + col as u16;
            if x >= dest.right() {
                break;
            }
            let (r1, g1, b1) = sample(img, col, row * 2, w, h);
            let (r2, g2, b2) = sample(img, col, row * 2 + 1, w, h);
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char('▀')
                    .set_fg(Color::Rgb(r1, g1, b1))
                    .set_bg(Color::Rgb(r2, g2, b2));
            }
        }
    }
}

pub fn time(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 {
        format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn state_label(state: i32) -> &'static str {
    match state {
        3 => "PLAYING",
        4 => "PAUSED",
        5 => "STOPPED",
        6 => "ENDED",
        7 => "ERROR",
        1 | 2 => "LOADING",
        _ => "READY",
    }
}

fn playback_placeholder(state: i32) -> &'static str {
    match state {
        1 | 2 => "Opening / buffering media…",
        3 => "Playing — waiting for video frames",
        4 => "Paused",
        5 => "Stopped — press Space to play again",
        6 => "Playback finished",
        7 => "Unable to play media — check the file or stream and VLC runtime/plugins",
        _ => "Ready — open media to start playback",
    }
}

fn short(s: &str, limit: usize) -> String {
    let mut r = s.chars().take(limit).collect::<String>();
    if s.chars().count() > limit {
        r.push('…');
    }
    r
}

fn centered(outer: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        outer.x + (outer.width.saturating_sub(width)) / 2,
        outer.y + (outer.height.saturating_sub(height)) / 2,
        width.min(outer.width),
        height.min(outer.height),
    )
}

// ──────────────────────────────────────────────────────────────────────
//  VLC Traffic Cone — Official High-Fidelity 3D Cone with TrueColor half-blocks
// ──────────────────────────────────────────────────────────────────────
fn blend_pixel(rgba: [u8; 4], bg: (u8, u8, u8)) -> (u8, u8, u8) {
    let a = rgba[3] as f32 / 255.0;
    (
        (rgba[0] as f32 * a + bg.0 as f32 * (1.0 - a)).round() as u8,
        (rgba[1] as f32 * a + bg.1 as f32 * (1.0 - a)).round() as u8,
        (rgba[2] as f32 * a + bg.2 as f32 * (1.0 - a)).round() as u8,
    )
}

fn vlc_cone_logo(f: &mut Frame, area: Rect) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let Some(cone) = VLC_CONE_IMAGE
        .get_or_init(|| {
            image::load_from_memory(VLC_CONE_PNG)
                .ok()
                .map(|img| img.to_rgba8())
        })
        .as_ref()
    else {
        return;
    };

    let (img_w, img_h) = (cone.width() as usize, cone.height() as usize);
    if img_w == 0 || img_h == 0 {
        return;
    }

    // 1 terminal row has 2 half-blocks vertically. To maintain authentic 1:1 square aspect ratio:
    // cone_w (columns) = cone_h (rows) * 2.
    let max_h = area.height.clamp(4, 16);
    let mut cone_h = max_h;
    let mut cone_w = cone_h * 2;
    if cone_w > area.width {
        cone_w = area.width;
        cone_h = (cone_w / 2).max(2);
    }

    let dest = centered(area, cone_w, cone_h);
    let buf = f.buffer_mut();
    let (cw, ch) = (cone_w as usize, cone_h as usize);
    let bg = (20u8, 23u8, 28u8); // BG_DARK

    for row in 0..ch {
        let y = dest.y + row as u16;
        if y >= dest.bottom() {
            break;
        }
        let y_top = row * 2;
        let y_bot = row * 2 + 1;
        let py_top = (y_top * img_h / (ch * 2)).min(img_h - 1);
        let py_bot = (y_bot * img_h / (ch * 2)).min(img_h - 1);

        for col in 0..cw {
            let x = dest.x + col as u16;
            if x >= dest.right() {
                break;
            }
            let px = (col * img_w / cw).min(img_w - 1);

            let top_px = cone.get_pixel(px as u32, py_top as u32).0;
            let bot_px = cone.get_pixel(px as u32, py_bot as u32).0;

            let rgb_top = blend_pixel(top_px, bg);
            let rgb_bot = blend_pixel(bot_px, bg);

            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char('▀')
                    .set_fg(Color::Rgb(rgb_top.0, rgb_top.1, rgb_top.2))
                    .set_bg(Color::Rgb(rgb_bot.0, rgb_bot.1, rgb_bot.2));
            }
        }
    }
}

/// Exact VLC Home Page View: Centered cone with clean prompts and instructions
fn empty_welcome(f: &mut Frame, area: Rect) {
    if area.width < 10 || area.height < 4 {
        return;
    }
    f.render_widget(Block::default().style(Style::default().bg(BG_DARK)), area);

    let cone_h = 14.min(area.height.saturating_sub(4));
    let cone_y = area.y + area.height.saturating_sub(cone_h + 4) / 2;
    let cone_area = Rect::new(area.x, cone_y, area.width, cone_h);
    vlc_cone_logo(f, cone_area);

    let text_y = cone_area.bottom().min(area.bottom().saturating_sub(3));
    let text_area = Rect::new(
        area.x,
        text_y,
        area.width,
        3.min(area.height.saturating_sub(text_y.saturating_sub(area.y))),
    );
    let text = Paragraph::new(vec![
        Line::styled("VLC-TTY", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
        Line::styled("Drag and drop any media file here, or press Ctrl+O to open.", Style::default().fg(PALE)),
        Line::styled("Space: Play/Pause  -  Ctrl+O: Open File  -  F: Fullscreen  -  Tab: Playlist  -  ?: Help", Style::default().fg(DIM)),
    ]).alignment(Alignment::Center);
    f.render_widget(text, text_area);
}

fn render_audio_meta_banner(f: &mut Frame, app: &App, area: Rect) {
    if area.width < 10 || area.height < 4 {
        return;
    }
    let meta_rect = Rect::new(area.x, area.bottom().saturating_sub(3), area.width, 3);
    f.render_widget(Clear, meta_rect);
    let title = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.title.as_deref())
        .or_else(|| {
            app.current
                .as_deref()
                .and_then(|c| c.rsplit(['/', '\\']).next())
        })
        .unwrap_or("Audio Track");
    let artist = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.artist.as_deref())
        .unwrap_or("Unknown Artist");
    let album = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.album.as_deref())
        .unwrap_or("Unknown Album");
    let meta_text = Paragraph::new(vec![
        Line::styled(
            format!(" {} ", short(title, area.width.saturating_sub(4) as usize)),
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!(
                " {}  -  {} ",
                short(artist, (area.width.saturating_sub(10) / 2) as usize),
                short(album, (area.width.saturating_sub(10) / 2) as usize)
            ),
            Style::default().fg(PALE),
        ),
    ])
    .alignment(Alignment::Center)
    .style(Style::default().bg(Color::Rgb(16, 18, 22)));
    f.render_widget(meta_text, meta_rect);
}

fn render_audio_card(f: &mut Frame, app: &App, info: &PlaybackSnapshot, area: Rect) {
    if area.width < 10 || area.height < 4 {
        return;
    }
    f.render_widget(Block::default().style(Style::default().bg(BG_DARK)), area);

    let title = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.title.as_deref())
        .or_else(|| {
            app.current
                .as_deref()
                .and_then(|c| c.rsplit(['/', '\\']).next())
        })
        .unwrap_or("Audio Track");
    let artist = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.artist.as_deref())
        .unwrap_or("Unknown Artist");
    let album = app
        .audio_metadata
        .as_ref()
        .and_then(|m| m.album.as_deref())
        .unwrap_or("Unknown Album");

    let card_w = 54.min(area.width.saturating_sub(4));
    let card_h = 9.min(area.height.saturating_sub(2));
    let card_rect = centered(area, card_w, card_h);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CYAN))
        .style(Style::default().bg(Color::Rgb(24, 28, 34)));
    let inner = block.inner(card_rect);
    f.render_widget(block, card_rect);

    let lines = vec![
        Line::styled(
            " [AUDIO PLAYBACK] ",
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            short(title, inner.width as usize),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!(
                "Artist: {}",
                short(artist, inner.width.saturating_sub(8) as usize)
            ),
            Style::default().fg(PALE),
        ),
        Line::styled(
            format!(
                "Album:  {}",
                short(album, inner.width.saturating_sub(8) as usize)
            ),
            Style::default().fg(DIM),
        ),
        Line::raw(""),
        Line::styled(
            format!(
                "Time: {} / {}   State: {}",
                time(info.time_ms),
                time(info.length_ms),
                state_label(info.state)
            ),
            Style::default().fg(CYAN),
        ),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

fn video(
    f: &mut Frame,
    image: Option<&VideoFrame>,
    graphics: &mut GraphicsRenderer,
    area: Rect,
    has_overlay: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );
    if let Some(img) = image {
        // If an overlay or dropdown is open over the video, fall back to fast ANSI half-blocks
        // so the video continues playing smoothly underneath while text widgets render clearly on top!
        if !graphics.is_ansi() && !has_overlay {
            graphics.draw(f, area);
        } else {
            render_ansi_video(f, img, area);
        }
    }
}

fn video_content_area(area: Rect, captions: bool) -> Rect {
    if captions && area.height >= 5 {
        Rect::new(area.x, area.y, area.width, area.height - 4)
    } else {
        area
    }
}

fn fullscreen_content_area(area: Rect, captions: bool) -> Rect {
    // Pixel images can cover terminal text. Keep the OSD rows outside their rectangle.
    let content = Rect::new(
        area.x,
        area.y.saturating_add(2),
        area.width,
        area.height.saturating_sub(2),
    );
    video_content_area(content, captions)
}

fn subtitle(f: &mut Frame, app: &App, info: &PlaybackSnapshot, area: Rect) {
    let Some(text) = app.subtitle_text(info.time_ms) else {
        return;
    };
    if area.width < 14 || area.height < 4 {
        return;
    }
    let text = short(&text, (area.width as usize).saturating_mul(3));
    let caption_width = area.width.saturating_sub(4).max(1) as usize;
    let rows = text
        .lines()
        .map(|line| line.chars().count().div_ceil(caption_width).max(1))
        .sum::<usize>()
        .clamp(1, 3) as u16;
    let dest = Rect::new(
        area.x + 2,
        area.bottom().saturating_sub(rows + 1),
        area.width.saturating_sub(4),
        rows,
    );
    f.render_widget(Clear, dest);
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: false }),
        dest,
    );
}

// ──────────────────────────────────────────────────────────────────────
//  Playlist / File Browser View (Toggled via Tab, Playlist button or View menu)
// ──────────────────────────────────────────────────────────────────────
fn browser(f: &mut Frame, app: &App, area: Rect, zones: &mut HitZones) {
    if area.width <= 12 {
        return;
    }
    let title = if app.view_mode == ViewMode::Playlist {
        format!(" PLAYBACK QUEUE · Repeat {:?} ", app.queue.repeat)
    } else {
        format!(
            " FILES - {} ",
            short(
                &app.cwd.to_string_lossy(),
                area.width.saturating_sub(16) as usize
            )
        )
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .style(Style::default().bg(BG_DARK));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 1 {
        return;
    }

    let visible = inner.height as usize;
    let first = app.selected.saturating_sub(visible.saturating_sub(1));
    let queue_paths: Vec<_> = app
        .queue
        .sources()
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    let entries = if app.view_mode == ViewMode::Playlist {
        &queue_paths
    } else {
        &app.entries
    };
    let items: Vec<_> = entries
        .iter()
        .skip(first)
        .take(visible)
        .map(|p| {
            let name = p
                .file_name()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default();
            let prefix = if p.is_dir() { "[Dir] " } else { "      " };
            let color = if p.is_dir() { ORANGE } else { PALE };
            ListItem::new(Line::styled(
                format!(
                    "{}{}",
                    prefix,
                    short(&name, inner.width.saturating_sub(7) as usize)
                ),
                Style::default().fg(color),
            ))
        })
        .collect();

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.selected.saturating_sub(first)));
    }
    let list = List::new(items).highlight_symbol("> ").highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(CYAN)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(list, inner, &mut state);
    zones.browser = Some((inner, first));
}

// ──────────────────────────────────────────────────────────────────────
//  Top Menu Bar (Media Playback Audio Video Subtitle Tools View Help)
// ──────────────────────────────────────────────────────────────────────
fn render_menu_bar(f: &mut Frame, area: Rect, app: &App, zones: &mut HitZones) {
    zones.menu_bar.clear();
    let mut x = area.x + 1;
    let mut spans = Vec::new();

    for &menu in MenuKind::ALL {
        let name = menu.name();
        let width = name.len() as u16 + 2;
        let is_active = app.active_menu == Some(menu);

        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(ORANGE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PALE)
        };

        let rect = Rect::new(x, area.y, width, 1);
        zones.menu_bar.push((rect, menu));
        x += width + 1;

        spans.push(Span::styled(format!(" {} ", name), style));
        spans.push(Span::raw(" "));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(BAR_BG)),
        area,
    );
}

// ──────────────────────────────────────────────────────────────────────
//  Dropdown Menu — Rendered with a solid opaque background so it never disappears over live video
// ──────────────────────────────────────────────────────────────────────
fn render_menu_dropdown(f: &mut Frame, app: &App, zones: &mut HitZones) {
    let Some(menu) = app.active_menu else {
        zones.menu_dropdown = None;
        return;
    };
    let screen = f.area();
    let items = menu.items();
    let menu_x = zones
        .menu_bar
        .iter()
        .find(|(_, m)| *m == menu)
        .map(|(r, _)| r.x)
        .unwrap_or(screen.x + 1);
    let max_len = items.iter().map(|s| s.len()).max().unwrap_or(24);
    let box_w = (max_len as u16 + 4).min(screen.width.saturating_sub(menu_x));
    let box_h = (items.len() as u16 + 2).min(screen.height.saturating_sub(screen.y + 1));

    let dropdown_rect = Rect::new(menu_x, screen.y + 1, box_w, box_h);

    // 1. Physically fill every single cell in dropdown_rect with an opaque background
    let buf = f.buffer_mut();
    for y in dropdown_rect.y..dropdown_rect.bottom() {
        for x in dropdown_rect.x..dropdown_rect.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ')
                    .set_bg(Color::Rgb(24, 28, 34))
                    .set_fg(PALE);
            }
        }
    }

    // 2. Draw border and dropdown body
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(Color::Rgb(24, 28, 34))),
        dropdown_rect,
    );

    let inner = dropdown_rect.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let mut hit_items = Vec::new();
    for (i, &item) in items.iter().enumerate() {
        if (i as u16) < inner.height {
            let row_rect = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            hit_items.push((row_rect, i));
            let is_sel = app.menu_selected == i;
            let (fg, bg) = if is_sel {
                (Color::Black, CYAN)
            } else {
                (PALE, Color::Rgb(24, 28, 34))
            };
            // Pad the string to the exact width of row_rect so the entire row has an opaque background
            let padded = format!(
                " {:<width$}",
                item,
                width = inner.width.saturating_sub(1) as usize
            );
            f.render_widget(
                Paragraph::new(padded).style(Style::default().fg(fg).bg(bg).add_modifier(
                    if is_sel {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    },
                )),
                row_rect,
            );
        }
    }
    zones.menu_dropdown = Some((dropdown_rect, hit_items));
}

fn overlay(
    f: &mut Frame,
    app: &App,
    info: &PlaybackSnapshot,
    image: Option<&VideoFrame>,
    zones: &mut HitZones,
) {
    let screen = f.area();
    if let Some(dialog) = app.input.as_ref() {
        let boxarea = centered(screen, screen.width.saturating_sub(6).min(88), 7);
        zones.overlay_box = Some(boxarea);
        let buf = f.buffer_mut();
        for y in boxarea.y..boxarea.bottom() {
            for x in boxarea.x..boxarea.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ')
                        .set_bg(Color::Rgb(20, 23, 28))
                        .set_fg(PALE);
                }
            }
        }
        let title = if dialog.purpose == InputPurpose::Media {
            " OPEN MEDIA / URL - file path, directory or network stream URL "
        } else {
            " LOAD EXTERNAL SUBTITLE - SRT / VTT / ASS "
        };
        f.render_widget(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
                .style(Style::default().bg(Color::Rgb(20, 23, 28))),
            boxarea,
        );
        let inner = boxarea.inner(Margin {
            horizontal: 2,
            vertical: 2,
        });
        f.render_widget(
            Paragraph::new(format!("> {}|", dialog.text))
                .style(Style::default().fg(PALE).bg(Color::DarkGray))
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    if app.overlay == Overlay::None {
        zones.overlay_box = None;
        return;
    }
    let size = match app.overlay {
        Overlay::Help => (68, 25),
        Overlay::Info => (70, 17),
        _ => (64, 18),
    };
    let dest = centered(
        screen,
        screen.width.saturating_sub(4).min(size.0),
        screen.height.saturating_sub(3).min(size.1),
    );
    zones.overlay_box = Some(dest);
    let buf = f.buffer_mut();
    for y in dest.y..dest.bottom() {
        for x in dest.x..dest.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ')
                    .set_bg(Color::Rgb(20, 23, 28))
                    .set_fg(PALE);
            }
        }
    }
    let title = match app.overlay {
        Overlay::Help => " HELP & KEYBOARD CONTROLS - VLC-TTY ",
        Overlay::Info => " MEDIA & CODEC INFORMATION ",
        Overlay::SubtitleTracks => " SUBTITLES - Enter to select - L to load external ",
        Overlay::AudioTracks => " AUDIO TRACKS - Enter to select ",
        _ => "",
    };
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(Color::Rgb(20, 23, 28))),
        dest,
    );
    let inner = dest.inner(Margin {
        horizontal: 2,
        vertical: 2,
    });
    match app.overlay {
        Overlay::Help => {
            let text = vec![
                "PLAYBACK CONTROLS                SUBTITLES & AUDIO",
                "Space         Play / Pause       v             Cycle subtitles",
                "s / x         Stop               b             Cycle audio tracks",
                "n / p         Next / Previous    Shift+S / S   Subtitle tracks menu",
                "Left / Right  Seek 10s           Shift+A / A   Audio tracks menu",
                "Shift+Left/Rt Seek 3s            l / L         Load subtitle file",
                "Ctrl+Left/Rt  Seek 60s           ,             Subtitle delay -100ms",
                "Alt+Left/Rt   Seek 5 min         h / .         Subtitle delay +100ms",
                "+ / Ctrl+Up   Volume +5%         j / k         Audio delay -/+100ms",
                "- / Ctrl+Down Volume -5%         ",
                "m             Mute / Unmute      INTERFACE & SHORTCUTS",
                "[ / ]         Speed -/+ 0.25x    f / F11       Fullscreen toggle",
                "=             Reset speed 1.0x   Tab / Ctrl+L  Toggle playlist/player",
                "e             Next video frame   Ctrl+O / o    Open file picker",
                "t             Show time / OSD    Ctrl+N / U    Open stream URL",
                "i / Ctrl+I    Media information  q / Ctrl+Q    Quit player",
                "",
                "DRAG & DROP: Drag any video, audio, or subtitle from Explorer",
                "directly into the terminal to start playing immediately.",
            ];
            f.render_widget(
                Paragraph::new(text.join("\n"))
                    .style(Style::default().fg(PALE))
                    .wrap(Wrap { trim: false }),
                inner,
            );
        }
        Overlay::Info => {
            let (w, h) = image.map(|x| (x.width, x.height)).unwrap_or((0, 0));
            let text = format!("Media: {}\n\nPosition: {} / {}\nStatus: {}     Speed: {:.2}x\nVideo resolution: {} x {} pixels\nAudio: {}     Volume: {}%     Audio delay: {} ms\nSubtitles: {}\nSubtitle delay: {} ms\n\nPlayback engine: LibVLC 3.x. Inline terminal rendering uses CPU buffers.",
                app.current.as_deref().unwrap_or("None"),
                time(info.time_ms), time(info.length_ms),
                state_label(info.state), info.rate, w, h,
                app.selected_audio, info.volume, app.audio_delay_ms,
                app.selected_subtitle, app.subtitle_delay_ms);
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(PALE)),
                inner,
            );
        }
        Overlay::SubtitleTracks | Overlay::AudioTracks => {
            let count = inner.height as usize;
            let first = app.track_selected.saturating_sub(count.saturating_sub(1));
            let items: Vec<_> = app
                .tracks
                .iter()
                .skip(first)
                .take(count)
                .map(|x| {
                    ListItem::new(Line::styled(
                        short(&x.label, inner.width.saturating_sub(4) as usize),
                        Style::default().fg(PALE),
                    ))
                })
                .collect();
            let mut state = ListState::default();
            if !items.is_empty() {
                state.select(Some(app.track_selected.saturating_sub(first)));
            }
            f.render_stateful_widget(
                List::new(items).highlight_symbol("> ").highlight_style(
                    Style::default()
                        .bg(CYAN)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
                inner,
                &mut state,
            );
            zones.track_list = Some((inner, first));
        }
        Overlay::None => {}
    }
}

pub fn draw(
    f: &mut Frame,
    app: &App,
    info: &PlaybackSnapshot,
    image: Option<&VideoFrame>,
    graphics: &mut GraphicsRenderer,
    native_window: bool,
) -> HitZones {
    let mut zones = HitZones::default();
    let screen = f.area();
    if screen.width < 24 || screen.height < 8 {
        f.render_widget(Paragraph::new("Expand terminal to at least 24x8"), screen);
        return zones;
    }
    let has_active_popup =
        app.active_menu.is_some() || app.overlay != Overlay::None || app.input.is_some();

    if app.fullscreen {
        // ── Fullscreen: 100% Edge-to-edge pure video without persistent text bars ──
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            screen,
        );

        // Reserve stable OSD and optional caption rows outside the pixel image.
        let video_area = screen;
        let content_area = fullscreen_content_area(video_area, app.sidecar_enabled);
        zones.video = Some(video_area);
        // Back zone at top-left allows clicking to exit fullscreen
        zones.back = Some(Rect::new(screen.x, screen.y, 16.min(screen.width), 1));

        if image.is_some() && !native_window {
            let picture_area = if app.is_audio() && content_area.height >= 6 {
                Rect::new(
                    content_area.x,
                    content_area.y,
                    content_area.width,
                    content_area.height.saturating_sub(3),
                )
            } else {
                content_area
            };
            video(f, image, graphics, picture_area, has_active_popup);
            if app.is_audio() && content_area.height >= 6 {
                render_audio_meta_banner(f, app, content_area);
            }
        } else if native_window {
            f.render_widget(
                Paragraph::new(
                    "Native VLC window mode\nVideo is displayed in separate VLC window.",
                )
                .alignment(Alignment::Center)
                .style(Style::default().fg(DIM)),
                video_area,
            );
        } else if app.is_audio() {
            render_audio_card(f, app, info, content_area);
        } else {
            if app.current.is_some() {
                f.render_widget(
                    Paragraph::new(playback_placeholder(info.state)).alignment(Alignment::Center),
                    video_area,
                );
            } else {
                empty_welcome(f, video_area);
            }
        }
        subtitle(f, app, info, video_area);

        // Floating OSD: only renders momentarily when user performs an action (e.g. presses 't', volume, seek)
        // Floats unobtrusively in top-right corner over the video and auto-disappears after 2 seconds.
        if app.show_osd() {
            let label = if app.status_visible() {
                format!(" {} ", app.status)
            } else {
                format!(
                    " {}   {} / {}   Vol {}%   {:.2}x ",
                    state_label(info.state),
                    time(info.time_ms),
                    time(info.length_ms),
                    info.volume,
                    info.rate
                )
            };
            let width = (label.chars().count() as u16 + 2).min(screen.width.saturating_sub(2));
            if width > 0 {
                let osd_rect = Rect::new(
                    screen.right().saturating_sub(width + 1),
                    screen.y + 1,
                    width,
                    1,
                );
                f.render_widget(Clear, osd_rect);
                f.render_widget(
                    Paragraph::new(label).style(
                        Style::default()
                            .fg(Color::White)
                            .bg(Color::Rgb(20, 23, 28))
                            .add_modifier(Modifier::BOLD),
                    ),
                    osd_rect,
                );
            }
        }
    } else {
        // ── Windowed layout: VLC Desktop Layout ───────────────────────────────
        let show_gap = screen.height >= 16;
        let layout = if show_gap {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // 0: Menu Bar
                    Constraint::Min(4),    // 1: Center Canvas (Player or Playlist + Player)
                    Constraint::Length(1), // 2: Timeline slider
                    Constraint::Length(1), // 3: Vertical spacer gap
                    Constraint::Length(1), // 4: VLC Transport Controls (Play, Prev, Stop, Next, Fullscreen, Playlist, CC, Audio, Volume)
                    Constraint::Length(1), // 5: Status / notifications bar
                ])
                .split(screen)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // 0: Menu Bar
                    Constraint::Min(4),    // 1: Center Canvas (Player or Playlist + Player)
                    Constraint::Length(1), // 2: Timeline slider
                    Constraint::Length(0), // 3: No spacer gap
                    Constraint::Length(1), // 4: VLC Transport Controls (Play, Prev, Stop, Next, Fullscreen, Playlist, CC, Audio, Volume)
                    Constraint::Length(1), // 5: Status / notifications bar
                ])
                .split(screen)
        };

        // 1. Top Menu Bar
        render_menu_bar(f, layout[0], app, &mut zones);

        // 2. Center Area: Either full-width player (default) or split playlist + player
        let center = layout[1];
        let video_area = match app.view_mode {
            ViewMode::Player => center,
            ViewMode::Playlist | ViewMode::Browser => {
                let columns = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(28), Constraint::Percentage(72)])
                    .split(center);
                browser(f, app, columns[0], &mut zones);
                columns[1]
            }
        };
        zones.video = Some(video_area);
        let content_area = video_content_area(video_area, app.sidecar_enabled);

        if image.is_some() && !native_window {
            let picture_area = if app.is_audio() && content_area.height >= 6 {
                Rect::new(
                    content_area.x,
                    content_area.y,
                    content_area.width,
                    content_area.height.saturating_sub(3),
                )
            } else {
                content_area
            };
            video(f, image, graphics, picture_area, has_active_popup);
            if app.is_audio() && content_area.height >= 6 {
                render_audio_meta_banner(f, app, content_area);
            }
        } else if native_window {
            f.render_widget(
                Block::default().style(Style::default().bg(BG_DARK)),
                video_area,
            );
            f.render_widget(Paragraph::new("Native VLC window mode active\nVideo displayed in a separate window.\nTUI controls remain active.")
                .alignment(Alignment::Center).style(Style::default().fg(DIM)), video_area);
        } else if app.is_audio() {
            render_audio_card(f, app, info, content_area);
        } else if app.current.is_none() {
            empty_welcome(f, video_area);
        } else {
            f.render_widget(
                Block::default().style(Style::default().bg(BG_DARK)),
                video_area,
            );
            f.render_widget(
                Paragraph::new(playback_placeholder(info.state))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(DIM)),
                video_area,
            );
        }
        subtitle(f, app, info, video_area);

        // 3. Timeline Slider across the bottom
        let time_row = layout[2];
        let t_cur = time(info.time_ms);
        let t_total = if info.length_ms > 0 {
            time(info.length_ms)
        } else {
            "--:--".into()
        };
        let ratio = if info.length_ms > 0 {
            (info.time_ms.max(0) as f64 / info.length_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let time_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(t_cur.len() as u16 + 2),
                Constraint::Min(8),
                Constraint::Length(t_total.len() as u16 + 2),
            ])
            .split(time_row);

        f.render_widget(
            Paragraph::new(format!(" {} ", t_cur)).style(Style::default().fg(PALE).bg(BAR_BG)),
            time_layout[0],
        );
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(ORANGE).bg(Color::Rgb(48, 54, 64)))
            .ratio(ratio)
            .label("");
        f.render_widget(gauge, time_layout[1]);
        zones.timeline = Some(time_layout[1]);
        f.render_widget(
            Paragraph::new(format!(" {} ", t_total)).style(Style::default().fg(DIM).bg(BAR_BG)),
            time_layout[2],
        );

        // Spacer row between timeline seek slider and transport controls
        if show_gap {
            f.render_widget(
                Block::default().style(Style::default().bg(BAR_BG)),
                layout[3],
            );
        }

        // 4. VLC Controls Toolbar (Text Buttons - NO emojis)
        let bar = layout[4];
        f.render_widget(Block::default().style(Style::default().bg(BAR_BG)), bar);
        let mut x = bar.x + 1;

        // Play/Pause button
        let play_text = if info.state == 3 { "[Pause]" } else { "[Play]" };
        let play_w = play_text.len() as u16;
        let play_rect = Rect::new(x, bar.y, play_w, 1);
        f.render_widget(
            Paragraph::new(play_text)
                .style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            play_rect,
        );
        zones.play = Some(play_rect);
        x += play_w + 1;

        // Stop button
        let stop_rect = Rect::new(x, bar.y, 6, 1);
        f.render_widget(
            Paragraph::new("[Stop]").style(Style::default().fg(PALE)),
            stop_rect,
        );
        zones.stop = Some(stop_rect);
        x += 7;

        // Previous button
        let prev_rect = Rect::new(x, bar.y, 6, 1);
        f.render_widget(
            Paragraph::new("[Prev]").style(Style::default().fg(PALE)),
            prev_rect,
        );
        zones.prev = Some(prev_rect);
        x += 7;

        // Next button
        let next_rect = Rect::new(x, bar.y, 6, 1);
        f.render_widget(
            Paragraph::new("[Next]").style(Style::default().fg(PALE)),
            next_rect,
        );
        zones.next = Some(next_rect);
        x += 7;

        // Fullscreen button
        let fs_rect = Rect::new(x, bar.y, 6, 1);
        f.render_widget(
            Paragraph::new("[Full]").style(Style::default().fg(CYAN)),
            fs_rect,
        );
        zones.fullscreen = Some(fs_rect);
        x += 7;

        // Playlist Toggle button
        let pl_style = if app.view_mode == ViewMode::Playlist {
            Style::default()
                .fg(Color::Black)
                .bg(ORANGE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PALE)
        };
        let pl_rect = Rect::new(x, bar.y, 10, 1);
        f.render_widget(Paragraph::new("[Playlist]").style(pl_style), pl_rect);
        zones.playlist_toggle = Some(pl_rect);
        x += 11;

        // Subtitles button
        let sub_rect = Rect::new(x, bar.y, 6, 1);
        f.render_widget(
            Paragraph::new("[Subs]").style(Style::default().fg(PALE)),
            sub_rect,
        );
        zones.subtitles = Some(sub_rect);
        x += 7;

        // Audio button
        let au_rect = Rect::new(x, bar.y, 7, 1);
        f.render_widget(
            Paragraph::new("[Audio]").style(Style::default().fg(PALE)),
            au_rect,
        );
        zones.audio = Some(au_rect);
        x += 8;

        // Spotify-style Volume Slider widget with draggable thumb knob on the right
        let vol_w = 26.min(bar.width.saturating_sub(x + 1));
        if vol_w >= 10 {
            let vol_x = bar.right().saturating_sub(vol_w);
            let vol_container = Rect::new(vol_x, bar.y, vol_w, 1);
            let vol_pct = info.volume.clamp(0, 125);
            let vol_ratio = (vol_pct as f64 / 125.0).clamp(0.0, 1.0);

            // Split into "Vol " (4 cols), Slider Track (remaining cols), " 100%" (5 cols)
            let vol_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(4),
                    Constraint::Length(5),
                ])
                .split(vol_container);

            f.render_widget(
                Paragraph::new("Vol ").style(Style::default().fg(PALE).bg(BAR_BG)),
                vol_layout[0],
            );

            let track_area = vol_layout[1];
            let track_w = track_area.width as usize;
            if track_w > 0 {
                let thumb_pos = if vol_pct <= 0 {
                    0
                } else if vol_pct >= 125 {
                    track_w.saturating_sub(1)
                } else {
                    ((vol_ratio * (track_w.saturating_sub(1) as f64)).round() as usize)
                        .min(track_w.saturating_sub(1))
                };

                let mut track_spans = Vec::with_capacity(3);
                // Active / filled portion before thumb
                if thumb_pos > 0 {
                    let filled_chars: String = "━".repeat(thumb_pos);
                    track_spans.push(Span::styled(
                        filled_chars,
                        Style::default().fg(CYAN).bg(BAR_BG),
                    ));
                }
                // Spotify-style thumb knob: '●' in bold white/cyan
                track_spans.push(Span::styled(
                    "●",
                    Style::default()
                        .fg(Color::White)
                        .bg(BAR_BG)
                        .add_modifier(Modifier::BOLD),
                ));
                // Inactive / unfilled portion after thumb
                if thumb_pos + 1 < track_w {
                    let unfilled_chars: String = "─".repeat(track_w.saturating_sub(thumb_pos + 1));
                    track_spans.push(Span::styled(
                        unfilled_chars,
                        Style::default().fg(Color::Rgb(65, 75, 90)).bg(BAR_BG),
                    ));
                }

                f.render_widget(Paragraph::new(Line::from(track_spans)), track_area);
            }

            f.render_widget(
                Paragraph::new(format!("{:>4}%", vol_pct))
                    .style(Style::default().fg(CYAN).bg(BAR_BG)),
                vol_layout[2],
            );
            // Save the exact track_area as the hit zone for volume click/drag with 1% resolution
            zones.volume = Some(track_area);
        }

        // 5. Status & notification bar
        let message = if let Some(warn) = graphics.warning() {
            warn.to_string()
        } else if app.status_visible() {
            app.status.clone()
        } else {
            format!(
                "VLC-TTY ready  -  {}  -  Ctrl+O Open  -  Drag & drop media  -  Space Play/Pause",
                graphics.label()
            )
        };
        f.render_widget(
            Paragraph::new(short(&message, layout[5].width as usize))
                .style(Style::default().fg(DIM).bg(BAR_BG)),
            layout[5],
        );
    }

    // Dropdowns and modals are rendered LAST with solid opaque cells to guarantee they are displayed directly over live video
    render_menu_dropdown(f, app, &mut zones);
    overlay(f, app, info, image, &mut zones);
    zones
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion() {
        let v = VideoFrame {
            width: 1,
            height: 1,
            pitch: 4,
            bgra: vec![10, 20, 30, 255],
            serial: 1,
        };
        assert_eq!(pixel(&v, 0, 0), (30, 20, 10));
    }
    #[test]
    fn aspect_ratio() {
        let v = VideoFrame {
            width: 640,
            height: 360,
            pitch: 2560,
            bgra: vec![],
            serial: 1,
        };
        let (w, h) = fit_dimensions(&v, Rect::new(0, 0, 80, 24));
        assert!(w <= 80 && h <= 48 && h % 2 == 0);
    }
    #[test]
    fn timer() {
        assert_eq!(time(61_000), "01:01");
    }
    #[test]
    fn fullscreen_pixels_do_not_cover_osd_or_caption_rows() {
        let screen = Rect::new(0, 0, 80, 24);
        let picture = fullscreen_content_area(screen, true);
        assert!(picture.y >= screen.y + 2);
        assert!(picture.bottom() <= screen.bottom() - 4);
        assert_eq!(picture.width, screen.width);
        let small = fullscreen_content_area(Rect::new(0, 0, 24, 8), true);
        assert!(small.height > 0);
        assert!(small.bottom() <= 8);
    }
    #[test]
    fn normal_layout_has_click_targets() {
        use crate::graphics::{GraphicsChoice, GraphicsRenderer};
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        let app = App::new(Some(std::path::Path::new(".")));
        let mut graphics = GraphicsRenderer::new(GraphicsChoice::Ansi, 60);
        let info = PlaybackSnapshot {
            time_ms: 0,
            length_ms: 8000,
            volume: 100,
            state: 5,
            rate: 1.0,
            frames: 0,
        };
        let mut zones = HitZones::default();
        terminal
            .draw(|f| zones = draw(f, &app, &info, None, &mut graphics, false))
            .unwrap();
        assert!(zones.back.is_none());
        assert!(zones.timeline.is_some());
        assert!(zones.play.is_some());
        assert!(zones.stop.is_some());
        assert!(zones.prev.is_some());
        assert!(zones.next.is_some());
        assert!(zones.playlist_toggle.is_some());
        assert!(zones.volume.is_some());
        assert!(zones.fullscreen.is_some());
        assert!(!zones.menu_bar.is_empty());
    }
    #[test]
    fn fullscreen_click_target() {
        use crate::graphics::{GraphicsChoice, GraphicsRenderer};
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        let mut app = App::new(Some(std::path::Path::new(".")));
        app.toggle_fullscreen();
        let mut graphics = GraphicsRenderer::new(GraphicsChoice::Ansi, 60);
        let info = PlaybackSnapshot {
            time_ms: 1000,
            length_ms: 2000,
            volume: 100,
            state: 3,
            rate: 1.0,
            frames: 0,
        };
        let mut hitzones = HitZones::default();
        terminal
            .draw(|f| hitzones = draw(f, &app, &info, None, &mut graphics, false))
            .unwrap();
        assert!(hitzones.back.is_some());
        assert!(hitzones.browser.is_none());
        assert!(hitzones.timeline.is_none());
    }
}
