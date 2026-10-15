"""Thin, version-pinned LibVLC 3.x ctypes adapter.

All C declarations here target the documented LibVLC 3.x API. VLC's native
playback clock and audio output remain authoritative; we only receive rendered
RV32 video pictures. Callbacks never write to the terminal.
"""
from __future__ import annotations

import ctypes as C
import ctypes.util
import os
import sys
from pathlib import Path
from threading import Lock
from typing import Optional

VOID = C.c_void_p
UINT = C.c_uint
UINTP = C.POINTER(UINT)

# 3.x callback ABI from include/vlc/libvlc_media_player.h.
VideoLock = C.CFUNCTYPE(VOID, VOID, C.POINTER(VOID))
VideoDisplay = C.CFUNCTYPE(None, VOID, VOID)
VideoFormat = C.CFUNCTYPE(UINT, C.POINTER(VOID), C.POINTER(C.c_char), UINTP, UINTP, UINTP, UINTP)


class LibVLCError(RuntimeError):
    pass


class TrackDescription(C.Structure):
    pass


TrackDescription._fields_ = [
    ("id", C.c_int),
    ("name", C.c_char_p),
    ("next", C.POINTER(TrackDescription)),
]


def _load_library():
    """Find libvlc without needing the python-vlc third-party wrapper."""
    override = os.environ.get("TERMVLC_LIBVLC_PATH")
    if override:
        libpath = Path(override).expanduser()
        if sys.platform == "win32" and hasattr(os, "add_dll_directory"):
            _DLL_DIR_HANDLES.append(os.add_dll_directory(str(libpath.parent.resolve())))
            os.environ.setdefault("VLC_PLUGIN_PATH", str(libpath.parent / "plugins"))
        return C.CDLL(str(libpath))
    candidates = []
    found = ctypes.util.find_library("vlc")
    if found:
        candidates.append(found)
    if sys.platform == "win32":
        roots = [
            os.environ.get("VLC_HOME"),
            os.path.join(os.environ.get("PROGRAMFILES", "C:\\Program Files"), "VideoLAN", "VLC"),
            os.path.join(os.environ.get("PROGRAMFILES(X86)", "C:\\Program Files (x86)"), "VideoLAN", "VLC"),
        ]
        for root in filter(None, roots):
            dll = Path(root) / "libvlc.dll"
            if dll.is_file():
                if hasattr(os, "add_dll_directory"):
                    # Keep handle alive while libvlc and its plugins are loaded.
                    _DLL_DIR_HANDLES.append(os.add_dll_directory(str(dll.parent)))
                os.environ.setdefault("VLC_PLUGIN_PATH", str(dll.parent / "plugins"))
                candidates.insert(0, str(dll))
        candidates.append("libvlc.dll")
    elif sys.platform == "darwin":
        vlc_plugins = Path("/Applications/VLC.app/Contents/MacOS/plugins")
        if vlc_plugins.is_dir():
            os.environ.setdefault("VLC_PLUGIN_PATH", str(vlc_plugins))
        candidates += [
            "/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib",
            "libvlc.dylib",
        ]
    else:
        candidates += ["libvlc.so.5", "libvlc.so"]
    for item in candidates:
        try:
            return C.CDLL(item)
        except OSError:
            pass
    raise LibVLCError(
        "Cannot load LibVLC. Install 64-bit VLC and ensure its libraries are "
        "available (Windows: install VLC from videolan.org; Linux: install libvlc5). "
        "You can override the library location with TERMVLC_LIBVLC_PATH."
    )


_DLL_DIR_HANDLES = []


def _bind(lib, name, result, *arguments):
    fn = getattr(lib, name)
    fn.restype = result
    fn.argtypes = list(arguments)
    return fn


