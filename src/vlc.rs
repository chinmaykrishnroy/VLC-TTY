//! Native LibVLC 3.x adapter.
//!
//! No VLC header/build-time linker dependency. libloading resolves the shared
//! library at runtime. LibVLC still requires its normal codecs/plugins on disk.
//! Video format negotiation uses RV32 with 32-byte aligned, per-picture buffers.
//! Frames default to the source resolution. To bound CPU work,
//! set TERMVLC_DECODE_MAX=1080; zero requests uncapped dimensions.
//! Display callbacks publish only the *latest presented* frame; no terminal
//! writes occur on decoder threads.

use std::{alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout},
    env, ffi::{c_char, c_int, c_void, CStr, CString},
    path::{Path, PathBuf}, ptr, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

use anyhow::{anyhow, bail, Context, Result};
use libloading::Library;

#[derive(Debug, Clone)]
pub struct VideoFrame { pub width: usize, pub height: usize, pub pitch: usize, pub bgra: Vec<u8>, pub serial: u64 }
#[derive(Default)]
struct Size { width: usize, height: usize, pitch: usize }
#[derive(Default)]
struct FrameStore { size: Mutex<Size>, latest: Mutex<Option<Arc<VideoFrame>>>, presented: AtomicU64 }
struct CallbackContext { frames: Arc<FrameStore> }

struct Picture {
    data: *mut u8,
    layout: Layout,
    width: usize,
    height: usize,
    pitch: usize,
}
impl Drop for Picture { fn drop(&mut self) { unsafe { dealloc(self.data, self.layout) } } }

type LockFn = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void;
type UnlockFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *const *mut c_void);
type DisplayFn = unsafe extern "C" fn(*mut c_void, *mut c_void);
type FormatFn = unsafe extern "C" fn(*mut *mut c_void, *mut c_char, *mut u32, *mut u32, *mut u32, *mut u32) -> u32;
type CleanupFn = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" fn configure_video(opaque: *mut *mut c_void, chroma: *mut c_char, width: *mut u32, height: *mut u32, pitches: *mut u32, lines: *mut u32) -> u32 {
    if opaque.is_null() || chroma.is_null() || width.is_null() || height.is_null() || pitches.is_null() || lines.is_null() { return 0; }
    let ctx = &*(*opaque as *const CallbackContext);
    let in_w = (*width).max(1) as f64;
    let in_h = (*height).max(1) as f64;
    // Decode at the original native media resolution decided directly by the media file.
    // Allow TERMVLC_DECODE_MAX to optionally cap (e.g. 1080) if explicitly requested.
    let requested_max = env::var("TERMVLC_DECODE_MAX").ok()
        .and_then(|s| s.parse::<u32>().ok()).unwrap_or(0); // 0 = uncapped native media dimensions
    let scale = if requested_max == 0 { 1.0 } else {
        (requested_max as f64 * (16.0 / 9.0) / in_w)
            .min(requested_max as f64 / in_h).min(1.0)
    };
    let w = ((in_w * scale).round() as u32).max(1);
    let h = ((in_h * scale).round() as u32).max(1);
    if w > 16_384 || h > 16_384 { return 0; }
    let Some(pitch) = w.checked_mul(4).and_then(|n| n.checked_add(31)).map(|n| n & !31) else { return 0; };
    let chroma_bytes = b"RV32";
    ptr::copy_nonoverlapping(chroma_bytes.as_ptr() as *const c_char, chroma, 4);
    *width = w; *height = h; *pitches = pitch; *lines = h;
    let mut s = ctx.frames.size.lock().unwrap_or_else(|e| e.into_inner());
    *s = Size { width: w as usize, height: h as usize, pitch: pitch as usize };
    1  // One buffer is supplied by each lock callback.
}

