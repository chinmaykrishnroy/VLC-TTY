# TermVLC project assessment — 16 October 2026

Historical baseline: the 17 October reliability milestone fixes several findings below and adds a playback queue. See [current validation](../VALIDATION.md) for the implemented behavior and remaining acceptance work.

Product clarification, 17 October: the user confirmed that terminal video playback is the purpose of this project. Ratatui and LibVLC remain the primary direction; the earlier conditional discussion of a desktop frontend is not the implementation roadmap. The next milestone is terminal playback quality and responsiveness.

Assessed commit `27f99f3`, with an initially clean working tree. This is an assessment, not a feature implementation or a complete security audit. Production Rust sources were not changed.

## Recommendation

Continue the project, but choose its product promise before expanding the interface. A VLC-inspired terminal player is a useful, distinctive scope. A close replacement for desktop VLC requires a graphical frontend with a native video surface. Keep prebuilt, version-tested LibVLC 3 shared libraries in either case; compiling upstream source or switching to FFmpeg will not solve the limitations of terminal display.

Ratatui can reproduce the menu structure and keyboard workflows, but it cannot supply native desktop widgets or an integrated GPU video surface by itself. The existing `--native-window` path displays video in a separate window rather than embedding it in the controls. See [architecture research](architecture-research.md) for primary-source references and licensing/packaging considerations.

## Validation performed

- `cargo test --offline --locked --all-targets`: compiled; **26 passed, 1 failed**. The HTTP playback test failed with `Network stream failed to start playing` at `src/vlc.rs:577`.
- Repeated with `cargo test --offline --locked --all-targets -- --test-threads=1`: **26 passed, 1 failed**, same HTTP test failure. Serial execution rules out parallel test interaction as a sufficient explanation for this failure.
- `target/release/termvlc.exe --doctor`: successfully loaded `C:\Program Files\VideoLAN/VLC/libvlc.dll`. This existing release executable predates this assessment; current sources were compiled by the test runs.
- Temporary probes imported the real application modules and used the installed LibVLC. These were observational probes, not regression tests asserting corrected behavior. Results are below. They do not establish audible sound quality or terminal graphics quality.
- A clean, muted local playback probe with installed LibVLC **3.0.24** delivered **183 frames over eight seconds**, with playback time advancing to 7302ms. This validates actual decoding/callback delivery rather than just an Opening/Playing state.
- A comparison run of installed VLC with dummy audio/video outputs completed the sample and exited normally. It was a headless runtime comparison, not a visual/audio acceptance test.
- No visual verification in Windows Terminal/Kitty/iTerm2, no audible sync verification, no clean-machine package validation, and no Linux/macOS runtime validation were performed.

## Reproduced behavior

| Finding | Reproduction / observation | Source |
| --- | --- | --- |
| Volume becomes negative | Set application volume to 0 and invoke `-`: application volume becomes **-5**. Signed `saturating_sub` does not clamp at zero. Other decrement controls use the same pattern. | `src/app.rs:701`, `src/app.rs:907` |
| External subtitle cycling gets stuck | With a loaded external SRT, no active embedded subtitle, and sidecar initially disabled: first cycle enables the external track; second cycle leaves it enabled. The current selection is read only from LibVLC, which cannot represent the local sidecar selection. | `src/app.rs:353` |
| Overlapping subtitles disappear early | Cue 1 runs 1–10s, cue 2 runs 2–3s: lookup at 4s returns **None**, although cue 1 is still active. Lookup checks only the last-started cue. Simultaneous cues also cannot be represented by the current single-cue result. | `src/subtitles.rs:90` |
| Unicode audio metadata is decoded incorrectly | A valid ID3 text frame using encoding 2 (UTF-16BE) containing `AB` is read as `䄀䈀`. The decoder treats both UTF-16 encodings as little-endian and does not handle BOM endianness correctly. Encoding 0 also uses UTF-8 decoding rather than ISO-8859-1. | `src/metadata.rs:221` |
| Opening a file does not update the directory used by Next/Previous | Open `samples/sample.mp4` while the browser points at the project root, then invoke Next: current file stays unchanged; status is **No media in this directory**. Next/Previous use browser entries rather than a playback queue or the opened file's folder. | `src/app.rs:240`, `src/app.rs:294` |

One combined control-path probe reached Playing with zero frames/time after five seconds; the subsequent clean playback probe succeeded with 183 frames. The earlier stall was not isolated to a specific cause and is not counted as an established general playback defect. State-only tests would miss such a stall.

## Defects established by tracing the implementation

These are code-path findings. They were not all exercised through an interactive terminal.

