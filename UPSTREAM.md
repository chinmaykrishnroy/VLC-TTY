# Upstream reference repos

Source inspection informed the architecture. **Neither upstream repository is vendored in this ZIP**, since GitHub DNS/network access was unavailable inside the packaging environment. This is a stand-alone Rust prototype.

- SparkPlayer: https://github.com/dividebysandwich/sparkplayer (see `crates/sparkplayer-core/src/backend.rs`, `crates/sparkplayer-native/src/backends.rs`, `video.rs`)
- VLC / LibVLC: https://github.com/videolan/vlc (see `include/vlc/libvlc_media_player.h`, `doc/standalone/libvlc.md`)
- LibVLC stable C API docs: https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html

If you wish to clone the originals for reference, these commands work with normal Git network access:

```bash
git clone --depth 1 https://github.com/dividebysandwich/sparkplayer.git upstream/sparkplayer
git clone --depth 1 https://github.com/videolan/vlc.git upstream/vlc
```

Important: SparkPlayer declares GPL-2.0-only; VLC's LibVLC is LGPL-2.1-or-later, and some VLC plugins have other licensing. Before incorporating actual upstream code, assess the licenses of the precise components and dependencies used.
