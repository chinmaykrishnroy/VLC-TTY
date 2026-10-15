mod app;
mod dialogs;
mod graphics;
mod renderer;
mod subtitles;
mod vlc;

use std::{io::{self,Stdout},path::Path,time::{Duration,Instant}};
use anyhow::{bail, Context, Result};
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self,Event,KeyCode,KeyEventKind,KeyModifiers,EnableBracketedPaste,DisableBracketedPaste},
        execute,
        terminal::{disable_raw_mode,enable_raw_mode,EnterAlternateScreen,LeaveAlternateScreen,
            BeginSynchronizedUpdate,EndSynchronizedUpdate},
    },
    Terminal,
};
use crate::{app::{App,HitZones},graphics::{GraphicsChoice,GraphicsRenderer},vlc::VlcPlayer};

#[derive(Debug)]
struct Options {
    media:Option<String>, subtitle:Option<String>, graphics:GraphicsChoice,
    fps:u32,native_window:bool,doctor:bool,
}
fn usage(){
    println!("VLC media player  v0.5.0  ·  Rust + Ratatui + LibVLC 3.x\n\n\
Usage: termvlc [OPTIONS] [VIDEO|DIRECTORY|URL]\n\n\
Options:\n\
  --graphics auto|kitty|sixel|iterm2|ansi     Auto detect (default)\n\
  --fps N                                  Encoding budget, 1-120 (default 60)\n\
  --subtitle <path>                        External .srt/.vtt/.ass subtitles\n\
  --native-window                          Separate GPU-accelerated VLC video window\n\
  --doctor                                 Print LibVLC diagnostic information\n\
  -h, --help                               Show help\n\n\
Controls: F/F11 fullscreen; Esc or Back button return; Space pause; ←/→ seek;\n\
S choose subtitles; L load external captions; A audio tracks; +/- volume;\n\
[ and ] speed; O / Ctrl+O open file; N/P next/previous; G graphics fallback;\n\
? help, I media information, Q quit.\n\n\
Drag and drop any video, folder or subtitle file into the terminal.\n\
Requires a compatible LibVLC 3.x shared library.\n");
}
fn options()->Result<Options>{
    let mut out=Options {media:None,subtitle:None,graphics:GraphicsChoice::Auto,
        fps:60,native_window:false,doctor:false};
    let mut args=std::env::args().skip(1);
    while let Some(arg)=args.next(){
        match arg.as_str(){
            "-h"|"--help"=>{usage();std::process::exit(0);}
            "--doctor"=>out.doctor=true,
            "--native-window"=>out.native_window=true,
            "--graphics"=>out.graphics=args.next().context("--graphics needs mode")?.parse().map_err(|e:String|anyhow::anyhow!(e))?,
            "--subtitle"=>out.subtitle=Some(args.next().context("--subtitle needs a file path")?),
            "--fps"=>out.fps=args.next().context("--fps needs a number")?.parse::<u32>()?,
            _ if arg.starts_with("--graphics=")=>out.graphics=arg[11..].parse().map_err(|e:String|anyhow::anyhow!(e))?,
            _ if arg.starts_with("--fps=")=>out.fps=arg[6..].parse::<u32>()?,
            _ if arg.starts_with("--subtitle=")=>out.subtitle=Some(arg[11..].to_string()),
            _ if arg.starts_with('-')=>bail!("Unknown option: {arg}  (see --help)"),
            _ if out.media.is_none()=>out.media=Some(arg),
            _=>bail!("Only one media/directory/URL may be specified"),
        }
    }
    if !(1..=120).contains(&out.fps){bail!("--fps must be between 1 and 120");}
    Ok(out)
}

/// Restore raw mode, mouse and alternate screen on any return path, including errors.
struct TerminalGuard;
impl TerminalGuard {
    fn enter()->Result<(Self,Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        if let Err(e)=execute!(io::stdout(),EnterAlternateScreen,cursor::Hide,
            event::EnableMouseCapture,EnableBracketedPaste){
            let _=disable_raw_mode();
            let _=execute!(io::stdout(),LeaveAlternateScreen,cursor::Show);
            return Err(e.into());
        }
        let guard=Self;
        let terminal=Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok((guard,terminal))
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self){
        let _=disable_raw_mode();
        let _=execute!(io::stdout(),event::DisableMouseCapture,DisableBracketedPaste,
            cursor::Show,LeaveAlternateScreen);
    }
}

