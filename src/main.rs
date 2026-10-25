mod app;
mod audio;
mod dialogs;
mod graphics;
mod help;
mod host_fullscreen;
mod input;
mod latest;
mod metadata;
mod options;
mod playback;
mod playlist;
mod profiling;
mod queue;
mod remote;
mod remote_file;
mod renderer;
mod sixel_video;
mod subtitles;
mod terminal_probe;
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
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
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
type AppTerminal = Terminal<CrosstermBackend<profiling::CountingWriter<Stdout>>>;
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
    fn enter(profiling_enabled: bool) -> Result<(Self, AppTerminal, Arc<AtomicU64>)> {
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
        let writer = profiling::CountingWriter::new(io::stdout(), profiling_enabled);
        let bytes = writer.bytes.clone();
        let terminal = Terminal::new(CrosstermBackend::new(writer))?;
        Ok((guard, terminal, bytes))
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
    if remote::dispatch()? {
        return Ok(());
    }
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
        println!("VLC-TTY {} diagnostics\nOS: {} / {}\nSSH profile: {}\nAudio: {:?}\nFPS limit: {}\nDecode height: {}\nTTY stdin/stdout: {}/{}",
            env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, opts.ssh,
            opts.audio_output, opts.fps, opts.decode_height,
            io::stdin().is_terminal(), io::stdout().is_terminal());
        let player = VlcPlayer::configured(
            false,
            opts.audio_output == audio::AudioOutput::Disabled,
            opts.decode_height,
        )
        .context("LibVLC 3.x unavailable: install its matching libraries and plugins")?;
        println!(
            "LibVLC {} loaded: {}",
            player.library_version(),
            player.library_path().display()
        );
        return Ok(());
    }
    if let Some(source) = opts
        .media
        .as_deref()
        .filter(|source| !source.contains("://"))
    {
        let path = Path::new(source);
        if !path.is_file() && !path.is_dir() {
            bail!(
                "Media path not found or not a regular file/folder: {}",
                path.display()
            );
        }
    }
    if !opts.smoke_test && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        bail!("Interactive playback requires a terminal on stdin and stdout. Use ssh -t, or --smoke-test MEDIA for headless decoding checks.");
    }
    let mut recorder = opts
        .metrics
        .as_deref()
        .map(|path| profiling::Recorder::create(Path::new(path)))
        .transpose()
        .context("Could not create profiling file; choose a new writable path")?;
    if opts.direct_ssh_audio_warning {
        eprintln!("SSH audio is disabled. To hear sound locally, launch `vlc-tty connect HOST MEDIA` on your computer. Use --silent to select silence, or --audio-output local for the server's speakers.");
    }
    let mut player = VlcPlayer::configured(
        opts.native_window,
        opts.audio_output == audio::AudioOutput::Disabled,
        opts.decode_height,
    )
    .context("Could not load LibVLC 3.x; run vlc-tty --doctor")?;
    if opts.audio_output == audio::AudioOutput::Remote {
        let text = std::fs::read_to_string(
            opts.audio_token_file
                .as_deref()
                .context("Missing audio session key file")?,
        )
        .context("Cannot read private audio session key")?;
        let key = audio::transport::token(&text)?;
        player.forward_audio(
            opts.audio_endpoint
                .as_deref()
                .context("Missing audio endpoint")?,
            &key,
        )?;
    }
    player.enable_profiling(recorder.is_some());
    if opts.smoke_test {
        return smoke_test(
            &mut player,
            opts.media.as_deref().context("Missing smoke-test media")?,
            &termination,
        );
    }
    let initial = opts.media.as_deref().filter(|s| !s.contains("://"));
    let mut app = App::new(initial.map(Path::new));
    app.fullscreen = opts.fullscreen;
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
    let (_guard, mut terminal, output_bytes) = TerminalGuard::enter(recorder.is_some())?;
    // Query pixel-graphics support before the event loop begins consuming stdin.
    let mut graphics = GraphicsRenderer::new(
        opts.graphics,
        opts.fps,
        recorder.as_ref().map(|recorder| recorder.encoder.clone()),
        opts.kitty_compression,
    );
    if opts.graphics == GraphicsChoice::Auto {
        app.notify(format!(
            "Rendering: {} (detected: {})",
            graphics.label(),
            graphics.detected_label()
        ));
    }
    if opts.direct_ssh_audio_warning {
        app.notify("SSH audio disabled: use vlc-tty connect on your local computer");
    }
    let mut zones = HitZones::default();
    let mut host_fullscreen = host_fullscreen::Controller::new(host_fullscreen::NativeHost::new(
        !opts.ssh && !opts.native_window,
    ));
    let mut previous_presentation = (
        app.fullscreen,
        app.media_revision,
        app.overlay,
        app.active_menu.is_some(),
        app.input.is_some(),
        app.sidecar_enabled,
        app.fullscreen && app.show_osd(),
        app.fullscreen && app.subtitle_text(player.snapshot().time_ms).is_some(),
    );
    let mut needs_screen_reset = false;
    let mut video_delay = audio::sync::VideoDelay::new();
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let interval = Duration::from_millis((1000 / opts.fps.max(1)) as u64);
    loop {
        if player.audio_status().is_some_and(|(failed, _, _)| failed) {
            bail!(
                "Remote audio connection failed; playback stopped. Reconnect with vlc-tty connect."
            );
        }
        if termination.requested() {
            return Ok(());
        }
        let startup_event = graphics.take_startup_event();
        if startup_event.is_some() || event::poll(Duration::from_millis(4))? {
            let first = match startup_event {
                Some(event) => event,
                None => event::read()?,
            };
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
                    if host_fullscreen.is_forwarded_key(&event) {
                        continue;
                    }
                    if matches!(event, Event::Resize(_, _)) {
                        needs_screen_reset = true;
                    }
                    match app.handle_event(event, &zones, &mut player)? {
                        app::AppEffect::Quit => return Ok(()),
                        app::AppEffect::ToggleRenderer => {
                            graphics.toggle_ansi();
                            app.notify(format!("Video renderer: {}", graphics.label()));
                            needs_screen_reset = true;
                        }
                        app::AppEffect::Continue => {}
                    }
                }
            }
        }
        app.tick(&mut player);
        if let Some(message) = host_fullscreen.update(app.fullscreen, Instant::now()) {
            app.notify(message);
        }
        if host_fullscreen.take_external_exit() && app.fullscreen {
            app.toggle_fullscreen();
        }
        let presentation = (
            app.fullscreen,
            app.media_revision,
            app.overlay,
            // Switching open headers changes text, not the pixel/text mode.
            app.active_menu.is_some(),
            app.input.is_some(),
            app.sidecar_enabled,
            app.fullscreen && app.show_osd(),
            app.fullscreen && app.subtitle_text(player.snapshot().time_ms).is_some(),
        );
        if previous_presentation != presentation {
            needs_screen_reset = true;
            previous_presentation = presentation;
        }
        let video_frame = player.latest_frame();
        let motion = video_frame.is_some();
        let mut frame = if opts.native_window {
            None
        } else {
            video_frame.or_else(|| app.audio_thumbnail())
        };
        if let Some((generation, paused, delay)) = player.audio_delay_state() {
            frame = video_delay.select(
                frame,
                generation,
                paused,
                delay.saturating_sub(graphics.encode_latency()),
            );
        }
        if !opts.native_window
            && !needs_screen_reset
            && app.overlay == app::Overlay::None
            && app.active_menu.is_none()
            && app.input.is_none()
        {
            graphics.prepare(frame.clone(), motion);
        }
        if last_draw.elapsed() >= interval {
            let info = player.snapshot();
            let sync_update = SynchronizedUpdate::enter()?;
            let draw_started = Instant::now();
            if needs_screen_reset {
                // Erase pixels and repaint text as one visible transition.
                reset_fullscreen(&mut terminal)?;
                graphics.invalidate();
                needs_screen_reset = false;
            }
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
            graphics.observe_draw(draw_started.elapsed());
            let drawn = graphics.take_drawn();
            if let Some(recorder) = &mut recorder {
                recorder.record(
                    player.decode_metrics(),
                    output_bytes.load(Ordering::Relaxed),
                    drawn,
                    draw_started.elapsed(),
                    &info,
                    player.audio_metrics(),
                )?;
            }
            // The limit measures start-to-start cadence. Encoding/output work
            // already consumed part (or all) of this interval.
            last_draw = draw_started;
        }
    }
}
