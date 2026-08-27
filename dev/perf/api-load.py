#!/usr/bin/env python3
"""Create subscriptions and context source registrations through the API.

    python3 dev/perf/api-load.py subscriptions --count 100000 --tenants 10000 \\
        --broker http://localhost:9090 --sink http://127.0.0.1:9800 [--mqtt mqtt://localhost:1883]
    python3 dev/perf/api-load.py registrations --count 100000 --tenants 10000 ...

Neither resource has a batch operation (5.6.7 and 5.6.8 exist for
entities only), so this is one POST per resource over a thread pool.
Subscription k watches type Vehicle with `q=speed>100` for tenant
t(k mod tenants) and notifies the sink; every tenth one notifies over
MQTT when --mqtt is given. Registration k claims
`urn:ngsi-ld:Vehicle:t<k mod tenants>:<k>.*` at the sink, so a query
touches only the registrations whose id pattern can match.

Prints created / failed counts and the rate; exits 1 when anything but
201 came back. Ids are deterministic, so a re-run is a 409 storm, not a
duplicate set: pass --offset to continue a partial load.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

CTX = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"


def subscription(k, tenants, sink, mqtt):
    uri = f"{mqtt}/antares/perf/{k}" if mqtt and k % 10 == 0 else f"{sink}/n/{k}"
    return f"t{k % tenants}", {
        "id": f"urn:ngsi-ld:Subscription:perf:{k}",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "q": "speed>100",
        "notification": {"endpoint": {"uri": uri, "accept": "application/json"}},
        "@context": CTX,
    }


def registration(k, tenants, sink, _mqtt):
    tenant = f"t{k % tenants}"
    return tenant, {
        "id": f"urn:ngsi-ld:ContextSourceRegistration:perf:{k}",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle", "idPattern": f"urn:ngsi-ld:Vehicle:{tenant}:{k}.*"}]}],
        "endpoint": f"{sink}/csr/{k}",
        "@context": CTX,
    }


KINDS = {
    "subscriptions": (subscription, "/ngsi-ld/v1/subscriptions"),
    "registrations": (registration, "/ngsi-ld/v1/csourceRegistrations"),
}


def post(url, tenant, body):
    req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                 headers={"Content-Type": "application/ld+json", "NGSILD-Tenant": tenant})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code
    except Exception:
        return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("kind", choices=KINDS)
    ap.add_argument("--count", type=int, required=True)
    ap.add_argument("--tenants", type=int, default=1)
    ap.add_argument("--offset", type=int, default=0)
    ap.add_argument("--broker", default="http://localhost:9090")
    ap.add_argument("--sink", default="http://127.0.0.1:9800")
    ap.add_argument("--mqtt", default=None)
    ap.add_argument("--threads", type=int, default=32)
    a = ap.parse_args()
    make, path = KINDS[a.kind]
    url = a.broker + path

    def one(k):
        tenant, body = make(k, a.tenants, a.sink, a.mqtt)
        return post(url, tenant, body)

    t0 = time.time()
    with ThreadPoolExecutor(a.threads) as ex:
        codes = list(ex.map(one, range(a.offset, a.offset + a.count)))
    dt = time.time() - t0
    ok = codes.count(201)
    bad = {}
    for c in codes:
        if c != 201:
            bad[c] = bad.get(c, 0) + 1
    print(f"{a.kind}: {ok}/{a.count} created in {dt:.1f} s ({ok / dt if dt else 0:.0f}/s)"
          + (f", failed by status {bad}" if bad else ""))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
