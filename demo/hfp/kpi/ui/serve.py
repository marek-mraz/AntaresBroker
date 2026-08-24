#!/usr/bin/env python3
"""Live HFP map + KPI panel, with a tenant-aware same-origin proxy.

Serves index.html on 0.0.0.0:42030 (host-published range) and proxies
    /api/<tenant>/<path>  ->  http://localhost:42020/ngsi-ld/v1/<path>
with the NGSILD-Tenant header set from the path segment. The broker sends no
CORS headers and lives outside the published range, so the proxy is load-bearing
for both reasons.

Sibling of ../map/serve.py, which is single-tenant and points at the compose
stack on 42010. This one drives the two-tenant demo: `helsinki` holds the live
vehicles and arrival events, `helsinki-kpi` holds the computed KPIs.
"""
import http.server
import urllib.error
import urllib.request
from pathlib import Path

BROKER = "http://localhost:42020/ngsi-ld/v1"
ROOT = Path(__file__).parent
PORT = 42030


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path.startswith("/api/"):
            rest = self.path[len("/api/"):]
            tenant, _, tail = rest.partition("/")
            url = f"{BROKER}/{tail}"
            headers = {"Accept": "application/json"}
            # The default tenant is addressed by omitting the header entirely;
            # sending an empty one would be a lookup for a tenant named "".
            if tenant and tenant != "-":
                headers["NGSILD-Tenant"] = tenant
            try:
                req = urllib.request.Request(url, headers=headers)
                with urllib.request.urlopen(req, timeout=20) as r:
                    body, status = r.read(), r.status
                    ctype = r.headers.get("Content-Type", "application/json")
            except urllib.error.HTTPError as e:
                # A 404 NonexistentTenant is normal before the first KPI lands;
                # pass it through so the UI can show "waiting" rather than break.
                body, status, ctype = e.read(), e.code, "application/json"
            except Exception as e:  # noqa: BLE001 - surface proxy failures as 502
                self.send_error(502, str(e))
                return
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
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
    print(f"live KPI demo on http://0.0.0.0:{PORT}  (broker {BROKER})", flush=True)
    http.server.ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