class LibVLC:
    """One media player and one latest-frame slot; caller owns its lifecycle."""

    def __init__(self, decode_width=224):
        self.lib = _load_library()
        self._configure_abi()
        self._mutex = Lock()
        self._frame = None  # (sequence, pixel_width, pixel_height, stride, BGRA bytes)
        self._sequence = 0
        self._backing = None
        self._plane_address = 0
        self._pitch = 0
        self._height = 0
        self._width = 0
        self.callback_error = None
        self.decode_width = max(64, min(512, int(decode_width)))
        self._lock_cb = VideoLock(self._lock_picture)
        self._display_cb = VideoDisplay(self._display_picture)
        self._format_cb = VideoFormat(self._configure_format)
        libvlc_options = (C.c_char_p * 2)(b"--quiet", b"--no-video-title-show")
        self.instance = self.lib.libvlc_new(2, libvlc_options)
        if not self.instance:
            raise LibVLCError("libvlc_new failed. Check VLC plugins and runtime dependencies.")
        self.player = self.lib.libvlc_media_player_new(self.instance)
        if not self.player:
            self.lib.libvlc_release(self.instance)
            raise LibVLCError("libvlc_media_player_new failed")
        # Most important: register the custom memory video output BEFORE play().
        self.lib.libvlc_video_set_callbacks(self.player, self._lock_cb, None, self._display_cb, None)
        self.lib.libvlc_video_set_format_callbacks(self.player, self._format_cb, None)
        self.current_source = None

    def _configure_abi(self):
        b = lambda name, result, *args: _bind(self.lib, name, result, *args)
        b("libvlc_new", VOID, C.c_int, C.POINTER(C.c_char_p))
        b("libvlc_get_version", C.c_char_p)
        b("libvlc_release", None, VOID)
        b("libvlc_media_player_new", VOID, VOID)
        b("libvlc_media_player_release", None, VOID)
        b("libvlc_media_new_path", VOID, VOID, C.c_char_p)
        b("libvlc_media_new_location", VOID, VOID, C.c_char_p)
        b("libvlc_media_release", None, VOID)
        b("libvlc_media_player_set_media", None, VOID, VOID)
        b("libvlc_media_player_play", C.c_int, VOID)
        b("libvlc_media_player_set_pause", None, VOID, C.c_int)
        b("libvlc_media_player_stop", None, VOID)
        b("libvlc_media_player_get_time", C.c_int64, VOID)
        b("libvlc_media_player_get_length", C.c_int64, VOID)
        b("libvlc_media_player_set_time", None, VOID, C.c_int64)
        b("libvlc_media_player_get_state", C.c_int, VOID)
        b("libvlc_audio_get_volume", C.c_int, VOID)
        b("libvlc_audio_set_volume", C.c_int, VOID, C.c_int)
        b("libvlc_audio_get_mute", C.c_int, VOID)
        b("libvlc_audio_set_mute", None, VOID, C.c_int)
        b("libvlc_media_player_get_rate", C.c_float, VOID)
        b("libvlc_media_player_set_rate", C.c_int, VOID, C.c_float)
        b("libvlc_video_set_callbacks", None, VOID, VideoLock, VOID, VideoDisplay, VOID)
        b("libvlc_video_set_format_callbacks", None, VOID, VideoFormat, VOID)
        b("libvlc_video_get_spu", C.c_int, VOID)
        b("libvlc_video_set_spu", C.c_int, VOID, C.c_int)
        b("libvlc_video_get_spu_description", C.POINTER(TrackDescription), VOID)
        b("libvlc_audio_get_track", C.c_int, VOID)
        b("libvlc_audio_set_track", C.c_int, VOID, C.c_int)
        b("libvlc_audio_get_track_description", C.POINTER(TrackDescription), VOID)
        b("libvlc_track_description_list_release", None, C.POINTER(TrackDescription))

    def _configure_format(self, _opaque, chroma, width, height, pitches, lines):
        try:
            source_w, source_h = max(1, int(width[0])), max(1, int(height[0]))
            # Width aligned to 32; pitch+line allocation padded to 32 bytes.
            w = ((self.decode_width + 31) // 32) * 32
            h = max(2, min(512, round(w * source_h / source_w)))
            if h % 2:
                h += 1
            pitch = w * 4
            padded_h = ((h + 31) // 32) * 32
            backing = C.create_string_buffer(pitch * padded_h + 31)
            addr = (C.addressof(backing) + 31) & ~31
            # Keep backing memory pinned for the life of these video callbacks.
            with self._mutex:
                self._backing = backing
                self._plane_address = addr
                self._pitch = pitch
                self._width = w
                self._height = h
                self._frame = None
            C.memmove(chroma, b"RV32", 4)
            width[0], height[0] = w, h
            pitches[0], lines[0] = pitch, padded_h
            return 1
        except Exception as exc:
            self.callback_error = f"format: {exc}"
            return 0

    def _lock_picture(self, _opaque, planes):
        try:
            # One LibVLC picture buffer is advertised by _configure_format().
            planes[0] = self._plane_address
        except Exception as exc:
            self.callback_error = f"lock: {exc}"
        return None

    def _display_picture(self, _opaque, _picture):
        try:
            # LibVLC decides WHEN a frame should display (A/V clock). Copy
            # outside the critical section, and don't queue old video frames.
            data = C.string_at(self._plane_address, self._pitch * self._height)
            with self._mutex:
                self._sequence += 1
                self._frame = (self._sequence, self._width, self._height, self._pitch, data)
        except Exception as exc:
            self.callback_error = f"display: {exc}"

    def latest_frame(self):
        with self._mutex:
            return self._frame

    def open(self, source):
        uri = str(source)
        if not uri.startswith(("http://", "https://", "rtsp://", "rtmp://")):
            path = Path(uri).expanduser().resolve()
            if not path.is_file():
                raise FileNotFoundError(path)
            media = self.lib.libvlc_media_new_path(self.instance, str(path).encode("utf-8"))
        else:
            media = self.lib.libvlc_media_new_location(self.instance, uri.encode("utf-8"))
        if not media:
            raise LibVLCError("LibVLC could not open the media source")
        try:
            self.lib.libvlc_media_player_stop(self.player)
            with self._mutex:
                self._frame = None
            self.lib.libvlc_media_player_set_media(self.player, media)
            status = self.lib.libvlc_media_player_play(self.player)
            if status != 0:
                raise LibVLCError(f"VLC failed to start playback (code {status})")
            self.current_source = uri
        finally:
            self.lib.libvlc_media_release(media)

    @property
    def state(self):
        states = ["idle", "opening", "buffering", "playing", "paused", "stopped", "ended", "error"]
        i = self.lib.libvlc_media_player_get_state(self.player)
        return states[i] if 0 <= i < len(states) else "unknown"

    def toggle_pause(self):
        s = self.state
        if s == "playing":
            self.lib.libvlc_media_player_set_pause(self.player, 1)
        elif s == "paused":
            self.lib.libvlc_media_player_set_pause(self.player, 0)
        elif s in {"ended", "stopped", "idle"}:
            self.lib.libvlc_media_player_play(self.player)

    @property
    def position(self):
        return max(0, self.lib.libvlc_media_player_get_time(self.player)) / 1000

    @property
    def duration(self):
        return max(0, self.lib.libvlc_media_player_get_length(self.player)) / 1000

    def seek(self, offset_seconds):
        desired = max(0, self.position + offset_seconds)
        if self.duration > 0:
            desired = min(desired, self.duration)
        self.lib.libvlc_media_player_set_time(self.player, int(desired * 1000))

    @property
    def volume(self):
        return max(0, self.lib.libvlc_audio_get_volume(self.player))

    def change_volume(self, step):
        self.lib.libvlc_audio_set_volume(self.player, max(0, min(200, self.volume + step)))

    def toggle_mute(self):
        now = self.lib.libvlc_audio_get_mute(self.player)
        self.lib.libvlc_audio_set_mute(self.player, 0 if now else 1)

    @property
    def rate(self):
        return self.lib.libvlc_media_player_get_rate(self.player)

    def change_rate(self, change):
        rate = max(0.25, min(4.0, round(self.rate + change, 2)))
        self.lib.libvlc_media_player_set_rate(self.player, C.c_float(rate))

    def _track_ids(self, function):
        head = function(self.player)
        if not head:
            return []
        result = []
        try:
            node = head
            for _ in range(256):
                if not node:
                    break
                t = node.contents
                result.append((t.id, (t.name or b"Unknown").decode("utf-8", errors="replace")))
                node = t.next
        finally:
            self.lib.libvlc_track_description_list_release(head)
        return result

    def cycle_subtitles(self):
        ids = [x[0] for x in self._track_ids(self.lib.libvlc_video_get_spu_description)]
        if -1 not in ids:
            ids.insert(0, -1)
        if not ids:
            return "No subtitles"
        current = self.lib.libvlc_video_get_spu(self.player)
        next_id = ids[(ids.index(current) + 1) % len(ids)] if current in ids else ids[0]
        self.lib.libvlc_video_set_spu(self.player, next_id)
        return f"Subtitles: {'off' if next_id == -1 else next_id}"

    def cycle_audio(self):
        ids = [x[0] for x in self._track_ids(self.lib.libvlc_audio_get_track_description) if x[0] >= 0]
        if not ids:
            return "No alternate audio tracks"
        current = self.lib.libvlc_audio_get_track(self.player)
        next_id = ids[(ids.index(current) + 1) % len(ids)] if current in ids else ids[0]
        self.lib.libvlc_audio_set_track(self.player, next_id)
        return f"Audio track: {next_id}"

    def close(self):
        if getattr(self, "player", None):
            # stop blocks until output callbacks end; never release memory first.
            self.lib.libvlc_media_player_stop(self.player)
            self.lib.libvlc_media_player_release(self.player)
            self.player = None
        if getattr(self, "instance", None):
            self.lib.libvlc_release(self.instance)
            self.instance = None
        with self._mutex:
            self._frame = None
            self._backing = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
