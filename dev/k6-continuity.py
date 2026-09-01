#!/usr/bin/env python3
"""The continuity harness every availability drill runs under.

One process does all three jobs:
  * writer   — POSTs monotonically numbered entities against BASE_URL at a
               steady rate, records every id the broker ACKED (201);
  * receiver — an HTTP server recording every notification delivered to it
               (the drill creates a subscription pointing here);
  * auditor  — at the end: re-reads every acked id and reports.

Assertions (exit 1 on any violation):
  1. zero connection errors and zero 5xx on writes — the drill's promise is
     "a roll is invisible"; a refused connection IS the bug the drain
     contract exists to fix;
  2. zero lost writes — every acked id is retrievable afterwards;
  3. notifications at-least-once — every acked id was seen by the receiver
     (duplicates fine, losses not). Enabled with --expect-notifications once
     the drill has created the subscription (drills without one skip it).

Usage:
  dev/k6-continuity.py --base http://localhost:9090 --seconds 30 \
      [--rate 20] [--listen-port 9299] [--expect-notifications] \
      [--type ContinuityProbe]

The drill script starts this, performs its chaos (roll, SIGTERM, SIGKILL),
then waits for the exit code. stdlib only.
"""
import argparse
import http.server
import json
import sys
import threading
import time
import urllib.error
import urllib.request

NS = "https://uri.etsi.org/ngsi-ld/default-context/"


class Receiver(http.server.BaseHTTPRequestHandler):
    seen = set()
    lock = threading.Lock()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try:
            data = json.loads(body)
            for e in data.get("data", []):
                with Receiver.lock:
                    Receiver.seen.add(e.get("id", ""))
        except json.JSONDecodeError:
            pass
        self.send_response(200)
        self.end_headers()

    def log_message(self, *_):
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--seconds", type=int, default=30)
    ap.add_argument("--rate", type=float, default=20.0, help="writes/second")
    ap.add_argument("--listen-port", type=int, default=9299)
    ap.add_argument("--expect-notifications", action="store_true")
    ap.add_argument("--type", default="ContinuityProbe")
    ap.add_argument("--run-id", default=str(int(time.time())))
    args = ap.parse_args()

    # All interfaces on purpose: the broker under drill runs in a container
    # and delivers to the host, so a loopback bind would never be reached.
    srv = http.server.ThreadingHTTPServer(("0.0.0.0", args.listen_port), Receiver)
    threading.Thread(target=srv.serve_forever, daemon=True).start()

    acked, conn_errors, http_5xx = [], [], []
    deadline = time.monotonic() + args.seconds
    seq = 0
    period = 1.0 / args.rate
    while time.monotonic() < deadline:
        t0 = time.monotonic()
        eid = f"urn:ngsi-ld:{args.type}:{args.run_id}:{seq:06d}"
        body = json.dumps(
            {"id": eid, "type": args.type, "seq": {"type": "Property", "value": seq}}
        ).encode()
        req = urllib.request.Request(
            f"{args.base}/ngsi-ld/v1/entities",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                if resp.status == 201:
                    acked.append(eid)
                elif resp.status >= 500:
                    http_5xx.append((eid, resp.status))
        except urllib.error.HTTPError as e:
            # 4xx here is a harness bug, count it loudly as well
            (http_5xx if e.code >= 500 else conn_errors).append((eid, e.code))
        except (urllib.error.URLError, ConnectionError, TimeoutError) as e:
            conn_errors.append((eid, str(e)))
        seq += 1
        time.sleep(max(0.0, period - (time.monotonic() - t0)))

    # 2. every acked id must be retrievable
    lost = []
    for eid in acked:
        req = urllib.request.Request(f"{args.base}/ngsi-ld/v1/entities/{eid}")
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                if resp.status != 200:
                    lost.append(eid)
        except Exception:
            lost.append(eid)

    # 3. at-least-once delivery, when the drill armed a subscription
    missing_notifications = []
    if args.expect_notifications:
        time.sleep(3)  # let in-flight deliveries land
        with Receiver.lock:
            seen = set(Receiver.seen)
        missing_notifications = [e for e in acked if e not in seen]

    print(
        f"k6: {seq} writes, {len(acked)} acked, {len(conn_errors)} conn-errors, "
        f"{len(http_5xx)} 5xx, {len(lost)} lost, "
        f"{len(missing_notifications)} unnotified"
    )
    for label, bad in (
        ("conn-error", conn_errors[:10]),
        ("5xx", http_5xx[:10]),
        ("lost", lost[:10]),
        ("unnotified", missing_notifications[:10]),
    ):
        for item in bad:
            print(f"  {label}: {item}", file=sys.stderr)
    ok = not conn_errors and not http_5xx and not lost and not missing_notifications
    # an idle run proves nothing — demand real traffic got through
    if not acked:
        print("k6: zero acked writes — the harness never reached the broker", file=sys.stderr)
        ok = False
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
