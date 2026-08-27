#!/usr/bin/env python3
"""Create subscriptions and context source registrations through the API.

    python3 dev/perf/api-load.py subscriptions --count 100000 --tenants 10000 \\
        --broker http://localhost:9090 --sink http://127.0.0.1:9800 [--mqtt mqtt://localhost:1883]
    python3 dev/perf/api-load.py registrations --count 100000 --tenants 10000 ...

Neither resource has a batch operation (5.6.7 and 5.6.8 exist for
entities only), so this is one POST per resource over a thread pool.
Subscription k belongs to tenant t(k mod tenants) and takes filter class
k // tenants mod 8 (SUB_CLASSES below: type, q, watchedAttributes,
idPattern, geoQ, scopeQ — each with a rule the update stream can evaluate, so the
notifications due are known); every tenth one notifies over MQTT when
--mqtt is given. Registration k takes class k // tenants mod 8
(CSR_CLASSES: entity type, id pattern, mode, operations, context source
properties, contextSourceInfo, expiry, location, scopes) at the sink; a federated query
touches only the registrations whose information can match.

Writes subs.md / csr.md (the class tables) into --out when given.
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


# Eight filter classes; every subscription is still unique: p = k // tenants
# (its index within the tenant) parametrises the filter — the q threshold,
# the idPattern tail digit, the geoQ polygon's eastern edge, the scopeQ
# branch — so no two subscriptions of one tenant share a filter, and the
# rule "does an update of entity n (new value v) fire subscription (c, p)"
# stays a closed form k6-fire.js evaluates (CLASS_FIRES).
def lon_cut(p):
    """gen.py point_of: lon = 16.8 + (n % 1000) / 1000 * 5.8; entities with
    n % 1000 < 250 + 5p lie strictly west of this edge."""
    return round(16.8 + (250 + 5 * p) / 1000 * 5.8 - 0.0001, 5)


def west_polygon(p):
    e = lon_cut(p)
    # south and west edges below every entity (gen.py starts at 16.8, 47.7):
    # "within" excludes the boundary, a point on the edge is outside
    return {"type": "Polygon", "coordinates": [[[16.7, 47.6], [e, 47.6], [e, 49.7], [16.7, 49.7], [16.7, 47.6]]]}


SCOPE_Q = ["/region/north/#", "/region/south/#", "/region/north/urban", "/region/south/rural"]

# (name, entities selector, extra members, when an update fires it), each a
# function of p
SUB_CLASSES = [
    ("vehicle-any", lambda p: [{"type": "Vehicle"}], lambda p: {"q": f"speed>{100 + p}"}, "Vehicle updates with speed > 100+p (all)"),
    ("vehicle-cold-attr", lambda p: [{"type": "Vehicle"}], lambda p: {"watchedAttributes": ["brand"], "q": f"speed>{p}"}, "never (updates touch speed)"),
    ("vehicle-high-speed", lambda p: [{"type": "Vehicle"}], lambda p: {"q": f"speed>{500000000 + p * 1000000}"}, "Vehicle updates with speed > 5e8 + p·1e6 (about half)"),
    ("vehicle-id-tail", lambda p: [{"type": "Vehicle", "idPattern": f".*{p % 10}$"}], lambda p: {"q": f"speed>{100 + p}"}, "Vehicle updates on ids ending in p % 10 (a tenth)"),
    ("building-any", lambda p: [{"type": "Building"}], lambda p: {"q": f"temperature>{20 + p}"}, "Building updates with temperature > 20+p (all)"),
    ("sensor-any", lambda p: [{"type": "Sensor"}], lambda p: {"q": f"value>{p}"}, "Sensor updates with value > p (all)"),
    ("vehicle-geo-west", lambda p: [{"type": "Vehicle"}],
     lambda p: {"geoQ": {"geometry": "Polygon", "georel": "within", "coordinates": west_polygon(p)["coordinates"]}},
     "Vehicle updates on ids with n % 1000 < 250 + 5p (west of the polygon's edge)"),
    ("any-scope", lambda p: [{"type": "Vehicle"}, {"type": "Building"}, {"type": "Sensor"}],
     lambda p: {"scopeQ": SCOPE_Q[p % 4]}, "updates on ids whose scope (n % 4) matches SCOPE_Q[p % 4]"),
]


def sub_class(k, tenants):
    return (k // tenants) % len(SUB_CLASSES)


def subscription(k, tenants, sink, mqtt):
    uri = f"{mqtt}/antares/perf/{k}" if mqtt and k % 10 == 0 else f"{sink}/n/{k}"
    name, entities, extra, _ = SUB_CLASSES[sub_class(k, tenants)]
    p = k // tenants
    return f"t{k % tenants}", {
        "id": f"urn:ngsi-ld:Subscription:perf:{k}",
        "type": "Subscription",
        "description": f"{name} p={p}",
        "entities": entities(p),
        **extra(p),
        "notification": {"endpoint": {"uri": uri, "accept": "application/json"}},
        "@context": CTX,
    }


# (name, entity type, mode, operations, extra members): six registration
# shapes so the registry index and the forward path see more than one row
CSR_CLASSES = [
    ("vehicle-inclusive", "Vehicle", "inclusive", ["federationOps"], {}),
    ("building-exclusive", "Building", "exclusive", ["retrieveOps"], {}),
    ("sensor-redirect", "Sensor", "redirect", ["queryEntity", "retrieveEntity"], {}),
    ("vehicle-auxiliary-csf", "Vehicle", "auxiliary", ["queryEntity"],
     {"sourceType": {"type": "Property", "value": "archive"}}),
    ("building-with-headers", "Building", "inclusive", ["queryEntity"],
     {"contextSourceInfo": [{"key": "X-Perf-Source", "value": "csr"}],
      "observationInterval": {"startAt": "2020-01-01T00:00:00Z"}}),
    ("sensor-expiring", "Sensor", "inclusive", ["queryEntity"], {"expiresAt": "2099-01-01T00:00:00Z"}),
    ("vehicle-geo-west", "Vehicle", "inclusive", ["queryEntity"], {"location": "west_polygon(p)"}),
    ("building-scope", "Building", "inclusive", ["queryEntity"], {"scopes": "[/region/north | /region/south by p % 2]"}),
]


def csr_extra(extra, p):
    """The two location-shaped classes vary with p like the subscriptions."""
    if "location" in extra:
        return {"location": west_polygon(p)}
    if "scopes" in extra:
        return {"scopes": ["/region/north" if p % 2 == 0 else "/region/south"]}
    return extra


def csr_class(k, tenants):
    return (k // tenants) % len(CSR_CLASSES)


def registration(k, tenants, sink, _mqtt):
    tenant = f"t{k % tenants}"
    name, etype, mode, ops, extra = CSR_CLASSES[csr_class(k, tenants)]
    p = k // tenants
    return tenant, {
        "id": f"urn:ngsi-ld:ContextSourceRegistration:perf:{k}",
        "type": "ContextSourceRegistration",
        "description": f"{name} p={p}",
        # k followed by "-": pattern k never covers pattern k0, k1, … (an
        # exclusive registration overlapping another is a 409, 4.3.6.3) and
        # no loaded entity matches (a redirect one over existing entities is
        # a conflict, 5.9.2.4)
        # 4.3.6.3: an exclusive registration names one entity id and its
        # Attributes; the others use an id pattern
        "information": [{"entities": [{"type": etype, "id": f"urn:ngsi-ld:{etype}:{tenant}:{k}-x"}],
                         "propertyNames": ["temperature"]} if mode == "exclusive" else
                        {"entities": [{"type": etype, "idPattern": f"urn:ngsi-ld:{etype}:{tenant}:{k}-.*"}]}],
        "mode": mode,
        "operations": ops,
        **csr_extra(extra, p),
        "endpoint": f"{sink}/csr/{k}",
        "@context": CTX,
    }


def class_table(kind, count, tenants):
    """Markdown table of the classes and how many of `count` fall in each."""
    if kind == "subscriptions":
        rows = [(n, json.dumps(e(0)), json.dumps(x(0)) + " … p = k // tenants", fires) for n, e, x, fires in SUB_CLASSES]
        head = "| class | entities (p=0) | filter (p=0) | fires on |"
        pick = sub_class
    else:
        rows = [(n, t, f"{m}, {json.dumps(o)}", json.dumps(x)) for n, t, m, o, x in CSR_CLASSES]
        head = "| class | type | mode, operations | extra |"
        pick = csr_class
    per = [0] * len(rows)
    for k in range(count):
        per[pick(k, tenants)] += 1
    out = [f"{count} {kind} over {tenants} tenants", "", head + " count |", "|---|---|---|---|---|"]
    for (a, b, c, d), n in zip(rows, per):
        out.append(f"| {a} | {b} | {c} | {d} | {n} |")
    return "\n".join(out) + "\n"


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
    ap.add_argument("--out", default=None, help="write the class table (subs.md / csr.md) here")
    a = ap.parse_args()
    if a.out:
        name = "subs.md" if a.kind == "subscriptions" else "csr.md"
        with open(f"{a.out}/{name}", "w") as f:
            f.write(class_table(a.kind, a.offset + a.count, a.tenants))
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
