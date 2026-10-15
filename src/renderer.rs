//! VLC media player — terminal interface. Intentionally doesn't touch decoder threads.
//! Fullscreen occupies the entire alternate screen with zero overlap artifacts.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use crate::{
    app::{App, HitZones, InputPurpose, MenuKind, Overlay},
    graphics::GraphicsRenderer,
    vlc::{PlaybackSnapshot, VideoFrame},
};

const ORANGE:       Color = Color::Rgb(255, 120,   0); // VLC safety orange
const ORANGE_DARK:  Color = Color::Rgb(210,  80,   0); // VLC cone base dark orange
const WHITE_BAND:   Color = Color::Rgb(245, 248, 252); // Reflective silver-white band
const BASE_GREY:    Color = Color::Rgb( 45,  48,  54); // Base shadow / border
const CYAN:         Color = Color::Rgb( 74, 203, 216);
const PALE:         Color = Color::Rgb(225, 234, 241);
const DIM:          Color = Color::Rgb(115, 131, 150);

fn pixel(frame: &VideoFrame, x: usize, y: usize) -> (u8, u8, u8) {
    if frame.width == 0 || frame.height == 0 { return (0,0,0); }
    let ix = x.min(frame.width-1); let iy = y.min(frame.height-1);
    let at = iy * frame.pitch + ix * 4;
    if at + 2 >= frame.bgra.len() { return (0,0,0); }
    (frame.bgra[at+2], frame.bgra[at+1], frame.bgra[at])
}
fn sample(img: &VideoFrame, x: usize, y: usize, w: usize, h: usize) -> (u8,u8,u8) {
    pixel(img, x*img.width/w.max(1), y*img.height/h.max(1))
}
fn fit_dimensions(img: &VideoFrame, area: Rect) -> (usize, usize) {
    let (w,h) = (area.width as usize, area.height as usize * 2);
    if w==0 || h<2 || img.width==0 || img.height==0 { return (0,0); }
    let factor = (w as f64 / img.width as f64).min(h as f64 / img.height as f64);
    let width = ((img.width as f64 * factor).floor() as usize).max(1).min(w);
    let height = (((img.height as f64 * factor).floor() as usize)/2 * 2).max(2).min(h);
    (width, height)
}
fn video_lines(img: &VideoFrame, area: Rect) -> Vec<Line<'static>> {
    let (w,h) = fit_dimensions(img,area);
    if w==0 || h==0 { return vec![]; }
    let mut lines = Vec::with_capacity(h/2);
    for row in 0..h/2 {
        let mut spans = Vec::with_capacity(w);
        for x in 0..w {
            let (r1,g1,b1) = sample(img,x,row*2,w,h);
            let (r2,g2,b2) = sample(img,x,row*2+1,w,h);
            spans.push(Span::styled("▀", Style::default().fg(Color::Rgb(r1,g1,b1)).bg(Color::Rgb(r2,g2,b2))));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn time(ms: i64) -> String {
    let s = ms.max(0)/1000;
    if s >= 3600 { format!("{:02}:{:02}:{:02}", s/3600, s/60%60, s%60) }
    else { format!("{:02}:{:02}", s/60, s%60) }
}
fn state_label(state: i32) -> &'static str {
    match state { 3 => "PLAYING", 4 => "PAUSED", 6 => "ENDED", 7 => "ERROR", 1|2 => "LOADING", _ => "READY" }
}
fn short(s: &str, limit: usize) -> String {
    let mut r = s.chars().take(limit).collect::<String>();
    if s.chars().count() > limit { r.push('…'); }
    r
}
fn centered(outer: Rect, width: u16, height: u16) -> Rect {
    Rect::new(outer.x + (outer.width.saturating_sub(width))/2,
        outer.y + (outer.height.saturating_sub(height))/2,
        width.min(outer.width), height.min(outer.height))
}

// ──────────────────────────────────────────────────────────────────────
//  VLC Traffic Cone — Authentic High-Fidelity ANSI/Unicode Block Art
// ──────────────────────────────────────────────────────────────────────
fn vlc_cone_logo(f: &mut Frame, area: Rect) {
    if area.width < 26 || area.height < 14 { return; }

    let bold = Modifier::BOLD;
    let st_o = Style::default().fg(ORANGE).add_modifier(bold);
    let st_w = Style::default().fg(WHITE_BAND).add_modifier(bold);
    let st_d = Style::default().fg(ORANGE_DARK).add_modifier(bold);
    let st_b = Style::default().fg(BASE_GREY);

    // Authentic VLC traffic cone constructed from Unicode half-blocks & full blocks:
    let rows: Vec<Line> = vec![
        Line::from(vec![Span::raw("           "), Span::styled("▄██▄", st_o)]),
        Line::from(vec![Span::raw("          "), Span::styled("██████", st_o)]),
        Line::from(vec![Span::raw("         "), Span::styled("▄██████▄", st_o)]),
        Line::from(vec![Span::raw("        "), Span::styled("██████████", st_o)]),
        Line::from(vec![Span::raw("       "), Span::styled("████████████", st_w)]),   // Upper reflective band
        Line::from(vec![Span::raw("      "), Span::styled("██████████████", st_w)]),   // Upper reflective band
        Line::from(vec![Span::raw("     "), Span::styled("████████████████", st_o)]),
        Line::from(vec![Span::raw("    "), Span::styled("██████████████████", st_o)]),
        Line::from(vec![Span::raw("   "), Span::styled("████████████████████", st_w)]), // Lower reflective band
        Line::from(vec![Span::raw("  "), Span::styled("██████████████████████", st_w)]), // Lower reflective band
        Line::from(vec![Span::raw(" "), Span::styled("████████████████████████", st_o)]),
        Line::from(vec![Span::styled("▄██████████████████████████▄", st_d)]),             // Base plate
        Line::from(vec![Span::styled("▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀", st_b)]),             // Base shadow
    ];

    let dest = centered(area, 28, rows.len() as u16);
    f.render_widget(Paragraph::new(rows).alignment(Alignment::Center), dest);
}

fn empty_welcome(f: &mut Frame, area: Rect) {
    let cone_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(6));
    vlc_cone_logo(f, cone_area);

    let text = Paragraph::new(vec![
        Line::styled("VLC media player", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
        Line::styled("Drag & drop any media file here, or press Ctrl+O to open.", Style::default().fg(PALE)),
        Line::from(""),
        Line::styled("Ctrl+O  Open File    Space  Play/Pause    F  Fullscreen    ?  Help", Style::default().fg(DIM)),
    ]).alignment(Alignment::Center);

    let boxarea = Rect::new(area.x, area.bottom().saturating_sub(6), area.width, 5.min(area.height));
    f.render_widget(text, boxarea);
}

fn video(f: &mut Frame, image: Option<&VideoFrame>, graphics: &mut GraphicsRenderer, area: Rect) {
    if area.width == 0 || area.height == 0 { return; }
    if let Some(img) = image {
        if !graphics.is_ansi() { graphics.draw(f, area); } else {
            let lines = video_lines(img, area);
            let dest = Rect::new(area.x, area.y + area.height.saturating_sub(lines.len() as u16)/2,
                area.width, (lines.len() as u16).min(area.height));
            f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), dest);
        }
    }
}

