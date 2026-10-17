# Objective

Make video playback useful inside a terminal, including a remote shell on a machine with no display. The first target is silent video from a Raspberry Pi 5 running Ubuntu Server over SSH.

VLC-TTY combines LibVLC decoding with Ratatui controls and terminal graphics. Familiar VLC controls help users find their way; the product is a terminal player.

## First usable release

- Open local files, folders and network URLs.
- Play, pause, stop, seek, change speed and step through frames.
- Navigate a playback queue independently of file browsing.
- Display external SRT/VTT captions and select available media tracks.
- Render through Kitty, Sixel or iTerm2, with ANSI half-block fallback.
- Run silently without an audio device, desktop session, X11 or Wayland.
- Ship tested native application binaries and clear runtime installation instructions.

## Acceptance

A real Pi 5 session must decode and display video through SSH, respond to controls, resize without stale images, show captions, exit cleanly and remain usable during sustained playback. Record terminal, media, output size, network conditions and observed performance. A cloud ARM64 test cannot establish these device results.

## Boundaries

The initial release does not forward audio to SSH client speakers. It does not promise all VLC desktop features, every terminal, hardware-accelerated callbacks or full-resolution video at source frame rate. Prefer predictable behavior and explicit failures over unsupported claims.
