mod app;
mod renderer;
mod vlc;

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

use crate::app::App;
use crate::vlc::VlcPlayer;

/// Always restore the terminal, including when the app returns an error.
struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, cursor::Hide, event::EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok((Self, terminal))
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), event::DisableMouseCapture, cursor::Show, LeaveAlternateScreen);
    }
}

struct CliOptions {
    source: Option<String>,
    fps: u32,
    decode_width: u32,
    ascii: bool,
    doctor: bool,
    help: bool,
}

fn parse_args() -> Result<CliOptions> {
    let mut args = std::env::args().skip(1);
    let mut opts = CliOptions {
        source: None,
        fps: 15,
        decode_width: 480,
        ascii: false,
        doctor: false,
        help: false,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => opts.help = true,
            "--doctor" => opts.doctor = true,
            "--ascii" => opts.ascii = true,
            "--fps" => {
                if let Some(val) = args.next() {
                    opts.fps = val.parse::<u32>().context("--fps requires a positive integer")?.clamp(1, 60);
                } else {
                    anyhow::bail!("--fps requires an argument");
                }
            }
            "--decode-width" => {
                if let Some(val) = args.next() {
                    opts.decode_width = val.parse::<u32>().context("--decode-width requires a positive integer")?.clamp(64, 1920);
                } else {
                    anyhow::bail!("--decode-width requires an argument");
                }
            }
            other if other.starts_with("--fps=") => {
                opts.fps = other[6..].parse::<u32>().context("--fps requires a positive integer")?.clamp(1, 60);
            }
            other if other.starts_with("--decode-width=") => {
                opts.decode_width = other[15..].parse::<u32>().context("--decode-width requires a positive integer")?.clamp(64, 1920);
            }
            other if !other.starts_with('-') && opts.source.is_none() => {
                opts.source = Some(other.to_string());
            }
            other => anyhow::bail!("unknown option: {other}"),
        }
    }
    Ok(opts)
}

fn main() -> Result<()> {
    let opts = parse_args()?;
    if opts.help {
        println!("TermVLC v0.2.0 | Rust + Ratatui + LibVLC\n\nUsage:\n  termvlc [OPTIONS] [FILE|DIRECTORY|URL]\n\nOptions:\n  --fps <N>           Terminal refresh limit (1 to 60, default 15)\n  --decode-width <N>  LibVLC video callback width (64 to 1920, default 480)\n  --ascii             Render in monochrome ASCII mode\n  --doctor            Check LibVLC runtime availability\n  -h, --help          Show help message\n\nControls:\n  Space pause  Left/Right seek  s subs  a audio  n/p next/prev\n  Up/Down browse  Enter play  Backspace parent  +/- volume  m mute\n  f focus video  [/] speed  q quit\n\nRequires VLC 3.x libraries (libvlc.dll / libvlc.so.5).");
        return Ok(());
    }
    if opts.doctor {
        println!("TermVLC diagnostics");
        println!("platform: {}", std::env::consts::OS);
        println!("VLC_HOME: {}", std::env::var("VLC_HOME").unwrap_or_else(|_| "not set".into()));
        match VlcPlayer::new(opts.decode_width) {
            Ok(_) => println!("LibVLC: OK (created media player)"),
            Err(e) => println!("LibVLC: FAILED\n{e:#}\nInstall 64-bit VLC 3.x and set VLC_HOME if needed."),
        }
        return Ok(());
    }

    let input = opts.source;
    let mut player = VlcPlayer::new(opts.decode_width).context("could not initialize LibVLC; run --doctor")?;
    let initial_path = input.as_deref().filter(|s| !s.contains("://"));
    let mut app = App::new(initial_path.map(Path::new));

    if let Some(ref value) = input {
        if value.contains("://") || Path::new(value).is_file() {
            player.open(value)?;
            app.set_current(value.clone());
        }
    }
    let (_guard, mut terminal) = TerminalGuard::enter()?;
    let frame_interval = Duration::from_micros((1_000_000.0 / opts.fps as f64).round() as u64);
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        let timeout = Duration::from_millis(15);
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }
                    if !app.handle_key(k, &mut player)? { break; }
                }
                Event::Mouse(m) => app.handle_mouse(m),
                Event::Resize(_, _) => terminal.clear()?,
                _ => {}
            }
        }
        if last_draw.elapsed() >= frame_interval {
            let frame = player.latest_frame();
            let snapshot = player.snapshot();
            terminal.draw(|f| renderer::draw(f, &app, &snapshot, frame.as_deref(), opts.ascii))?;
            last_draw = Instant::now();
        }
    }
    Ok(())
}
