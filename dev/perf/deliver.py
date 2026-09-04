#!/usr/bin/env python3
"""Delivery throughput of the notification pipeline, without k6.

    python3 dev/perf/deliver.py --seed --tenants 10 --entities 500
    python3 dev/perf/deliver.py --tenants 10 --entities 500 --rate 2000

`fire.sh` measures the same path but needs k6 and a rented runner, so the
pipeline could only be measured by dispatching `scale-weekly`. This drives
it with the standard library alone: it seeds Vehicles the subscription
classes of `api-load.py` can match, PATCHes `speed` at a paced arrival
rate, and reports what reached `sink.py` once delivery goes quiet.

Subscriptions are created by `api-load.py subscriptions`; run that between
the seed and the measurement.

The number to read is `matches_per_second`: one match is one (subscription,
entity) pair, which is the unit of work both matching and delivery scale
with. `changes_per_second` divides by how many subscriptions each change
fires, so it moves when the subscription set changes even though the
pipeline is doing the same amount of work.
"""

import argparse
import http.client
import json
import threading
import time
import urllib.request
from urllib.parse import urlparse

CTX = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
LINK = f'<{CTX}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"'


def conn(url):
    u = urlparse(url)
    return http.client.HTTPConnection(u.hostname, u.port, timeout=30)


def req(c, method, path, body=None, tenant=None):
    h = {"Content-Type": "application/json", "Link": LINK}
    if tenant:
        h["NGSILD-Tenant"] = tenant
    data = json.dumps(body).encode() if body is not None else None
    if data is not None:
        h["Content-Length"] = str(len(data))
    c.request(method, path, data, h)
    r = c.getresponse()
    return r.status, r.read()


def seed(broker, tenants, per_tenant):
    """One batch create per tenant. Scope and location derive from the entity
    number the way gen.py derives them, so the geoQ and scopeQ classes of
    api-load.py have a computable outcome."""
    c = conn(broker)
    for t in range(tenants):
        tenant = f"t{t}"
        docs = []
        for n in range(per_tenant):
            docs.append({
                "id": f"urn:ngsi-ld:Vehicle:{tenant}:{n}",
                "type": "Vehicle",
                "scope": ["/region/north/urban", "/region/north/rural",
                          "/region/south/urban", "/region/south/rural"][n % 4],
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [
                    round(16.8 + (n % 1000) / 1000 * 5.8, 5),
                    round(47.7 + ((n // 1000) % 1000) / 1000 * 1.9, 5)]}},
                "brand": {"type": "Property", "value": "Skoda"},
                "speed": {"type": "Property", "value": 10},
                "mileage": {"type": "Property", "value": 1000},
                "colour": {"type": "Property", "value": "red"},
            })
        st, body = req(c, "POST", "/ngsi-ld/v1/entityOperations/create", docs, tenant)
        if st not in (201, 204, 207):
            raise SystemExit(f"seed {tenant}: {st} {body[:400]}")
    c.close()


def health(broker):
    with urllib.request.urlopen(broker + "/q/health") as r:
        return json.load(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--broker", default="http://127.0.0.1:9090")
    ap.add_argument("--sink", default="http://127.0.0.1:9800")
    ap.add_argument("--tenants", type=int, default=10)
    ap.add_argument("--entities", type=int, default=500, help="per tenant")
    ap.add_argument("--duration", type=int, default=20)
    ap.add_argument("--writers", type=int, default=32)
    ap.add_argument("--rate", type=float, default=0.0,
                    help="target PATCH/s over all writers; 0 saturates open-loop")
    ap.add_argument("--seed", action="store_true", help="create the entities and exit")
    ap.add_argument("--speed-max", type=int, default=0,
                    help="keep the written speed below this, so the q= classes of "
                         "api-load.py evaluate and do not match: the matcher pays "
                         "its full per-candidate cost and delivery pays almost none")
    a = ap.parse_args()

    if a.seed:
        seed(a.broker, a.tenants, a.entities)
        print(f"seeded {a.tenants * a.entities} entities")
        return

    urllib.request.urlopen(urllib.request.Request(a.sink + "/stats", method="DELETE")).read()
    h0 = health(a.broker)
    ok = [0] * a.writers
    bad = [0] * a.writers
    stop = time.time() + a.duration
    # Paced open-loop: each writer owns rate/writers of the arrival rate and
    # keeps its own schedule, so a slow response delays that writer rather
    # than the whole stream.
    per = (a.rate / a.writers) if a.rate else 0.0
    begin = time.time()

    def writer(w):
        c = conn(a.broker)
        v = 1000 + w * 100000
        i = 0
        while time.time() < stop:
            if per:
                gap = begin + i / per - time.time()
                if gap > 0:
                    time.sleep(gap)
            i += 1
            t = w % a.tenants
            n = (v // 7) % a.entities
            v += 1
            # every write must carry a NEW value or it is not a change and
            # notifies nothing (5.8.6), so this walks a band instead of a point
            val = (v % (a.speed_max - 1) + 1) if a.speed_max else v
            body = {"speed": {"type": "Property", "value": val}}
            try:
                st, _ = req(c, "PATCH",
                            f"/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:t{t}:{n}/attrs",
                            body, f"t{t}")
                ok[w] += st == 204
                bad[w] += st != 204
            except OSError:
                bad[w] += 1
                c.close()
                c = conn(a.broker)
        c.close()

    threads = [threading.Thread(target=writer, args=(w,)) for w in range(a.writers)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    t_end = time.time()

    # quiet = the sink's count unchanged for 5 s (cap 120 s)
    prev, quiet = -1, 0
    while quiet < 5 and time.time() - t_end < 120:
        with urllib.request.urlopen(a.sink + "/stats") as r:
            cur = json.load(r)["posts"]
        quiet = quiet + 1 if cur == prev else 0
        prev = cur
        time.sleep(1)

    with urllib.request.urlopen(a.sink + "/stats") as r:
        s = json.load(r)
    h1 = health(a.broker)
    delta = lambda k: int(h1.get(k, 0)) - int(h0.get(k, 0))
    wall = t_end - begin
    span = (s["last"] - s["first"]) if s.get("first") and s.get("last") else 0
    dropped = delta("changesDropped")
    print(json.dumps({
        "target_rate": a.rate or "saturate",
        "patches_ok": sum(ok), "patches_failed": sum(bad),
        "patch_rate": round(sum(ok) / wall, 1),
        "posts": s["posts"],
        "matches": s["entities"],
        "matches_per_second": round(s["entities"] / span, 1) if span else None,
        "changes_per_second": round((sum(ok) - dropped) / span) if span else None,
        "matches_per_post": round(s["entities"] / s["posts"], 1) if s["posts"] else None,
        "subscriptions_fired": s.get("subscriptions", 0),
        "delivery_span_s": round(span, 1),
        "drain_after_writes_s": round(max(0.0, (s["last"] or t_end) - t_end), 1),
        "changes_dropped": dropped, "dead_letters": delta("deadLetters"),
        "delivery_width": h1.get("limits", {}).get("deliveryWidth"),
        "delivery_width_per_tenant": h1.get("limits", {}).get("deliveryWidthPerTenant"),
    }, indent=2))


main()
