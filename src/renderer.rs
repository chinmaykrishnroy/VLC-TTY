//! VLC-inspired terminal interface; intentionally doesn't touch decoder threads.
//! Fullscreen occupies the entire alternate screen and only shows small overlays.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use crate::{app::{App, HitZones, InputPurpose, Overlay}, graphics::GraphicsRenderer,
    vlc::{PlaybackSnapshot, VideoFrame}};

const ORANGE: Color = Color::Rgb(255, 145, 50);
const PALE: Color = Color::Rgb(225, 234, 241);
const CYAN: Color = Color::Rgb(74, 203, 216);
const DIM: Color = Color::Rgb(115, 131, 150);

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
    (width,height)
}
fn video_lines(img: &VideoFrame, area: Rect) -> Vec<Line<'static>> {
    let (w,h) = fit_dimensions(img,area);
    if w==0 || h==0 {return vec![];}
    let mut lines=Vec::with_capacity(h/2);
    for row in 0..h/2 {
        let mut spans=Vec::with_capacity(w);
        for x in 0..w {
            let (r1,g1,b1)=sample(img,x,row*2,w,h);
            let (r2,g2,b2)=sample(img,x,row*2+1,w,h);
            spans.push(Span::styled("▀",Style::default().fg(Color::Rgb(r1,g1,b1)).bg(Color::Rgb(r2,g2,b2))));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn time(ms:i64)->String {
    let s=ms.max(0)/1000;
    if s>=3600 { format!("{:02}:{:02}:{:02}",s/3600,s/60%60,s%60) }
    else { format!("{:02}:{:02}",s/60,s%60) }
}
fn state_label(state:i32)-> &'static str {
    match state { 3=>"PLAYING", 4=>"PAUSED", 6=>"ENDED", 7=>"ERROR", 1|2=>"LOADING", _=>"READY" }
}
fn short(s:&str,limit:usize)->String {
    let mut r=s.chars().take(limit).collect::<String>();
    if s.chars().count()>limit {r.push('…');}
    r
}
fn centered(outer:Rect,width:u16,height:u16)->Rect {
    Rect::new(outer.x+(outer.width.saturating_sub(width))/2,
        outer.y+(outer.height.saturating_sub(height))/2, width.min(outer.width), height.min(outer.height))
}

/// Original ANSI-art traffic cone: orange bands alternate with light reflective bands.
fn logo(f:&mut Frame, area:Rect, compact:bool) {
    if area.width < 16 || area.height < if compact {3} else {10} {return;}
    let shape=if compact {vec!["    /\\    ","   /==\\   ","  /____\\  "]}
    else {vec![
        "       /\\       ",
        "      /##\\      ",
        "     /====\\     ",
        "    /######\\    ",
        "   /========\\   ",
        "  /##########\\  ",
        " /============\\ ",
        "/##############\\",
        "|________________|",
        "  |__________|  ",
    ]};
    let mut lines=Vec::new();
    for s in shape {
        let color=if s.contains('=') || s.contains('_') { PALE } else { ORANGE };
        lines.push(Line::styled(s,Style::default().fg(color).add_modifier(Modifier::BOLD)));
    }
    let dest=centered(area,17,lines.len() as u16);
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), dest);
}

fn video(f:&mut Frame, image:Option<&VideoFrame>, graphics:&mut GraphicsRenderer, area:Rect) {
    if area.width==0 || area.height==0 {return;}
    if let Some(img)=image {
        if !graphics.is_ansi() {graphics.draw(f,area);} else {
            let lines=video_lines(img,area);
            let dest=Rect::new(area.x,area.y + area.height.saturating_sub(lines.len() as u16)/2,
                area.width,(lines.len() as u16).min(area.height));
            f.render_widget(Paragraph::new(lines).alignment(Alignment::Center),dest);
        }
    }
}