unsafe extern "C" fn lock_video(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void {
    if opaque.is_null() || planes.is_null() { return ptr::null_mut(); }
    let ctx = &*(opaque as *const CallbackContext);
    let s = ctx.frames.size.lock().unwrap_or_else(|e| e.into_inner());
    let (width, height, pitch) = (s.width, s.height, s.pitch);
    if width == 0 || height == 0 || pitch == 0 { return ptr::null_mut(); }
    let Some(bytes) = pitch.checked_mul(height) else { return ptr::null_mut(); };
    let Ok(layout) = Layout::from_size_align(bytes, 32) else { return ptr::null_mut(); };
    let data = alloc_zeroed(layout);
    if data.is_null() { handle_alloc_error(layout) }
    *planes = data.cast();
    Box::into_raw(Box::new(Picture { data, layout, width, height, pitch })).cast()
}

unsafe extern "C" fn unlock_video(_opaque: *mut c_void, _picture: *mut c_void, _planes: *const *mut c_void) {
    // LibVLC still owns the completed picture until it invokes display_video.
}

unsafe extern "C" fn display_video(opaque: *mut c_void, picture: *mut c_void) {
    if picture.is_null() { return; }
    // Every successful lock returns an independent picture. We reclaim it
    // after LibVLC presents it, and never expose decoder-owned memory to UI.
    let pic = Box::from_raw(picture as *mut Picture);
    if opaque.is_null() { return; }
    let ctx = &*(opaque as *const CallbackContext);
    let data = std::slice::from_raw_parts(pic.data, pic.layout.size()).to_vec();
    let serial = ctx.frames.presented.fetch_add(1, Ordering::Relaxed) + 1;
    let frame = Arc::new(VideoFrame { width: pic.width, height: pic.height, pitch: pic.pitch, bgra: data, serial });
    *ctx.frames.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
}

#[repr(C)]
struct TrackDescription {
    id: c_int,
    name: *const c_char,
    next: *mut TrackDescription,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaTrack {
    pub id: i32,
    pub name: String,
}

// Simple C signatures from the LibVLC 3.x public ABI.
struct Api {
    _library: Library,
    loaded_from: PathBuf,
    version: String,
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> *mut c_void,
    release: unsafe extern "C" fn(*mut c_void),
    media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    media_new_location: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    media_release: unsafe extern "C" fn(*mut c_void),
    player_new: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    player_release: unsafe extern "C" fn(*mut c_void),
    player_set_media: unsafe extern "C" fn(*mut c_void, *mut c_void),
    player_play: unsafe extern "C" fn(*mut c_void) -> c_int,
    player_set_pause: unsafe extern "C" fn(*mut c_void, c_int),
    player_next_frame: unsafe extern "C" fn(*mut c_void),
    player_stop: unsafe extern "C" fn(*mut c_void),
    player_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    audio_get_volume: unsafe extern "C" fn(*mut c_void) -> c_int,
    audio_set_volume: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    audio_set_delay: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    #[cfg(test)]
    audio_get_delay: unsafe extern "C" fn(*mut c_void) -> i64,
    player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    #[allow(dead_code)]
    video_get_spu: unsafe extern "C" fn(*mut c_void) -> c_int,
    video_set_spu: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    video_set_spu_delay: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    #[cfg(test)]
    video_get_spu_delay: unsafe extern "C" fn(*mut c_void) -> i64,
    video_get_spu_description: unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription,
    video_set_subtitle_file: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    #[allow(dead_code)]
    audio_get_track: unsafe extern "C" fn(*mut c_void) -> c_int,
    audio_set_track: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    audio_get_track_description: unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription,
    track_description_list_release: unsafe extern "C" fn(*mut TrackDescription),
    video_set_callbacks: unsafe extern "C" fn(*mut c_void, Option<LockFn>, Option<UnlockFn>, Option<DisplayFn>, *mut c_void),
    video_set_format_callbacks: unsafe extern "C" fn(*mut c_void, Option<FormatFn>, Option<CleanupFn>),
    errmsg: unsafe extern "C" fn() -> *const c_char,
}

impl Api {
    fn load() -> Result<Self> {
        let mut failures = Vec::new();
        for name in libvlc_candidates() {
            // Loading libvlc.dll also needs libvlccore.dll and the VLC plugin
            // directory; normal VLC installation doesn't modify PATH on Windows.
            #[cfg(windows)]
            if let Some(folder) = name.parent().filter(|dir| dir.is_dir()) {
                let existing = env::var_os("PATH").unwrap_or_default();
                let joined = env::join_paths(std::iter::once(folder.to_path_buf()).chain(env::split_paths(&existing)))
                    .unwrap_or(existing);
                env::set_var("PATH", joined);
            }
            match unsafe { Library::new(&name) } {
                Ok(lib) => match unsafe { Self::symbols(lib, name.clone()) } {
                    Ok(api) => return Ok(api),
                    Err(e) => failures.push(format!("{}: incompatible library: {e:#}", name.display())),
                },
                Err(e) => failures.push(format!("{}: {}", name.display(), e)),
            }
        }
        bail!("LibVLC 3.x shared library not found. Install VLC for your OS and matching CPU architecture.\nTried:\n{}", failures.join("\n"))
    }

    /// LibVLC's codec and access modules live outside libvlc itself.
    /// On Linux system packages know their plugin directories already, so avoid
    /// globally overriding them. App bundles on macOS and installers on Windows
    /// sometimes need a relative plugin directory explicitly provided.
    fn configure_plugin_path(&self) {
        if env::var_os("VLC_PLUGIN_PATH").is_some() { return; }
        #[cfg(any(windows, target_os = "macos"))]
        for candidate in plugin_candidates(&self.loaded_from) {
            if candidate.is_dir() {
                env::set_var("VLC_PLUGIN_PATH", candidate);
                break;
            }
        }
    }

    unsafe fn symbols(lib: Library, loaded_from: PathBuf) -> Result<Self> {
        let get_version = lib.get::<unsafe extern "C" fn() -> *const c_char>(b"libvlc_get_version\0")?;
        let version_ptr = get_version();
        if version_ptr.is_null() { bail!("LibVLC returned an empty version"); }
        let version = CStr::from_ptr(version_ptr).to_string_lossy().into_owned();
        if !version.starts_with("3.") { bail!("LibVLC 3.x is required; found {version}"); }
        macro_rules! f { ($s:literal) => { *lib.get(concat!($s, "\0").as_bytes()).with_context(|| format!("missing VLC symbol {}", $s))? } }
        Ok(Self {
            new: f!("libvlc_new"), release: f!("libvlc_release"),
            media_new_path: f!("libvlc_media_new_path"), media_new_location: f!("libvlc_media_new_location"),
            media_release: f!("libvlc_media_release"),
            player_new: f!("libvlc_media_player_new"), player_release: f!("libvlc_media_player_release"),
            player_set_media: f!("libvlc_media_player_set_media"),
            player_play: f!("libvlc_media_player_play"), player_set_pause: f!("libvlc_media_player_set_pause"),
            player_next_frame: f!("libvlc_media_player_next_frame"),
            player_stop: f!("libvlc_media_player_stop"),
            player_get_state: f!("libvlc_media_player_get_state"),
            player_get_time: f!("libvlc_media_player_get_time"), player_get_length: f!("libvlc_media_player_get_length"),
            player_set_time: f!("libvlc_media_player_set_time"),
            audio_get_volume: f!("libvlc_audio_get_volume"), audio_set_volume: f!("libvlc_audio_set_volume"),
            audio_set_delay: f!("libvlc_audio_set_delay"),
            #[cfg(test)]
            audio_get_delay: f!("libvlc_audio_get_delay"),
            player_set_rate: f!("libvlc_media_player_set_rate"),
            video_get_spu: f!("libvlc_video_get_spu"),
            video_set_spu: f!("libvlc_video_set_spu"),
            video_set_spu_delay: f!("libvlc_video_set_spu_delay"),
            #[cfg(test)]
            video_get_spu_delay: f!("libvlc_video_get_spu_delay"),
            video_get_spu_description: f!("libvlc_video_get_spu_description"),
            video_set_subtitle_file: f!("libvlc_video_set_subtitle_file"),
            audio_get_track: f!("libvlc_audio_get_track"),
            audio_set_track: f!("libvlc_audio_set_track"),
            audio_get_track_description: f!("libvlc_audio_get_track_description"),
            track_description_list_release: f!("libvlc_track_description_list_release"),
            video_set_callbacks: f!("libvlc_video_set_callbacks"),
            video_set_format_callbacks: f!("libvlc_video_set_format_callbacks"),
            errmsg: f!("libvlc_errmsg"),
            _library: lib,
            loaded_from,
            version,
        })
    }
}

fn library_name() -> &'static str {
    #[cfg(windows)] { "libvlc.dll" }
    #[cfg(target_os = "linux")] { "libvlc.so.5" }
    #[cfg(target_os = "macos")] { "libvlc.dylib" }
}

/// Runtime loading avoids a compile-time libvlc link requirement, so native
/// Windows, Linux and macOS builds all use the same Cargo sources.
fn libvlc_candidates() -> Vec<PathBuf> {
    let mut names = Vec::new();
    if let Some(home) = env::var_os("VLC_HOME") {
        let dir = PathBuf::from(home);
        if dir.is_file() { names.push(dir.clone()); }
        names.push(dir.join(library_name()));
        names.push(dir.join("lib").join(library_name()));
        #[cfg(target_os = "macos")]
        {
            names.push(dir.join("Contents/MacOS/lib/libvlc.dylib"));
            names.push(dir.join("Contents/MacOS/libvlc.dylib"));
            names.push(dir.join("VLC.app/Contents/MacOS/lib/libvlc.dylib"));
        }
    }
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = env::var_os(var) {
                names.push(PathBuf::from(base).join("VideoLAN/VLC/libvlc.dll"));
            }
        }
        names.push(PathBuf::from("libvlc.dll"));
    }
    #[cfg(target_os = "linux")]
    {
        names.extend(["libvlc.so.5", "libvlc.so"].map(PathBuf::from));
    }
    #[cfg(target_os = "macos")]
    {
        names.extend([
            "/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib",
            "/Applications/VLC.app/Contents/MacOS/libvlc.dylib",
            "/opt/homebrew/lib/libvlc.dylib",   // Apple Silicon Homebrew
            "/usr/local/lib/libvlc.dylib",      // Intel Homebrew
            "libvlc.dylib",
        ].map(PathBuf::from));
    }
    names.dedup();
    names
}

