"""Zero-dependency TrueColor half-block renderer for real terminal video.

LibVLC feeds RV32 (little-endian BGRA) pictures; each Unicode upper-half block
uses foreground color for the top pixel and background for the bottom pixel.
"""
from __future__ import annotations

import math
import os
import shutil
import sys

CSI = "\x1b["
RESET = CSI + "0m"


def ellipsis(text: str, width: int) -> str:
    if width <= 0:
        return ""
    if len(text) <= width:
        return text
    if width <= 3:
        return text[:width]
    return text[:width - 3] + "..."


def time_label(secs: float) -> str:
    i = max(0, int(secs))
    h, rem = divmod(i, 3600)
    m, s = divmod(rem, 60)
    return f"{h}:{m:02}:{s:02}" if h else f"{m:02}:{s:02}"


def progress_bar(pos, duration, length):
    length = max(0, int(length))
    if not length:
        return ""
    frac = min(1.0, max(0.0, pos / duration)) if duration > 0 else 0
    filled = min(length, max(0, round(frac * length)))
    return "━" * filled + "─" * (length - filled)


def _pixel(data: bytes, pitch: int, x: int, y: int):
    offset = pitch * y + 4 * x
    b, g, r = data[offset], data[offset + 1], data[offset + 2]
    return r, g, b


def halfblock_rows(frame, max_cols: int, max_rows: int, *, ascii_mode=False):
    """Return video image rows, sized to a terminal viewport without cropping.

    Returns (rows, pixel_cols, terminal_rows). Ignores RV32 alpha intentionally.
    Doesn't require numpy/Pillow; performs nearest-neighbor scaling.
    """
    _, src_w, src_h, pitch, data = frame
    if src_w < 1 or src_h < 1 or max_cols < 1 or max_rows < 1:
        return [], 0, 0
    scale = min(max_cols / src_w, (max_rows * 2) / src_h)
    out_w = max(1, min(max_cols, round(src_w * scale)))
    out_h = max(2, min(max_rows * 2, 2 * round(src_h * scale / 2)))
    if out_h % 2:
        out_h -= 1
    rows = []
    if ascii_mode:
        ramp = " .:-=+*#%@"
        for y in range(0, out_h, 2):
            sy = min(src_h - 1, int((y + 0.5) * src_h / out_h))
            chars = []
            for x in range(out_w):
                sx = min(src_w - 1, int((x + 0.5) * src_w / out_w))
                r, g, b = _pixel(data, pitch, sx, sy)
                brightness = min(len(ramp) - 1, round((0.2126 * r + 0.7152 * g + 0.0722 * b) * (len(ramp) - 1) / 255))
                chars.append(ramp[brightness])
            rows.append("".join(chars))
        return rows, out_w, len(rows)

    # Remember last colors on the row, reducing escape sequence bandwidth.
    for y in range(0, out_h, 2):
        sy_top = min(src_h - 1, int(y * src_h / out_h))
        sy_bottom = min(src_h - 1, int((y + 1) * src_h / out_h))
        parts = []
        last_fg = last_bg = None
        for x in range(out_w):
            sx = min(src_w - 1, int(x * src_w / out_w))
            fg = _pixel(data, pitch, sx, sy_top)
            bg = _pixel(data, pitch, sx, sy_bottom)
            if fg != last_fg:
                parts.append(f"{CSI}38;2;{fg[0]};{fg[1]};{fg[2]}m")
                last_fg = fg
            if bg != last_bg:
                parts.append(f"{CSI}48;2;{bg[0]};{bg[1]};{bg[2]}m")
                last_bg = bg
            parts.append("▀")
        parts.append(RESET)
        rows.append("".join(parts))
    return rows, out_w, len(rows)


def cursor(row, col):
    return f"{CSI}{row};{col}H"