fn subtitle(f:&mut Frame,app:&App,info:&PlaybackSnapshot,area:Rect) {
    let Some(text)=app.subtitle_text(info.time_ms) else {return;};
    if area.width<14 || area.height<4 {return;}
    let text = short(text, (area.width as usize).saturating_mul(3));
    let caption_width = area.width.saturating_sub(4).max(1) as usize;
    let rows = text.lines().map(|line| line.chars().count().div_ceil(caption_width).max(1))
        .sum::<usize>().clamp(1,3) as u16;
    let dest=Rect::new(area.x+2,
        area.bottom().saturating_sub(rows+1),
        area.width.saturating_sub(4),rows);
    f.render_widget(Clear,dest);
    f.render_widget(Paragraph::new(text).alignment(Alignment::Center)
        .style(Style::default().fg(Color::White).bg(Color::Black).add_modifier(Modifier::BOLD))
        .wrap(Wrap {trim:false}), dest);
}

fn empty_welcome(f:&mut Frame,area:Rect) {
    logo(f,Rect::new(area.x,area.y,area.width,area.height.saturating_sub(6)),false);
    let text = Paragraph::new(vec![
        Line::styled("T E R M V L C",Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
        Line::styled("Your media. Your terminal.",Style::default().fg(PALE)),
        Line::from(""),
        Line::styled("Enter  Play    O  Open    ?  Help",Style::default().fg(DIM)),
    ]).alignment(Alignment::Center);
    let boxarea=Rect::new(area.x,area.bottom().saturating_sub(6),area.width,5.min(area.height));
    f.render_widget(text,boxarea);
}

fn browser(f:&mut Frame,app:&App,area:Rect,zones:&mut HitZones) {
    if area.width<=12 {return;}
    let block=Block::default().title(format!(" FILES · {} ",short(&app.cwd.to_string_lossy(),area.width.saturating_sub(14) as usize)))
        .borders(Borders::ALL).border_style(Style::default().fg(DIM));
    let inner=block.inner(area);
    f.render_widget(block,area);
    if inner.height<1 {return;}
    // Always show the recognizable ANSI traffic-cone mark when there is room.
    let list_area=if inner.height>=13 {
        logo(f,Rect::new(inner.x,inner.y,inner.width,3),true);
        Rect::new(inner.x,inner.y+4,inner.width,inner.height-4)
    } else {inner};
    let visible=list_area.height as usize;
    let first=app.selected.saturating_sub(visible.saturating_sub(1));
    let items:Vec<_>=app.entries.iter().skip(first).take(visible).map(|p| {
        let name=p.file_name().map(|x|x.to_string_lossy().into_owned()).unwrap_or_default();
        let prefix=if p.is_dir() {"▸ "}else{"  "};
        let color=if p.is_dir(){ORANGE}else{PALE};
        ListItem::new(Line::styled(format!("{}{}",prefix,short(&name,list_area.width.saturating_sub(5) as usize)),Style::default().fg(color)))
    }).collect();
    let mut state=ListState::default();
    if !items.is_empty(){state.select(Some(app.selected.saturating_sub(first)));}
    let list=List::new(items).highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Black).bg(CYAN).add_modifier(Modifier::BOLD));
    f.render_stateful_widget(list,list_area,&mut state);
    zones.browser=Some((list_area,first));
}