#[cfg(any(windows, target_os = "macos"))]
fn plugin_candidates(library: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = library.parent() {
        dirs.push(parent.join("plugins"));
        if let Some(parent2) = parent.parent() {
            dirs.push(parent2.join("plugins"));
        }
    }
    if let Some(home) = env::var_os("VLC_HOME") {
        let dir = PathBuf::from(home);
        dirs.push(dir.join("plugins"));
        #[cfg(target_os = "macos")]
        {
            dirs.push(dir.join("Contents/MacOS/plugins"));
            dirs.push(dir.join("Contents/plugins"));
        }
    }
    dirs
}

#[derive(Clone, Debug)]
pub struct PlaybackSnapshot {
    pub time_ms: i64, pub length_ms: i64, pub volume: i32,
    pub state: i32, pub rate: f32,
    #[allow(dead_code)]
    pub frames: u64,
}

pub struct VlcPlayer {
    api: Api,
    instance: *mut c_void,
    player: *mut c_void,
    context: Box<CallbackContext>,
    rate: f32,
    native_window: bool,
}

fn clean_path_for_vlc(path: &Path) -> Result<String> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut s = canonical.to_string_lossy().into_owned();
    if s.starts_with(r"\\?\UNC\") {
        s = format!(r"\\{}", &s[8..]);
    } else if s.starts_with(r"\\?\") {
        s = s[4..].to_string();
    }
    Ok(s)
}