class Screen:
    """Small terminal compositor; video and text share one output owner."""

    def __init__(self, *, fps=12, ascii_mode=False):
        self.fps = fps
        self.ascii_mode = ascii_mode
        self._last_video_sequence = -1
        self._last_dims = None

    @staticmethod
    def size():
        size = shutil.get_terminal_size(fallback=(90, 30))
        return size.columns, size.lines

    @staticmethod
    def output(value):
        # Always write Unicode/ANSI in one batch, avoid unwanted print newlines.
        sys.stdout.write(value)
        sys.stdout.flush()

    @staticmethod
    def row(row, col, text, maxlen=None):
        if maxlen is not None:
            text = ellipsis(text, maxlen)
        return cursor(row, col) + text + RESET

    def repaint(self, app, *, force=False):
        cols, lines = self.size()
        if cols < 45 or lines < 13:
            self.output(cursor(1, 1) + CSI + "2J" + "Please enlarge your terminal (minimum 45x13).")
            return
        sidebar = 23 if cols >= 96 and not app.focus_video else 0
        x = sidebar + 2 if sidebar else 2
        width = cols - x - 1
        video_top = 4
        video_height = max(3, lines - video_top - 5)
        geom = (cols, lines, sidebar, app.focus_video)
        force = force or geom != self._last_dims
        if force:
            self._last_video_sequence = -1
            self._last_dims = geom
            self.output(CSI + "2J")

        out = []
        app_title = f"  ◈ TERMVLC   │   {app.display_source()}"
        out += [self.row(1, 1, f"{CSI}1;96m" + ellipsis(app_title, cols) + RESET + " " * max(0, cols - len(ellipsis(app_title, cols))))]
        status = f"  {app.engine.state.upper():<9}   {app.message[:max(0, cols - 26)]}"
        out += [self.row(2, 1, ellipsis(status.ljust(cols), cols))]
        out += [self.row(3, 1, "─" * cols)]
        if sidebar:
            out += [self.row(4, 2, f"{CSI}1mLIBRARY{RESET}")]
            entries = app.entries
            max_entries = max(0, video_height - 2)
            first = max(0, min(app.selected - 3, max(0, len(entries) - max_entries)))
            for i in range(max_entries):
                idx = first + i
                label = ""
                if idx < len(entries):
                    name, is_folder, _ = entries[idx]
                    label = ("▸ " if is_folder else "  ") + ellipsis(name, sidebar - 3)
                    if idx == app.selected:
                        label = CSI + "7m" + label.ljust(sidebar - 1) + RESET
                out.append(self.row(5 + i, 2, label.ljust(sidebar - 1)))
            for y in range(4, video_top + video_height + 1):
                out.append(self.row(y, sidebar + 1, "│"))

        frame = app.engine.latest_frame()
        seq = frame[0] if frame else -1
        if seq != self._last_video_sequence or force:
            self._last_video_sequence = seq
            rows = []
            fit_w = 0
            if frame:
                rows, fit_w, _ = halfblock_rows(frame, width, video_height, ascii_mode=self.ascii_mode)
            first_line = video_top + max(0, (video_height - len(rows)) // 2)
            # Painting the region before new image removes stale pixels on resize / file change.
            for y in range(video_top, video_top + video_height):
                line = rows[y - first_line] if first_line <= y < first_line + len(rows) else ""
                space = max(0, (width - fit_w) // 2) if line else 0
                line = (" " * space) + line
                # SGR reset means clearing trailing portion with default background.
                visible_len = space + (fit_w if line.strip() else 0)
                out.append(cursor(y, x) + line + RESET + " " * max(0, width - visible_len))

        progress_y = lines - 4
        duration = app.engine.duration
        position = app.engine.position
        out.append(self.row(progress_y, 2, f"{CSI}96m" + progress_bar(position, duration, cols - 4) + RESET))
        info = f"  {time_label(position)} / {time_label(duration)}   │   Vol {app.engine.volume}%   │   {app.engine.rate:.2f}x   │   {app.engine.state}"
        out.append(self.row(lines - 3, 1, ellipsis(info.ljust(cols), cols)))
        controls = "  SPACE pause   ←/→ seek 5s   +/- vol   m mute   [/ ] speed   s subs   a audio"
        out.append(self.row(lines - 2, 1, ellipsis(controls.ljust(cols), cols)))
        extra = "  q quit   f fullscreen   ↑/↓ select   Enter play/open   n/p next/previous"
        out.append(self.row(lines - 1, 1, ellipsis(extra.ljust(cols), cols)))
        self.output("".join(out))
