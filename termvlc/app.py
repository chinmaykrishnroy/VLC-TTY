"""Application coordinator. The only thread touching the terminal is this one."""
from __future__ import annotations

import time
from pathlib import Path
from .render import Screen

VIDEO_EXTS = {".mp4", ".mkv", ".mov", ".webm", ".avi", ".m4v", ".flv", ".ts", ".mpg", ".mpeg", ".wmv", ".mp3", ".flac", ".wav", ".ogg", ".aac", ".m4a"}


class App:
    def __init__(self, engine, source=None, *, fps=12, ascii_mode=False):
        self.engine = engine
        self.screen = Screen(fps=fps, ascii_mode=ascii_mode)
        self.focus_video = False
        self.message = "Ready"
        self.message_until = 0
        self.running = True
        self.current_dir = Path.cwd()
        self.entries = []
        self.selected = 0
        self.active = ""
        if source and not source.startswith(("http://", "https://", "rtsp://", "rtmp://")):
            target = Path(source).expanduser().resolve()
            self.current_dir = target if target.is_dir() else target.parent
        self.refresh_entries()
        if source and not source.startswith(("http://", "https://", "rtsp://", "rtmp://")):
            wanted = Path(source).expanduser().resolve()
            self.selected = next((i for i, entry in enumerate(self.entries) if entry[2] == wanted), self.selected)
        if source and (source.startswith(("http://", "https://", "rtsp://", "rtmp://")) or Path(source).expanduser().is_file()):
            self.play(source)
        else:
            self.notice("Use Enter to open a file or directory")

    def notice(self, message):
        self.message = message
        self.message_until = time.monotonic() + 5

    def display_source(self):
        return self.active.split("/")[-1].split("\\")[-1] if self.active else "Open a file (Enter)"

    def refresh_entries(self):
        values = []
        if self.current_dir.parent != self.current_dir:
            values.append(("..", True, self.current_dir.parent))
        try:
            for p in sorted(self.current_dir.iterdir(), key=lambda v: (not v.is_dir(), v.name.lower())):
                if p.is_dir() or p.suffix.lower() in VIDEO_EXTS:
                    values.append((p.name, p.is_dir(), p))
        except (OSError, PermissionError) as exc:
            self.notice(str(exc))
        self.entries = values
        self.selected = min(self.selected, max(0, len(values) - 1))

    def play(self, source):
        try:
            self.engine.open(str(source))
            self.active = str(source)
            self.notice("Playing " + self.display_source())
        except (OSError, RuntimeError) as exc:
            self.notice(f"Error: {exc}")

    def activate(self):
        if not self.entries:
            return
        _, is_folder, target = self.entries[self.selected]
        if is_folder:
            self.current_dir = target
            self.selected = 0
            self.refresh_entries()
            self.notice(str(target))
        else:
            self.play(target)

    def adjacent(self, step):
        if not self.entries:
            return
        media = [e[2] for e in self.entries if not e[1]]
        if not media:
            return
        active = Path(self.active) if not self.active.startswith(("http:", "https:")) else None
        idx = media.index(active) if active in media else (-1 if step > 0 else 0)
        next_path = media[(idx + step) % len(media)]
        self.play(next_path)
        self.selected = next((i for i, entry in enumerate(self.entries) if entry[2] == next_path), self.selected)

    def on_key(self, key):
        if key in ("q", "\x03"):
            self.running = False
        elif key == " ":
            self.engine.toggle_pause()
        elif key in ("right", "l"):
            self.engine.seek(5)
        elif key in ("left", "h"):
            self.engine.seek(-5)
        elif key in ("+", "="):
            self.engine.change_volume(5)
        elif key == "-":
            self.engine.change_volume(-5)
        elif key == "m":
            self.engine.toggle_mute()
        elif key == "[":
            self.engine.change_rate(-0.25)
        elif key == "]":
            self.engine.change_rate(0.25)
        elif key == "s":
            self.notice(self.engine.cycle_subtitles())
        elif key == "a":
            self.notice(self.engine.cycle_audio())
        elif key == "f":
            self.focus_video = not self.focus_video
        elif key == "n":
            self.adjacent(1)
        elif key == "p":
            self.adjacent(-1)
        elif key == "up":
            self.selected = max(0, self.selected - 1)
        elif key == "down":
            self.selected = min(max(0, len(self.entries) - 1), self.selected + 1)
        elif key in ("\r", "\n"):
            self.activate()

    def run(self, terminal):
        period = 1 / self.screen.fps
        last_render = 0.0
        last_ui = 0.0
        self.screen.repaint(self, force=True)
        while self.running:
            event = terminal.poll_key()
            if event:
                self.on_key(event)
            now = time.monotonic()
            if self.message_until < now:
                self.message = f"Directory: {self.current_dir}"
                self.message_until = now + 10
            if self.engine.callback_error:
                self.notice(f"Video callback: {self.engine.callback_error}")
                self.engine.callback_error = None
            if (now - last_render >= period) or (now - last_ui >= .4):
                self.screen.repaint(self)
                last_render = now
                last_ui = now
            time.sleep(.015)