fn main() -> Result<()> {
    // Windows default thread stack is only 1 MB, which can overflow when large terminal
    // UI trees, LibVLC callbacks, and image encoding pipelines interact.
    // Run the main UI loop on an 8 MB stack thread for rock-solid stability.
    let builder = std::thread::Builder::new()
        .name("termvlc-main".into())
        .stack_size(8 * 1024 * 1024);
    let handler = builder.spawn(run)?;
    match handler.join() {
        Ok(res) => res,
        Err(e) => std::panic::resume_unwind(e),
    }
}

fn run() -> Result<()> {
    let opts=options()?;
    if opts.doctor {
        println!("VLC media player  diagnostics\nOS: {}\nVLC_HOME: {}",
            std::env::consts::OS, std::env::var("VLC_HOME").unwrap_or_else(|_| "(unset)".into()));
        match VlcPlayer::new() {
            Ok(p)=>println!("LibVLC loaded: {}\nRun the sample to check actual decoding.",p.library_path().display()),
            Err(e)=>{println!("LibVLC unavailable: {e:#}"); std::process::exit(1);}
        }
        return Ok(());
    }
    let mut player=VlcPlayer::new_with_window(opts.native_window)
        .context("Could not load LibVLC. Run `termvlc --doctor` first")?;
    let initial=opts.media.as_deref().filter(|s|!s.contains("://"));
    let mut app=App::new(initial.map(Path::new));
    if let Some(ref source)=opts.media {
        if source.contains("://") || Path::new(source).is_file() {
            app.open_media(source,&mut player)?;
        }
    }
    if let Some(ref file)=opts.subtitle {
        app.load_subtitle(Path::new(file),&mut player)?;
    }
    let (_guard,mut terminal)=TerminalGuard::enter()?;
    // Query pixel-graphics support before the event loop begins consuming stdin.
    let mut graphics=GraphicsRenderer::new(opts.graphics,opts.fps);
    if opts.graphics==GraphicsChoice::Auto {
        app.notify(format!("Rendering: {} (detected: {})",graphics.label(),graphics.detected_label()));
    }
    let mut zones=HitZones::default();
    let mut previous_fullscreen=app.fullscreen;
    let mut previous_media=app.current.clone();
    let mut previous_modal=(app.overlay,app.input.is_some());
    let mut last_draw=Instant::now()-Duration::from_secs(1);
    let interval=Duration::from_millis((1000/opts.fps.max(1)) as u64);
    loop {
        if event::poll(Duration::from_millis(4))? {
            match event::read()? {
                Event::Key(key) if key.kind==KeyEventKind::Press => {
                    if key.code==KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL){break;}
                    if key.code==KeyCode::Char('g') && app.input.is_none() && app.overlay==app::Overlay::None {
                        graphics.toggle_ansi();
                        app.notify(format!("Video renderer: {}",graphics.label()));
                        terminal.clear()?;
                        graphics.invalidate();
                    } else if !app.handle_key(key,&mut player)? {break;}
                }
                Event::Paste(s)=>{ let _ = app.handle_drop_or_paste(&s, &mut player); }
                Event::Mouse(m)=>app.handle_mouse(m,&zones,&mut player)?,
                Event::Resize(_,_)=>{terminal.clear()?;graphics.invalidate();}
                _=>{}
            }
        }
        if previous_fullscreen!=app.fullscreen || previous_media!=app.current
            || previous_modal!=(app.overlay,app.input.is_some()) {
            terminal.clear()?;
            graphics.invalidate();
            previous_fullscreen=app.fullscreen;
            previous_media=app.current.clone();
            previous_modal=(app.overlay,app.input.is_some());
        }
        let frame=if opts.native_window {None}else{player.latest_frame()};
        if !opts.native_window {graphics.prepare(frame.clone());}
        if last_draw.elapsed()>=interval {
            let info=player.snapshot();
            let _ = execute!(io::stdout(), BeginSynchronizedUpdate);
            terminal.draw(|f|zones=renderer::draw(f,&app,&info,frame.as_deref(),&mut graphics,opts.native_window))?;
            let _ = execute!(io::stdout(), EndSynchronizedUpdate);
            last_draw=Instant::now();
        }
    }
    Ok(())
}
