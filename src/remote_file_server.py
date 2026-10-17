"""Single-file, loopback-only range server whose lifetime is its SSH stdin pipe."""
import http.server
import os
import re
import stat
import sys
import threading
import urllib.parse


def serve(path, token, port):
    descriptor = os.open(os.path.expanduser(path), os.O_RDONLY)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("stream requires a regular file")
        size = info.st_size
        route = "/" + token + "/" + urllib.parse.quote(os.path.basename(path), safe="")

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_HEAD(self):
                self.respond(False)

            def do_GET(self):
                self.respond(True)

            def respond(self, body):
                if self.path != route:
                    self.send_error(404)
                    return
                start, end = 0, size - 1
                value = self.headers.get("Range")
                if value is not None:
                    match = re.fullmatch(r"bytes=(\d*)-(\d*)", value.strip())
                    if not match or not any(match.groups()):
                        self.send_error(400, "A single byte range is required")
                        return
                    first, last = match.groups()
                    if first:
                        start = int(first)
                        end = min(int(last), end) if last else end
                    else:
                        start = max(0, size - int(last))
                    if start > end or start >= size:
                        self.send_response(416)
                        self.send_header("Content-Range", f"bytes */{size}")
                        self.send_header("Content-Length", "0")
                        self.end_headers()
                        return
                length = max(0, end - start + 1)
                self.send_response(206 if value is not None else 200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Length", str(length))
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Connection", "close")
                self.send_header("Cache-Control", "no-store")
                if value is not None:
                    self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
                self.end_headers()
                if body:
                    try:
                        while length:
                            chunk = os.pread(descriptor, min(length, 65536), start)
                            if not chunk:
                                break
                            self.wfile.write(chunk)
                            start += len(chunk)
                            length -= len(chunk)
                    except (BrokenPipeError, ConnectionResetError):
                        pass  # LibVLC closes old reads when seeking.

        server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        print("READY", flush=True)
        try:
            sys.stdin.buffer.read(1)  # EOF on local exit or SSH disconnect.
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
    finally:
        os.close(descriptor)


if __name__ == "__main__":
    serve(sys.argv[1], sys.argv[2], int(sys.argv[3]))
