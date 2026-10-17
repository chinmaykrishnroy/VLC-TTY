//! Shared help for the thin player and the portable launcher.
pub fn print(portable: bool) {
    println!(
        "VLC-TTY {} — video inside your terminal\n\n\
Usage: vlc-tty [OPTIONS] [FILE|FOLDER|URL]\n\n\
  -h, --help            Show this help and exit\n\
  -V, --version         Show the application version\n\
  --ssh                 Silent SSH profile: 15 FPS, 720p callback cap\n\
  --silent              Disable audio and audio-device requirements\n\
  --audio               Play audio on the server/local machine\n\
  --graphics MODE       auto|kitty|sixel|iterm2|ansi (default: auto)\n\
  --fps N               Draw/encode limit, 1–120 (local default: 60)\n\
  --decode-height N     Callback cap, 0–4320; 0 uncapped (local: 1080)\n\
  --subtitle PATH       Load an external subtitle file\n\
  --playlist PATH       Open a UTF-8 M3U/M3U8 queue (relative to its file)\n\
  --export-playlist PATH Save supplied media/playlist and exit without VLC\n\
  --repeat MODE         off|all|one (default: off)\n\
  --shuffle             Shuffle queue, keeping the current item first\n\
  --volume N            Initial volume, 0–125 (default: 100)\n\
  --rate N              Initial speed, 0.25–4 (default: 1)\n\
  --native-window       Separate video window, local desktop only\n\
  --doctor              Print runtime/session diagnostics without a TUI\n\
  --smoke-test MEDIA    Require decoded frames and an advancing clock\n\
  --                    End options, including for paths starting with -\n\n\
SSH sessions are detected automatically; explicit flags override defaults.\n\
Allocate a terminal with ssh -t. Pixel graphics depend on your client terminal;\n\
ANSI half-blocks are the fallback. SSH does not forward audio.\n\n\
Keyboard: Space pause; s stop; f/F11 fullscreen; arrows seek; +/- volume;\n\
m mute; [/] speed; = normal speed; e next frame; n/p next/previous;\n\
z shuffle; Shift+R repeat off/all/one;\n\
Tab queue; o open; l captions; Shift+S/A track menus; ,/. subtitle delay;\n\
j/k audio delay; g renderer; i info; ? help; q quit.\n\n\
Examples:\n\
  vlc-tty movie.mp4\n\
  vlc-tty --ssh --silent --graphics ansi movie.mkv\n\
  vlc-tty --subtitle movie.en.srt movie.mkv\n\
  vlc-tty --smoke-test movie.mp4",
        env!("CARGO_PKG_VERSION")
    );
    if portable {
        println!("\nPortable options:\n  --bundle-info         Show embedded build/runtime identity\n  --extract-to DIR      Extract player, runtime and notices into a new directory\n\nThe runtime is embedded and extracted privately for each run. No installer.\nVLC_TTY_RUNTIME overrides the bundled runtime for replacement/debugging.\nTMPDIR/TEMP controls extraction location; it must allow executable files.");
    } else {
        println!("\nThis source build requires LibVLC 3.x and matching plugins.\nVLC_HOME overrides runtime discovery; TERMVLC_DECODE_MAX is the legacy height cap.");
    }
}
