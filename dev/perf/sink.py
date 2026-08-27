#!/usr/bin/env python3
"""Notification and registration sink for the load rigs.

    python3 dev/perf/sink.py 9800

Counts every POST (notifications from subscriptions), the entities in
their data arrays and the distinct subscriptionIds they carry and answers every
GET with an empty entity list (a registered context source that holds
nothing, so forwarded queries cost the broker the fan-out and nothing
else). `GET /stats` returns the counters as JSON; `DELETE /stats` resets
them. Threaded, so a slow client never stalls the broker's delivery.
"""

import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

lock = threading.Lock()
stats = {"posts": 0, "entities": 0, "bytes": 0, "gets": 0, "first": None, "last": None}
subscriptions = set()
by_sub = {}   # subscriptionId -> entities delivered (fire.sh folds per class)
csr_gets = {}  # /csr/<k> -> calls (the federated-query stage)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def _json(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n)
        now = time.time()
        sid, items = None, 0
        try:
            doc = json.loads(body)
            sid = doc.get("subscriptionId")
            items = len(doc.get("data") or [])
        except Exception:
            pass
        with lock:
            if sid:
                subscriptions.add(sid)
                by_sub[sid] = by_sub.get(sid, 0) + items
            stats["posts"] += 1
            stats["entities"] += items
            stats["bytes"] += n
            stats["first"] = stats["first"] or now
            stats["last"] = now
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path == "/stats":
            with lock:
                s = dict(stats)
                s["subscriptions"] = len(subscriptions)
                s["by_sub"] = dict(by_sub)
                s["csr_gets"] = dict(csr_gets)
            span = (s["last"] - s["first"]) if s["first"] and s["last"] and s["last"] > s["first"] else 0
            s["posts_per_second"] = round(s["posts"] / span, 1) if span else None
            return self._json(200, s)
        with lock:
            stats["gets"] += 1
            if self.path.startswith("/csr/"):
                key = self.path.split("?", 1)[0]
                csr_gets[key] = csr_gets.get(key, 0) + 1
        self._json(200, [])

    def do_DELETE(self):
        with lock:
            stats.update(posts=0, entities=0, bytes=0, gets=0, first=None, last=None)
            subscriptions.clear()
            by_sub.clear()
            csr_gets.clear()
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9800
    ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
