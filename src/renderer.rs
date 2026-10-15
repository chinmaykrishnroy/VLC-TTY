//! TrueColor Ratatui renderer: one terminal cell represents two vertical pixels.
//! The UI is intentionally independent from the LibVLC callbacks.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use crate::{app::App, graphics::GraphicsRenderer, vlc::{PlaybackSnapshot, VideoFrame}};

fn pixel(frame: &VideoFrame, x: usize, y: usize) -> (u8, u8, u8) {
    if frame.width == 0 || frame.height == 0 { return (0, 0, 0); }
    let ix = x.min(frame.width - 1);
    let iy = y.min(frame.height - 1);
    let at = iy * frame.pitch + ix * 4;
    if at + 2 >= frame.bgra.len() { return (0, 0, 0); }
    (frame.bgra[at+2], frame.bgra[at+1], frame.bgra[at]) // RV32 little-endian B,G,R,X
}

fn sample(frame: &VideoFrame, target_x: usize, target_y: usize, w: usize, h: usize) -> (u8, u8, u8) {
    pixel(frame, target_x * frame.width / w.max(1), target_y * frame.height / h.max(1))
}

fn fit_dimensions(frame: &VideoFrame, area: Rect) -> (usize, usize) {
    let available_w = area.width as usize;
    let available_h = area.height as usize * 2;
    if available_w == 0 || available_h < 2 || frame.width == 0 || frame.height == 0 { return (0, 0); }
    let scale = (available_w as f64 / frame.width as f64)
        .min(available_h as f64 / frame.height as f64);
    let width = ((frame.width as f64 * scale).floor() as usize).max(1).min(available_w);
    let height = (((frame.height as f64 * scale).floor() as usize) / 2 * 2).max(2).min(available_h);
    (width, height)
}

fn video_lines(frame: &VideoFrame, area: Rect) -> Vec<Line<'static>> {
    let (width, height) = fit_dimensions(frame, area);
    if width == 0 || height == 0 { return Vec::new(); }
    let mut rows = Vec::with_capacity(height / 2);
    for y in 0..height / 2 {
        let mut spans = Vec::with_capacity(width);
        for x in 0..width {
            let (r1, g1, b1) = sample(frame, x, y*2, width, height);
            let (r2, g2, b2) = sample(frame, x, y*2+1, width, height);
            spans.push(Span::styled("▀", Style::default().fg(Color::Rgb(r1,g1,b1)).bg(Color::Rgb(r2,g2,b2))));
        }
        rows.push(Line::from(spans));
    }
    rows
}

fn human_time(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

pub fn draw(f: &mut Frame, app: &App, info: &PlaybackSnapshot, image: Option<&VideoFrame>, graphics: &mut GraphicsRenderer, native_window: bool) {
    let area = f.area();
    if area.width < 25 || area.height < 9 {
        f.render_widget(Paragraph::new("Increase terminal size (min. 25 columns × 9 rows)").wrap(Wrap { trim: true }), area);
        return;
    }
    let layout = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(3), Constraint::Length(2)]).split(area);
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" TERM", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("VLC ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  {}  |  ", if native_window { "Native VLC window".to_owned() } else { graphics.label() })),
        Span::styled(if info.state == 3 { "PLAYING" } else if info.state == 4 { "PAUSED" } else if info.state == 6 { "ENDED" } else { "READY" }, Style::default().fg(Color::Green)),
    ])).block(Block::default().borders(Borders::ALL).title(" TERMINAL VIDEO PLAYER "));
    f.render_widget(header, layout[0]);

    let cols = if app.focus_video { Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(0), Constraint::Percentage(100)]).split(layout[1]) }
               else { Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(30), Constraint::Percentage(70)]).split(layout[1]) };
    if !app.focus_video && cols[0].width > 5 {
        let visible = cols[0].height.saturating_sub(2) as usize;
        let selected = app.selected;
        let start = if visible > 0 { selected.saturating_sub(visible - 1) } else { 0 };
        let items: Vec<ListItem> = app.entries.iter().skip(start).take(visible).map(|p| {
            let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "..".to_string());
            let lead = if p.is_dir() { "▸ " } else { "  " };
            let color = if p.is_dir() { Color::Yellow } else { Color::White };
            ListItem::new(Line::from(Span::styled(format!("{lead}{name}"), Style::default().fg(color))))
        }).collect();
        let mut state = ListState::default();
        if !items.is_empty() { state.select(Some(selected.saturating_sub(start))); }
        let dir = app.cwd.to_string_lossy();
        let title = format!(" FILES: {} ", dir.chars().take(25).collect::<String>());
        let browser = List::new(items).block(Block::default().borders(Borders::ALL).title(title))
            .highlight_symbol("▶ ").highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(browser, cols[0], &mut state);
    }

    let name = app.current.as_deref().unwrap_or("No media");
    let title = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let box_video = Block::default().borders(Borders::ALL).title(format!(" ▶ {} ", title.chars().take(55).collect::<String>()));
    let inner = box_video.inner(cols[1]);
    f.render_widget(box_video, cols[1]);
    if let Some(img) = image {
        if !graphics.is_ansi() {
            graphics.draw(f, inner);
        } else {
        let lines = video_lines(img, inner);
        let video_height = lines.len() as u16;
        let offset_y = inner.height.saturating_sub(video_height) / 2;
        let dest = Rect { x: inner.x, y: inner.y + offset_y, width: inner.width, height: video_height.min(inner.height) };
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), dest);
        }
    } else {
        let message = if info.state == 1 || info.state == 2 { "Loading / buffering video…" }
                      else if native_window { "Native VLC window: video should open in a separate window.\nKeep TermVLC focused for keyboard controls." }
                      else { "Select a video and press Enter\n\nAuto graphics: Kitty / Sixel / iTerm2 / ANSI fallback\nAudio and A/V sync: LibVLC" };
        f.render_widget(Paragraph::new(message).alignment(Alignment::Center).wrap(Wrap { trim: true }), inner.inner(Margin { horizontal: 1, vertical: 1 }));
    }

    let progress_ratio = if info.length_ms > 0 { (info.time_ms.max(0) as f64 / info.length_ms as f64).clamp(0.0, 1.0) } else { 0.0 };
    let progress_title = format!(" {} / {} | Vol {}% | Speed {:.2}x | Frames {} ", human_time(info.time_ms), human_time(info.length_ms), info.volume.max(0), info.rate, info.frames);
    let progress = Gauge::default().block(Block::default().borders(Borders::ALL).title(progress_title))
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .ratio(progress_ratio).label("");
    f.render_widget(progress, layout[2]);
    let footer_text = "Space Pause  ←/→ ±10s  ↑/↓ Browse  Enter Open  Backspace Parent  +/- Vol  M Mute  F Focus  G Graphics/ANSI  [/] Speed  Q Quit";
    f.render_widget(Paragraph::new(footer_text).style(Style::default().fg(Color::Gray)).wrap(Wrap { trim: false }), layout[3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn bgr_to_rgb() {
        let f = VideoFrame { width: 1, height: 1, pitch: 4, bgra: vec![10,20,30,255], serial: 1 };
        assert_eq!(pixel(&f,0,0), (30,20,10));
    }
    #[test] fn proportions() {
        let f = VideoFrame { width: 640, height: 360, pitch: 2560, bgra: Vec::new(), serial: 1 };
        let (w,h) = fit_dimensions(&f, Rect::new(0,0,80,24));
        assert!(w <= 80 && h <= 48 && h % 2 == 0);
    }
    #[test] fn timestamps() { assert_eq!(human_time(3_661_000), "01:01:01"); }
}
