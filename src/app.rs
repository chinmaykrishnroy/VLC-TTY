use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind, MouseButton};
use crate::vlc::VlcPlayer;

#[derive(Debug)]
pub struct App {
    pub cwd: PathBuf,
    pub entries: Vec<PathBuf>,
    pub selected: usize,
    pub current: Option<String>,
    pub message: String,
    pub focus_video: bool,
    pub browser_scroll: usize,
    pub volume: i32,
    saved_volume: i32,
}

fn supported(p: &Path) -> bool {
    const EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "mp3", "flac", "ogg", "wav", "m4a", "aac"];
    p.is_dir() || p.extension().and_then(|x| x.to_str()).map(|s| EXTS.contains(&s.to_ascii_lowercase().as_str())).unwrap_or(false)
}

impl App {
    pub fn new(path: Option<&Path>) -> Self {
        let resolved = path.unwrap_or_else(|| Path::new("."));
        let cwd = if resolved.is_dir() { resolved.to_path_buf() } else { resolved.parent().unwrap_or_else(|| Path::new(".")).to_path_buf() };
        let mut app = Self { cwd, entries: Vec::new(), selected: 0, current: None, message: "Select a file and press Enter, or pass a path on the command line".into(), focus_video: false, browser_scroll: 0, volume: 100, saved_volume: 100 };
        app.refresh();
        if resolved.is_file() {
            if let Some(i) = app.entries.iter().position(|p| p == resolved) { app.selected = i; }
        }
        app
    }

    pub fn set_current(&mut self, s: String) { self.message = format!("Playing: {s}"); self.current = Some(s); }

    fn refresh(&mut self) {
        self.entries = fs::read_dir(&self.cwd).map(|iter| iter.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| supported(p)).take(1200).collect()).unwrap_or_default();
        self.entries.sort_by(|a, b| (a.is_file(), a.file_name().map(|s| s.to_string_lossy().to_ascii_lowercase())).cmp(&(b.is_file(), b.file_name().map(|s| s.to_string_lossy().to_ascii_lowercase()))));
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.browser_scroll = 0;
    }

    pub fn enter(&mut self, player: &mut VlcPlayer) -> Result<()> {
        if let Some(path) = self.entries.get(self.selected).cloned() {
            if path.is_dir() { self.cwd = path; self.selected = 0; self.refresh(); }
            else {
                let path_str = path.to_string_lossy().into_owned();
                match player.open(&path_str) {
                    Ok(()) => self.set_current(path_str),
                    Err(e) => self.message = format!("Error: {e:#}"),
                }
            }
        }
        Ok(())
    }

    pub fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_owned(); self.selected = 0; self.refresh();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, player: &mut VlcPlayer) -> Result<bool> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
            KeyCode::Char(' ') => player.toggle_pause(),
            KeyCode::Left => player.seek_relative(-10_000),
            KeyCode::Right => player.seek_relative(10_000),
            KeyCode::Char('+') | KeyCode::Char('=') => { self.volume = (self.volume + 5).min(200); player.set_volume(self.volume); }
            KeyCode::Char('-') => { self.volume = (self.volume - 5).max(0); player.set_volume(self.volume); }
            KeyCode::Char('m') => {
                if self.volume != 0 { self.saved_volume = self.volume; self.volume = 0; }
                else { self.volume = self.saved_volume.max(10); }
                player.set_volume(self.volume);
            }
            KeyCode::Char('f') => self.focus_video = !self.focus_video,
            KeyCode::Char('[') => player.set_rate((player.rate() - 0.25).max(0.25)),
            KeyCode::Char(']') => player.set_rate((player.rate() + 0.25).min(4.0)),
            KeyCode::Up | KeyCode::Char('k') => { self.selected = self.selected.saturating_sub(1); },
            KeyCode::Down | KeyCode::Char('j') => { self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1)); },
            KeyCode::Backspace => self.parent(),
            KeyCode::Enter => self.enter(player)?,
            KeyCode::Char('r') => self.refresh(),
            _ => {}
        }
        Ok(true)
    }

    pub fn handle_mouse(&mut self, evt: MouseEvent) {
        match evt.kind {
            MouseEventKind::ScrollDown => self.selected = (self.selected + 2).min(self.entries.len().saturating_sub(1)),
            MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(2),
            MouseEventKind::Down(MouseButton::Left) => {},
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn extension_filter() {
        assert!(supported(Path::new("movie.MKV")));
        assert!(supported(Path::new("sound.flac")));
        assert!(!supported(Path::new("notes.txt")));
    }
    #[test] fn empty_browser_never_underflows() { let a = App::new(Some(Path::new("/nonexistent/termvlc"))); assert!(a.entries.is_empty()); }
}
