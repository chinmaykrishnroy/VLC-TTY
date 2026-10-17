mod app;
mod dialogs;
mod graphics;
mod help;
mod input;
mod latest;
mod metadata;
mod options;
mod playback;
mod playlist;
mod queue;
mod renderer;
mod subtitles;
mod termination;
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
    help::print(false);
}

fn smoke_test(
    player: &mut VlcPlayer,
    source: &str,
    termination: &termination::Termination,
) -> Result<()> {
    player.open(source)?;
    let started = Instant::now();
    loop {
        if termination.requested() {
            player.stop();
            bail!("Playback interrupted");
        }
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
/// Ratatui's clear() preserves the cursor by querying the terminal. On a remote
/// terminal without cursor-report replies that query times out and consumes input.
/// Resizing our fullscreen viewport to its current size resets its buffers and
/// clears it without a cursor query; the next draw supplies the complete screen.
fn reset_fullscreen<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
) -> std::result::Result<(), B::Error> {
    let area = terminal.size()?.into();
    terminal.resize(area)
}
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
    let termination = termination::Termination::install()?;
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
    let playlist_sources = opts
        .playlist
        .as_deref()
        .map(|path| playlist::load(Path::new(path)))
        .transpose()?;
    if let Some(destination) = &opts.export_playlist {
        let sources = playlist_sources.unwrap_or_else(|| opts.media.clone().into_iter().collect());
        if sources.iter().any(|source| Path::new(source).is_dir()) {
            bail!("Export needs individual media entries or --playlist, not a folder");
        }
        playlist::save(Path::new(destination), &sources)?;
        println!("Saved {} entries to {destination}", sources.len());
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
            &termination,
        );
    }
    let initial = opts.media.as_deref().filter(|s| !s.contains("://"));
    let mut app = App::new(initial.map(Path::new));
    app.native_dialogs = !opts.ssh;
    app.volume = opts.volume;
    player.set_volume(opts.volume);
    if let Some(sources) = playlist_sources {
        let first = sources[0].clone();
        app.open_media(&first, &mut player)?;
        app.queue.replace(sources);
    }
    if let Some(ref source) = opts.media {
        if source.contains("://") || Path::new(source).is_file() {
            app.open_media(source, &mut player)?;
        }
    }
    if let Some(ref file) = opts.subtitle {
        app.load_subtitle(Path::new(file), &mut player)?;
    }
    player.set_rate(opts.rate);
    app.queue.repeat = opts.repeat;
    if opts.shuffle {
        app.queue.toggle_shuffle();
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
        if termination.requested() {
            return Ok(());
        }
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
                        reset_fullscreen(&mut terminal)?;
                        graphics.invalidate();
                    }
                    match app.handle_event(event, &zones, &mut player)? {
                        app::AppEffect::Quit => return Ok(()),
                        app::AppEffect::ToggleRenderer => {
                            graphics.toggle_ansi();
                            app.notify(format!("Video renderer: {}", graphics.label()));
                            reset_fullscreen(&mut terminal)?;
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
            reset_fullscreen(&mut terminal)?;
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