fn overlay(f:&mut Frame,app:&App,info:&PlaybackSnapshot,image:Option<&VideoFrame>,zones:&mut HitZones) {
    let screen=f.area();
    if let Some(dialog)=app.input.as_ref() {
        let boxarea=centered(screen,screen.width.saturating_sub(6).min(88),7);
        f.render_widget(Clear,boxarea);
        let title=if dialog.purpose==InputPurpose::Media {" OPEN MEDIA · local file, directory or URL "}
            else {" LOAD EXTERNAL SUBTITLE · SRT / VTT / ASS "};
        f.render_widget(Block::default().title(title).borders(Borders::ALL)
            .border_style(Style::default().fg(ORANGE)).style(Style::default().bg(Color::Black)),boxarea);
        let inner=boxarea.inner(Margin {horizontal:2,vertical:2});
        f.render_widget(Paragraph::new(format!("> {}▏",dialog.text))
            .style(Style::default().fg(PALE).bg(Color::DarkGray)).wrap(Wrap {trim:false}),inner);
        return;
    }
    if app.overlay==Overlay::None{return;}
    let size = match app.overlay {Overlay::Help=>(64,23), Overlay::Info=>(67,15), _=>(64,18)};
    let dest=centered(screen,screen.width.saturating_sub(4).min(size.0),screen.height.saturating_sub(3).min(size.1));
    f.render_widget(Clear,dest);
    let title=match app.overlay {Overlay::Help=>" HELP · keyboard controls ",Overlay::Info=>" MEDIA INFORMATION ",
        Overlay::SubtitleTracks=>" SUBTITLES · Enter to select · L to load external ",
        Overlay::AudioTracks=>" AUDIO TRACKS · Enter to select ",_=>""};
    f.render_widget(Block::default().borders(Borders::ALL).title(title)
        .border_style(Style::default().fg(ORANGE)).style(Style::default().bg(Color::Black)),dest);
    let inner=dest.inner(Margin {horizontal:2,vertical:2});
    match app.overlay {
        Overlay::Help=>{
            let text=vec![
                "PLAYBACK                    BROWSER / MEDIA",
                "Space     Play / pause      ↑↓ / j k   Select file",
                "← / →     Seek 10 seconds    Enter      Play / open",
                "+ / -     Volume ±5%         Backspace  Parent folder",
                "M         Mute              O          Open path / URL",
                "X         Stop              N / P      Next / previous",
                "[ / ]     Playback speed    R          Refresh files",
                "F / F11   Fullscreen        Tab        Change pane",
                "Esc       Exit fullscreen   Q          Quit player",
                "", "SUBTITLES & TRACKS",
                "S         Subtitle menu     A          Audio track menu",
                "L         Load subtitle     , / .      Delay ±100 ms",
                "", "INTERFACE",
                "G         Pixel / ANSI      I          Media information",
                "? / F1    Help               Mouse      Click controls",
                "", "Esc closes a menu; in fullscreen it returns to the library.",
            ];
            f.render_widget(Paragraph::new(text.join("\n"))
                .style(Style::default().fg(PALE)).wrap(Wrap {trim:false}),inner);
        }
        Overlay::Info=>{
            let (w,h)=image.map(|x|(x.width,x.height)).unwrap_or((0,0));
            let text=format!("Media: {}\n\nPosition: {} / {}\nStatus: {}     Speed: {:.2}×\nVideo callback: {} × {} pixels\nAudio: {}     Volume: {}%\nSubtitles: {}\nDelay: {} ms\n\nNote: codec details require a future LibVLC metadata panel.",
                app.current.as_deref().unwrap_or("None"),time(info.time_ms),time(info.length_ms),state_label(info.state),info.rate,w,h,app.selected_audio,info.volume,app.selected_subtitle,app.subtitle_delay_ms);
            f.render_widget(Paragraph::new(text).wrap(Wrap {trim:false}).style(Style::default().fg(PALE)),inner);
        }
        Overlay::SubtitleTracks | Overlay::AudioTracks =>{
            let count=inner.height as usize;
            let first=app.track_selected.saturating_sub(count.saturating_sub(1));
            let items:Vec<_>=app.tracks.iter().skip(first).take(count).map(|x|
                ListItem::new(Line::styled(short(&x.label,inner.width.saturating_sub(4) as usize),Style::default().fg(PALE)))).collect();
            let mut state=ListState::default();
            if !items.is_empty(){state.select(Some(app.track_selected.saturating_sub(first)));}
            f.render_stateful_widget(List::new(items).highlight_symbol("▶ ")
                .highlight_style(Style::default().bg(CYAN).fg(Color::Black)),inner,&mut state);
            zones.track_list=Some((inner,first));
        }
        Overlay::None=>{}
    }
}

