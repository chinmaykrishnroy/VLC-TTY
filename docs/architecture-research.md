# TermVLC architecture assessment

Researched 16 October 2026 against the current source and primary documentation. These are architecture recommendations; they do not establish that a feature works on a physical device or terminal.

## Recommendation

The user clarified on 17 October 2026 that the product's purpose is video playback **inside a terminal**. Continue with **Ratatui, terminal image protocols and prebuilt LibVLC 3 shared libraries** as the primary architecture. Adapt familiar VLC controls and workflows to terminal use. Prioritize terminal rendering performance, captions, input responsiveness and playback quality. The existing separate native-video-window mode remains an optional fallback.

| Option | Assessment for this project |
| --- | --- |
| Prebuilt LibVLC DLL/shared library | Best initial choice: reuse VLC's playback engine and expand the application controls rather than rebuilding demuxing, decoding and playback management. Pin a tested ABI/runtime and ship or locate a coherent runtime. |
| Compile VLC/LibVLC source | Appropriate when a documented engine defect requires a patch, or when reproducible custom runtime builds become necessary. It does not itself reproduce VLC's desktop interface or remove terminal rendering costs. It adds dependency builds and patch maintenance. |
| FFmpeg libraries | Good building blocks when custom media processing is the main requirement. Replacing LibVLC would require implementing more player policy, rendering, subtitle handling and synchronization; it is a substantial expansion of scope. Use later for specific processing tasks if needed. |

LibVLC is explicitly intended as an embeddable engine, with runtime plugins; VideoLAN currently describes major version 3 as stable and 4 as preview/development. The FFmpeg project describes separate codec, format, filtering, scaling and resampling libraries and its SDL-based `ffplay` player. VLC's own Windows build instructions describe building its core, modules, LibVLC and a large set of dependencies, including FFmpeg. This also shows why “VLC versus FFmpeg” is not a simple either/or choice. Sources: [VideoLAN LibVLC overview](https://images.videolan.org/vlc/libvlc.html), [FFmpeg overview](https://ffmpeg.org/about.html), [official Windows build guide](https://github.com/videolan/vlc/blob/master/doc/BUILD-win32.md). The build guide describes the development branch; it is evidence of build complexity, not a claim about the exact steps for a pinned 3.x release.

## The main constraint is the display path

Ratatui renders a grid of character cells through a terminal backend. This can reproduce familiar menu names, controls, timeline structure and keyboard workflows, but a cell grid is a different canvas from a native desktop GUI. Exact widget geometry, typography and integrated GPU video are not capabilities that Ratatui alone supplies. The current project adds terminal image protocols through `ratatui-image` and a Sixel encoder. Sources: [Ratatui rendering model](https://ratatui.rs/concepts/rendering/under-the-hood/), current `Cargo.toml` and `src/graphics.rs`. The product suitability judgment is an inference from these constraints.

The current inline path is `LibVLC -> RV32 CPU memory -> Rust frame copy -> scaling/image encoding -> terminal output`. LibVLC's own API warns that custom-memory callbacks are substantially less efficient than native window output: acceleration may be disabled or require a GPU-to-CPU copy, subtitle compositing and conversions may use the CPU, and extra copying is required. Switching decoder libraries cannot remove the terminal transport/encoding stage. For desktop-like playback, use the platform's native LibVLC window embedding API rather than treating terminal image frames as a GPU video surface. Source: [LibVLC 3 media-player API, video callbacks and platform window handles](https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html).

For subtitles, that same API explicitly discusses CPU compositing of subtitles into callback frames. Therefore, callbacks do **not inherently prove that embedded subtitles are unavailable**. This repository's README correctly exposes a practical limitation, but the actual failure must be reproduced and traced. Do not commit to rebuilding ASS/SSA/bitmap subtitle rendering before testing LibVLC's compositor in the current callback path. The separate local SRT/VTT parser is not equivalent to all subtitle formats and styling supported by VLC. Source: [LibVLC callback documentation](https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html), current `README.md`.

The application receives display callbacks when VLC's playback clock says to present a frame, but it then encodes/transfers that frame separately. As an architectural inference, audio/video synchronization inside LibVLC does not by itself guarantee that the delayed terminal image remains synchronized. Measure terminal presentation delay and dropped frames under load before claiming VLC-level playback quality.

## Runtime packaging and compatibility

Ship or locate a **matching runtime set**, rather than copying only `libvlc.dll`: Windows output includes `libvlccore.dll` and the plugin hierarchy. Keep architecture, version and plugin paths coherent; verify startup on a clean machine without a developer's PATH. Source: [official Windows build/runtime layout](https://github.com/videolan/vlc/blob/master/doc/BUILD-win32.md). The current `src/vlc.rs` already performs dynamic loading and attempts plugin discovery; this needs clean-system testing rather than a backend rewrite.

The adapter manually declares C function pointers and currently targets the LibVLC 3 ABI. Pin and report the loaded runtime, check its version, and handle optional functions deliberately. Do not assume that successfully loading some familiar symbols establishes compatibility with a later major version. Source: current `src/vlc.rs`; [VideoLAN's stable/development major-version description](https://images.videolan.org/vlc/libvlc.html).

## Licensing and product identity

TermVLC's MIT code does not determine the license obligations for a bundled media runtime. VideoLAN distinguishes its LGPL engine from the GPL desktop player, and its engine documentation notes that some plugins have more restrictive licenses. Record the exact runtime/plugins and their notices and source obligations before distributing a bundle. Copying desktop VLC implementation code is a different decision from linking to LibVLC. Sources: [VideoLAN relicensing statement](https://images.videolan.org/press/lgpl-libvlc.html), [current LibVLC source documentation](https://github.com/videolan/vlc/blob/master/doc/standalone/libvlc.md), [VLC source README](https://github.com/videolan/vlc).

FFmpeg is not an automatic licensing shortcut: its official legal guidance distinguishes LGPL builds from builds enabling GPL components, and the exact build configuration matters. Source: [FFmpeg license guidance](https://ffmpeg.org/legal.html).

Branding is separate from code licensing. VideoLAN's official relicensing FAQ expressly rejects using the VLC name for another application. The present name and cone-inspired branding deserve review before public release; a non-affiliation sentence does not itself establish permission. Use an independent identity and describe the engine factually, or obtain applicable permission. This is a release consideration, not a legal determination about this project's present name. Source: [VideoLAN's name-use FAQ](https://images.videolan.org/press/lgpl-libvlc.html).

## First product scope

Prioritize reliable file/network opening, play/pause/stop, seek and accurate progress, volume/mute, audio and subtitle track selection, external subtitles with delay, playback speed, stable fullscreen, queue/next/previous, repeat/shuffle, persisted settings and resume history. This is a recommended starting scope based on the stated product goal, not a measured ranking of VLC users' habits.

Require a real playback matrix before claiming replacement quality: common containers/codecs; multiple audio and subtitle types; seek/pause/stop/open transitions; playlist boundaries; long sessions; resizing/fullscreen; Unicode paths; missing runtime/plugins; malformed media; live streams; and at least one terminal per supported graphics protocol. Advanced DVD/Blu-ray navigation, conversion/streaming, capture, casting, HDR/color fidelity and effects can follow after the everyday playback path is reliable.
