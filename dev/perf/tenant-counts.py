#!/usr/bin/env python3
"""What the broker holds, from its own tenant inventory.

    python3 dev/perf/tenant-counts.py http://127.0.0.1:9090          # totals
    python3 dev/perf/tenant-counts.py http://127.0.0.1:9090 t7       # one tenant

`GET /q/tenants` answers names only, so the counts come from
`GET /q/tenants/{tenant}`, fetched over a small pool. Prints
`entities subscriptions registrations tenants` for the totals form and
`entities subscriptions registrations` for one tenant; a tenant that
answers anything but 200 counts as zeros.
"""

import json
import sys
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

KINDS = ("entities", "subscriptions", "registrations")


def get(url):
    with urllib.request.urlopen(url, timeout=30) as r:
        return json.load(r)


def counts(base, tenant):
    try:
        return get(f"{base}/q/tenants/{tenant}").get("counts", {})
    except (urllib.error.HTTPError, urllib.error.URLError, ValueError):
        return {}


def main():
    base = sys.argv[1].rstrip("/")
    if len(sys.argv) > 2:
        c = counts(base, sys.argv[2])
        print(" ".join(str(int(c.get(k, 0))) for k in KINDS))
        return
    names = get(f"{base}/q/tenants")
    with ThreadPoolExecutor(max_workers=16) as pool:
        rows = list(pool.map(lambda t: counts(base, t), names))
    print(" ".join(str(sum(int(c.get(k, 0)) for c in rows)) for k in KINDS), len(names))


if __name__ == "__main__":
    main()