fn subtitle(f: &mut Frame, app: &App, info: &PlaybackSnapshot, area: Rect) {
    let Some(text) = app.subtitle_text(info.time_ms) else { return; };
    if area.width < 14 || area.height < 4 { return; }
    let text = short(text, (area.width as usize).saturating_mul(3));
    let caption_width = area.width.saturating_sub(4).max(1) as usize;
    let rows = text.lines().map(|line| line.chars().count().div_ceil(caption_width).max(1))
        .sum::<usize>().clamp(1, 3) as u16;
    let dest = Rect::new(area.x+2, area.bottom().saturating_sub(rows+1),
        area.width.saturating_sub(4), rows);
    f.render_widget(Clear, dest);
    f.render_widget(Paragraph::new(text).alignment(Alignment::Center)
        .style(Style::default().fg(Color::White).bg(Color::Black).add_modifier(Modifier::BOLD))
        .wrap(Wrap {trim: false}), dest);
}

// ──────────────────────────────────────────────────────────────────────
//  Clean File Browser (starts directly at top; no weird triangles)
// ──────────────────────────────────────────────────────────────────────
fn browser(f: &mut Frame, app: &App, area: Rect, zones: &mut HitZones) {
    if area.width <= 12 { return; }
    let title = format!(" FILES · {} ", short(&app.cwd.to_string_lossy(), area.width.saturating_sub(14) as usize));
    let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 1 { return; }

    let visible = inner.height as usize;
    let first = app.selected.saturating_sub(visible.saturating_sub(1));
    let items: Vec<_> = app.entries.iter().skip(first).take(visible).map(|p| {
        let name = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        let prefix = if p.is_dir() { "▸ " } else { "  " };
        let color = if p.is_dir() { ORANGE } else { PALE };
        ListItem::new(Line::styled(
            format!("{}{}", prefix, short(&name, inner.width.saturating_sub(5) as usize)),
            Style::default().fg(color),
        ))
    }).collect();

    let mut state = ListState::default();
    if !items.is_empty() { state.select(Some(app.selected.saturating_sub(first))); }
    let list = List::new(items).highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Black).bg(CYAN).add_modifier(Modifier::BOLD));
    f.render_stateful_widget(list, inner, &mut state);
    zones.browser = Some((inner, first));
}

