# Experimental SSH audio

## Prefer original-file streaming for remote music

```sh
vlc-tty stream roy@pi '/home/roy/Music/Inn Lamhon Ke Daaman Mein.mp3'
```

`stream` transfers the original compressed bytes through an SSH local forward into local LibVLC. The local player handles decoding, audio, volume, pause and seeking. It needs OpenSSH locally and Python 3 on a Linux/macOS server, rather than a matching remote VLC-TTY build. A temporary single-file HTTP range server binds only to remote loopback, serves a random per-session URL and exits when its SSH input pipe closes. It does not expose a public listener, write a remote helper file, or transcode media. Local LibVLC may read ahead; this is not a constant-bit-rate transport. Only one regular file is supported; remote directory/playlist expansion and TUI artwork/tag extraction from network media remain open.

`connect` has a different purpose: the server runs LibVLC and renders the terminal UI, and PCM is forwarded to the client's audio device. It remains experimental. For a remote MP3, `stream` avoids that live PCM delivery path altogether.

## Server-rendered playback

The audio subsystem is experimental. Existing downloaded executables may not contain these commands. This is not a claim of validated lip-sync, movie-length reliability, or Linux/macOS speaker acceptance. Windows device output and a short Pi-to-Windows tunneled session have been exercised; see [validation](audio-and-fullscreen-validation.md).

Both client and server need an audio-capable VLC-TTY build. The server can use `cargo build --locked --release --no-default-features --bin vlc-tty`; this omits CPAL and local audio-library build dependencies, while retaining LibVLC PCM capture and transmission. Desktop clients use the default `audio-client` feature. Linux desktop builds require `libasound2-dev` at build time.

```sh
vlc-tty connect roy@pi /path/on/pi/movie.mkv --graphics ansi
vlc-tty connect --help
```

The helper opens the local default audio device, creates a random session key, transfers it through a non-PTY SSH setup request into a private remote temporary file, and starts an interactive SSH connection. Video and PCM travel through separate channels of that connection. Audio uses a reverse loopback tunnel. OpenSSH handles saved keys, host-key checks, terminal input and resize handling. No public audio listener is needed. A randomly chosen remote port can collide; forwarding fails explicitly rather than falling back to an exposed address. The remote player must already be installed; this command does not upload or update it.

Cleanup removes the remote session file on normal exit and attempts removal after setup/startup failures. If both connections are forcibly killed or the network disappears before cleanup, a private file can remain in remote `/tmp`; remove stale `vlc-tty-audio.*` files owned by your account after confirming no session uses them. Session keys are never placed in process arguments or terminal output.

## Output modes

`--audio-output auto|local|remote|disabled` resolves before LibVLC starts. Local auto uses the local device. Direct SSH auto remains silent and explains how to use the helper. `--silent` selects disabled; `--audio` selects the server/local device. Smoke tests always disable audio. Remote output requires `--audio-endpoint` and `--audio-token-file`; it never starts with `--no-audio`.

For a manually managed tunnel, securely share a fresh 64-character hexadecimal key file between the two machines, restrict it to your account, then run:

```sh
# Client
vlc-tty audio-receive --listen 127.0.0.1:47650 --token-file client-session.key
# Separate client terminal; server-session.key is the matching private server file
ssh -tt -o ExitOnForwardFailure=yes \
  -R 127.0.0.1:47650:127.0.0.1:47650 roy@pi \
  'vlc-tty --ssh --audio-output remote --audio-endpoint 127.0.0.1:47650 --audio-token-file server-session.key movie.mkv'
```

The helper accepts `--remote-command PATH` after MEDIA for an alternate installed server executable. Use `--silent` to bypass local receiver/tunnel setup for silent playback, or `--audio-output local` for server speakers.

The receiver accepts one authenticated session. Endpoints must be numeric loopback addresses. Do not reuse session keys; remove manual key files yourself.

## Pipeline and limits

LibVLC supplies S16N 48 kHz stereo into a preallocated callback pool. Bounded queues move it to a network worker, which applies the requested gain and serializes S16LE with explicit version, format, length, generation and playback timestamps. The UI reports the same requested gain instead of the custom native output's unsupported volume readout. Already transmitted samples retain their prior gain until consumed; changing volume does not flush/reprime playback. Pause, seek, media changes, rate changes and track switches invalidate queued audio. The receiver anchors a continuous timestamp timeline once per generation with a provisional 400 ms jitter allowance, then lets the device clock consume contiguous samples rather than inserting silence when a callback arrives slightly early. Retained ready audio is bounded to 750 ms plus the active block, using a fixed 128-block pool. It converts to the device's sample format/rate and supplies silence when data is unavailable. Neither callback performs network I/O. Receiver telemetry reports queue depth, underrun callbacks and estimated audio presentation time using CPAL callback/playback timestamps. A bounded server video history compensates for estimated audio lag and recent encoder time; this is not a physical speaker or terminal presentation measurement. The increased audio buffer can exceed the existing 250 ms video-delay correction limit, so server-rendered A/V alignment is explicitly unvalidated.

PCM needs approximately 192 kB/s before framing/SSH overhead. Device failure or tunnel loss stops playback explicitly. Automatic device switching, reconnect/resume, adaptive jitter tuning and externally measured A/V alignment remain acceptance work. A shared LibVLC clock alone does not establish synchronized terminal video and local speakers.

Required acceptance includes Pi-to-Windows video/sound, repeated pause/seek/track/rate changes, resize/fullscreen, device loss, network jitter/disconnection, Linux/macOS clients, and a 60-minute run with bounded memory and measured A/V offset. Keep these gates visible until their evidence exists.

API references: [LibVLC callbacks](https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html), [CPAL](https://docs.rs/cpal/0.16.0/cpal/), [OpenSSH forwarding](https://man.openbsd.org/ssh).
