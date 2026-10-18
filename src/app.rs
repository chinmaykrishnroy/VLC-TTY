//! UI state, media library, mouse+keyboard control. All media I/O stays in LibVLC.
use crate::{playback::Playback, renderer::time, subtitles::SubtitleFile, vlc::VideoFrame};
use anyhow::Result;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::Rect,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Player,
    Playlist,
    Browser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEffect {
    Continue,
    Quit,
    ToggleRenderer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Info,
    SubtitleTracks,
    AudioTracks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPurpose {
    Media,
    Subtitle,
}

#[derive(Clone, Debug)]
pub struct InputDialog {
    pub purpose: InputPurpose,
    pub text: String,
}

#[derive(Clone, Debug)]
pub enum TrackAction {
    SubtitleOff,
    SubtitleExternal,
    SubtitleEmbedded(i32),
    Audio(i32),
}

#[derive(Clone, Debug)]
pub struct TrackOption {
    pub label: String,
    pub action: TrackAction,
}

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
                "Open Network Stream...     Ctrl+N",
                "Open Location / URL...     Ctrl+U",
                "Open Directory...          Ctrl+D",
                "Quit                       Ctrl+Q",
            ],
            Self::Playback => &[
                "Play / Pause               Space",
                "Stop                       S / X",
                "Previous                   P",
                "Next                       N",
                "Faster (+0.25x)            ]",
                "Slower (-0.25x)            [",
                "Normal Speed (1.0x)        =",
                "Jump Forward 10s           Right",
                "Jump Backward 10s          Left",
                "Next Frame                 E",
                "Repeat: Off / All / One     Shift+R",
                "Shuffle                    Z",
            ],
            Self::Audio => &[
                "Cycle Audio Track          B",
                "Audio Tracks Menu...       Shift+A",
                "Volume Up (+5%)            + / Ctrl+Up",
                "Volume Down (-5%)          - / Ctrl+Down",
                "Mute                       M",
            ],
            Self::Video => &[
                "Fullscreen                 F / F11",
                "Toggle Renderer (Pixel/ANSI) G",
            ],
            Self::Subtitle => &[
                "Cycle Subtitles            V",
                "Subtitle Tracks Menu...    Shift+S",
                "Add Subtitle File...       L",
                "Delay +100 ms              . / H",
                "Delay -100 ms              ,",
            ],
            Self::Tools => &["Media Information          Ctrl+I"],
            Self::View => &[
                "Toggle Playlist / Player   Tab / Ctrl+L",
                "Refresh Library Files      R",
                "Browse Files",
            ],
            Self::Help => &["Help & Controls            F1 / ?", "About VLC-TTY"],
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct HitZones {
    pub back: Option<Rect>,
    pub fullscreen: Option<Rect>,
    pub video: Option<Rect>,
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
    pub overlay_box: Option<Rect>,
    pub menu_bar: Vec<(Rect, MenuKind)>,
    pub menu_dropdown: Option<(Rect, Vec<(Rect, usize)>)>,
}

fn hit(area: Option<Rect>, x: u16, y: u16) -> bool {
    area.is_some_and(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
}

fn is_subtitle_path(p: &Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "srt" | "vtt" | "ass" | "sub"
            )
        })
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct App {
    pub native_dialogs: bool,
    pub queue: crate::queue::PlaybackQueue,
    last_state: i32,
    pub cwd: PathBuf,
    pub entries: Vec<PathBuf>,
    pub selected: usize,
    pub current: Option<String>,
    pub media_revision: u64,
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
    pub audio_delay_ms: i64,
    pub selected_subtitle: String,
    pub selected_audio: String,
    pub last_osd: Instant,
    pub last_file_click: Option<(Instant, usize)>,
    pub last_video_click: Option<Instant>,
    pub active_menu: Option<MenuKind>,
    pub menu_selected: usize,
    pub audio_metadata: Option<crate::metadata::AudioMetadata>,
    metadata_poll_at: Instant,
    artwork_loader: crate::metadata::ArtworkLoader,
    pending_native_subtitle: Option<PathBuf>,
}

fn supported(p: &Path) -> bool {
    const EXTS: &[&str] = &[
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "mpeg", "mpg", "3gp", "vob",
        "mp3", "flac", "ogg", "wav", "m4a", "aac", "opus", "wma", "ape", "alac", "aiff", "aif",
        "mka", "mid", "midi", "ac3", "dts", "ra", "m4b", "m4p", "oga",
    ];
    p.is_dir()
        || p.extension()
            .and_then(|x| x.to_str())
            .is_some_and(|s| EXTS.contains(&s.to_ascii_lowercase().as_str()))
}

