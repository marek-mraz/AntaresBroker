#!/usr/bin/env python3
"""Notification and registration sink for the load rigs.

    python3 dev/perf/sink.py 9800

Counts every POST (notifications from subscriptions), the entities in
their data arrays and the distinct subscriptionIds they carry and answers every
GET with an empty entity list (a registered context source that holds
nothing, so forwarded queries cost the broker the fan-out and nothing
else). `GET /stats` returns the counters as JSON; `DELETE /stats` resets
them. Threaded, so a slow client never stalls the broker's delivery; with a
worker count as the second argument it forks that many processes on the
next ports and the first port only folds their /stats — one CPython process
tops out near 5 000 requests/s, which is below what the broker delivers.
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


class Aggregate(BaseHTTPRequestHandler):
    """The front door of a multi-process sink: /stats folds the workers'
    counters, DELETE /stats resets every worker. Traffic goes to the workers
    directly (ports port+1 … port+N), never here."""
    protocol_version = "HTTP/1.1"
    ports = []

    def log_message(self, *_):
        pass

    def do_GET(self):
        import urllib.request
        if self.path != "/stats":
            self.send_response(404); self.send_header("Content-Length", "0"); self.end_headers(); return
        tot = {"posts": 0, "entities": 0, "bytes": 0, "gets": 0, "first": None, "last": None,
               "subscriptions": 0, "by_sub": {}, "csr_gets": {}}
        for p in self.ports:
            s = json.load(urllib.request.urlopen(f"http://127.0.0.1:{p}/stats"))
            for k in ("posts", "entities", "bytes", "gets", "subscriptions"):
                tot[k] += s[k]
            for k in ("by_sub", "csr_gets"):
                for sid, n in s[k].items():
                    tot[k][sid] = tot[k].get(sid, 0) + n
            if s["first"]:
                tot["first"] = min(tot["first"] or s["first"], s["first"])
            if s["last"]:
                tot["last"] = max(tot["last"] or s["last"], s["last"])
        span = (tot["last"] - tot["first"]) if tot["first"] and tot["last"] and tot["last"] > tot["first"] else 0
        tot["posts_per_second"] = round(tot["posts"] / span, 1) if span else None
        tot["workers"] = len(self.ports)
        data = json.dumps(tot).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_DELETE(self):
        import urllib.request
        for p in self.ports:
            urllib.request.urlopen(urllib.request.Request(f"http://127.0.0.1:{p}/stats", method="DELETE"))
        self.send_response(204); self.send_header("Content-Length", "0"); self.end_headers()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9800
    workers = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    if workers == 0:
        ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
    import os
    Aggregate.ports = [port + 1 + i for i in range(workers)]
    for p in Aggregate.ports:
        if os.fork() == 0:
            ThreadingHTTPServer(("0.0.0.0", p), Handler).serve_forever()
    ThreadingHTTPServer(("0.0.0.0", port), Aggregate).serve_forever()
