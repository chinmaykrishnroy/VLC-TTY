# Objective

Make media playback useful inside a terminal, including files stored on a machine with no display. The original target was silent video from a Raspberry Pi 5 running Ubuntu Server over SSH. Ordinary remote files now use compressed-file streaming into a local player; server-rendered terminal playback remains available.

VLC-TTY combines LibVLC decoding with Ratatui controls and terminal graphics. Familiar VLC controls help users find their way; the product is a terminal player.

## First usable release

- Open local files, folders and network URLs.
- Play, pause, stop, seek, change speed and step through frames.
- Navigate a playback queue independently of file browsing.
- Display external SRT/VTT captions and select available media tracks.
- Render through Kitty, Sixel or iTerm2, with ANSI half-block fallback.
- Run silently without an audio device, desktop session, X11 or Wayland.
- Stream original remote file bytes over SSH with local decoding, audio and seeking.
- Ship tested native portable executables with embedded LibVLC and matching plugins.

## Acceptance

A real Pi 5 session must decode and display video through SSH, respond to controls, resize without stale images, show captions, exit cleanly and remain usable during sustained playback. Record terminal, media, output size, network conditions and observed performance. A cloud ARM64 test cannot establish these device results.

For remote-file streaming, verify local controls, tags, supported embedded artwork, seeking and tunnel cleanup. For server-rendered playback with forwarded audio, separately measure speaker/video alignment and continuity through seeks, pauses and network jitter. Short device checks and server encoder counters do not establish full-length movie acceptance.

## Boundaries

Plain SSH playback defaults to silence. The implemented `connect` helper forwards PCM to client speakers experimentally; its physical A/V synchronization remains unvalidated. Prefer `stream` for ordinary remote files. The project does not promise all VLC desktop features, every terminal, hardware-accelerated callbacks or full-resolution video at source frame rate. Public publication remains gated on runtime source correspondence and distribution checks.