impl App {
    pub fn handle_event(
        &mut self,
        event: ratatui::crossterm::event::Event,
        zones: &HitZones,
        player: &mut dyn Playback,
    ) -> Result<AppEffect> {
        use ratatui::crossterm::event::{Event, KeyEventKind};
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(AppEffect::Quit);
                }
                self.handle_key(key, player)
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse, zones, player),
            Event::Paste(text) => {
                if let Err(error) = self.handle_drop_or_paste(&text, player) {
                    self.notify(format!("Open error: {error:#}"));
                }
                Ok(AppEffect::Continue)
            }
            _ => Ok(AppEffect::Continue),
        }
    }
    pub fn new(path: Option<&Path>) -> Self {
        let resolved = path.unwrap_or_else(|| Path::new("."));
        let cwd = if resolved.is_dir() {
            resolved.to_path_buf()
        } else {
            resolved
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };
        let is_dir_arg = path.is_some_and(|p| p.is_dir());
        let mut app = Self {
            native_dialogs: true,
            queue: crate::queue::PlaybackQueue::default(),
            last_state: 0,
            cwd,
            entries: vec![],
            selected: 0,
            current: None,
            media_revision: 0,
            status: "VLC-TTY ready. Drag and drop media or press Ctrl+O to open.".into(),
            status_at: Instant::now(),
            fullscreen: false,
            // Default to Player view (exact VLC home page / video view) unless launched on a directory
            view_mode: if is_dir_arg {
                ViewMode::Browser
            } else {
                ViewMode::Player
            },
            overlay: Overlay::None,
            input: None,
            tracks: vec![],
            track_selected: 0,
            volume: 100,
            saved_volume: 100,
            sidecar: None,
            sidecar_enabled: false,
            subtitle_delay_ms: 0,
            audio_delay_ms: 0,
            selected_subtitle: "Off".into(),
            selected_audio: "Default".into(),
            last_osd: Instant::now() - Duration::from_secs(10),
            last_file_click: None,
            last_video_click: None,
            active_menu: None,
            menu_selected: 0,
            audio_metadata: None,
            metadata_poll_at: Instant::now() - Duration::from_secs(1),
            artwork_loader: Default::default(),
            pending_native_subtitle: None,
        };
        app.refresh();
        if resolved.is_file() {
            let absolute = resolved
                .canonicalize()
                .unwrap_or_else(|_| resolved.to_path_buf());
            if let Some(i) = app
                .entries
                .iter()
                .position(|p| p.canonicalize().ok().as_ref() == Some(&absolute))
            {
                app.selected = i;
            }
        }
        app
    }

    pub fn audio_thumbnail(&self) -> Option<Arc<VideoFrame>> {
        self.audio_metadata.as_ref()?.thumbnail.clone()
    }
    pub fn is_audio(&self) -> bool {
        self.current
            .as_deref()
            .is_some_and(crate::metadata::is_audio_source)
    }

    pub fn audio_title(&self) -> String {
        let source = self.current.as_deref();
        if let Some(title) = self.audio_metadata.as_ref().and_then(|m| m.title.as_ref()) {
            if let Some(source) = source.filter(|s| s.contains("://")) {
                let name = source
                    .split(['?', '#'])
                    .next()
                    .unwrap_or(source)
                    .rsplit('/')
                    .next()
                    .unwrap_or(source);
                // LibVLC may use the encoded MRL basename as a provisional title.
                if title == source || title == name {
                    return crate::metadata::display_name(source);
                }
            }
            return title.clone();
        }
        source
            .map(crate::metadata::display_name)
            .unwrap_or_else(|| "Audio Track".into())
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_at = Instant::now();
        self.last_osd = Instant::now();
    }
    pub fn status_visible(&self) -> bool {
        self.status_at.elapsed() < Duration::from_secs(4)
    }
    pub fn show_osd(&self) -> bool {
        self.last_osd.elapsed() < Duration::from_secs(2)
    }
    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        self.overlay = Overlay::None;
        self.active_menu = None;
        // Start fullscreen completely clean without forced OSD
        self.last_osd = Instant::now() - Duration::from_secs(10);
    }

    pub fn open_media(&mut self, source: &str, player: &mut dyn Playback) -> Result<()> {
        player.open(source)?;
        self.media_revision = self.media_revision.wrapping_add(1);
        self.metadata_poll_at = Instant::now() - Duration::from_secs(1);
        let normalized = if source.contains("://") {
            source.to_owned()
        } else {
            Path::new(source)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(source))
                .to_string_lossy()
                .into_owned()
        };
        let source = normalized.as_str();
        let siblings = if source.contains("://") {
            vec![source.to_owned()]
        } else {
            Path::new(source)
                .parent()
                .and_then(|folder| fs::read_dir(folder).ok())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && supported(path))
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let mut siblings = siblings;
        siblings.sort_by_key(|source| source.to_lowercase());
        self.queue.select(source, siblings);
        self.last_state = 0;
        self.current = Some(source.to_owned());
        self.view_mode = ViewMode::Player; // Immediately switch to player view like VLC desktop
        self.overlay = Overlay::None;
        self.active_menu = None;

        let path = Path::new(source);
        if crate::metadata::is_audio_file(path) && path.is_file() {
            self.audio_metadata = Some(crate::metadata::extract_metadata(path));
        } else {
            self.audio_metadata = None;
        }

        self.sidecar = if source.contains("://") {
            None
        } else {
            SubtitleFile::discover(Path::new(source))
        };
        self.sidecar_enabled = self.sidecar.is_some();
        self.pending_native_subtitle = None;
        self.subtitle_delay_ms = 0;
        self.audio_delay_ms = 0;
        self.selected_audio = "Default".into();
        self.last_video_click = None;
        if self.sidecar_enabled {
            if player.is_native_window() {
                self.pending_native_subtitle =
                    self.sidecar.as_ref().map(|sidecar| sidecar.path.clone());
                self.sidecar_enabled = false;
            } else {
                player.select_subtitle(-1);
            }
        }
        self.selected_subtitle = if let Some(ref sidecar) = self.sidecar {
            sidecar
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            "VLC default".to_string()
        };
        self.notify(format!(
            "[Play] {}",
            source.rsplit(['/', '\\']).next().unwrap_or(source)
        ));
        Ok(())
    }

    pub fn refresh(&mut self) {
        let mut entries: Vec<_> = fs::read_dir(&self.cwd)
            .ok()
            .into_iter()
            .flat_map(|x| x.flatten())
            .map(|entry| entry.path())
            .filter(|p| supported(p))
            .take(3000)
            .collect();
        entries.sort_by(|a, b| {
            (
                a.is_file(),
                a.file_name()
                    .map(|x| x.to_string_lossy().to_ascii_lowercase()),
            )
                .cmp(&(
                    b.is_file(),
                    b.file_name()
                        .map(|x| x.to_string_lossy().to_ascii_lowercase()),
                ))
        });
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub fn enter(&mut self, player: &mut dyn Playback) -> Result<()> {
        if self.view_mode == ViewMode::Playlist {
            if let Some(source) = self.queue.sources().get(self.selected).cloned() {
                if let Err(error) = self.open_media(&source, player) {
                    self.notify(format!("Open error: {error:#}"));
                } else {
                    self.queue.set_position(self.selected);
                }
            }
            return Ok(());
        }
        if let Some(p) = self.entries.get(self.selected).cloned() {
            if p.is_dir() {
                self.cwd = p;
                self.selected = 0;
                self.refresh();
            } else if let Err(e) = self.open_media(&p.to_string_lossy(), player) {
                self.notify(format!("Open error: {e:#}"));
            }
        }
        Ok(())
    }

    pub fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.selected = 0;
            self.refresh();
        }
    }

    pub fn adjacent(&mut self, player: &mut dyn Playback, step: isize) {
        let Some(index) = self.queue.adjacent_index(step, false) else {
            self.notify(if step < 0 {
                "Start of playback queue"
            } else {
                "End of playback queue"
            });
            return;
        };
        let source = self.queue.sources()[index].clone();
        if let Err(error) = self.open_media(&source, player) {
            self.notify(format!("Open error: {error:#}"));
        } else {
            self.queue.set_position(index);
        }
    }

    fn toggle_shuffle(&mut self) {
        self.queue.toggle_shuffle();
        self.notify(if self.queue.shuffled {
            "Shuffle on"
        } else {
            "Shuffle off"
        });
    }

    pub fn tick(&mut self, player: &mut dyn Playback) {
        if self.is_audio() {
            if self.metadata_poll_at.elapsed() >= Duration::from_millis(250) {
                self.metadata_poll_at = Instant::now();
                let mut incoming = player.media_metadata();
                let network = self.current.as_deref().is_some_and(|s| s.contains("://"));
                if let Some(source) = self.current.as_deref().filter(|_| network) {
                    let encoded_name = source
                        .split(['?', '#'])
                        .next()
                        .unwrap_or(source)
                        .rsplit('/')
                        .next()
                        .unwrap_or(source);
                    if incoming.title.as_deref().is_some_and(|t| {
                        t == source
                            || t == encoded_name
                            || t == crate::metadata::display_name(source)
                    }) {
                        incoming.title = None; // Do not overwrite parsed tags with a provisional URL title.
                    }
                }
                let meta = self.audio_metadata.get_or_insert_with(Default::default);
                for (current, value) in [
                    (&mut meta.title, incoming.title),
                    (&mut meta.artist, incoming.artist),
                    (&mut meta.album, incoming.album),
                ] {
                    if value.is_some() && (network || current.is_none()) {
                        *current = value;
                    }
                }
                if meta.thumbnail.is_none() {
                    if let Some(url) = incoming.artwork_url.filter(|u| u.starts_with("file://")) {
                        self.artwork_loader.request(self.media_revision, &url);
                    } else if let Some(source) = &self.current {
                        self.artwork_loader.request(self.media_revision, source);
                    }
                }
            }
            if let Some(found) = self.artwork_loader.poll(self.media_revision) {
                let meta = self.audio_metadata.get_or_insert_with(Default::default);
                for (current, value) in [
                    (&mut meta.title, found.title),
                    (&mut meta.artist, found.artist),
                    (&mut meta.album, found.album),
                ] {
                    if value.is_some() {
                        *current = value;
                    }
                }
                if found.thumbnail.is_some() {
                    meta.thumbnail = found.thumbnail;
                }
            }
        }
        let state = player.snapshot().state;
        if state == self.last_state {
            return;
        }
        self.last_state = state;
        if state == 3 {
            if let Some(path) = self.pending_native_subtitle.take() {
                if let Err(error) = player.load_vlc_subtitle(&path) {
                    self.notify(format!("Subtitle error: {error:#}"));
                }
            } else if self.sidecar_enabled {
                player.select_subtitle(-1);
            }
        }
        if state == 7 {
            self.notify(
                "Playback failed. Check the file, stream address, and VLC runtime/plugins.",
            );
        }
        if state == 6 {
            if let Some(index) = self.queue.adjacent_index(1, true) {
                let source = self.queue.sources()[index].clone();
                let view = self.view_mode;
                let overlay = self.overlay;
                let menu = self.active_menu;
                if let Err(error) = self.open_media(&source, player) {
                    self.notify(format!("Open error: {error:#}"));
                } else {
                    self.queue.set_position(index);
                    // A natural transition must not dismiss help or pull the
                    // user out of a file/queue browser. Track pickers describe
                    // the old media and still close when that media changes.
                    self.view_mode = view;
                    if matches!(overlay, Overlay::Help | Overlay::Info) {
                        self.overlay = overlay;
                    }
                    self.active_menu = menu;
                }
            } else {
                self.notify("End of playback queue");
            }
        }
    }

    fn list_len(&self) -> usize {
        if self.view_mode == ViewMode::Playlist {
            self.queue.sources().len()
        } else {
            self.entries.len()
        }
    }

    fn toggle_playlist(&mut self) {
        self.view_mode = if self.view_mode == ViewMode::Player {
            ViewMode::Playlist
        } else {
            ViewMode::Player
        };
        self.selected = 0;
    }

    pub fn subtitle_text(&self, time_ms: i64) -> Option<std::borrow::Cow<'_, str>> {
        if self.sidecar_enabled {
            self.sidecar
                .as_ref()?
                .current(time_ms, self.subtitle_delay_ms)
        } else {
            None
        }
    }

    pub fn load_subtitle(&mut self, path: &Path, player: &mut dyn Playback) -> Result<()> {
        let ext = path
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if (ext == "srt" || ext == "vtt") && !player.is_native_window() {
            let captions = SubtitleFile::load(path)?;
            self.selected_subtitle = captions
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.sidecar = Some(captions);
            self.sidecar_enabled = true;
            player.select_subtitle(-1);
            self.notify(format!("Subtitles: {}", self.selected_subtitle));
        } else {
            player.load_vlc_subtitle(path)?;
            self.sidecar_enabled = false;
            self.selected_subtitle = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.notify(format!("VLC subtitles: {}", self.selected_subtitle));
        }
        Ok(())
    }

    pub fn open_tracks(&mut self, which: Overlay, player: &dyn Playback) {
        self.overlay = which;
        self.active_menu = None;
        let mut options = Vec::new();
        match which {
            Overlay::SubtitleTracks => {
                options.push(TrackOption {
                    label: "Off".into(),
                    action: TrackAction::SubtitleOff,
                });
                if let Some(ref s) = self.sidecar {
                    options.push(TrackOption {
                        label: format!(
                            "External  ·  {}",
                            s.path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        action: TrackAction::SubtitleExternal,
                    });
                }
                options.extend(
                    player
                        .subtitle_tracks()
                        .into_iter()
                        .filter(|t| t.id != -1)
                        .map(|t| TrackOption {
                            label: format!("{} · #{}", t.name, t.id),
                            action: TrackAction::SubtitleEmbedded(t.id),
                        }),
                );
            }
            Overlay::AudioTracks => {
                options.extend(
                    player
                        .audio_tracks()
                        .into_iter()
                        .filter(|t| t.id >= 0)
                        .map(|t| TrackOption {
                            label: format!("{} · #{}", t.name, t.id),
                            action: TrackAction::Audio(t.id),
                        }),
                );
            }
            _ => {}
        }
        if options.is_empty() {
            self.overlay = Overlay::None;
            self.notify("No available tracks");
        }
        self.track_selected = 0;
        self.tracks = options;
        self.last_osd = Instant::now();
    }

    pub fn cycle_subtitle(&mut self, player: &mut dyn Playback) {
        let mut tracks: Vec<(i32, String)> = vec![(-1, "Off".to_string())];
        if !player.is_native_window() {
            if let Some(ref s) = self.sidecar {
                tracks.push((
                    -2,
                    format!(
                        "External: {}",
                        s.path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                ));
            }
        }
        for t in player.subtitle_tracks() {
            if t.id != -1 {
                tracks.push((t.id, t.name));
            }
        }
        if tracks.is_empty() {
            self.notify("No subtitle tracks");
            return;
        }
        let cur_id = if self.sidecar_enabled {
            -2
        } else {
            player.current_subtitle_id()
        };
        let cur_idx = tracks.iter().position(|(id, _)| *id == cur_id).unwrap_or(0);
        let next_idx = (cur_idx + 1) % tracks.len();
        let (next_id, ref name) = tracks[next_idx];
        if next_id == -2 {
            if !player.select_subtitle(-1) {
                self.notify("Couldn't disable the embedded subtitle track");
                return;
            }
            self.sidecar_enabled = true;
        } else {
            if !player.select_subtitle(next_id) {
                self.notify("Couldn't activate this subtitle track");
                return;
            }
            self.sidecar_enabled = false;
        }
        self.selected_subtitle = name.clone();
        self.notify(format!("Subtitles: {}", name));
    }

    fn adjust_audio_delay(&mut self, delta_ms: i64, player: &mut dyn Playback) {
        let delay = self.audio_delay_ms.saturating_add(delta_ms);
        if player.set_audio_delay(delay) {
            self.audio_delay_ms = delay;
            self.notify(format!("Audio delay: {delay} ms"));
        } else {
            self.notify("Audio delay unavailable; start playback first");
        }
    }

    fn adjust_subtitle_delay(&mut self, delta_ms: i64, player: &mut dyn Playback) {
        let delay = self.subtitle_delay_ms.saturating_add(delta_ms);
        if self.sidecar_enabled || player.set_subtitle_delay(delay) {
            self.subtitle_delay_ms = delay;
            self.notify(format!("Subtitle delay: {delay} ms"));
        } else {
            self.notify("Subtitle delay unavailable; start playback first");
        }
    }

    pub fn cycle_audio(&mut self, player: &mut dyn Playback) {
        let tracks: Vec<_> = player
            .audio_tracks()
            .into_iter()
            .filter(|t| t.id >= 0)
            .collect();
        if tracks.is_empty() {
            self.notify("No audio tracks");
            return;
        }
        let cur_id = player.current_audio_id();
        let cur_idx = tracks.iter().position(|t| t.id == cur_id).unwrap_or(0);
        let next_idx = (cur_idx + 1) % tracks.len();
        let next = &tracks[next_idx];
        if !player.select_audio(next.id) {
            self.notify("Couldn't activate this audio track");
            return;
        }
        self.selected_audio = next.name.clone();
        self.notify(format!("Audio: {}", next.name));
    }

    fn choose_track(&mut self, player: &mut dyn Playback) {
        if let Some(option) = self.tracks.get(self.track_selected).cloned() {
            let success = match option.action {
                TrackAction::SubtitleOff => {
                    let success = player.select_subtitle(-1);
                    if success {
                        self.sidecar_enabled = false;
                        self.selected_subtitle = "Off".into();
                    }
                    success
                }
                TrackAction::SubtitleExternal => {
                    let success = if player.is_native_window() {
                        self.sidecar
                            .as_ref()
                            .is_some_and(|sidecar| player.load_vlc_subtitle(&sidecar.path).is_ok())
                    } else {
                        player.select_subtitle(-1)
                    };
                    if success {
                        self.sidecar_enabled = !player.is_native_window();
                        self.selected_subtitle = option.label.clone();
                    }
                    success
                }
                TrackAction::SubtitleEmbedded(id) => {
                    let success = player.select_subtitle(id);
                    if success {
                        self.sidecar_enabled = false;
                        self.selected_subtitle = option.label.clone();
                    }
                    success
                }
                TrackAction::Audio(id) => {
                    let success = player.select_audio(id);
                    if success {
                        self.selected_audio = option.label.clone();
                    }
                    success
                }
            };
            if success {
                self.notify(format!("Track: {}", option.label));
            } else {
                self.notify(format!("Couldn't activate track: {}", option.label));
            }
        }
        self.overlay = Overlay::None;
    }
    fn accept_input(&mut self, player: &mut dyn Playback) {
        let Some(dialog) = self.input.take() else {
            return;
        };
        let value = dialog.text.trim().trim_matches('"');
        if value.is_empty() {
            return;
        }
        match dialog.purpose {
            InputPurpose::Media => {
                let p = Path::new(value);
                if p.is_dir() {
                    self.cwd = p.to_path_buf();
                    self.selected = 0;
                    self.refresh();
                    self.view_mode = ViewMode::Browser;
                } else if let Err(e) = self.open_media(value, player) {
                    self.notify(format!("Open error: {e:#}"));
                }
            }
            InputPurpose::Subtitle => {
                if let Err(e) = self.load_subtitle(Path::new(value), player) {
                    self.notify(format!("Subtitle error: {e:#}"));
                }
            }
        }
    }

    /// Handles both clipboard pasting and direct file/directory drag-and-drop.
    pub fn handle_drop_or_paste(&mut self, text: &str, player: &mut dyn Playback) -> Result<()> {
        let trimmed = text
            .trim()
            .trim_start_matches('&')
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        if let Some(ref mut dialog) = self.input {
            let remaining = 4096_usize.saturating_sub(dialog.text.len());
            dialog
                .text
                .extend(trimmed.chars().filter(|c| !c.is_control()).take(remaining));
            return Ok(());
        }

        self.active_menu = None;
        if self.overlay != Overlay::None {
            self.overlay = Overlay::None;
        }

        let path = Path::new(trimmed);
        if path.is_dir() {
            self.cwd = path.to_path_buf();
            self.selected = 0;
            self.refresh();
            self.view_mode = ViewMode::Browser;
            self.notify(format!("[Folder] Switched to: {}", path.display()));
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
    pub fn open_file_picker(&mut self, player: &mut dyn Playback) -> Result<()> {
        if !self.native_dialogs || !cfg!(windows) {
            self.input = Some(InputDialog {
                purpose: InputPurpose::Media,
                text: String::new(),
            });
            return Ok(());
        }
        if let Some(path) = crate::dialogs::open_file_dialog() {
            let p_str = path.to_string_lossy().to_string();
            self.handle_drop_or_paste(&p_str, player)?;
        }
        Ok(())
    }

    pub fn trigger_menu_action(
        &mut self,
        menu: MenuKind,
        item_idx: usize,
        player: &mut dyn Playback,
    ) -> Result<AppEffect> {
        self.active_menu = None;
        match (menu, item_idx) {
            // Media
            (MenuKind::Media, 0) => self.open_file_picker(player)?,
            (MenuKind::Media, 1) => {
                self.input = Some(InputDialog {
                    purpose: InputPurpose::Media,
                    text: String::new(),
                })
            }
            (MenuKind::Media, 2) => {
                self.input = Some(InputDialog {
                    purpose: InputPurpose::Media,
                    text: String::new(),
                })
            }
            (MenuKind::Media, 3) => {
                self.input = Some(InputDialog {
                    purpose: InputPurpose::Media,
                    text: self.cwd.to_string_lossy().into_owned(),
                })
            }
            (MenuKind::Media, 4) => return Ok(AppEffect::Quit), // quit

            // Playback
            (MenuKind::Playback, 0) => {
                player.toggle_pause();
                self.last_osd = Instant::now();
            }
            (MenuKind::Playback, 1) => {
                player.stop();
                self.notify("Stopped");
            }
            (MenuKind::Playback, 2) => self.adjacent(player, -1),
            (MenuKind::Playback, 3) => self.adjacent(player, 1),
            (MenuKind::Playback, 4) => {
                player.set_rate((player.rate() + 0.25).min(4.0));
                self.notify(format!("Speed {:.2}x", player.rate()));
            }
            (MenuKind::Playback, 5) => {
                player.set_rate((player.rate() - 0.25).max(0.25));
                self.notify(format!("Speed {:.2}x", player.rate()));
            }
            (MenuKind::Playback, 6) => {
                player.set_rate(1.0);
                self.notify("Speed: 1.00x");
            }
            (MenuKind::Playback, 7) => {
                player.seek_relative(10_000);
                self.last_osd = Instant::now();
            }
            (MenuKind::Playback, 8) => {
                player.seek_relative(-10_000);
                self.last_osd = Instant::now();
            }
            (MenuKind::Playback, 9) => {
                player.next_frame();
                self.last_osd = Instant::now();
                self.notify("Next frame");
            }
            (MenuKind::Playback, 10) => {
                self.queue.cycle_repeat();
                self.notify(format!("Repeat: {:?}", self.queue.repeat));
            }
            (MenuKind::Playback, 11) => self.toggle_shuffle(),

            // Audio
            (MenuKind::Audio, 0) => self.cycle_audio(player),
            (MenuKind::Audio, 1) => self.open_tracks(Overlay::AudioTracks, player),
            (MenuKind::Audio, 2) => {
                self.volume = (self.volume + 5).min(125);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            (MenuKind::Audio, 3) => {
                self.volume = self.volume.saturating_sub(5).max(0);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            (MenuKind::Audio, 4) => {
                if self.volume > 0 {
                    self.saved_volume = self.volume;
                    self.volume = 0;
                } else {
                    self.volume = self.saved_volume.max(10);
                }
                player.set_volume(self.volume);
                self.notify(if self.volume == 0 {
                    "Muted".into()
                } else {
                    format!("Volume: {}%", self.volume)
                });
            }

            // Video
            (MenuKind::Video, 0) => self.toggle_fullscreen(),
            (MenuKind::Video, 1) => return Ok(AppEffect::ToggleRenderer),

            // Subtitle
            (MenuKind::Subtitle, 0) => self.cycle_subtitle(player),
            (MenuKind::Subtitle, 1) => self.open_tracks(Overlay::SubtitleTracks, player),
            (MenuKind::Subtitle, 2) => {
                self.input = Some(InputDialog {
                    purpose: InputPurpose::Subtitle,
                    text: String::new(),
                })
            }
            (MenuKind::Subtitle, 3) => {
                self.adjust_subtitle_delay(100, player);
            }
            (MenuKind::Subtitle, 4) => {
                self.adjust_subtitle_delay(-100, player);
            }

            // Tools
            (MenuKind::Tools, 0) => self.overlay = Overlay::Info,

            // View
            (MenuKind::View, 0) => self.toggle_playlist(),
            (MenuKind::View, 1) => self.refresh(),
            (MenuKind::View, 2) => {
                self.view_mode = ViewMode::Browser;
                self.selected = 0;
                self.refresh();
            }

            // Help
            (MenuKind::Help, 0) | (MenuKind::Help, 1) => self.overlay = Overlay::Help,
            _ => {}
        }
        Ok(AppEffect::Continue)
    }

    pub fn handle_key(&mut self, key: KeyEvent, player: &mut dyn Playback) -> Result<AppEffect> {
        if let Some(ref mut dialog) = self.input {
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => self.accept_input(player),
                KeyCode::Backspace => {
                    dialog.text.pop();
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && dialog.text.len() < 4096 =>
                {
                    dialog.text.push(c);
                }
                _ => {}
            }
            return Ok(AppEffect::Continue);
        }

        // Active VLC menu bar dropdown navigation
        if let Some(menu) = self.active_menu {
            let items = menu.items();
            match key.code {
                KeyCode::Esc => self.active_menu = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.menu_selected = self.menu_selected.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.menu_selected = (self.menu_selected + 1).min(items.len().saturating_sub(1))
                }
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
                KeyCode::Enter => {
                    return self.trigger_menu_action(menu, self.menu_selected, player)
                }
                _ => {}
            }
            return Ok(AppEffect::Continue);
        }

        if self.overlay == Overlay::SubtitleTracks || self.overlay == Overlay::AudioTracks {
            match key.code {
                KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('b') => {
                    self.overlay = Overlay::None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.track_selected = self.track_selected.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.track_selected =
                        (self.track_selected + 1).min(self.tracks.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    self.choose_track(player);
                    self.overlay = Overlay::None;
                }
                _ => {}
            }
            return Ok(AppEffect::Continue);
        }

        if self.overlay != Overlay::None {
            match key.code {
                KeyCode::Esc
                | KeyCode::Char('q')
                | KeyCode::Char('i')
                | KeyCode::Char('?')
                | KeyCode::F(1) => self.overlay = Overlay::None,
                _ => {}
            }
            return Ok(AppEffect::Continue);
        }

        // Ctrl combinations (standard VLC desktop shortcuts)
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('o') => {
                    self.open_file_picker(player)?;
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('n') | KeyCode::Char('u') => {
                    self.input = Some(InputDialog {
                        purpose: InputPurpose::Media,
                        text: String::new(),
                    });
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('d') => {
                    self.input = Some(InputDialog {
                        purpose: InputPurpose::Media,
                        text: self.cwd.to_string_lossy().into_owned(),
                    });
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('l') => {
                    self.toggle_playlist();
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('i') => {
                    self.overlay = Overlay::Info;
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('q') => return Ok(AppEffect::Quit),
                KeyCode::Up => {
                    self.volume = (self.volume + 5).min(125);
                    player.set_volume(self.volume);
                    self.notify(format!("Volume: {}%", self.volume));
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Down => {
                    self.volume = self.volume.saturating_sub(5).max(0);
                    player.set_volume(self.volume);
                    self.notify(format!("Volume: {}%", self.volume));
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Left => {
                    player.seek_relative(-60_000); // 1 minute backward
                    self.last_osd = Instant::now();
                    self.notify("Seek -1 min");
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Right => {
                    player.seek_relative(60_000); // 1 minute forward
                    self.last_osd = Instant::now();
                    self.notify("Seek +1 min");
                    return Ok(AppEffect::Continue);
                }
                _ => {}
            }
        }

        // Alt combinations (VLC long jump)
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Left => {
                    player.seek_relative(-300_000); // 5 minutes backward
                    self.last_osd = Instant::now();
                    self.notify("Seek -5 min");
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Right => {
                    player.seek_relative(300_000); // 5 minutes forward
                    self.last_osd = Instant::now();
                    self.notify("Seek +5 min");
                    return Ok(AppEffect::Continue);
                }
                _ => {}
            }
        }

        // Shift combinations (VLC very short jump 3s / dialogs)
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Left => {
                    player.seek_relative(-3_000); // 3 seconds backward
                    self.last_osd = Instant::now();
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Right => {
                    player.seek_relative(3_000); // 3 seconds forward
                    self.last_osd = Instant::now();
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('S') => {
                    self.open_tracks(Overlay::SubtitleTracks, player);
                    return Ok(AppEffect::Continue);
                }
                KeyCode::Char('A') => {
                    self.open_tracks(Overlay::AudioTracks, player);
                    return Ok(AppEffect::Continue);
                }
                _ => {}
            }
        }

        // Standard key bindings
        match key.code {
            KeyCode::Char('q') => return Ok(AppEffect::Quit),
            KeyCode::Esc => {
                if self.fullscreen {
                    self.toggle_fullscreen();
                }
            }
            KeyCode::F(11) | KeyCode::Char('f') => self.toggle_fullscreen(),
            KeyCode::Char(' ') => {
                player.toggle_pause();
                self.last_osd = Instant::now();
            }
            KeyCode::Char('s') | KeyCode::Char('x') => {
                player.stop();
                self.notify("Stopped");
            }
            KeyCode::Left => {
                player.seek_relative(-10_000);
                self.last_osd = Instant::now();
            }
            KeyCode::Right => {
                player.seek_relative(10_000);
                self.last_osd = Instant::now();
            }
            KeyCode::Char('+') => {
                self.volume = (self.volume + 5).min(125);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            KeyCode::Char('-') => {
                self.volume = self.volume.saturating_sub(5).max(0);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            KeyCode::Char('m') => {
                if self.volume > 0 {
                    self.saved_volume = self.volume;
                    self.volume = 0;
                } else {
                    self.volume = self.saved_volume.max(10);
                }
                player.set_volume(self.volume);
                self.notify(if self.volume == 0 {
                    "Muted".into()
                } else {
                    format!("Volume: {}%", self.volume)
                });
            }
            KeyCode::Char('[') => {
                player.set_rate((player.rate() - 0.25).max(0.25));
                self.notify(format!("Speed {:.2}x", player.rate()));
            }
            KeyCode::Char(']') => {
                player.set_rate((player.rate() + 0.25).min(4.0));
                self.notify(format!("Speed {:.2}x", player.rate()));
            }
            KeyCode::Char('=') => {
                player.set_rate(1.0);
                self.notify("Speed: 1.00x");
            }
            KeyCode::Char('e') => {
                player.next_frame();
                self.last_osd = Instant::now();
                self.notify("Next frame");
            }
            KeyCode::Char('t') => {
                let snap = player.snapshot();
                self.last_osd = Instant::now();
                self.notify(format!(
                    "Time: {} / {}",
                    time(snap.time_ms),
                    time(snap.length_ms)
                ));
            }
            KeyCode::Char('v') => self.cycle_subtitle(player),
            KeyCode::Char('b') => self.cycle_audio(player),
            KeyCode::Char('L') | KeyCode::Char('l') => {
                self.input = Some(InputDialog {
                    purpose: InputPurpose::Subtitle,
                    text: String::new(),
                })
            }
            KeyCode::Char('o') => self.open_file_picker(player)?,
            KeyCode::Char('?') | KeyCode::F(1) => self.overlay = Overlay::Help,
            KeyCode::Char('i') => self.overlay = Overlay::Info,
            KeyCode::Char(',') => {
                self.adjust_subtitle_delay(-100, player);
            }
            KeyCode::Char('g') => return Ok(AppEffect::ToggleRenderer),
            KeyCode::Char('.') | KeyCode::Char('h') => {
                self.adjust_subtitle_delay(100, player);
            }
            KeyCode::Char('j') if self.view_mode == ViewMode::Player => {
                self.adjust_audio_delay(-100, player);
            }
            KeyCode::Char('k') if self.view_mode == ViewMode::Player => {
                self.adjust_audio_delay(100, player);
            }
            KeyCode::Char('n') => self.adjacent(player, 1),
            KeyCode::Char('p') => self.adjacent(player, -1),
            KeyCode::Tab => self.toggle_playlist(),
            KeyCode::Up | KeyCode::Char('k')
                if self.view_mode != ViewMode::Player && !self.fullscreen =>
            {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.view_mode != ViewMode::Player && !self.fullscreen =>
            {
                self.selected = (self.selected + 1).min(self.list_len().saturating_sub(1));
            }
            KeyCode::Backspace if self.fullscreen => self.toggle_fullscreen(),
            KeyCode::Backspace if self.view_mode == ViewMode::Browser => self.parent(),
            KeyCode::Enter if self.view_mode != ViewMode::Player && !self.fullscreen => {
                self.enter(player)?
            }
            KeyCode::Char('z') => self.toggle_shuffle(),
            KeyCode::Char('R') => {
                self.queue.cycle_repeat();
                self.notify(format!("Repeat: {:?}", self.queue.repeat));
            }
            KeyCode::Char('r') => self.refresh(),
            _ => {}
        }
        Ok(AppEffect::Continue)
    }

    pub fn handle_mouse(
        &mut self,
        evt: MouseEvent,
        zones: &HitZones,
        player: &mut dyn Playback,
    ) -> Result<AppEffect> {
        let (x, y) = (evt.column, evt.row);
        if self.input.is_some() {
            return Ok(AppEffect::Continue);
        }

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
                    return Ok(AppEffect::Continue);
                }
            }
        }

        // 2. Click inside open dropdown menu
        if let Some((dropdown_box, ref items)) = zones.menu_dropdown {
            if matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(menu) = self.active_menu {
                    for (item_rect, idx) in items {
                        if hit(Some(*item_rect), x, y) {
                            return self.trigger_menu_action(menu, *idx, player);
                        }
                    }
                }
                // If clicked outside dropdown box, dismiss it
                if !hit(Some(dropdown_box), x, y) {
                    self.active_menu = None;
                    return Ok(AppEffect::Continue);
                }
                return Ok(AppEffect::Continue);
            }
        }

        // Close menu if clicked anywhere else
        if self.active_menu.is_some() && matches!(evt.kind, MouseEventKind::Down(MouseButton::Left))
        {
            self.active_menu = None;
            return Ok(AppEffect::Continue);
        }

        // 3. Overlay dialog clicks (Help, Info, Subtitle tracks, Audio tracks)
        if self.overlay != Overlay::None {
            if matches!(evt.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(box_rect) = zones.overlay_box {
                    if hit(Some(box_rect), x, y) {
                        // Clicked inside modal dialog: handle track item selection if applicable
                        if self.overlay == Overlay::SubtitleTracks
                            || self.overlay == Overlay::AudioTracks
                        {
                            if let Some((rect, first)) =
                                zones.track_list.filter(|(r, _)| hit(Some(*r), x, y))
                            {
                                let choice = first + usize::from(y.saturating_sub(rect.y));
                                if choice < self.tracks.len() {
                                    self.track_selected = choice;
                                    self.choose_track(player);
                                    self.overlay = Overlay::None;
                                }
                            }
                        }
                        return Ok(AppEffect::Continue);
                    } else {
                        // Clicked outside modal dialog: dismiss it
                        self.overlay = Overlay::None;
                        return Ok(AppEffect::Continue);
                    }
                }
            }
            if self.overlay == Overlay::SubtitleTracks || self.overlay == Overlay::AudioTracks {
                match evt.kind {
                    MouseEventKind::ScrollDown => {
                        self.track_selected =
                            (self.track_selected + 1).min(self.tracks.len().saturating_sub(1))
                    }
                    MouseEventKind::ScrollUp => {
                        self.track_selected = self.track_selected.saturating_sub(1)
                    }
                    _ => {}
                }
                return Ok(AppEffect::Continue);
            }
            return Ok(AppEffect::Continue);
        }

        match evt.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if (self.fullscreen && hit(zones.back, x, y)) || hit(zones.fullscreen, x, y) {
                    self.toggle_fullscreen();
                } else if hit(zones.play, x, y) {
                    player.toggle_pause();
                    self.last_osd = Instant::now();
                } else if hit(zones.stop, x, y) {
                    player.stop();
                    self.notify("Stopped");
                } else if hit(zones.prev, x, y) {
                    self.adjacent(player, -1);
                } else if hit(zones.next, x, y) {
                    self.adjacent(player, 1);
                } else if hit(zones.playlist_toggle, x, y) {
                    self.toggle_playlist();
                } else if hit(zones.subtitles, x, y) {
                    self.open_tracks(Overlay::SubtitleTracks, player);
                } else if hit(zones.audio, x, y) {
                    self.open_tracks(Overlay::AudioTracks, player);
                } else if let Some(vol_rect) = zones.volume.filter(|r| hit(Some(*r), x, y)) {
                    let frac = if vol_rect.width <= 1 {
                        1.0
                    } else {
                        (f64::from(x.saturating_sub(vol_rect.x))
                            / f64::from(vol_rect.width.saturating_sub(1)))
                        .clamp(0.0, 1.0)
                    };
                    let new_vol = (frac * 125.0).round() as i32;
                    self.volume = new_vol;
                    player.set_volume(new_vol);
                    self.notify(format!("Volume: {new_vol}%"));
                } else if let Some(progress) = zones.timeline.filter(|r| hit(Some(*r), x, y)) {
                    let snap = player.snapshot();
                    if snap.length_ms > 0 {
                        let fraction = f64::from(x.saturating_sub(progress.x))
                            / f64::from(progress.width.max(1));
                        player.seek_to((fraction * snap.length_ms as f64) as i64);
                        self.last_osd = Instant::now();
                    }
                } else if let Some((rect, start)) =
                    zones.browser.filter(|(r, _)| hit(Some(*r), x, y))
                {
                    let row = (y - rect.y) as usize;
                    let index = (start + row).min(self.list_len().saturating_sub(1));
                    let double = self.last_file_click.is_some_and(|(at, prev)| {
                        prev == index && at.elapsed() < Duration::from_millis(450)
                    });
                    self.last_file_click = Some((Instant::now(), index));
                    self.selected = index;
                    if double {
                        self.enter(player)?;
                        self.last_file_click = None;
                    }
                } else if hit(zones.video, x, y) || self.fullscreen {
                    let now = Instant::now();
                    let is_double = self
                        .last_video_click
                        .is_some_and(|at| at.elapsed() < Duration::from_millis(400));
                    self.last_video_click = Some(now);
                    if is_double {
                        self.toggle_fullscreen();
                        self.last_video_click = None;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(rect) = zones.timeline.filter(|r| hit(Some(*r), x, y)) {
                    let snap = player.snapshot();
                    if snap.length_ms > 0 {
                        let fraction =
                            f64::from(x.saturating_sub(rect.x)) / f64::from(rect.width.max(1));
                        player.seek_to((fraction * snap.length_ms as f64) as i64);
                    }
                } else if let Some(vol_rect) = zones.volume.filter(|r| hit(Some(*r), x, y)) {
                    let frac = if vol_rect.width <= 1 {
                        1.0
                    } else {
                        (f64::from(x.saturating_sub(vol_rect.x))
                            / f64::from(vol_rect.width.saturating_sub(1)))
                        .clamp(0.0, 1.0)
                    };
                    let new_vol = (frac * 125.0).round() as i32;
                    self.volume = new_vol;
                    player.set_volume(new_vol);
                }
            }
            MouseEventKind::ScrollDown
                if self.view_mode != ViewMode::Player && !self.fullscreen =>
            {
                self.selected = (self.selected + 3).min(self.list_len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp if self.view_mode != ViewMode::Player && !self.fullscreen => {
                self.selected = self.selected.saturating_sub(3);
            }
            MouseEventKind::ScrollUp => {
                self.volume = (self.volume + 1).min(125);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            MouseEventKind::ScrollDown => {
                self.volume = self.volume.saturating_sub(1).max(0);
                player.set_volume(self.volume);
                self.notify(format!("Volume: {}%", self.volume));
            }
            _ => {}
        }
        Ok(AppEffect::Continue)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn repeated_playlist_entries_advance_by_occurrence() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        let first = "https://fixture/first.mp4";
        let last = "https://fixture/last.mp4";
        app.open_media(first, &mut player).unwrap();
        app.queue
            .replace(vec![first.into(), first.into(), last.into()]);
        app.adjacent(&mut player, 1);
        assert_eq!(app.current.as_deref(), Some(first));
        app.adjacent(&mut player, 1);
        assert_eq!(app.current.as_deref(), Some(last));
        app.adjacent(&mut player, 1);
        assert_eq!(
            player.opened.len(),
            3,
            "Repeat off must stop after last occurrence"
        );
    }

    use super::*;
    use crate::playback::test_support::FakePlayback;

    #[test]
    fn streamed_tags_update_while_playback_state_stays_playing() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        app.open_media("http://fixture/Inn%20Lamhon.mp3?source=pi", &mut player)
            .unwrap();
        player.metadata.title = Some("Inn%20Lamhon.mp3".into());
        app.tick(&mut player);
        assert_eq!(app.audio_title(), "Inn Lamhon.mp3");
        player.metadata = crate::metadata::MediaMetadata {
            title: Some("Inn Lamhon Ke Daaman Mein".into()),
            artist: Some("A.R. Rahman".into()),
            album: Some("Jodhaa Akbar".into()),
            ..Default::default()
        };
        app.metadata_poll_at = Instant::now() - Duration::from_secs(1);
        app.tick(&mut player);
        assert_eq!(app.audio_title(), "Inn Lamhon Ke Daaman Mein");
        assert_eq!(
            app.audio_metadata.as_ref().unwrap().artist.as_deref(),
            Some("A.R. Rahman")
        );
        assert_eq!(
            app.audio_metadata.as_ref().unwrap().album.as_deref(),
            Some("Jodhaa Akbar")
        );
        assert_eq!(app.last_state, 3);
        app.open_media("http://fixture/second.mp3", &mut player)
            .unwrap();
        assert_eq!(app.audio_title(), "second.mp3");
        assert!(app.audio_thumbnail().is_none());
    }
    #[test]
    fn reopening_same_source_invalidates_presentation_and_remote_open_stays_in_terminal() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        app.open_media("http://fixture/movie.mp4", &mut player)
            .unwrap();
        let revision = app.media_revision;
        app.open_media("http://fixture/movie.mp4", &mut player)
            .unwrap();
        assert_ne!(app.media_revision, revision);
        app.native_dialogs = false;
        app.open_file_picker(&mut player).unwrap();
        assert!(app.input.is_some());
    }
    #[test]
    fn natural_queue_advance_preserves_help_and_browser_context() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        app.open_media("http://fixture/movie.mp4", &mut player)
            .unwrap();
        app.queue.repeat = crate::queue::RepeatMode::All;
        app.overlay = Overlay::Help;
        app.view_mode = ViewMode::Browser;
        player.state = 6;
        app.tick(&mut player);
        assert_eq!(player.opened.len(), 2);
        assert_eq!(app.overlay, Overlay::Help);
        assert_eq!(app.view_mode, ViewMode::Browser);
    }

    #[test]
    fn queue_navigation_is_independent_of_browser_and_advances_once_at_end() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        let sources: Vec<String> =
            vec!["http://fixture/a.mp4".into(), "http://fixture/b.mp4".into()];
        app.queue.select(&sources[0], sources.clone());
        app.cwd = PathBuf::from("/unrelated-browser-folder");
        app.entries.clear();
        app.adjacent(&mut player, 1);
        assert_eq!(player.opened, [sources[1].clone()]);
        app.adjacent(&mut player, -1);
        assert_eq!(app.current.as_deref(), Some(sources[0].as_str()));
        player.state = 6;
        app.tick(&mut player);
        assert_eq!(app.current.as_deref(), Some(sources[1].as_str()));
        app.tick(&mut player);
        assert_eq!(player.opened.len(), 3);
        player.state = 5;
        app.tick(&mut player);
        assert_eq!(player.opened.len(), 3, "Stop must not advance playback");
        player.state = 6;
        app.tick(&mut player);
        assert_eq!(player.opened.len(), 3, "End must not wrap with repeat off");
    }

    #[test]
    fn backend_failures_leave_selection_and_sync_values_unchanged() {
        let mut app = App::new(None);
        app.queue.select("http://fixture/a.mp4", vec![]);
        app.current = Some("http://fixture/a.mp4".into());
        let mut player = FakePlayback {
            reject_open: true,
            reject_tracks: true,
            reject_delays: true,
            ..Default::default()
        };
        assert!(app.open_media("http://fixture/b.mp4", &mut player).is_err());
        assert_eq!(app.current.as_deref(), Some("http://fixture/a.mp4"));
        assert_eq!(app.queue.sources(), &["http://fixture/a.mp4"]);
        app.adjust_audio_delay(100, &mut player);
        app.adjust_subtitle_delay(100, &mut player);
        assert_eq!((app.audio_delay_ms, app.subtitle_delay_ms), (0, 0));
        app.tracks = vec![TrackOption {
            label: "Track 2".into(),
            action: TrackAction::SubtitleEmbedded(2),
        }];
        app.choose_track(&mut player);
        assert_eq!(app.selected_subtitle, "Off");
        assert!(app.status.starts_with("Couldn't activate"));
    }

    #[test]
    fn error_state_is_reported_once_without_auto_advancing() {
        let mut app = App::new(None);
        let mut player = FakePlayback {
            state: 7,
            ..Default::default()
        };
        app.tick(&mut player);
        assert!(app.status.contains("Playback failed"));
        let notified_at = app.status_at;
        app.tick(&mut player);
        assert_eq!(app.status_at, notified_at);
        assert!(player.opened.is_empty());
    }

    #[test]
    fn rapid_shortcuts_reach_the_same_event_dispatcher_as_terminal_input() {
        use ratatui::crossterm::event::Event;
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        let mut effects = Vec::new();
        for code in [KeyCode::Char('f'), KeyCode::Char('g'), KeyCode::Char('-')] {
            effects.push(
                app.handle_event(
                    Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
                    &HitZones::default(),
                    &mut player,
                )
                .unwrap(),
            );
        }
        assert!(app.fullscreen);
        assert_eq!(
            effects,
            [
                AppEffect::Continue,
                AppEffect::ToggleRenderer,
                AppEffect::Continue
            ]
        );
        assert_eq!(player.volume, 95);
    }

    #[test]
    fn volume_decrement_controls_clamp_at_zero() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        app.volume = 0;
        app.handle_key(
            KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE),
            &mut player,
        )
        .unwrap();
        assert_eq!((app.volume, player.volume), (0, 0));
        app.trigger_menu_action(MenuKind::Audio, 3, &mut player)
            .unwrap();
        assert_eq!((app.volume, player.volume), (0, 0));
        app.view_mode = ViewMode::Player;
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            &HitZones::default(),
            &mut player,
        )
        .unwrap();
        assert_eq!((app.volume, player.volume), (0, 0));
    }

    #[test]
    fn subtitle_cycle_returns_to_off_after_local_sidecar() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        app.load_subtitle(Path::new("samples/sample.srt"), &mut player)
            .unwrap();
        app.cycle_subtitle(&mut player);
        assert!(!app.sidecar_enabled);
        assert_eq!(app.selected_subtitle, "Off");
        app.cycle_subtitle(&mut player);
        assert!(app.sidecar_enabled);
        assert_eq!(player.subtitle, -1);
    }

    #[test]
    fn mouse_menu_propagates_quit_and_renderer_effects() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        let rect = Rect::new(0, 1, 35, 1);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        for (menu, index, expected) in [
            (MenuKind::Media, 4, AppEffect::Quit),
            (MenuKind::Video, 1, AppEffect::ToggleRenderer),
        ] {
            app.active_menu = Some(menu);
            let zones = HitZones {
                menu_dropdown: Some((rect, vec![(rect, index)])),
                ..Default::default()
            };
            assert_eq!(
                app.handle_mouse(click, &zones, &mut player).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn sync_and_frame_controls_reach_playback() {
        let mut app = App::new(None);
        app.view_mode = ViewMode::Player;
        let mut player = FakePlayback::default();
        app.handle_key(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            &mut player,
        )
        .unwrap();
        assert_eq!(player.audio_delay, 100);
        app.handle_key(
            KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
            &mut player,
        )
        .unwrap();
        assert_eq!(player.subtitle_delay, 100);
        app.handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            &mut player,
        )
        .unwrap();
        assert_eq!(player.frame_steps, 1);
        assert!(player.seeks.is_empty());
    }

    #[test]
    fn video_double_click_preserves_playback_state() {
        let mut app = App::new(None);
        let mut player = FakePlayback::default();
        let zones = HitZones {
            video: Some(Rect::new(0, 0, 80, 24)),
            ..Default::default()
        };
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 20,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(click, &zones, &mut player).unwrap();
        app.handle_mouse(click, &zones, &mut player).unwrap();
        assert!(app.fullscreen);
        assert_eq!(player.state, 3);
        assert_eq!(player.paused, 0);
    }
    #[test]
    fn extension_filter() {
        assert!(supported(Path::new("movie.MKV")));
        assert!(supported(Path::new("sound.flac")));
        assert!(!supported(Path::new("notes.txt")));
    }
    #[test]
    fn empty_browser_never_underflows() {
        let a = App::new(Some(Path::new("/nonexistent/termvlc")));
        assert!(a.entries.is_empty());
    }
    #[test]
    fn fullscreen_is_reversible() {
        let mut app = App::new(Some(Path::new(".")));
        app.toggle_fullscreen();
        assert!(app.fullscreen);
        app.toggle_fullscreen();
        assert!(!app.fullscreen);
    }
    #[test]
    fn hit_test_border() {
        let r = Rect::new(3, 4, 10, 2);
        assert!(hit(Some(r), 3, 4));
        assert!(!hit(Some(r), 13, 4));
    }
}