// ──────────────────────────────────────────────────────────────────────
//  Authentic VLC Menu Bar & Dropdown Menus
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
            Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PALE)
        };

        let rect = Rect::new(x, area.y, width, 1);
        zones.menu_bar.push((rect, menu));
        x += width + 1;

        spans.push(Span::styled(format!(" {} ", name), style));
        spans.push(Span::raw(" "));
    }

    f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(28, 32, 38))), area);

    // If a menu is open, render its dropdown
    if let Some(menu) = app.active_menu {
        let items = menu.items();
        let menu_x = zones.menu_bar.iter().find(|(_, m)| *m == menu).map(|(r, _)| r.x).unwrap_or(area.x + 1);
        let max_len = items.iter().map(|s| s.len()).max().unwrap_or(24);
        let box_w = (max_len as u16 + 4).min(f.area().width.saturating_sub(menu_x));
        let box_h = (items.len() as u16 + 2).min(f.area().height.saturating_sub(area.bottom()));

        let dropdown_rect = Rect::new(menu_x, area.bottom(), box_w, box_h);
        f.render_widget(Clear, dropdown_rect);
        f.render_widget(Block::default().borders(Borders::ALL).border_style(Style::default().fg(ORANGE))
            .style(Style::default().bg(Color::Black)), dropdown_rect);

        let inner = dropdown_rect.inner(Margin { horizontal: 1, vertical: 1 });
        let mut hit_items = Vec::new();
        for (i, &item) in items.iter().enumerate() {
            if (i as u16) < inner.height {
                let row_rect = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
                hit_items.push((row_rect, i));
                let is_sel = app.menu_selected == i;
                let style = if is_sel {
                    Style::default().fg(Color::Black).bg(CYAN).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(PALE)
                };
                f.render_widget(Paragraph::new(format!(" {} ", item)).style(style), row_rect);
            }
        }
        zones.menu_dropdown = Some((dropdown_rect, hit_items));
    } else {
        zones.menu_dropdown = None;
    }
}