pub fn draw(f:&mut Frame,app:&App,info:&PlaybackSnapshot,image:Option<&VideoFrame>,
    graphics:&mut GraphicsRenderer,native_window:bool)->HitZones {
    let mut zones=HitZones::default();
    let screen=f.area();
    if screen.width<24 || screen.height<8 {
        f.render_widget(Paragraph::new("Expand terminal to at least 24×8"),screen);
        return zones;
    }
    let graphics_modal = (app.overlay != Overlay::None || app.input.is_some()) && !graphics.is_ansi();
    if app.fullscreen {
        // Full-frame clear; absolutely no normal header/browser/progress/footer.
        f.render_widget(Clear,screen);
        // Only 2 lines are reserved at the very top for a clickable Back
        // target. The rest is exclusively video, without regular TUI panels.
        // Pixel graphics may cover text in the same cells, so reserving the
        // strip is safer than overlaying the button directly onto image pixels.
        let movie_area=Rect::new(screen.x,screen.y+2,screen.width,screen.height.saturating_sub(2));
        if !native_window {
            // For image protocols put sidecar captions outside the placement.
            let video_area=if app.sidecar_enabled && !graphics.is_ansi() {
                Rect::new(movie_area.x,movie_area.y,movie_area.width,movie_area.height.saturating_sub(4))
            } else {movie_area};
            if !graphics_modal { video(f,image,graphics,video_area); }
        } else {empty_welcome(f,movie_area);}
        subtitle(f,app,info,screen);
        let back=Rect::new(screen.x+1,screen.y,18.min(screen.width-2),1);
        f.render_widget(Paragraph::new(" ‹ BACK  [Esc / F] ")
            .style(Style::default().bg(Color::Black).fg(ORANGE).add_modifier(Modifier::BOLD)),back);
        zones.back=Some(back);
        if app.show_osd(){
            let label=format!(" {}   {} / {}   {}%   {:.2}× ",state_label(info.state),time(info.time_ms),time(info.length_ms),info.volume,info.rate);
            let row=Rect::new(screen.x+1,screen.bottom().saturating_sub(1),screen.width.saturating_sub(2),1);
            f.render_widget(Paragraph::new(short(&label,row.width as usize)).style(Style::default().fg(PALE).bg(Color::Black)),row);
        }
    } else {
        let layout=Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(3),Constraint::Min(4),Constraint::Length(3),Constraint::Length(2),Constraint::Length(1)])
            .split(screen);
        let title=Paragraph::new(Line::from(vec![
            Span::styled("   ▲  ",Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled("TERM",Style::default().fg(PALE).add_modifier(Modifier::BOLD)),
            Span::styled("VLC",Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  ·  {}",graphics.label()),Style::default().fg(DIM)),
            Span::styled(format!("  [ {} ]",state_label(info.state)),Style::default().fg(CYAN)),
        ])).block(Block::default().title(" TERMINAL MEDIA PLAYER ").borders(Borders::ALL)
            .border_style(Style::default().fg(DIM)));
        f.render_widget(title,layout[0]);
        let columns=Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(27),Constraint::Percentage(73)]).split(layout[1]);
        browser(f,app,columns[0],&mut zones);
        let media=app.current.as_deref().unwrap_or("No media selected");
        let box_video=Block::default().borders(Borders::ALL)
            .title(format!(" ▶  {} ",short(media.rsplit(['/', '\\']).next().unwrap_or(media),columns[1].width.saturating_sub(7) as usize)))
            .border_style(Style::default().fg(DIM));
        let video_area=box_video.inner(columns[1]);
        f.render_widget(box_video,columns[1]);
        if image.is_some() && !native_window {
            let picture_area=if app.sidecar_enabled && !graphics.is_ansi() {
                Rect::new(video_area.x,video_area.y,video_area.width,video_area.height.saturating_sub(4))
            } else {video_area};
            if !graphics_modal { video(f,image,graphics,picture_area); }
        }
        else if native_window {
            f.render_widget(Paragraph::new("Native VLC window mode\nVideo opens separately with conventional GPU output.\nTUI controls remain active.")
                .alignment(Alignment::Center).style(Style::default().fg(DIM)),video_area);
        } else if app.current.is_none() { empty_welcome(f,video_area); }
        else { f.render_widget(Paragraph::new("Opening / buffering media…").alignment(Alignment::Center)
            .style(Style::default().fg(DIM)),video_area); }
        subtitle(f,app,info,video_area);

        let ratio=if info.length_ms>0{(info.time_ms.max(0) as f64/info.length_ms as f64).clamp(0.0,1.0)}else{0.0};
        let gauge=Gauge::default().block(Block::default().borders(Borders::ALL).title(format!(" {} / {}  ·  {}  ·  {}%  ·  {:.2}× ",
            time(info.time_ms),time(info.length_ms),state_label(info.state),info.volume,info.rate)))
            .gauge_style(Style::default().fg(ORANGE).bg(Color::Rgb(43,48,57))).ratio(ratio).label("");
        f.render_widget(gauge,layout[2]);
        zones.timeline=Some(layout[2].inner(Margin {horizontal:1,vertical:1}));
        let bar=layout[3];
        let play_area=Rect::new(bar.x+2,bar.y,17.min(bar.width),1);
        f.render_widget(Paragraph::new(if info.state==3{" ❚❚  PAUSE [Space]"}else{" ▶  PLAY  [Space] "})
            .style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),play_area);
        zones.play=Some(play_area);
        let sub_area=Rect::new(bar.x+21,bar.y,16.min(bar.width.saturating_sub(21)),1);
        f.render_widget(Paragraph::new(" CC  SUBS [S] ").style(Style::default().fg(PALE)),sub_area);
        zones.subtitles=Some(sub_area);
        let audio_area=Rect::new(bar.x+38,bar.y,16.min(bar.width.saturating_sub(38)),1);
        f.render_widget(Paragraph::new(" ♫  AUDIO [A] ").style(Style::default().fg(PALE)),audio_area);
        zones.audio=Some(audio_area);
        let fs_width=21.min(bar.width);
        let fs=Rect::new(bar.right().saturating_sub(fs_width),bar.y,fs_width,1);
        f.render_widget(Paragraph::new(" ⛶  FULLSCREEN [F] ")
            .alignment(Alignment::Right).style(Style::default().fg(CYAN)),fs);
        zones.fullscreen=Some(fs);
        let message=if let Some(warn)=graphics.warning(){warn.to_string()}
            else if app.status_visible(){app.status.clone()}
            else {"←/→ Seek   +/- Volume   O Open   L Load subtitles   G Graphics   ? Help   Q Quit".into()};
        f.render_widget(Paragraph::new(short(&message,layout[4].width as usize))
            .style(Style::default().fg(DIM)),layout[4]);
    }
    overlay(f,app,info,image,&mut zones);
    zones
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn conversion(){
        let v=VideoFrame{width:1,height:1,pitch:4,bgra:vec![10,20,30,255],serial:1};
        assert_eq!(pixel(&v,0,0),(30,20,10));
    }
    #[test] fn aspect_ratio(){
        let v=VideoFrame{width:640,height:360,pitch:2560,bgra:vec![],serial:1};
        let(w,h)=fit_dimensions(&v,Rect::new(0,0,80,24));
        assert!(w<=80 && h<=48 && h%2==0);
    }
    #[test] fn timer(){assert_eq!(time(61_000),"01:01");}
    #[test] fn normal_layout_has_click_targets(){
        use ratatui::{backend::TestBackend, Terminal};
        use crate::graphics::{GraphicsRenderer, GraphicsChoice};
        let mut terminal=Terminal::new(TestBackend::new(100,32)).unwrap();
        let app=App::new(Some(std::path::Path::new(".")));
        let mut graphics=GraphicsRenderer::new(GraphicsChoice::Ansi,24);
        let info=PlaybackSnapshot{time_ms:0,length_ms:8000,volume:100,state:5,rate:1.0,frames:0};
        let mut zones=HitZones::default();
        terminal.draw(|f|zones=draw(f,&app,&info,None,&mut graphics,false)).unwrap();
        assert!(zones.back.is_none());
        assert!(zones.timeline.is_some());
        assert!(zones.play.is_some());
        assert!(zones.fullscreen.is_some());
    }
    #[test] fn fullscreen_click_target(){
        use ratatui::{backend::TestBackend, Terminal};
        use crate::graphics::{GraphicsRenderer, GraphicsChoice};
        let mut terminal=Terminal::new(TestBackend::new(90,30)).unwrap();
        let mut app=App::new(Some(std::path::Path::new(".")));
        app.toggle_fullscreen();
        let mut graphics=GraphicsRenderer::new(GraphicsChoice::Ansi,24);
        let info=PlaybackSnapshot{time_ms:1000,length_ms:2000,volume:100,state:3,rate:1.0,frames:0};
        let mut hitzones=HitZones::default();
        terminal.draw(|f|hitzones=draw(f,&app,&info,None,&mut graphics,false)).unwrap();
        assert!(hitzones.back.is_some());
        assert!(hitzones.browser.is_none());
        assert!(hitzones.timeline.is_none());
    }
}
