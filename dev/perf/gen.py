#!/usr/bin/env python3
"""Streaming entity generator for the load rigs.

    python3 dev/perf/gen.py --entities 100000000 --tenants 10000 | dev/bulk-load.sh /dev/stdin

Writes one line per entity to stdout in the store's internal form (the
shape dev/bulk-load.sh takes): expanded IRIs, every attribute an array of
instances, `type` short. Each line is `<tenant>\\x02<json>`; the loader
splits on the byte and stores the row under that tenant. Tenants are
`t<k>`, entities round-robin over them, ids `urn:ngsi-ld:Vehicle:t<k>:<n>`.

Three entity types, chosen by the entity number: n % 3 == 0 a Vehicle
(brand, speed, mileage, colour, location — coraine's benchmark shape plus
a GeoProperty), 1 a Building (temperature plus twenty facility
properties, a wide row) and 2 a Sensor (value plus a ~2 KB metadata
blob, a fat row). Every entity carries a scope (/region/{north,south}/
{urban,rural} by n % 4) and a location derived from n (n % 1000 < 500 lies
west of 19.7°E), so geoQ and scopeQ outcomes are computable. Ids are `urn:ngsi-ld:<Type>:t<k>:<n>`, so a consumer
that knows n knows the type. Deterministic for a seed, so
two runs of the same command produce byte-identical streams and a load
can be resumed or compared. Nothing is buffered: 100 M lines stream in
constant memory.

    --self-test      run the invariants below and exit
"""

import argparse
import json
import random
import sys

DC = "https://uri.etsi.org/ngsi-ld/default-context/"
LOC = "https://uri.etsi.org/ngsi-ld/location"
BRANDS = ["Mercedes", "Skoda", "Volvo", "Toyota", "Tesla", "Renault"]
COLOURS = ["red", "blue", "white", "black", "silver"]


TYPES = ["Vehicle", "Building", "Sensor"]


def type_of(n):
    return TYPES[n % 3]


def point_of(n):
    """lon walks west→east with n % 1000 (n % 1000 < 500 = western half,
    lon < 19.7), lat with (n // 1000) % 1000; both inside Slovakia's box."""
    return round(16.8 + (n % 1000) / 1000 * 5.8, 5), round(47.7 + ((n // 1000) % 1000) / 1000 * 1.9, 5)


SCOPES = ["/region/north/urban", "/region/north/rural", "/region/south/urban", "/region/south/rural"]


def scope_of(n):
    return SCOPES[n % 4]


def prop(v, **extra):
    return [{"type": "Property", "value": v, **extra}]


def entity(tenant, n, rnd):
    kind = type_of(n)
    doc = {
        "id": f"urn:ngsi-ld:{kind}:{tenant}:{n}",
        "type": DC + kind,
        # scope and location derive from n, not the seed: a subscription's
        # geoQ / scopeQ outcome must be computable by the update stream
        "scope": [scope_of(n)],
        LOC: [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": list(point_of(n))}}],
    }
    if kind == "Vehicle":
        doc[DC + "brand"] = prop(rnd.choice(BRANDS))
        doc[DC + "speed"] = prop(rnd.randint(0, 130))
        doc[DC + "mileage"] = prop(rnd.randint(0, 400000), unitCode="KMT")
        doc[DC + "colour"] = prop(rnd.choice(COLOURS))
    elif kind == "Building":
        doc[DC + "temperature"] = prop(round(rnd.uniform(15.0, 30.0), 1), unitCode="CEL")
        doc[DC + "floors"] = prop(rnd.randint(1, 30))
        doc[DC + "category"] = prop(rnd.choice(["office", "school", "housing", "industrial"]))
        for i in range(20):
            doc[DC + f"facility{i}"] = prop(rnd.randint(0, 1000))
    else:
        doc[DC + "value"] = prop(round(rnd.uniform(0.0, 100.0), 3))
        doc[DC + "unit"] = prop("percent")
        # ~2 KB of opaque metadata: the fat-row case for the jsonb column
        doc[DC + "metadata"] = prop("".join(rnd.choice("abcdefghijklmnopqrstuvwxyz0123456789") for _ in range(2048)))
    return doc


def stream(entities, tenants, seed, offset=0):
    """Yield (tenant, doc) for entity numbers offset..offset+entities."""
    for n in range(offset, offset + entities):
        tenant = f"t{n % tenants}"
        # one generator per entity: byte n is the same whatever offset a
        # parallel stream starts at
        yield tenant, entity(tenant, n, random.Random((seed << 40) ^ n))


def self_test():
    rows = list(stream(100, 7, 1))
    assert len(rows) == 100
    ids = {d["id"] for _, d in rows}
    assert len(ids) == 100, "ids must be unique"
    spread = {}
    for t, _ in rows:
        spread[t] = spread.get(t, 0) + 1
    assert sorted(spread.values()) == [14, 14, 14, 14, 14, 15, 15], spread
    again = list(stream(100, 7, 1))
    assert rows == again, "same seed must give the same stream"
    assert list(stream(10, 7, 1, offset=90)) == rows[90:], "offset must resume the stream"
    kinds = {}
    for _, d in rows:
        kind = d["type"].rsplit("/", 1)[1]
        kinds[kind] = kinds.get(kind, 0) + 1
        assert d["id"].startswith(f"urn:ngsi-ld:{kind}:"), d["id"]
        assert all(isinstance(v, list) for k, v in d.items() if k not in ("id", "type"))
        lon, lat = d[LOC][0]["value"]["coordinates"]
        assert 16.8 <= lon <= 22.6 and 47.7 <= lat <= 49.6
        assert d["scope"][0].startswith("/region/")
        if kind == "Vehicle":
            assert set(d) == {"id", "type", "scope", DC + "brand", DC + "speed", DC + "mileage", DC + "colour", LOC}
        elif kind == "Building":
            assert len(d) == 27 and DC + "temperature" in d
        else:
            assert len(d[DC + "metadata"][0]["value"]) == 2048
    assert kinds == {"Vehicle": 34, "Building": 33, "Sensor": 33}, kinds
    print("gen.py self-test ok")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--entities", type=int, default=1000)
    ap.add_argument("--tenants", type=int, default=1)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--offset", type=int, default=0, help="first entity number (parallel streams)")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        self_test()
        return
    out = sys.stdout
    for tenant, doc in stream(a.entities, a.tenants, a.seed, a.offset):
        out.write(tenant)
        out.write("\x02")
        out.write(json.dumps(doc, separators=(",", ":")))
        out.write("\n")


if __name__ == "__main__":
    main()