fn overlay(f: &mut Frame, app: &App, info: &PlaybackSnapshot, image: Option<&VideoFrame>, zones: &mut HitZones) {
    let screen = f.area();
    if let Some(dialog) = app.input.as_ref() {
        let boxarea = centered(screen, screen.width.saturating_sub(6).min(88), 7);
        f.render_widget(Clear, boxarea);
        let title = if dialog.purpose == InputPurpose::Media { " OPEN MEDIA · file path, directory or URL " }
            else { " LOAD EXTERNAL SUBTITLE · SRT / VTT / ASS " };
        f.render_widget(Block::default().title(title).borders(Borders::ALL)
            .border_style(Style::default().fg(ORANGE)).style(Style::default().bg(Color::Black)), boxarea);
        let inner = boxarea.inner(Margin {horizontal:2, vertical:2});
        f.render_widget(Paragraph::new(format!("> {}▏", dialog.text))
            .style(Style::default().fg(PALE).bg(Color::DarkGray)).wrap(Wrap {trim: false}), inner);
        return;
    }
    if app.overlay == Overlay::None { return; }
    let size = match app.overlay { Overlay::Help => (66,24), Overlay::Info => (68,16), _ => (64,18) };
    let dest = centered(screen, screen.width.saturating_sub(4).min(size.0), screen.height.saturating_sub(3).min(size.1));
    f.render_widget(Clear, dest);
    let title = match app.overlay {
        Overlay::Help => " HELP & CONTROLS · VLC media player ",
        Overlay::Info => " MEDIA & CODEC INFORMATION ",
        Overlay::SubtitleTracks => " SUBTITLES · Enter to select · L to load external ",
        Overlay::AudioTracks => " AUDIO TRACKS · Enter to select ",
        _ => "",
    };
    f.render_widget(Block::default().borders(Borders::ALL).title(title)
        .border_style(Style::default().fg(ORANGE)).style(Style::default().bg(Color::Black)), dest);
    let inner = dest.inner(Margin {horizontal:2, vertical:2});
    match app.overlay {
        Overlay::Help => {
            let text = vec![
                "OPENING & PLAYBACK          SUBTITLES & AUDIO",
                "Ctrl+O    Open File dialog   A          Audio tracks menu",
                "Space     Play / pause       S          Subtitle menu",
                "← / →     Seek 10s           L          Load subtitle file",
                "+ / -     Volume ±5%         , / .      Subtitle delay ±100ms",
                "M         Mute audio         ",
                "X         Stop playback      INTERFACE & VIEW",
                "[ / ]     Speed ±0.25x       F / F11    Fullscreen toggle",
                "N / P     Next / Previous    G          Renderer: Pixel / ANSI",
                "Tab       Toggle browser     I          Media information",
                "Q         Quit               ? / F1     Help",
                "",
                "DRAG & DROP: Drag any video, audio, or subtitle from Windows",
                "Explorer directly into the terminal window to play immediately.",
            ];
            f.render_widget(Paragraph::new(text.join("\n"))
                .style(Style::default().fg(PALE)).wrap(Wrap {trim: false}), inner);
        }
        Overlay::Info => {
            let (w,h) = image.map(|x| (x.width, x.height)).unwrap_or((0,0));
            let text = format!("Media: {}\n\nPosition: {} / {}\nStatus: {}     Speed: {:.2}×\nVideo resolution: {} × {} pixels (Native Uncapped)\nAudio: {}     Volume: {}%\nSubtitles: {}\nDelay: {} ms\n\nDirect hardware-accelerated decode via native LibVLC 3.x.",
                app.current.as_deref().unwrap_or("None"),
                time(info.time_ms), time(info.length_ms),
                state_label(info.state), info.rate, w, h,
                app.selected_audio, info.volume, app.selected_subtitle, app.subtitle_delay_ms);
            f.render_widget(Paragraph::new(text).wrap(Wrap {trim: false}).style(Style::default().fg(PALE)), inner);
        }
        Overlay::SubtitleTracks | Overlay::AudioTracks => {
            let count = inner.height as usize;
            let first = app.track_selected.saturating_sub(count.saturating_sub(1));
            let items: Vec<_> = app.tracks.iter().skip(first).take(count).map(|x|
                ListItem::new(Line::styled(short(&x.label, inner.width.saturating_sub(4) as usize),
                    Style::default().fg(PALE)))).collect();
            let mut state = ListState::default();
            if !items.is_empty() { state.select(Some(app.track_selected.saturating_sub(first))); }
            f.render_stateful_widget(List::new(items).highlight_symbol("▶ ")
                .highlight_style(Style::default().bg(CYAN).fg(Color::Black)), inner, &mut state);
            zones.track_list = Some((inner, first));
        }
        Overlay::None => {}
    }
}

