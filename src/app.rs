//! UI state, media library, mouse+keyboard control. All media I/O stays in LibVLC.
use std::{fs, path::{Path, PathBuf}, time::{Duration, Instant}};
use anyhow::Result;
use ratatui::{layout::Rect, crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind}};
use crate::{subtitles::SubtitleFile, vlc::VlcPlayer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Player,
    Playlist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay { None, Help, Info, SubtitleTracks, AudioTracks }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPurpose { Media, Subtitle }

#[derive(Clone, Debug)]
pub struct InputDialog { pub purpose: InputPurpose, pub text: String }

#[derive(Clone, Debug)]
pub enum TrackAction { SubtitleOff, SubtitleExternal, SubtitleEmbedded(i32), Audio(i32) }

#[derive(Clone, Debug)]
pub struct TrackOption { pub label: String, pub action: TrackAction }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    Media,
    Playback,
    Audio,
    Video,
    Subtitle,
    Tools,
    View,
    Help,
}

impl MenuKind {
    pub const ALL: &'static [MenuKind] = &[
        MenuKind::Media,
        MenuKind::Playback,
        MenuKind::Audio,
        MenuKind::Video,
        MenuKind::Subtitle,
        MenuKind::Tools,
        MenuKind::View,
        MenuKind::Help,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Playback => "Playback",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Subtitle => "Subtitle",
            Self::Tools => "Tools",
            Self::View => "View",
            Self::Help => "Help",
        }
    }

    pub fn items(&self) -> &'static [&'static str] {
        match self {
            Self::Media => &[
                "Open File...               Ctrl+O",
                "Open Location / URL...     Ctrl+U",
                "Open Directory...          Ctrl+D",
                "Quit                       Q",
            ],
            Self::Playback => &[
                "Play / Pause               Space",
                "Stop                       X",
                "Previous                   P",
                "Next                       N",
                "Faster (+0.25x)            ]",
                "Slower (-0.25x)            [",
                "Jump Forward 10s           →",
                "Jump Backward 10s          ←",
            ],
            Self::Audio => &[
                "Audio Tracks...            A",
                "Volume Up (+5%)            +",
                "Volume Down (-5%)          -",
                "Mute                       M",
            ],
            Self::Video => &[
                "Fullscreen                 F",
                "Toggle Renderer            G",
            ],
            Self::Subtitle => &[
                "Subtitle Tracks...         S",
                "Add Subtitle File...       L",
                "Delay +100 ms              .",
                "Delay -100 ms              ,",
            ],
            Self::Tools => &[
                "Media Information          I",
            ],
            Self::View => &[
                "Toggle Playlist / Player   Tab",
                "Refresh Library Files      R",
            ],
            Self::Help => &[
                "Controls & Help            ?",
                "About VLC media player",
            ],
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct HitZones {
    pub back: Option<Rect>,
    pub fullscreen: Option<Rect>,
    pub play: Option<Rect>,
    pub stop: Option<Rect>,
    pub prev: Option<Rect>,
    pub next: Option<Rect>,
    pub playlist_toggle: Option<Rect>,
    pub timeline: Option<Rect>,
    pub subtitles: Option<Rect>,
    pub audio: Option<Rect>,
    pub volume: Option<Rect>,
    pub browser: Option<(Rect, usize)>,
    pub track_list: Option<(Rect, usize)>,
    pub menu_bar: Vec<(Rect, MenuKind)>,
    pub menu_dropdown: Option<(Rect, Vec<(Rect, usize)>)>,
}

fn hit(area: Option<Rect>, x: u16, y: u16) -> bool {
    area.is_some_and(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
}

fn is_subtitle_path(p: &Path) -> bool {
    p.extension().and_then(|x| x.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "srt" | "vtt" | "ass" | "sub"))
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct App {
    pub cwd: PathBuf,
    pub entries: Vec<PathBuf>,
    pub selected: usize,
    pub current: Option<String>,
    pub status: String,
    pub status_at: Instant,
    pub fullscreen: bool,
    pub view_mode: ViewMode,
    pub overlay: Overlay,
    pub input: Option<InputDialog>,
    pub tracks: Vec<TrackOption>,
    pub track_selected: usize,
    pub volume: i32,
    saved_volume: i32,
    pub sidecar: Option<SubtitleFile>,
    pub sidecar_enabled: bool,
    pub subtitle_delay_ms: i64,
    pub selected_subtitle: String,
    pub selected_audio: String,
    pub last_osd: Instant,
    pub last_file_click: Option<(Instant, usize)>,
    pub active_menu: Option<MenuKind>,
    pub menu_selected: usize,
}

fn supported(p: &Path) -> bool {
    const EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "mpeg", "mpg", "3gp", "vob", "mp3", "flac", "ogg", "wav", "m4a", "aac", "opus"];
    p.is_dir() || p.extension().and_then(|x| x.to_str())
        .is_some_and(|s| EXTS.contains(&s.to_ascii_lowercase().as_str()))
}

impl App {
    pub fn new(path: Option<&Path>) -> Self {
        let resolved = path.unwrap_or_else(|| Path::new("."));
        let cwd = if resolved.is_dir() { resolved.to_path_buf() }
            else { resolved.parent().unwrap_or_else(|| Path::new(".")).to_path_buf() };
        let is_dir_arg = path.is_some_and(|p| p.is_dir());
        let mut app = Self {
            cwd, entries: vec![], selected: 0, current: None,
            status: "VLC media player ready. Drag and drop media or press Ctrl+O to open.".into(),
            status_at: Instant::now(), fullscreen: false,
            // Default to Player view (exact VLC home page / video view) unless launched on a directory
            view_mode: if is_dir_arg { ViewMode::Playlist } else { ViewMode::Player },
            overlay: Overlay::None, input: None, tracks: vec![], track_selected: 0,
            volume: 100, saved_volume: 100, sidecar: None, sidecar_enabled: false,
            subtitle_delay_ms: 0, selected_subtitle: "Off".into(),
            selected_audio: "Default".into(), last_osd: Instant::now(), last_file_click: None,
            active_menu: None, menu_selected: 0,
        };
        app.refresh();
        if resolved.is_file() {
            let absolute = resolved.canonicalize().unwrap_or_else(|_| resolved.to_path_buf());
            if let Some(i) = app.entries.iter().position(|p| p.canonicalize().ok().as_ref() == Some(&absolute)) { app.selected = i; }
        }
        app
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_at = Instant::now();
        self.last_osd = Instant::now();
    }
    pub fn status_visible(&self) -> bool { self.status_at.elapsed() < Duration::from_secs(5) }
    pub fn show_osd(&self) -> bool { self.last_osd.elapsed() < Duration::from_secs(4) }
    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        self.overlay = Overlay::None;
        self.active_menu = None;
        self.last_osd = Instant::now();
    }

    pub fn open_media(&mut self, source: &str, player: &mut VlcPlayer) -> Result<()> {
        player.open(source)?;
        self.current = Some(source.to_owned());
        self.view_mode = ViewMode::Player; // Immediately switch to player view like VLC desktop
        self.overlay = Overlay::None;
        self.active_menu = None;
        self.sidecar = if source.contains("://") { None } else { SubtitleFile::discover(Path::new(source)) };
        self.sidecar_enabled = self.sidecar.is_some();
        self.subtitle_delay_ms = 0;
        if self.sidecar_enabled {
            if player.is_native_window() {
                if let Some(ref sidecar) = self.sidecar {
                    if player.load_vlc_subtitle(&sidecar.path).is_ok() { self.sidecar_enabled = false; }
                }
            } else {
                player.select_subtitle(-1);
            }
        }
        self.selected_subtitle = if let Some(ref sidecar) = self.sidecar {
            sidecar.path.file_name().unwrap_or_default().to_string_lossy().to_string()
        } else { "VLC default".to_string() };
        self.notify(format!("▶ {}", source.rsplit(['/', '\\']).next().unwrap_or(source)));
        Ok(())
    }

    pub fn refresh(&mut self) {
        let mut entries: Vec<_> = fs::read_dir(&self.cwd).ok().into_iter().flat_map(|x| x.flatten())
            .map(|entry| entry.path()).filter(|p| supported(p)).take(3000).collect();
        entries.sort_by(|a,b| (a.is_file(), a.file_name().map(|x| x.to_string_lossy().to_ascii_lowercase()))
            .cmp(&(b.is_file(), b.file_name().map(|x| x.to_string_lossy().to_ascii_lowercase()))));
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub fn enter(&mut self, player: &mut VlcPlayer) -> Result<()> {
        if let Some(p) = self.entries.get(self.selected).cloned() {
            if p.is_dir() { self.cwd = p; self.selected = 0; self.refresh(); }
            else if let Err(e) = self.open_media(&p.to_string_lossy(), player) { self.notify(format!("Open error: {e:#}")); }
        }
        Ok(())
    }

    pub fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent() { self.cwd = parent.to_path_buf(); self.selected = 0; self.refresh(); }
    }

    pub fn adjacent(&mut self, player: &mut VlcPlayer, step: isize) {
        let files: Vec<PathBuf> = self.entries.iter().filter(|p| p.is_file()).cloned().collect();
        if files.is_empty() { self.notify("No media in this directory"); return; }
        let current = self.current.as_ref().map(Path::new);
        let idx = current.and_then(|p| files.iter().position(|q| q.as_path() == p));
        let next = match idx { Some(i) => (i as isize + step).rem_euclid(files.len() as isize) as usize,
            None if step < 0 => files.len() - 1, None => 0 };
        if let Some(sel) = self.entries.iter().position(|p| p == &files[next]) { self.selected = sel; }
        if let Err(e) = self.open_media(&files[next].to_string_lossy(), player) { self.notify(format!("Open error: {e:#}")); }
    }

    pub fn subtitle_text(&self, time_ms: i64) -> Option<&str> {
        if self.sidecar_enabled { self.sidecar.as_ref()?.current(time_ms, self.subtitle_delay_ms) }
        else { None }
    }

    pub fn load_subtitle(&mut self, path: &Path, player: &mut VlcPlayer) -> Result<()> {
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        if (ext == "srt" || ext == "vtt") && !player.is_native_window() {
            let captions = SubtitleFile::load(path)?;
            self.selected_subtitle = captions.path.file_name().unwrap_or_default().to_string_lossy().to_string();
            self.sidecar = Some(captions);
            self.sidecar_enabled = true;
            player.select_subtitle(-1);
            self.notify(format!("Subtitles: {}", self.selected_subtitle));
        } else {
            player.load_vlc_subtitle(path)?;
            self.sidecar_enabled = false;
            self.selected_subtitle = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            self.notify(format!("VLC subtitles: {}", self.selected_subtitle));
        }
        Ok(())
    }

    pub fn open_tracks(&mut self, which: Overlay, player: &VlcPlayer) {
        self.overlay = which;
        self.active_menu = None;
        let mut options = Vec::new();
        match which {
            Overlay::SubtitleTracks => {
                options.push(TrackOption {label: "Off".into(), action: TrackAction::SubtitleOff});
                if let Some(ref s) = self.sidecar {
                    options.push(TrackOption {label: format!("External  ·  {}", s.path.file_name().unwrap_or_default().to_string_lossy()), action: TrackAction::SubtitleExternal});
                }
                options.extend(player.subtitle_tracks().into_iter().filter(|t| t.id != -1)
                    .map(|t| TrackOption {label: format!("{} · #{}", t.name, t.id), action: TrackAction::SubtitleEmbedded(t.id)}));
            }
            Overlay::AudioTracks => {
                options.extend(player.audio_tracks().into_iter().filter(|t| t.id >= 0)
                    .map(|t| TrackOption {label: format!("{} · #{}", t.name, t.id), action: TrackAction::Audio(t.id)}));
            }
            _ => {}
        }
        if options.is_empty() { self.overlay = Overlay::None; self.notify("No available tracks"); }
        self.track_selected = 0;
        self.tracks = options;
        self.last_osd = Instant::now();
    }

    fn choose_track(&mut self, player: &mut VlcPlayer) {
        if let Some(option) = self.tracks.get(self.track_selected).cloned() {
            match option.action {
                TrackAction::SubtitleOff => {
                    player.select_subtitle(-1);
                    self.sidecar_enabled = false;
                    self.selected_subtitle = "Off".into();
                }
                TrackAction::SubtitleExternal => {
                    if player.is_native_window() {
                        if let Some(ref sidecar) = self.sidecar {
                            if player.load_vlc_subtitle(&sidecar.path).is_ok() {
                                self.selected_subtitle = option.label.clone();
                            }
                        }
                    } else {
                        player.select_subtitle(-1);
                        self.sidecar_enabled = true;
                        self.selected_subtitle = option.label.clone();
                    }
                }
                TrackAction::SubtitleEmbedded(id) => {
                    if player.select_subtitle(id) {
                        self.sidecar_enabled = false;
                        self.selected_subtitle = option.label.clone();
                    } else { self.notify("Couldn't activate this subtitle track"); }
                }
                TrackAction::Audio(id) => {
                    if player.select_audio(id) { self.selected_audio = option.label.clone(); }
                    else { self.notify("Couldn't activate this audio track"); }
                }
            }
            self.notify(format!("Track: {}", option.label));
        }
        self.overlay = Overlay::None;
    }

    fn accept_input(&mut self, player: &mut VlcPlayer) {
        let Some(dialog) = self.input.take() else { return };
        let value = dialog.text.trim().trim_matches('"');
        if value.is_empty() { return; }
        match dialog.purpose {
            InputPurpose::Media => {
                let p = Path::new(value);
                if p.is_dir() { self.cwd = p.to_path_buf(); self.selected = 0; self.refresh(); self.view_mode = ViewMode::Playlist; }
                else if let Err(e) = self.open_media(value, player) { self.notify(format!("Open error: {e:#}")); }
            }
            InputPurpose::Subtitle => {
                if let Err(e) = self.load_subtitle(Path::new(value), player) { self.notify(format!("Subtitle error: {e:#}")); }
            }
        }
    }

    /// Handles both clipboard pasting and direct file/directory drag-and-drop.
    pub fn handle_drop_or_paste(&mut self, text: &str, player: &mut VlcPlayer) -> Result<()> {
        let trimmed = text.trim().trim_matches('"').trim_matches('\'').trim();
        if trimmed.is_empty() { return Ok(()); }

        if let Some(ref mut dialog) = self.input {
            let remaining = 4096_usize.saturating_sub(dialog.text.len());
            dialog.text.extend(trimmed.chars().filter(|c| !c.is_control()).take(remaining));
            return Ok(());
        }

        let path = Path::new(trimmed);
        if path.is_dir() {
            self.cwd = path.to_path_buf();
            self.selected = 0;
            self.refresh();
            self.view_mode = ViewMode::Playlist;
            self.notify(format!("📂 Switched to: {}", path.display()));
        } else if is_subtitle_path(path) {
            self.load_subtitle(path, player)?;
        } else if path.is_file() || trimmed.contains("://") {
            self.open_media(trimmed, player)?;
        } else {
            self.notify(format!("Cannot open: {}", trimmed));
        }
        Ok(())
    }

    /// Trigger native desktop Open File dialog.
    pub fn open_file_picker(&mut self, player: &mut VlcPlayer) -> Result<()> {
        if let Some(path) = crate::dialogs::open_file_dialog() {
            let p_str = path.to_string_lossy().to_string();
            self.handle_drop_or_paste(&p_str, player)?;
        }
        Ok(())
    }

    pub fn trigger_menu_action(&mut self, menu: MenuKind, item_idx: usize, player: &mut VlcPlayer) -> Result<bool> {
        self.active_menu = None;
        match (menu, item_idx) {
            // Media
            (MenuKind::Media, 0) => self.open_file_picker(player)?,
            (MenuKind::Media, 1) => self.input = Some(InputDialog { purpose: InputPurpose::Media, text: String::new() }),
            (MenuKind::Media, 2) => self.input = Some(InputDialog { purpose: InputPurpose::Media, text: self.cwd.to_string_lossy().into_owned() }),
            (MenuKind::Media, 3) => return Ok(false), // quit

            // Playback
            (MenuKind::Playback, 0) => { player.toggle_pause(); self.last_osd = Instant::now(); }
            (MenuKind::Playback, 1) => { player.stop(); self.notify("■ Stopped"); }
            (MenuKind::Playback, 2) => self.adjacent(player, -1),
            (MenuKind::Playback, 3) => self.adjacent(player, 1),
            (MenuKind::Playback, 4) => { player.set_rate((player.rate() + 0.25).min(4.0)); self.notify(format!("Speed {:.2}×", player.rate())); }
            (MenuKind::Playback, 5) => { player.set_rate((player.rate() - 0.25).max(0.25)); self.notify(format!("Speed {:.2}×", player.rate())); }
            (MenuKind::Playback, 6) => { player.seek_relative(10_000); self.last_osd = Instant::now(); }
            (MenuKind::Playback, 7) => { player.seek_relative(-10_000); self.last_osd = Instant::now(); }

            // Audio
            (MenuKind::Audio, 0) => self.open_tracks(Overlay::AudioTracks, player),
            (MenuKind::Audio, 1) => { self.volume = (self.volume + 5).min(200); player.set_volume(self.volume); self.notify(format!("Volume: {}%", self.volume)); }
            (MenuKind::Audio, 2) => { self.volume = (self.volume - 5).max(0); player.set_volume(self.volume); self.notify(format!("Volume: {}%", self.volume)); }
            (MenuKind::Audio, 3) => {
                if self.volume > 0 { self.saved_volume = self.volume; self.volume = 0; }
                else { self.volume = self.saved_volume.max(10); }
                player.set_volume(self.volume);
                self.notify(if self.volume == 0 { "Muted".into() } else { format!("Volume: {}%", self.volume) });
            }

            // Video
            (MenuKind::Video, 0) => self.toggle_fullscreen(),
            (MenuKind::Video, 1) => { /* Graphics toggle handled in main */ }

            // Subtitle
            (MenuKind::Subtitle, 0) => self.open_tracks(Overlay::SubtitleTracks, player),
            (MenuKind::Subtitle, 1) => self.input = Some(InputDialog { purpose: InputPurpose::Subtitle, text: String::new() }),
            (MenuKind::Subtitle, 2) => { self.subtitle_delay_ms += 100; self.notify(format!("Subtitle delay: {} ms", self.subtitle_delay_ms)); }
            (MenuKind::Subtitle, 3) => { self.subtitle_delay_ms -= 100; self.notify(format!("Subtitle delay: {} ms", self.subtitle_delay_ms)); }

            // Tools
            (MenuKind::Tools, 0) => self.overlay = Overlay::Info,

            // View
            (MenuKind::View, 0) => self.view_mode = if self.view_mode == ViewMode::Player { ViewMode::Playlist } else { ViewMode::Player },
            (MenuKind::View, 1) => self.refresh(),

            // Help
            (MenuKind::Help, 0) => self.overlay = Overlay::Help,
            (MenuKind::Help, 1) => self.overlay = Overlay::Help,
            _ => {}
        }
        Ok(true)
    }

    pub fn handle_key(&mut self, key: KeyEvent, player: &mut VlcPlayer) -> Result<bool> {
        if let Some(ref mut dialog) = self.input {
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => self.accept_input(player),
                KeyCode::Backspace => { dialog.text.pop(); },
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if dialog.text.len() < 4096 { dialog.text.push(c); }
                },
                _ => {}
            }
            return Ok(true);
        }

        // Active VLC menu bar dropdown navigation
        if let Some(menu) = self.active_menu {
            let items = menu.items();
            match key.code {
                KeyCode::Esc => self.active_menu = None,
                KeyCode::Up | KeyCode::Char('k') => self.menu_selected = self.menu_selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.menu_selected = (self.menu_selected + 1).min(items.len().saturating_sub(1)),
                KeyCode::Left => {
                    let all = MenuKind::ALL;
                    let idx = all.iter().position(|&m| m == menu).unwrap_or(0);
                    let prev = (idx + all.len() - 1) % all.len();
                    self.active_menu = Some(all[prev]);
                    self.menu_selected = 0;
                }
                KeyCode::Right => {
                    let all = MenuKind::ALL;
                    let idx = all.iter().position(|&m| m == menu).unwrap_or(0);
                    let next = (idx + 1) % all.len();
                    self.active_menu = Some(all[next]);
                    self.menu_selected = 0;
                }
                KeyCode::Enter => return self.trigger_menu_action(menu, self.menu_selected, player),
                _ => {}
            }
            return Ok(true);
        }

        if self.overlay == Overlay::SubtitleTracks || self.overlay == Overlay::AudioTracks {
            match key.code {
                KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('a') => self.overlay = Overlay::None,
                KeyCode::Up | KeyCode::Char('k') => self.track_selected = self.track_selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.track_selected = (self.track_selected + 1).min(self.tracks.len().saturating_sub(1)),
                KeyCode::Enter => self.choose_track(player),
                _ => {}
            }
            return Ok(true);
        }

        if self.overlay != Overlay::None {
            self.overlay = Overlay::None;
            return Ok(true);
        }

        // Ctrl combinations
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('o') => { self.open_file_picker(player)?; return Ok(true); }
                KeyCode::Char('u') => { self.input = Some(InputDialog { purpose: InputPurpose::Media, text: String::new() }); return Ok(true); }
                KeyCode::Char('d') => { self.input = Some(InputDialog { purpose: InputPurpose::Media, text: self.cwd.to_string_lossy().into_owned() }); return Ok(true); }
                KeyCode::Char('q') => return Ok(false),
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') => return Ok(false),
            KeyCode::Esc => { if self.fullscreen { self.toggle_fullscreen(); } },
            KeyCode::F(11) | KeyCode::Char('f') => self.toggle_fullscreen(),
            KeyCode::Char(' ') => { player.toggle_pause(); self.last_osd = Instant::now(); },
            KeyCode::Char('x') => { player.stop(); self.notify("■ Stopped"); },
            KeyCode::Left => { player.seek_relative(-10_000); self.last_osd = Instant::now(); },
            KeyCode::Right => { player.seek_relative(10_000); self.last_osd = Instant::now(); },
            KeyCode::Char('+') | KeyCode::Char('=') => { self.volume = (self.volume + 5).min(200); player.set_volume(self.volume); self.notify(format!("Volume: {}%", self.volume)); },
            KeyCode::Char('-') => { self.volume = (self.volume - 5).max(0); player.set_volume(self.volume); self.notify(format!("Volume: {}%", self.volume)); },
            KeyCode::Char('m') => { if self.volume > 0 { self.saved_volume = self.volume; self.volume = 0; }
                else { self.volume = self.saved_volume.max(10); }
                player.set_volume(self.volume); self.notify(if self.volume == 0 { "Muted".into() } else { format!("Volume: {}%", self.volume) }); },
            KeyCode::Char('[') => { player.set_rate((player.rate() - 0.25).max(0.25)); self.notify(format!("Speed {:.2}×", player.rate())); },
            KeyCode::Char(']') => { player.set_rate((player.rate() + 0.25).min(4.0)); self.notify(format!("Speed {:.2}×", player.rate())); },
            KeyCode::Char('s') => self.open_tracks(Overlay::SubtitleTracks, player),
            KeyCode::Char('a') => self.open_tracks(Overlay::AudioTracks, player),
            KeyCode::Char('L') | KeyCode::Char('l') => self.input = Some(InputDialog { purpose: InputPurpose::Subtitle, text: String::new() }),
            KeyCode::Char('o') => self.open_file_picker(player)?,
            KeyCode::Char('?') | KeyCode::F(1) => self.overlay = Overlay::Help,
            KeyCode::Char('i') => self.overlay = Overlay::Info,
            KeyCode::Char(',') => { self.subtitle_delay_ms -= 100; self.notify(format!("Subtitle delay: {} ms", self.subtitle_delay_ms)); },
            KeyCode::Char('.') => { self.subtitle_delay_ms += 100; self.notify(format!("Subtitle delay: {} ms", self.subtitle_delay_ms)); },
            KeyCode::Char('n') => self.adjacent(player, 1),
            KeyCode::Char('p') => self.adjacent(player, -1),
            KeyCode::Tab => self.view_mode = if self.view_mode == ViewMode::Player { ViewMode::Playlist } else { ViewMode::Player },
            KeyCode::Up | KeyCode::Char('k') if self.view_mode == ViewMode::Playlist && !self.fullscreen => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.view_mode == ViewMode::Playlist && !self.fullscreen => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Backspace if self.fullscreen => self.toggle_fullscreen(),
            KeyCode::Backspace if self.view_mode == ViewMode::Playlist => self.parent(),
            KeyCode::Enter if self.view_mode == ViewMode::Playlist && !self.fullscreen => self.enter(player)?,
            KeyCode::Char('r') => self.refresh(),
            _ => {}
        }
        Ok(true)
    }

    pub fn handle_mouse(&mut self, evt: MouseEvent, zones: &HitZones, player: &mut VlcPlayer) -> Result<()> {
        let (x, y) = (evt.column, evt.row);
        if self.input.is_some() { return Ok(()); }

        // 1. Click on menu bar headers (Media, Playback, Audio, Video, Subtitle, Tools, View, Help)
        if !self.fullscreen && matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) {
            for (rect, menu) in &zones.menu_bar {
                if hit(Some(*rect), x, y) {
                    if self.active_menu == Some(*menu) {
                        self.active_menu = None;
                    } else {
                        self.active_menu = Some(*menu);
                        self.menu_selected = 0;
                    }
                    return Ok(());
                }
            }
        }

        // 2. Click inside open dropdown menu
        if let Some((_, ref items)) = zones.menu_dropdown {
            if matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(menu) = self.active_menu {
                    for (item_rect, idx) in items {
                        if hit(Some(*item_rect), x, y) {
                            let _ = self.trigger_menu_action(menu, *idx, player);
                            self.active_menu = None;
                            return Ok(());
                        }
                    }
                }
                self.active_menu = None;
                return Ok(());
            }
        }

        // Close menu if clicked anywhere else
        if self.active_menu.is_some() && matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.active_menu = None;
            return Ok(());
        }

        if self.overlay == Overlay::SubtitleTracks || self.overlay == Overlay::AudioTracks {
            match evt.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((rect, first)) = zones.track_list.filter(|(r, _)| hit(Some(*r), x, y)) {
                        let choice = first + usize::from(y - rect.y);
                        if choice < self.tracks.len() { self.track_selected = choice; self.choose_track(player); }
                    } else { self.overlay = Overlay::None; }
                }
                MouseEventKind::ScrollDown => self.track_selected = (self.track_selected + 1).min(self.tracks.len().saturating_sub(1)),
                MouseEventKind::ScrollUp => self.track_selected = self.track_selected.saturating_sub(1),
                _ => {}
            }
            return Ok(());
        }

        if self.overlay != Overlay::None {
            if matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) { self.overlay = Overlay::None; }
            return Ok(());
        }

        match evt.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.fullscreen && hit(zones.back, x, y) { self.toggle_fullscreen(); }
                else if hit(zones.fullscreen, x, y) { self.toggle_fullscreen(); }
                else if hit(zones.play, x, y) { player.toggle_pause(); self.last_osd = Instant::now(); }
                else if hit(zones.stop, x, y) { player.stop(); self.notify("■ Stopped"); }
                else if hit(zones.prev, x, y) { self.adjacent(player, -1); }
                else if hit(zones.next, x, y) { self.adjacent(player, 1); }
                else if hit(zones.playlist_toggle, x, y) {
                    self.view_mode = if self.view_mode == ViewMode::Player { ViewMode::Playlist } else { ViewMode::Player };
                }
                else if hit(zones.subtitles, x, y) { self.open_tracks(Overlay::SubtitleTracks, player); }
                else if hit(zones.audio, x, y) { self.open_tracks(Overlay::AudioTracks, player); }
                else if let Some(vol_rect) = zones.volume.filter(|r| hit(Some(*r), x, y)) {
                    let frac = (f64::from(x.saturating_sub(vol_rect.x)) / f64::from(vol_rect.width.max(1))).clamp(0.0, 1.0);
                    let new_vol = (frac * 125.0).round() as i32;
                    self.volume = new_vol;
                    player.set_volume(new_vol);
                    self.notify(format!("🔊 Volume: {new_vol}%"));
                }
                else if let Some(progress) = zones.timeline.filter(|r| hit(Some(*r), x, y)) {
                    let snap = player.snapshot();
                    if snap.length_ms > 0 {
                        let fraction = f64::from(x.saturating_sub(progress.x)) / f64::from(progress.width.max(1));
                        player.seek_to((fraction * snap.length_ms as f64) as i64);
                        self.last_osd = Instant::now();
                    }
                } else if let Some((rect, start)) = zones.browser.filter(|(r, _)| hit(Some(*r), x, y)) {
                    let row = (y - rect.y) as usize;
                    let index = (start + row).min(self.entries.len().saturating_sub(1));
                    let double = self.last_file_click.is_some_and(|(at, prev)| prev == index && at.elapsed() < Duration::from_millis(450));
                    self.last_file_click = Some((Instant::now(), index));
                    self.selected = index;
                    if double { self.enter(player)?; self.last_file_click = None; }
                } else if self.fullscreen { player.toggle_pause(); self.last_osd = Instant::now(); }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(rect) = zones.timeline.filter(|r| hit(Some(*r), x, y)) {
                    let snap = player.snapshot();
                    if snap.length_ms > 0 {
                        let fraction = f64::from(x.saturating_sub(rect.x)) / f64::from(rect.width.max(1));
                        player.seek_to((fraction * snap.length_ms as f64) as i64);
                    }
                } else if let Some(vol_rect) = zones.volume.filter(|r| hit(Some(*r), x, y)) {
                    let frac = (f64::from(x.saturating_sub(vol_rect.x)) / f64::from(vol_rect.width.max(1))).clamp(0.0, 1.0);
                    let new_vol = (frac * 125.0).round() as i32;
                    self.volume = new_vol;
                    player.set_volume(new_vol);
                }
            }
            MouseEventKind::ScrollDown if self.view_mode == ViewMode::Playlist && !self.fullscreen => {
                self.selected = (self.selected + 3).min(self.entries.len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp if self.view_mode == ViewMode::Playlist && !self.fullscreen => {
                self.selected = self.selected.saturating_sub(3);
            }
            MouseEventKind::ScrollUp => {
                self.volume = (self.volume + 5).min(125);
                player.set_volume(self.volume);
                self.notify(format!("🔊 Volume: {}%", self.volume));
            }
            MouseEventKind::ScrollDown => {
                self.volume = self.volume.saturating_sub(5);
                player.set_volume(self.volume);
                self.notify(format!("🔊 Volume: {}%", self.volume));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_filter() {
        assert!(supported(Path::new("movie.MKV")));
        assert!(supported(Path::new("sound.flac")));
        assert!(!supported(Path::new("notes.txt")));
    }
    #[test]
    fn empty_browser_never_underflows() { let a = App::new(Some(Path::new("/nonexistent/termvlc"))); assert!(a.entries.is_empty()); }
    #[test]
    fn fullscreen_is_reversible() {
        let mut app = App::new(Some(Path::new(".")));
        app.toggle_fullscreen(); assert!(app.fullscreen);
        app.toggle_fullscreen(); assert!(!app.fullscreen);
    }
    #[test]
    fn hit_test_border() {
        let r = Rect::new(3, 4, 10, 2);
        assert!(hit(Some(r),3,4));
        assert!(!hit(Some(r),13,4));
    }
}
