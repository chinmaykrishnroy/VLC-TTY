"""Command line entry: python -m termvlc movie.mp4"""
import argparse
import sys
from .engine import LibVLC, LibVLCError
from .app import App
from .terminal import Terminal


def main(argv=None):
    parser = argparse.ArgumentParser(prog="termvlc", description="VLC-powered video playback, directly in your terminal")
    parser.add_argument("source", nargs="?", help="Media path, folder, or http(s)/rtsp streaming URL")
    parser.add_argument("--fps", type=int, default=12, help="Terminal refresh limit (default 12)")
    parser.add_argument("--decode-width", type=int, default=224, help="LibVLC video callback width (64 to 512; default 224)")
    parser.add_argument("--ascii", action="store_true", help="Monochrome ASCII mode for limited terminals")
    parser.add_argument("--doctor", action="store_true", help="Check if the LibVLC runtime is discoverable")
    args = parser.parse_args(argv)
    if not 1 <= args.fps <= 30:
        parser.error("--fps must be between 1 and 30")
    try:
        if args.doctor:
            with LibVLC(decode_width=args.decode_width) as player:
                version = player.lib.libvlc_get_version().decode("utf-8", errors="replace")
                print(f"LibVLC: OK ({version})")
                print("Runtime library:", player.lib._name)
                print("Terminal:", "interactive" if sys.stdout.isatty() else "non-interactive")
                print("Note: runtime detection does not validate video decoding or audio output.")
            return 0
        with LibVLC(decode_width=args.decode_width) as player:
            app = App(player, args.source, fps=args.fps, ascii_mode=args.ascii)
            with Terminal() as term:
                app.run(term)
        return 0
    except (LibVLCError, RuntimeError, OSError, KeyboardInterrupt) as exc:
        print(f"termvlc: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
