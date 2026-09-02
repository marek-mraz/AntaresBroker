#!/usr/bin/env python3
"""What the broker holds, summed over its tenants.

    python3 dev/perf/tenant_totals.py http://127.0.0.1:9090
    3000000 10000 10000 100      # entities subscriptions registrations tenants

`GET /q/tenants` lists names and nothing else: at the 10 000-tenant target
a list carrying per-kind counts would cost a count per kind per tenant. The
counts come from `GET /q/tenants/{tenant}`, one request per name.

That list also carries the tenants the broker mints for its own bookkeeping
(`snap-<uuid>`, `snap-index`, `distsub-index`), so an operator can see one a
deleted snapshot left behind, while the per-tenant resource refuses them —
addressing one would read broker state out from under the resource that owns
it. They hold no loaded data, so a refused name is left out of the totals.
"""

import json
import sys
import urllib.error
import urllib.parse
import urllib.request

KINDS = ("entities", "subscriptions", "registrations")


def get(url):
    with urllib.request.urlopen(url, timeout=30) as r:
        return json.load(r)


def counts(base, name):
    """One tenant's counts, or None for a name the broker does not address."""
    try:
        row = get(f"{base}/q/tenants/{urllib.parse.quote(name, safe='')}")
    except urllib.error.HTTPError as e:
        if e.code in (400, 404):
            return None
        raise
    return row.get("counts", {})


def main():
    base = sys.argv[1].rstrip("/")
    total = dict.fromkeys(KINDS, 0)
    tenants = 0
    for name in get(f"{base}/q/tenants"):
        row = counts(base, name)
        if row is None:
            continue
        tenants += 1
        for kind in KINDS:
            total[kind] += int(row.get(kind, 0))
    print(*(total[k] for k in KINDS), tenants)


main()