impl VlcPlayer {
    pub fn new() -> Result<Self> { Self::new_with_window(false) }

    pub fn library_path(&self) -> &Path { &self.api.loaded_from }
    pub fn library_version(&self) -> &str { &self.api.version }
    pub fn is_native_window(&self) -> bool { self.native_window }

    pub fn new_with_window(native_window: bool) -> Result<Self> {
        Self::new_with_options(native_window, &[])
    }

    fn new_with_options(native_window: bool, options: &[&str]) -> Result<Self> {
        let api = Api::load()?;
        api.configure_plugin_path();
        let args: Vec<CString> = ["--quiet", "--no-video-title-show"].into_iter().chain(options.iter().copied())
            .map(CString::new).collect::<std::result::Result<_, _>>()?;
        let ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        let instance = unsafe { (api.new)(ptrs.len() as c_int, ptrs.as_ptr()) };
        if instance.is_null() { bail!("libvlc_new failed; check VLC plugins and installation") }
        let player = unsafe { (api.player_new)(instance) };
        if player.is_null() {
            unsafe { (api.release)(instance) };
            bail!("libvlc_media_player_new failed")
        }
        let mut context = Box::new(CallbackContext { frames: Arc::new(FrameStore::default()) });
        if !native_window {
            unsafe {
                (api.video_set_callbacks)(player, Some(lock_video), Some(unlock_video), Some(display_video), (&mut *context as *mut CallbackContext).cast());
                (api.video_set_format_callbacks)(player, Some(configure_video), None);
            }
        }
        Ok(Self { api, instance, player, context, rate: 1.0, native_window })
    }

