"""Exercise the embedded server's real HTTP range and SSH-EOF lifetime contract."""
import http.client
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import unittest
import urllib.parse


@unittest.skipUnless(hasattr(os, "pread"), "Remote server runs on Linux/macOS")
class RemoteFileServerTests(unittest.TestCase):
    def test_single_file_ranges_head_and_pipe_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "song's #1 é.mp3"
            data = bytes(range(256)) * 1024
            path.write_bytes(data)
            with socket.socket() as reserve:
                reserve.bind(("127.0.0.1", 0))
                port = reserve.getsockname()[1]
            server = Path(__file__).resolve().parent.parent / "src/remote_file_server.py"
            process = subprocess.Popen([sys.executable, "-u", str(server), str(path), "private-test-route", str(port)],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.assertEqual(process.stdout.readline(), b"READY\n")
                route = "/private-test-route/" + urllib.parse.quote(path.name, safe="")

                def request(method="GET", target=route, headers=None):
                    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
                    try:
                        connection.request(method, target, headers=headers or {})
                        response = connection.getresponse()
                        return response.status, dict(response.getheaders()), response.read()
                    finally:
                        connection.close()

                self.assertEqual(request()[2], data)
                status, headers, body = request(headers={"Range": "bytes=100-299"})
                self.assertEqual((status, headers["Content-Range"], body),
                                 (206, f"bytes 100-299/{len(data)}", data[100:300]))
                self.assertEqual(request(headers={"Range": "bytes=-32"})[2], data[-32:])
                self.assertEqual(request(headers={"Range": "bytes=262130-"})[2], data[262130:])
                self.assertEqual(request("HEAD")[2], b"")
                self.assertEqual(request(target="/private-test-route/../other.mp3")[0], 404)
                self.assertEqual(request(target="/wrong/song.mp3")[0], 404)
                self.assertEqual(request(headers={"Range": "bytes=999999-"})[0], 416)
                self.assertEqual(request(headers={"Range": "bytes=1-2,5-6"})[0], 400)
                process.stdin.close()
                process.wait(timeout=4)
                self.assertEqual(process.returncode, 0, process.stderr.read().decode())
                with self.assertRaises(OSError):
                    socket.create_connection(("127.0.0.1", port), timeout=1)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=3)
                process.stdout.close()
                process.stderr.close()


if __name__ == "__main__":
    unittest.main()
