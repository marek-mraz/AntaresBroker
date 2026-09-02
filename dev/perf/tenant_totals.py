#!/usr/bin/env python3
"""What the broker holds, summed over its tenants.

    python3 dev/perf/tenant_totals.py http://127.0.0.1:9090
    3000000 10000 10000 100      # entities subscriptions registrations tenants

`GET /q/tenants` lists names and nothing else: at the 10 000-tenant target
a list carrying per-kind counts would cost a count per kind per tenant. The
counts come from `GET /q/tenants/{tenant}`, one request per name.
"""

import json
import sys
import urllib.parse
import urllib.request

KINDS = ("entities", "subscriptions", "registrations")


def get(url):
    with urllib.request.urlopen(url, timeout=30) as r:
        return json.load(r)


def main():
    base = sys.argv[1].rstrip("/")
    names = get(f"{base}/q/tenants")
    total = dict.fromkeys(KINDS, 0)
    for name in names:
        counts = get(f"{base}/q/tenants/{urllib.parse.quote(name, safe='')}").get(
            "counts", {}
        )
        for kind in KINDS:
            total[kind] += int(counts.get(kind, 0))
    print(*(total[k] for k in KINDS), len(names))


main()
