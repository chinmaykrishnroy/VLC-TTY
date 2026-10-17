# Video over SSH

The server decodes media and sends terminal graphics to your SSH client. It needs no desktop or connected display. Your client terminal controls the available graphics protocol. Silent playback is the initial target; SSH does not forward media audio.

## Ubuntu Server / Raspberry Pi 5

Use the Linux ARM64 build on a 64-bit Ubuntu installation. Install the runtime without a desktop:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends libvlc5 vlc-plugin-base
```

If packages have no candidate, check that Ubuntu's Universe repository is enabled. Let APT install matching core libraries and plugins.

Extract the matching application archive, place `vlc-tty` in your PATH, and connect from a terminal that supports Kitty, Sixel or iTerm2 graphics:

```sh
ssh -t user@pi 'vlc-tty --ssh --silent /path/to/movie.mp4'
```

An interactive SSH session is detected automatically. `--ssh` is useful when environment variables are filtered. It selects silent audio, 15 FPS and a 720p callback height cap. Explicit flags override these budgets:

```sh
vlc-tty --ssh --fps 10 --decode-height 480 --graphics ansi movie.mp4
vlc-tty --ssh --graphics kitty --subtitle movie.en.srt movie.mkv
```

Start with a small terminal/video area on slow connections. Decoding resolution, image encoding, terminal rendering and network bandwidth all affect smoothness. ANSI mode is lower detail but works without an image protocol. `g` toggles the fallback during playback.

## Diagnose before opening the UI

```sh
vlc-tty --ssh --doctor
vlc-tty --ssh --smoke-test /path/to/movie.mp4
```

The smoke check requires decoded frames and advancing media time; it produces no graphics and exits with an error if decoding fails. Run it without `DISPLAY` or `WAYLAND_DISPLAY` to check headless decoding. This does not test the receiving terminal or SSH throughput.

## Multiplexers and terminal behavior

Direct SSH is the first acceptance path. With tmux, your version/configuration must permit graphics passthrough, for example `set -g allow-passthrough on` where supported. Test the terminal directly before adding a multiplexer. Capability detection can fall back to ANSI; force a protocol only when your client supports it.

Captions loaded as local SRT/VTT are drawn in reserved text rows. Embedded subtitles and image/text ordering need visual checks. Fullscreen changes the TUI layout; it does not operate the client terminal's OS window.

`--audio` opts into sound on the server's device. It does not send sound to your laptop. `--native-window` is incompatible with the SSH profile.

## Device acceptance record

Before claiming Pi support verified, record: Ubuntu version/architecture, LibVLC version, terminal/version, direct SSH or tmux, video codec/resolution, chosen renderer/FPS/cap, network link, CPU/memory/temperature, session duration and observations for pause/seek/resize/captions/exit. See [validation](../VALIDATION.md).
