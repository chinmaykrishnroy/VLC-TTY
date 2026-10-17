mod app;
mod dialogs;
mod graphics;
mod input;
mod latest;
mod metadata;
mod options;
mod playback;
mod queue;
mod renderer;
mod subtitles;
mod vlc;

use crate::{
    app::{App, HitZones},
    graphics::{GraphicsChoice, GraphicsRenderer},
    vlc::VlcPlayer,
};
use anyhow::{bail, Context, Result};
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{
            self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
            KeyModifiers,
        },
        execute,
        terminal::{
            disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
            EnterAlternateScreen, LeaveAlternateScreen,
        },
    },
    Terminal,
};
use std::{
    io::{self, IsTerminal, Stdout},
    path::Path,
    time::{Duration, Instant},
};

use crate::options::Options;

fn usage() {
    println!(
        "VLC-TTY {} — video playback inside your terminal\n\n\
Usage: vlc-tty [OPTIONS] [FILE|FOLDER|URL]\n\n\
  --ssh                 SSH profile: silent, 15 FPS, 720p callback buffers\n\
  --silent              Disable audio; no sound device or desktop required\n\
  --audio               Play sound on the machine running VLC-TTY\n\
  --graphics MODE       auto|kitty|sixel|iterm2|ansi (default auto)\n\
  --fps N               Encoding/drawing limit, 1–120\n\
  --decode-height N     Callback height cap, 0 = uncapped (max 4320)\n\
  --subtitle PATH       External subtitle file\n\
  --native-window       Optional separate video window (local desktop only)\n\
  --doctor              Runtime/session diagnostics without opening the TUI\n\
  --smoke-test MEDIA    Silent headless decoding check, no terminal required\n\
  -V, --version         Show version\n\
  -h, --help            Show help\n\n\
SSH sessions are detected automatically. Explicit flags override profile defaults.\n\
Use ssh -t when launching the player as a remote command. Your client terminal\n\
must support Kitty/Sixel/iTerm2 for pixels; ANSI half-block fallback is available.\n\n\
Controls: Space play/pause; s stop; f fullscreen; arrows seek; +/- volume;\n\
Shift+S subtitles; Shift+A audio tracks; l load captions; ,/. subtitle delay;\n\
j/k audio delay; n/p next/previous; Tab queue; g renderer; o open; q quit.\n\n\
Requires LibVLC 3.x and its plugins. --silent does not forward sound over SSH.",
        env!("CARGO_PKG_VERSION")
    );
}

fn smoke_test(player: &mut VlcPlayer, source: &str) -> Result<()> {
    player.open(source)?;
    let started = Instant::now();
    loop {
        let info = player.snapshot();
        if info.frames >= 3 && info.time_ms > 200 {
            let frame = player
                .latest_frame()
                .context("Decoded frames were not available")?;
            println!(
                "Decoded {} frames; clock {} ms; output {}x{}; LibVLC {}",
                info.frames,
                info.time_ms,
                frame.width,
                frame.height,
                player.library_version()
            );
            player.stop();
            return Ok(());
        }
        if info.state == 7 {
            bail!("LibVLC failed to decode {source}; check the runtime plugins and media path");
        }
        if started.elapsed() > Duration::from_secs(10) {
            bail!("Decode smoke test timed out: {info:?}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
/// Restore raw mode, mouse and alternate screen on any return path, including errors.
struct TerminalGuard;
struct SynchronizedUpdate;
impl SynchronizedUpdate {
    fn enter() -> io::Result<Self> {
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
        Ok(Self)
    }
}
impl Drop for SynchronizedUpdate {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), EndSynchronizedUpdate);
    }
}
impl TerminalGuard {
    fn enter() -> Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        if let Err(e) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            cursor::Hide,
            event::EnableMouseCapture,
            EnableBracketedPaste
        ) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
            return Err(e.into());
        }
        let guard = Self;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok((guard, terminal))
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            event::DisableMouseCapture,
            DisableBracketedPaste,
            cursor::Show,
            LeaveAlternateScreen
        );
    }
}

fn main() -> Result<()> {
    // Windows default thread stack is only 1 MB, which can overflow when large terminal
    // UI trees, LibVLC callbacks, and image encoding pipelines interact.
    // Run the main UI loop on an 8 MB stack thread for rock-solid stability.
    let builder = std::thread::Builder::new()
        .name("vlc-tty-main".into())
        .stack_size(8 * 1024 * 1024);
    let handler = builder.spawn(run)?;
    match handler.join() {
        Ok(res) => res,
        Err(e) => std::panic::resume_unwind(e),
    }
}