    fn error_message(&self) -> String {
        unsafe {
            let p = (self.api.errmsg)();
            if p.is_null() { "unknown LibVLC error".into() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    }

    pub fn open(&mut self, locator: &str) -> Result<()> {
        let is_url = locator.contains("://");
        let text = if is_url {
            locator.to_owned()
        } else {
            clean_path_for_vlc(Path::new(locator))?
        };
        let c = CString::new(text.clone()).map_err(|_| anyhow!("path contains a NUL byte"))?;
        unsafe {
            (self.api.player_stop)(self.player);
            *self.context.frames.latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let media = if is_url { (self.api.media_new_location)(self.instance, c.as_ptr()) } else { (self.api.media_new_path)(self.instance, c.as_ptr()) };
            if media.is_null() { bail!("failed to open {text}: {}", self.error_message()); }
            (self.api.player_set_media)(self.player, media);
            (self.api.media_release)(media);
            if (self.api.player_play)(self.player) != 0 { bail!("could not play {text}: {}", self.error_message()); }
        }
        Ok(())
    }

    pub fn toggle_pause(&mut self) {
        unsafe {
            let s = (self.api.player_get_state)(self.player);
            if s == 3 { (self.api.player_set_pause)(self.player, 1); }
            else if s == 4 { (self.api.player_set_pause)(self.player, 0); }
            else { (self.api.player_play)(self.player); }
        }
    }
    pub fn seek_relative(&mut self, delta_ms: i64) {
        unsafe {
            let current = (self.api.player_get_time)(self.player);
            let length = (self.api.player_get_length)(self.player);
            let target = current.max(0).saturating_add(delta_ms).max(0);
            (self.api.player_set_time)(self.player, if length > 0 { target.min(length) } else { target });
        }
    }
    pub fn set_volume(&mut self, volume: i32) { unsafe { (self.api.audio_set_volume)(self.player, volume.clamp(0, 125)); } }
    pub fn next_frame(&mut self) {
        unsafe {
            (self.api.player_set_pause)(self.player, 1);
            (self.api.player_next_frame)(self.player);
        }
    }
    pub fn set_audio_delay(&mut self, delay_ms: i64) -> bool {
        unsafe { (self.api.audio_set_delay)(self.player, delay_ms.saturating_mul(1000)) == 0 }
    }
    pub fn set_subtitle_delay(&mut self, delay_ms: i64) -> bool {
        unsafe { (self.api.video_set_spu_delay)(self.player, delay_ms.saturating_mul(1000)) == 0 }
    }
    pub fn rate(&self) -> f32 { self.rate }
    pub fn set_rate(&mut self, value: f32) {
        if unsafe { (self.api.player_set_rate)(self.player, value) } == 0 { self.rate = value; }
    }
    /// The LibVLC 3.x linked-list is owned by LibVLC; copy strings and release it promptly.
    unsafe fn track_descriptions(
        &self,
        getter: unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription,
    ) -> Vec<MediaTrack> {
        let head = getter(self.player);
        let mut cursor = head;
        let mut tracks = Vec::new();
        for _ in 0..256 {
            if cursor.is_null() { break; }
            let item = &*cursor;
            let name = if item.name.is_null() { String::from("Unnamed track") }
                else { CStr::from_ptr(item.name).to_string_lossy().into_owned() };
            tracks.push(MediaTrack { id: item.id, name });
            cursor = item.next;
        }
        if !head.is_null() { (self.api.track_description_list_release)(head); }
        tracks
    }
    pub fn subtitle_tracks(&self) -> Vec<MediaTrack> {
        unsafe { self.track_descriptions(self.api.video_get_spu_description) }
    }
    #[allow(dead_code)]
    pub fn current_subtitle_id(&self) -> i32 {
        unsafe { (self.api.video_get_spu)(self.player) }
    }
    pub fn select_subtitle(&mut self, id: i32) -> bool {
        unsafe { (self.api.video_set_spu)(self.player, id) == 0 }
    }
    pub fn audio_tracks(&self) -> Vec<MediaTrack> {
        unsafe { self.track_descriptions(self.api.audio_get_track_description) }
    }
    #[allow(dead_code)]
    pub fn current_audio_id(&self) -> i32 {
        unsafe { (self.api.audio_get_track)(self.player) }
    }
    pub fn select_audio(&mut self, id: i32) -> bool {
        unsafe { (self.api.audio_set_track)(self.player, id) == 0 }
    }
    pub fn load_vlc_subtitle(&mut self, path: &Path) -> Result<()> {
        let text = clean_path_for_vlc(path)?;
        let c = CString::new(text.as_bytes())?;
        if unsafe { (self.api.video_set_subtitle_file)(self.player, c.as_ptr()) } == 0 {
            bail!("VLC could not load this subtitle (or media isn't ready yet): {}", path.display());
        }
        Ok(())
    }
    pub fn seek_to(&mut self, ms: i64) {
        let length = unsafe { (self.api.player_get_length)(self.player) };
        let value = if length > 0 { ms.clamp(0, length) } else { ms.max(0) };
        unsafe { (self.api.player_set_time)(self.player, value); }
    }
    pub fn stop(&mut self) {
        unsafe { (self.api.player_stop)(self.player); }
        *self.context.frames.latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn latest_frame(&self) -> Option<Arc<VideoFrame>> {
        self.context.frames.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        unsafe { PlaybackSnapshot {
            time_ms: (self.api.player_get_time)(self.player), length_ms: (self.api.player_get_length)(self.player),
            volume: (self.api.audio_get_volume)(self.player), state: (self.api.player_get_state)(self.player),
            rate: self.rate, frames: self.context.frames.presented.load(Ordering::Relaxed),
        } }
    }
}

impl Drop for VlcPlayer {
    fn drop(&mut self) {
        unsafe { (self.api.player_stop)(self.player); (self.api.player_release)(self.player); (self.api.release)(self.instance); }
        // `context` and then the dynamic library are dropped afterward.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn candidates_include_platform_shared_library() {
        let candidates = libvlc_candidates();
        assert!(candidates.iter().any(|p| p.to_string_lossy().contains(library_name())));
        #[cfg(target_os = "macos")]
        assert!(candidates.iter().any(|p| p.to_string_lossy().contains("VLC.app")));
        #[cfg(windows)]
        assert!(candidates.iter().any(|p| p.to_string_lossy().contains("libvlc.dll")));
    }
    #[test] fn size_defaults_to_zero() { let x = Size::default(); assert_eq!(x.width, 0); }
    #[test] fn layout_is_32_byte_aligned() { let l = Layout::from_size_align(320 * 4 * 240, 32).unwrap(); assert_eq!(l.align(), 32); }
    #[test] fn video_callbacks_publish_an_owned_frame() {
        let store = Arc::new(FrameStore::default());
        let mut ctx = Box::new(CallbackContext { frames: Arc::clone(&store) });
        let mut opaque: *mut c_void = (&mut *ctx as *mut CallbackContext).cast();
        let mut chroma = [0 as c_char; 4];
        let (mut w, mut h, mut pitch, mut lines) = (8_u32, 4_u32, 0_u32, 0_u32);
        unsafe {
            let count = configure_video(&mut opaque, chroma.as_mut_ptr(), &mut w, &mut h, &mut pitch, &mut lines);
            assert_eq!(count, 1);
            assert_eq!(chroma.map(|x| x as u8), *b"RV32");
            assert!(pitch >= w * 4);
            let mut plane: *mut c_void = ptr::null_mut();
            let picture = lock_video(opaque, &mut plane);
            assert!(!picture.is_null());
            assert_eq!(plane as usize % 32, 0);
            ptr::write_bytes(plane, 42, (pitch * lines) as usize);
            unlock_video(opaque, picture, &plane);
            display_video(opaque, picture);
        }
        let frame = store.latest.lock().unwrap().as_ref().unwrap().clone();
        assert_eq!(frame.serial, 1);
        assert_eq!(frame.width, w as usize);
        assert_eq!(frame.bgra[0], 42);
    }
    fn wait_for_decoded_progress(player: &VlcPlayer) {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(8) {
            let snap = player.snapshot();
            if snap.frames >= 3 && snap.time_ms > 200 {
                eprintln!("Decoded {} frames; playback time {} ms", snap.frames, snap.time_ms);
                assert!(player.latest_frame().is_some());
                return;
            }
            assert_ne!(snap.state, 7, "LibVLC entered Error: {snap:?}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("No decoded progress before timeout: {:?}", player.snapshot());
    }

    #[test]
    #[ignore = "requires installed LibVLC 3; run with --ignored --test-threads=1"]
    fn real_local_playback_and_controls() {
        let mut player = VlcPlayer::new_with_options(false, &["--aout=dummy"]).expect("LibVLC 3 is required for runtime tests");
        player.set_volume(0);
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/sample.mp4");
        player.open(sample.to_str().expect("fixture path must be UTF-8")).unwrap();
        wait_for_decoded_progress(&player);
        assert!(player.set_audio_delay(100));
        assert!(player.set_subtitle_delay(-100));
        unsafe {
            assert_eq!((player.api.audio_get_delay)(player.player), 100_000);
            assert_eq!((player.api.video_get_spu_delay)(player.player), -100_000);
        }
        player.toggle_pause();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(player.snapshot().state, 4);
        let before = player.snapshot().frames;
        player.next_frame();
        let started = std::time::Instant::now();
        while player.snapshot().frames <= before && started.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(player.snapshot().frames > before, "Frame stepping must present a frame");
        assert_eq!(player.snapshot().state, 4, "Frame stepping must leave playback paused");
        player.seek_to(2000);
        player.toggle_pause();
        let started = std::time::Instant::now();
        while player.snapshot().time_ms < 2000 && started.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(player.snapshot().time_ms >= 2000, "Seeking must advance the playback clock");
        player.stop();
        assert!(player.latest_frame().is_none());
        assert_eq!(player.snapshot().state, 5);
    }

    fn http_response(request: &str, body: &[u8]) -> Vec<u8> {
        let range = request.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("Range").then(|| value.trim())
        });
        let (start, end) = match range {
            Some(value) => {
                let (first, last) = value.strip_prefix("bytes=").unwrap_or("").split_once('-').unwrap_or(("", ""));
                let start = first.parse::<usize>().unwrap_or(body.len());
                let end = if last.is_empty() { body.len().saturating_sub(1) } else { last.parse::<usize>().unwrap_or(0).min(body.len().saturating_sub(1)) };
                if start >= body.len() || start > end {
                    return format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                }
                (start, end)
            }
            None => (0, body.len().saturating_sub(1)),
        };
        let data = if body.is_empty() { body } else { &body[start..=end] };
        let status = if range.is_some() { "206 Partial Content" } else { "200 OK" };
        let mut header = format!("HTTP/1.1 {status}\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n", data.len());
        if range.is_some() { header.push_str(&format!("Content-Range: bytes {start}-{end}/{}\r\n", body.len())); }
        header.push_str("\r\n");
        let mut response = header.into_bytes();
        if !request.starts_with("HEAD ") { response.extend_from_slice(data); }
        response
    }

    #[test]
    fn http_fixture_supports_ranges_and_head() {
        let response = http_response("GET /sample.mp4 HTTP/1.1\r\nRange: bytes=1-2\r\n\r\n", b"abcd");
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 206"));
        assert!(response.contains("Content-Range: bytes 1-2/4\r\n"));
        assert!(response.ends_with("\r\n\r\nbc"));
        let head = http_response("HEAD /sample.mp4 HTTP/1.1\r\n\r\n", b"abcd");
        assert!(head.ends_with(b"\r\n\r\n"));
        assert!(http_response("GET / HTTP/1.1\r\nRange: bytes=99-\r\n", b"abcd").starts_with(b"HTTP/1.1 416"));
    }

    struct HttpFixture {
        port: u16,
        running: Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl HttpFixture {
        fn new(body: Vec<u8>) -> Self {
            use std::{io::{Read, Write}, net::TcpListener, time::Duration};
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let alive = Arc::clone(&running);
            let thread = std::thread::spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    if let Ok((mut stream, _)) = listener.accept() {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                        let mut request = Vec::new();
                        let mut buffer = [0; 1024];
                        while request.len() < 8192 && !request.windows(4).any(|window| window == b"\r\n\r\n") {
                            match stream.read(&mut buffer) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => request.extend_from_slice(&buffer[..count]),
                            }
                        }
                        if let Ok(request) = std::str::from_utf8(&request) { let _ = stream.write_all(&http_response(request, &body)); }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Self { port, running, thread: Some(thread) }
        }
    }
    impl Drop for HttpFixture {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() { thread.join().unwrap(); }
        }
    }

    #[test]
    #[ignore = "requires installed LibVLC 3 and loopback sockets; run with --ignored --test-threads=1"]
    fn real_http_playback_decodes_frames() {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/sample.mp4");
        let server = HttpFixture::new(std::fs::read(sample).unwrap());
        let mut player = VlcPlayer::new_with_options(false, &["--aout=dummy"]).expect("LibVLC 3 is required for runtime tests");
        player.set_volume(0);
        player.open(&format!("http://127.0.0.1:{}/sample.mp4", server.port)).unwrap();
        wait_for_decoded_progress(&player);
        player.stop();
    }
}
