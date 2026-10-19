# Architecture

```text
file / URL -> LibVLC 3 -> latest BGRA frame -> bounded encoder -> terminal
                  ^               |                |
                  |               +-> ANSI fallback |
               Playback trait <-> App / queue <-> Ratatui + input
```

LibVLC is dynamically loaded at runtime. It supplies media access, demuxing, decoding and playback controls. Its libraries and matching plugins must be installed. Building VLC from source or replacing decoding with an FFmpeg subprocess would add distribution and control work without solving terminal transport; the current implementation keeps the LibVLC 3 adapter.

On macOS, preload the sibling `libvlccore.dylib` before LibVLC and retain both handles. VLC.app uses an `@rpath` core dependency that an external executable does not automatically inherit. This follows the loading order in [VideoLAN's Python binding](https://mailman.videolan.org/pipermail/vlc-commits/2018-February/047473.html); CI checks actual loading from a copied official bundle rather than masking it with a developer's library path.

`src/vlc.rs` owns the FFI, media player and video callbacks. Decode callbacks publish the latest presented RV32/BGRA frame without writing terminal output. Each lock gets a distinct Rust-owned allocation with an aligned pixel region; display transfers that allocation to the immutable snapshot without copying the full frame. Retained frames survive later decodes and player teardown. Frame dimensions use a startup budget. LibVLC still performs CPU color conversion and writes the supplied buffers; this does not imply GPU-equivalent performance to desktop VLC.

`src/playback.rs` defines the control seam. `src/app.rs` coordinates actions; `src/queue.rs` owns ordering/repeat independently of file browsing. Fake playback supports deterministic control tests; separate runtime tests require actual decoded frames.

`src/graphics.rs` performs scaling and encoding on a worker. One-slot request and response mailboxes replace old work. Generation IDs reject output from an old media/layout state. A single owned Kitty image ID replaces previous frames and is deleted on invalidation/exit. Kitty transfers image data inline, so the SSH client does not need access to server files. tmux passthrough escapes every inner ESC.

`src/renderer.rs` draws the terminal layout and ANSI fallback. External captions have reserved text rows outside pixel graphics. Menus and dialogs use the text renderer to avoid image overlap. Terminal image/text ordering still requires testing in each client.

`src/options.rs` resolves flags and SSH defaults before decoder/terminal initialization. `--ssh` selects silent audio, 15 FPS and a 720p callback cap; explicit flags override those defaults. Local defaults are 60 FPS and a 1080p cap. `--doctor` and `--smoke-test` work without a TTY.

The portable launcher embeds a compressed player/runtime payload, validates its digest, extracts to a fresh private temporary directory and launches a child with the runtime paths set before its loader initializes. Normal exit removes the directory. An explicit replacement-runtime override is retained; strict runtime discovery prevents a broken bundle from silently using a system VLC installation.

Native CI builds six OS/architecture targets and verifies actual decoding from the standalone files. Linux also decodes in a base container without VLC packages. Distribution includes component notices, runtime provenance, checksums and source companions. See [building](BUILDING.md), [releasing](RELEASE.md) and [standalone research](docs/standalone-research.md).
