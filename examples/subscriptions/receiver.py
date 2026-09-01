#!/usr/bin/env python3
"""Prints every NGSI-LD notification it receives. Usage: receiver.py [port]"""
import json, sys
from http.server import HTTPServer, BaseHTTPRequestHandler

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        print(f"notification {body['id']}: "
              + ", ".join(f"{e['id']} -> {k}={v.get('value')}"
                          for e in body["data"]
                          for k, v in e.items() if isinstance(v, dict)))
        sys.stdout.flush()
        self.send_response(200); self.end_headers()
    def log_message(self, *a): pass

# All interfaces, not loopback: a broker running in the quickstart
# container reaches this sink from outside the host network namespace.
HTTPServer(("0.0.0.0", int(sys.argv[1]) if len(sys.argv) > 1 else 9491), H).serve_forever()