pub fn draw(f: &mut Frame, app: &App, info: &PlaybackSnapshot, image: Option<&VideoFrame>,
    graphics: &mut GraphicsRenderer, native_window: bool) -> HitZones {
    let mut zones = HitZones::default();
    let screen = f.area();
    if screen.width < 24 || screen.height < 8 {
        f.render_widget(Paragraph::new("Expand terminal to at least 24×8"), screen);
        return zones;
    }
    let graphics_modal = (app.overlay != Overlay::None || app.input.is_some()) && !graphics.is_ansi();

    if app.fullscreen {
        // ── Fullscreen: Clean, non-overlapping layout with zero screen wiping ─
        let show_osd = app.show_osd() || info.state == 3 || info.state == 4;
        let osd_h = if show_osd { 1 } else { 0 };

        // Top bar (height 1): Back button + Current Media Name
        let top_bar = Rect::new(screen.x, screen.y, screen.width, 1);
        let media_name = app.current.as_deref().unwrap_or("VLC media player");
        let top_title = format!(" ‹ Back [Esc/F]   ▶ {} ", short(media_name.rsplit(['/', '\\']).next().unwrap_or(media_name), screen.width.saturating_sub(22) as usize));
        f.render_widget(Paragraph::new(top_title).style(Style::default().bg(Color::Black).fg(ORANGE).add_modifier(Modifier::BOLD)), top_bar);
        zones.back = Some(Rect::new(screen.x, screen.y, 20.min(screen.width), 1));

        // Video area: strictly between row 1 and (bottom - osd_h)
        let video_area = Rect::new(
            screen.x,
            screen.y + 1,
            screen.width,
            screen.height.saturating_sub(1 + osd_h),
        );

        if !native_window {
            let picture_area = if app.sidecar_enabled && !graphics.is_ansi() {
                Rect::new(video_area.x, video_area.y, video_area.width, video_area.height.saturating_sub(3))
            } else { video_area };
            if !graphics_modal { video(f, image, graphics, picture_area); }
        } else {
            empty_welcome(f, video_area);
        }
        subtitle(f, app, info, video_area);

        // Bottom OSD bar (height 1):
        if show_osd {
            let label = format!(" {}   {} / {}   Vol {}%   {:.2}× ",
                state_label(info.state), time(info.time_ms), time(info.length_ms), info.volume, info.rate);
            let row = Rect::new(screen.x, screen.bottom().saturating_sub(1), screen.width, 1);
            f.render_widget(Paragraph::new(short(&label, row.width as usize))
                .style(Style::default().fg(PALE).bg(Color::Black)), row);
        }
    } else {
        // ── Windowed layout: VLC Desktop style with Menu Bar ─────────────────
        let layout = Layout::default().direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),   // Menu Bar (Media Playback Audio Video...)
                Constraint::Length(3),   // Title bar: VLC media player
                Constraint::Min(4),      // Columns: Files Browser | Video player
                Constraint::Length(3),   // Timeline / Seek gauge
                Constraint::Length(2),   // Control bar (Play, CC, Audio, Fullscreen)
                Constraint::Length(1),   // Status & hint bar
            ])
            .split(screen);

        // 1. VLC Desktop Menu Bar
        render_menu_bar(f, layout[0], app, &mut zones);

        // 2. Title header
        let title = Paragraph::new(Line::from(vec![
            Span::styled("  ▲ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled("VLC", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(" media player", Style::default().fg(PALE).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  ·  {}", graphics.label()), Style::default().fg(DIM)),
            Span::styled(format!("  [ {} ]", state_label(info.state)), Style::default().fg(CYAN)),
        ])).block(Block::default().title(" VLC MEDIA PLAYER ").borders(Borders::ALL)
            .border_style(Style::default().fg(DIM)));
        f.render_widget(title, layout[1]);

        // 3. Main columns (Files Browser + Video)
        let columns = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(27), Constraint::Percentage(73)])
            .split(layout[2]);
        browser(f, app, columns[0], &mut zones);

        let media = app.current.as_deref().unwrap_or("No media selected");
        let box_video = Block::default().borders(Borders::ALL)
            .title(format!(" ▶  {} ", short(media.rsplit(['/', '\\']).next().unwrap_or(media),
                columns[1].width.saturating_sub(7) as usize)))
            .border_style(Style::default().fg(DIM));
        let video_area = box_video.inner(columns[1]);
        f.render_widget(box_video, columns[1]);

        if image.is_some() && !native_window {
            let picture_area = if app.sidecar_enabled && !graphics.is_ansi() {
                Rect::new(video_area.x, video_area.y, video_area.width, video_area.height.saturating_sub(3))
            } else { video_area };
            if !graphics_modal { video(f, image, graphics, picture_area); }
        } else if native_window {
            f.render_widget(Paragraph::new("Native VLC window mode\nVideo opens separately with GPU output.\nTUI controls remain active.")
                .alignment(Alignment::Center).style(Style::default().fg(DIM)), video_area);
        } else if app.current.is_none() {
            empty_welcome(f, video_area);
        } else {
            f.render_widget(Paragraph::new("Opening / buffering media…")
                .alignment(Alignment::Center).style(Style::default().fg(DIM)), video_area);
        }
        subtitle(f, app, info, video_area);

        // 4. Timeline gauge
        let ratio = if info.length_ms > 0 {
            (info.time_ms.max(0) as f64 / info.length_ms as f64).clamp(0.0, 1.0)
        } else { 0.0 };
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL)
                .title(format!(" {} / {}  ·  {}  ·  {}%  ·  {:.2}× ",
                    time(info.time_ms), time(info.length_ms),
                    state_label(info.state), info.volume, info.rate)))
            .gauge_style(Style::default().fg(ORANGE).bg(Color::Rgb(43,48,57)))
            .ratio(ratio).label("");
        f.render_widget(gauge, layout[3]);
        zones.timeline = Some(layout[3].inner(Margin {horizontal:1, vertical:1}));

        // 5. Control bar
        let bar = layout[4];
        let play_area = Rect::new(bar.x+2, bar.y, 17.min(bar.width), 1);
        f.render_widget(Paragraph::new(if info.state==3 { " ❚❚  PAUSE [Space]" } else { " ▶  PLAY  [Space] " })
            .style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)), play_area);
        zones.play = Some(play_area);

        let sub_area = Rect::new(bar.x+21, bar.y, 16.min(bar.width.saturating_sub(21)), 1);
        f.render_widget(Paragraph::new(" CC  SUBS [S] ").style(Style::default().fg(PALE)), sub_area);
        zones.subtitles = Some(sub_area);

        let audio_area = Rect::new(bar.x+38, bar.y, 16.min(bar.width.saturating_sub(38)), 1);
        f.render_widget(Paragraph::new(" ♫  AUDIO [A] ").style(Style::default().fg(PALE)), audio_area);
        zones.audio = Some(audio_area);

        let fs_width = 21.min(bar.width);
        let fs = Rect::new(bar.right().saturating_sub(fs_width), bar.y, fs_width, 1);
        f.render_widget(Paragraph::new(" ⛶  FULLSCREEN [F] ")
            .alignment(Alignment::Right).style(Style::default().fg(CYAN)), fs);
        zones.fullscreen = Some(fs);

        // 6. Status & hint bar
        let message = if let Some(warn) = graphics.warning() { warn.to_string() }
            else if app.status_visible() { app.status.clone() }
            else { "Ctrl+O Open file   Drag & drop file   Space Pause   ←/→ Seek   +/- Vol   F Fullscreen   ? Help".into() };
        f.render_widget(Paragraph::new(short(&message, layout[5].width as usize))
            .style(Style::default().fg(DIM)), layout[5]);
    }

    overlay(f, app, info, image, &mut zones);
    zones
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn conversion() {
        let v = VideoFrame{width:1, height:1, pitch:4, bgra:vec![10,20,30,255], serial:1};
        assert_eq!(pixel(&v,0,0),(30,20,10));
    }
    #[test] fn aspect_ratio() {
        let v = VideoFrame{width:640, height:360, pitch:2560, bgra:vec![], serial:1};
        let(w,h) = fit_dimensions(&v, Rect::new(0,0,80,24));
        assert!(w<=80 && h<=48 && h%2==0);
    }
    #[test] fn timer() { assert_eq!(time(61_000), "01:01"); }
    #[test] fn normal_layout_has_click_targets() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::graphics::{GraphicsRenderer, GraphicsChoice};
        let mut terminal = Terminal::new(TestBackend::new(100,32)).unwrap();
        let app = App::new(Some(std::path::Path::new(".")));
        let mut graphics = GraphicsRenderer::new(GraphicsChoice::Ansi, 60);
        let info = PlaybackSnapshot{time_ms:0, length_ms:8000, volume:100, state:5, rate:1.0, frames:0};
        let mut zones = HitZones::default();
        terminal.draw(|f| zones = draw(f, &app, &info, None, &mut graphics, false)).unwrap();
        assert!(zones.back.is_none());
        assert!(zones.timeline.is_some());
        assert!(zones.play.is_some());
        assert!(zones.fullscreen.is_some());
        assert!(!zones.menu_bar.is_empty());
    }
    #[test] fn fullscreen_click_target() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::graphics::{GraphicsRenderer, GraphicsChoice};
        let mut terminal = Terminal::new(TestBackend::new(90,30)).unwrap();
        let mut app = App::new(Some(std::path::Path::new(".")));
        app.toggle_fullscreen();
        let mut graphics = GraphicsRenderer::new(GraphicsChoice::Ansi, 60);
        let info = PlaybackSnapshot{time_ms:1000, length_ms:2000, volume:100, state:3, rate:1.0, frames:0};
        let mut hitzones = HitZones::default();
        terminal.draw(|f| hitzones = draw(f, &app, &info, None, &mut graphics, false)).unwrap();
        assert!(hitzones.back.is_some());
        assert!(hitzones.browser.is_none());
        assert!(hitzones.timeline.is_none());
    }
}