fn run() -> Result<()> {
    let remote =
        std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    let decode_env = std::env::var("TERMVLC_DECODE_MAX").ok();
    let opts = Options::parse(std::env::args().skip(1), remote, decode_env.as_deref())?;
    if opts.help {
        usage();
        return Ok(());
    }
    if opts.version {
        println!("VLC-TTY {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if opts.doctor {
        println!("VLC-TTY {} diagnostics\nOS: {} / {}\nSSH profile: {}\nAudio: {}\nFPS limit: {}\nDecode height: {}\nTTY stdin/stdout: {}/{}",
            env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, opts.ssh,
            if opts.silent { "disabled" } else { "server/local device (not SSH forwarding)" }, opts.fps, opts.decode_height,
            io::stdin().is_terminal(), io::stdout().is_terminal());
        let player = VlcPlayer::configured(false, opts.silent, opts.decode_height)
            .context("LibVLC 3.x unavailable: install its matching libraries and plugins")?;
        println!(
            "LibVLC {} loaded: {}",
            player.library_version(),
            player.library_path().display()
        );
        return Ok(());
    }
    if !opts.smoke_test && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        bail!("Interactive playback requires a terminal on stdin and stdout. Use ssh -t, or --smoke-test MEDIA for headless decoding checks.");
    }
    let mut player = VlcPlayer::configured(opts.native_window, opts.silent, opts.decode_height)
        .context("Could not load LibVLC 3.x; run vlc-tty --doctor")?;
    if opts.smoke_test {
        return smoke_test(
            &mut player,
            opts.media.as_deref().context("Missing smoke-test media")?,
        );
    }
    let initial = opts.media.as_deref().filter(|s| !s.contains("://"));
    let mut app = App::new(initial.map(Path::new));
    app.native_dialogs = !opts.ssh;
    if let Some(ref source) = opts.media {
        if source.contains("://") || Path::new(source).is_file() {
            app.open_media(source, &mut player)?;
        }
    }
    if let Some(ref file) = opts.subtitle {
        app.load_subtitle(Path::new(file), &mut player)?;
    }
    let (_guard, mut terminal) = TerminalGuard::enter()?;
    // Query pixel-graphics support before the event loop begins consuming stdin.
    let mut graphics = GraphicsRenderer::new(opts.graphics, opts.fps);
    if opts.graphics == GraphicsChoice::Auto {
        app.notify(format!(
            "Rendering: {} (detected: {})",
            graphics.label(),
            graphics.detected_label()
        ));
    }
    let mut zones = HitZones::default();
    let mut previous_presentation = (
        app.fullscreen,
        app.media_revision,
        app.overlay,
        app.active_menu,
        app.input.is_some(),
        app.sidecar_enabled,
    );
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let interval = Duration::from_millis((1000 / opts.fps.max(1)) as u64);
    loop {
        if event::poll(Duration::from_millis(4))? {
            let first = event::read()?;
            let batch_text = matches!(&first, Event::Key(k) if k.kind == KeyEventKind::Press
                && matches!(k.code, KeyCode::Char(_))
                && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
                && app.input.is_none()
                && app.overlay == app::Overlay::None
                && app.active_menu.is_none();
            let mut events = if batch_text {
                input::read_batch(first, |wait| {
                    if event::poll(wait)? {
                        Ok(Some(event::read()?))
                    } else {
                        Ok(None)
                    }
                })?
            } else {
                vec![first]
            };
            if let Some(drop) = input::dropped_path(&events).filter(|_| batch_text) {
                if let Err(error) = app.handle_drop_or_paste(&drop.path, &mut player) {
                    app.notify(format!("Open error: {error:#}"));
                }
                events = events.split_off(drop.consumed);
            }
            {
                for event in events {
                    if matches!(event, Event::Resize(_, _)) {
                        terminal.clear()?;
                        graphics.invalidate();
                    }
                    match app.handle_event(event, &zones, &mut player)? {
                        app::AppEffect::Quit => return Ok(()),
                        app::AppEffect::ToggleRenderer => {
                            graphics.toggle_ansi();
                            app.notify(format!("Video renderer: {}", graphics.label()));
                            terminal.clear()?;
                            graphics.invalidate();
                        }
                        app::AppEffect::Continue => {}
                    }
                }
            }
        }
        app.tick(&mut player);
        let presentation = (
            app.fullscreen,
            app.media_revision,
            app.overlay,
            app.active_menu,
            app.input.is_some(),
            app.sidecar_enabled,
        );
        if previous_presentation != presentation {
            terminal.clear()?;
            graphics.invalidate();
            previous_presentation = presentation;
        }
        let frame = if opts.native_window {
            None
        } else {
            player.latest_frame().or_else(|| app.audio_thumbnail())
        };
        if !opts.native_window
            && app.overlay == app::Overlay::None
            && app.active_menu.is_none()
            && app.input.is_none()
        {
            graphics.prepare(frame.clone());
        }
        if last_draw.elapsed() >= interval {
            let info = player.snapshot();
            let sync_update = SynchronizedUpdate::enter()?;
            terminal.draw(|f| {
                zones = renderer::draw(
                    f,
                    &app,
                    &info,
                    frame.as_deref(),
                    &mut graphics,
                    opts.native_window,
                )
            })?;
            drop(sync_update);
            last_draw = Instant::now();
        }
    }
}
