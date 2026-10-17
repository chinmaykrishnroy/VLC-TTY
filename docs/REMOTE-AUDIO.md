# Experimental SSH audio

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

LibVLC supplies S16N 48 kHz stereo into a preallocated callback pool. Bounded queues move it to a network worker, which serializes S16LE with explicit version, format, length, generation and playback timestamps. Pause, seek, media changes, rate changes and track switches invalidate queued audio. The receiver rejects stale generations, schedules samples with a provisional 100 ms jitter allowance, converts to the device's sample format/rate, and supplies silence when data is unavailable. Neither callback performs network I/O. Receiver telemetry reports queue depth, underrun callbacks and estimated audio presentation time using CPAL callback/playback timestamps. A bounded server video history compensates for estimated audio lag and recent encoder time; this is not a physical speaker or terminal presentation measurement.

PCM needs approximately 192 kB/s before framing/SSH overhead. Device failure or tunnel loss stops playback explicitly. Automatic device switching, reconnect/resume, adaptive jitter tuning and externally measured A/V alignment remain acceptance work. A shared LibVLC clock alone does not establish synchronized terminal video and local speakers.

Required acceptance includes Pi-to-Windows video/sound, repeated pause/seek/track/rate changes, resize/fullscreen, device loss, network jitter/disconnection, Linux/macOS clients, and a 60-minute run with bounded memory and measured A/V offset. Keep these gates visible until their evidence exists.

API references: [LibVLC callbacks](https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html), [CPAL](https://docs.rs/cpal/0.16.0/cpal/), [OpenSSH forwarding](https://man.openbsd.org/ssh).
