//! Native LibVLC 3.x adapter.
//!
//! No VLC header/build-time linker dependency. libloading resolves the shared
//! library at runtime. LibVLC still requires its normal codecs/plugins on disk.
//! Video format negotiation uses RV32 with 32-byte aligned, per-picture buffers.
//! Unlike v0.2, frames are NOT capped at 480x300. Default cap is 1920x1080;
//! set TERMVLC_DECODE_MAX=0 for full source resolution (large allocations!).
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
    // Decode at 720p or original source size, whichever is smaller.
    // 720p matches or exceeds the pixel density of terminal windows while cutting
    // memory bandwidth and decode CPU usage by >50%. Allow TERMVLC_DECODE_MAX=1080
    // or TERMVLC_DECODE_MAX=0 for full source resolution.
    let requested_max = env::var("TERMVLC_DECODE_MAX").ok()
        .and_then(|s| s.parse::<u32>().ok()).unwrap_or(720);
    let scale = if requested_max == 0 { 1.0 } else {
        (requested_max as f64 * (16.0 / 9.0) / in_w)
            .min(requested_max as f64 / in_h).min(1.0)
    };
    let w = ((in_w * scale).round() as u32).max(1);
    let h = ((in_h * scale).round() as u32).max(1);
    let pitch = (w * 4 + 31) & !31;
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
    let layout = Layout::from_size_align(pitch * height, 32).expect("valid frame layout");
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

// Simple C signatures from the LibVLC 3.x public ABI.
struct Api {
    _library: Library,
    loaded_from: PathBuf,
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
    player_stop: unsafe extern "C" fn(*mut c_void),
    player_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    audio_get_volume: unsafe extern "C" fn(*mut c_void) -> c_int,
    audio_set_volume: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    video_set_callbacks: unsafe extern "C" fn(*mut c_void, Option<LockFn>, Option<UnlockFn>, Option<DisplayFn>, *mut c_void),
    video_set_format_callbacks: unsafe extern "C" fn(*mut c_void, Option<FormatFn>, Option<CleanupFn>),
    errmsg: unsafe extern "C" fn() -> *const c_char,
}

impl Api {
    fn load() -> Result<Self> {
        let mut failures = Vec::new();
        for name in libvlc_candidates() {
            #[cfg(windows)]
            if let Some(parent) = name.parent() {
                if parent.is_dir() {
                    let old_path = env::var("PATH").unwrap_or_default();
                    env::set_var("PATH", format!("{};{}", parent.display(), old_path));
                    let plugins = parent.join("plugins");
                    if plugins.is_dir() && env::var_os("VLC_PLUGIN_PATH").is_none() {
                        env::set_var("VLC_PLUGIN_PATH", plugins);
                    }
                }
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
        macro_rules! f { ($s:literal) => { *lib.get(concat!($s, "\0").as_bytes()).with_context(|| format!("missing VLC symbol {}", $s))? } }
        Ok(Self {
            new: f!("libvlc_new"), release: f!("libvlc_release"),
            media_new_path: f!("libvlc_media_new_path"), media_new_location: f!("libvlc_media_new_location"),
            media_release: f!("libvlc_media_release"),
            player_new: f!("libvlc_media_player_new"), player_release: f!("libvlc_media_player_release"),
            player_set_media: f!("libvlc_media_player_set_media"),
            player_play: f!("libvlc_media_player_play"), player_set_pause: f!("libvlc_media_player_set_pause"),
            player_stop: f!("libvlc_media_player_stop"),
            player_get_state: f!("libvlc_media_player_get_state"),
            player_get_time: f!("libvlc_media_player_get_time"), player_get_length: f!("libvlc_media_player_get_length"),
            player_set_time: f!("libvlc_media_player_set_time"),
            audio_get_volume: f!("libvlc_audio_get_volume"), audio_set_volume: f!("libvlc_audio_set_volume"),
            player_set_rate: f!("libvlc_media_player_set_rate"),
            video_set_callbacks: f!("libvlc_video_set_callbacks"),
            video_set_format_callbacks: f!("libvlc_video_set_format_callbacks"),
            errmsg: f!("libvlc_errmsg"),
            _library: lib,
            loaded_from,
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
    pub state: i32, pub rate: f32, pub frames: u64,
}

pub struct VlcPlayer {
    api: Api,
    instance: *mut c_void,
    player: *mut c_void,
    context: Box<CallbackContext>,
    rate: f32,
}

impl VlcPlayer {
    pub fn new() -> Result<Self> { Self::new_with_window(false) }

    pub fn library_path(&self) -> &Path { &self.api.loaded_from }

    pub fn new_with_window(native_window: bool) -> Result<Self> {
        let api = Api::load()?;
        api.configure_plugin_path();
        let args = [CString::new("--quiet")?, CString::new("--no-video-title-show")?];
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
        Ok(Self { api, instance, player, context, rate: 1.0 })
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
            let p = Path::new(locator);
            if !p.exists() {
                bail!("file not found: {locator}");
            }
            let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            let mut s = canonical.to_string_lossy().into_owned();
            if s.starts_with(r"\\?\UNC\") {
                s = format!(r"\\{}", &s[8..]);
            } else if s.starts_with(r"\\?\") {
                s = s[4..].to_string();
            }
            s
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
    pub fn set_volume(&mut self, volume: i32) { unsafe { (self.api.audio_set_volume)(self.player, volume); } }
    pub fn rate(&self) -> f32 { self.rate }
    pub fn set_rate(&mut self, value: f32) {
        if unsafe { (self.api.player_set_rate)(self.player, value) } == 0 { self.rate = value; }
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
    #[test]
    fn test_switch_videos() {
        let mut player = match VlcPlayer::new() {
            Ok(p) => p,
            Err(_) => return,
        };
        let file1 = r"E:\data\Media\TV\Breaking Bad\Season 02\Breaking Bad - S02E03.mkv";
        let file2 = r"E:\data\Media\TV\Breaking Bad\Season 02\Breaking Bad - S02E04.mkv";
        if std::path::Path::new(file1).exists() {
            let _ = player.open(file1);
            std::thread::sleep(std::time::Duration::from_millis(500));
            if std::path::Path::new(file2).exists() {
                let _ = player.open(file2);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}