1. **Rapid ordinary keypresses can be mistaken for a dropped path.** `src/main.rs:184` batches characters arriving within 35–40ms. A mixed burst such as `fg` is sent to file-drop handling, so its shortcuts are lost. The burst-draining function also consumes non-character keys, mouse events, and resize events without forwarding them (`src/main.rs:125`).
2. **Audio-sync controls do not affect audio.** `j`/`k` only update `audio_delay_ms` and a notification (`src/app.rs:725`). The LibVLC adapter exposes no audio-delay setter, and the field is not used to adjust playback.
3. **Subtitle-sync controls affect only the local SRT/VTT overlay.** Embedded/native subtitles are delegated to LibVLC but the control changes only `subtitle_delay_ms` (`src/app.rs:530`, `src/app.rs:723`); no LibVLC subtitle-delay call is made.
4. **The Video → Toggle Renderer menu entry does nothing.** Its action is empty (`src/app.rs:524`). Only a lower-case `g` intercepted in `main.rs` changes the renderer; this also conflicts with the subtitle menu's advertised `G` delay shortcut.
5. **Clicking Media → Quit cannot exit.** The menu action returns `false` (`src/app.rs:496`), but mouse handling discards that return value (`src/app.rs:770`), and the outer event loop receives only `Result<()>` from mouse handling. Keyboard menu activation does propagate the quit result.
6. **Next Frame is a fixed 40ms seek.** Both the menu and `E` call `seek_relative(40)` (`src/app.rs:508`, `src/app.rs:711`). That is neither a frame-step operation nor correct across different frame rates, and it does not pause playback.
7. **Double-clicking video also changes pause state.** The first click immediately toggles pause; the second click toggles fullscreen (`src/app.rs:864`). Thus fullscreen activation has a playback side effect. This was established by tracing the click handlers; the combined control probe did not reach advancing video for an interactive confirmation.
8. **Playback errors are poorly surfaced.** Opening is asynchronous, the adapter requests `--quiet`, and the UI does not turn LibVLC error state into a useful error message. Without an image, the normal renderer shows `Opening / buffering media…` even for stopped or failed media (`src/renderer.rs:630`).

## Missing capabilities and architectural risks

These are distinct from the defects above:

- The “playlist” is a directory browser. There is no independent playback queue, automatic progression at end-of-media, queue editing, repeat, shuffle, playlist-file import/export, or persisted playback history/settings.
- Embedded and styled subtitle parity is unverified. The local overlay supports UTF-8 SRT/VTT with simplified text; it is not an ASS/SSA or bitmap-subtitle renderer. LibVLC callback documentation explicitly describes subtitle blending, so callbacks alone do not prove embedded subtitles are unsupported.
- The current inline path allocates CPU buffers, copies presented frames, scales/encodes them, and transports them through a terminal image protocol. LibVLC's A/V clock does not account for the subsequent terminal rendering delay. Actual visual latency and frame drops need measurement.
- `src/vlc.rs:49` now defaults to **uncapped** source dimensions, contrary to the README and module comment claiming a default 1080p cap. A 3840×2160 RV32 frame alone is about 33MB before additional copies/encoding. This is a performance risk, not a measured claim of unusability.
- RV32 is an 8-bit display path and the custom Sixel encoder quantizes to 256 colors. Native HDR/color fidelity cannot be assumed.
- Text captions and pixel video occupy the same area (`src/renderer.rs:253`, `src/renderer.rs:635`), contrary to the README claim that captions are outside pixel graphics. Protocol-dependent image/text overlap remains unverified.
- Fullscreen also injects a global Windows F11 event and sends terminal escape sequences; terminal-specific behavior and focus interactions remain unverified. On other systems the Windows helper is a no-op.
- Native file selection is implemented only on Windows. On other platforms `open_file_dialog()` returns None, so Open File does nothing; path entry is available through other actions.
- Runtime discovery is implemented, but a releasable bundle needs a coherent LibVLC/core/plugin set, explicit supported ABI/version/architecture, a clean-system check, and runtime license notices. MIT licensing of this application does not cover arbitrary copied VLC code/plugins.
- Present UI/help strings repeatedly call this application `VLC media player`, and it uses a VLC cone asset. Product identity and permission need review before public distribution; backend reuse does not make this an official VLC product.
- Existing playback tests may return success without loading LibVLC. The local playback test accepts Opening/Buffering states, and on non-Windows its backslash sample path can prevent the playback branch from running. CI passing would not prove actual decoding, sound, or terminal output.

## Suggested initial release scope

1. Establish reliable decoding through control transitions, useful playback errors, seek/pause/stop/open behavior, audio output, and graceful shutdown. Resolve the HTTP test failure and isolate the combined control sequence's stall before adding cosmetic features. Clean local decoding already succeeds.
2. Fix the controls above and introduce a coherent playback model independent of the browser: queue, active item, end/error transitions, and authoritative track/volume/rate state.
3. Complete everyday workflows: local files and folders, network URLs, queue/next/previous, repeat/shuffle, volume/mute, speed, audio tracks, subtitles and real synchronization, familiar fullscreen, media information, saved settings, and resume position. This is a practical scope recommendation, not a measured ranking of VLC feature usage.
4. Validate long sessions, common codecs/containers, multiple subtitle formats/languages, Unicode paths, live streams, graphics protocols, terminal resizing, and clean-machine packaging.
5. Add advanced features later: equalizer/effects, snapshots, recording, conversion/streaming, capture, casting, disc menus, and HDR-specific behavior.

If desktop resemblance is a hard requirement, start the native frontend before expanding the Ratatui presentation. Separate the playback interface from both frontends so the engine and playback policy remain reusable.
