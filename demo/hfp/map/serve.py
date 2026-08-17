#!/usr/bin/env python3
"""Static map UI + same-origin proxy to the Antares HFP broker.

Serves map/index.html on 0.0.0.0:42080 (host-published range) and proxies
/api/* -> http://localhost:42010/ngsi-ld/v1/* (broker port is outside the
published 42080-42099 range and the broker sends no CORS headers).
"""
import http.server
import urllib.request
from pathlib import Path

BROKER = "http://localhost:42010/ngsi-ld/v1"
ROOT = Path(__file__).parent


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/api/"):
            url = BROKER + self.path[len("/api"):]
            try:
                req = urllib.request.Request(url, headers={"Accept": "application/json"})
                with urllib.request.urlopen(req, timeout=20) as r:
                    body = r.read()
                    self.send_response(r.status)
                    self.send_header("Content-Type", r.headers.get("Content-Type", "application/json"))
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
            except urllib.error.HTTPError as e:
                body = e.read()
                self.send_response(e.code)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except Exception as e:
                self.send_error(502, str(e))
            return
        body = (ROOT / "index.html").read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    http.server.ThreadingHTTPServer(("0.0.0.0", 42080), Handler).serve_forever()
