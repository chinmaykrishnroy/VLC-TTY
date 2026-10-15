mod app;
mod graphics;
mod renderer;
mod vlc;

use std::{io::{self, Stdout}, path::Path, time::{Duration, Instant}};

use anyhow::{bail, Context, Result};
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};

use crate::{app::App, graphics::{GraphicsChoice, GraphicsRenderer}, vlc::VlcPlayer};

struct Options {
    media: Option<String>,
    graphics: GraphicsChoice,
    fps: u32,
    native_window: bool,
    doctor: bool,
}

fn usage() {
    println!("TermVLC v0.4.0 — LibVLC + Rust/Ratatui + pixel graphics\n\n\
Usage: termvlc [OPTIONS] [VIDEO|DIRECTORY|URL]\n\n\
  --graphics auto|kitty|sixel|iterm2|ansi   (default: auto)\n\
  --fps N             Maximum requested terminal frame rate (1..60, default 24)\n\
  --native-window     Use LibVLC's normal video window for true GPU video output\n\
  --doctor            Check installed LibVLC\n\
  -h, --help          Show help\n\n\
Hotkeys: Space pause, Left/Right seek 10s, Up/Down select, Enter open,\n\
Backspace parent, +/- volume, m mute, f focus, g graphics/ANSI,\n\
[ and ] playback speed, q quit.\n\n\
Auto mode probes Kitty, Sixel and iTerm2 support before falling back to ANSI.\n\
Windows Terminal: Sixel where supported; Kitty: Kitty graphics; macOS iTerm2: iTerm2 protocol.\n\
Linux: Kitty/Sixel when supported, macOS: iTerm2/Kitty; auto falls back to ANSI.\n\
Set TERMVLC_DECODE_MAX=0 for original source dimensions (default 1080p cap).\n");
}

fn options() -> Result<Options> {
    let mut opts = Options { media: None, graphics: GraphicsChoice::Auto, fps: 24, native_window: false, doctor: false };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => { usage(); std::process::exit(0); },
            "--doctor" => opts.doctor = true,
            "--native-window" => opts.native_window = true,
            "--graphics" => {
                let value = args.next().context("--graphics expects auto, kitty, sixel, iterm2 or ansi")?;
                opts.graphics = value.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            }
            "--fps" => {
                let s = args.next().context("--fps expects a number from 1 to 60")?;
                opts.fps = s.parse::<u32>().context("invalid --fps value")?;
                if !(1..=60).contains(&opts.fps) { bail!("--fps must be 1..60"); }
            }
            _ if arg.starts_with('-') => bail!("Unknown option '{arg}'. Run --help"),
            _ if opts.media.is_none() => opts.media = Some(arg),
            _ => bail!("Only one video, directory, or URL may be opened at once"),
        }
    }
    Ok(opts)
}

/// Always restore the console even after an error in the event loop.
struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, cursor::Hide, event::EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        let guard = Self;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok((guard, terminal))
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), EndSynchronizedUpdate, event::DisableMouseCapture, cursor::Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
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
    let opts = options()?;
    if opts.doctor {
        println!("TermVLC diagnostics\nOS: {}\nVLC_HOME: {}\nRendering request: {:?}",
            std::env::consts::OS, std::env::var("VLC_HOME").unwrap_or_else(|_| "unset".into()), opts.graphics);
        match VlcPlayer::new() {
            Ok(player) => println!("LibVLC 3.x: detected; player initialized\nLoaded library: {}\nRun a sample video to verify real decoding and audio.", player.library_path().display()),
            Err(e) => println!("LibVLC: NOT READY\n{e:#}\nInstall VLC 3.x (matching CPU architecture) and set VLC_HOME."),
        }
        return Ok(());
    }

    let mut player = VlcPlayer::new_with_window(opts.native_window)
        .context("Could not initialize LibVLC. Try: cargo run --release -- --doctor")?;
    let initial_path = opts.media.as_deref().filter(|s| !s.contains("://"));
    let mut app = App::new(initial_path.map(Path::new));
    if let Some(ref value) = opts.media {
        if value.contains("://") || Path::new(value).is_file() {
            player.open(value)?;
            app.set_current(value.to_owned());
        }
    }

    let (_guard, mut terminal) = TerminalGuard::enter()?;
    // Query graphics *after* entering raw mode and *before* reading input.
    let mut graphics = GraphicsRenderer::new(opts.graphics, opts.fps);
    if opts.graphics == GraphicsChoice::Auto {
        app.message = format!("Detected graphics: {} | Selected: {}", graphics.detected_label(), graphics.label());
    }
    let draw_interval = Duration::from_millis(1000 / opts.fps.clamp(1, 60) as u64);
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        if event::poll(Duration::from_millis(8))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) { break; }
                    if k.code == KeyCode::Char('g') {
                        graphics.toggle_ansi();
                        terminal.clear()?;
                    } else if !app.handle_key(k, &mut player)? { break; }
                }
                Event::Mouse(m) => app.handle_mouse(m),
                Event::Resize(_, _) => terminal.clear()?,
                _ => {}
            }
        }
        let current_frame = player.latest_frame();
        if !opts.native_window {
            graphics.prepare(current_frame.clone());
        }
        if last_draw.elapsed() >= draw_interval {
            let snapshot = player.snapshot();
            let _ = execute!(io::stdout(), BeginSynchronizedUpdate);
            terminal.draw(|f| renderer::draw(f, &app, &snapshot, current_frame.as_deref(), &mut graphics, opts.native_window))?;
            let _ = execute!(io::stdout(), EndSynchronizedUpdate);
            last_draw = Instant::now();
        }
    }
    Ok(())
}
