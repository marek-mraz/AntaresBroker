#!/usr/bin/env python3
"""Streaming entity generator for the load rigs.

    python3 dev/perf/gen.py --entities 100000000 --tenants 10000 | dev/bulk-load.sh /dev/stdin

Writes one line per entity to stdout in the store's internal form (the
shape dev/bulk-load.sh takes): expanded IRIs, every attribute an array of
instances, `type` short. Each line is `<tenant>\\x02<json>`; the loader
splits on the byte and stores the row under that tenant. Tenants are
`t<k>`, entities round-robin over them, ids `urn:ngsi-ld:Vehicle:t<k>:<n>`.

Five attributes per entity, coraine's benchmark shape plus a GeoProperty:
brand, speed, mileage, colour and location. Deterministic for a seed, so
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


def entity(tenant, n, rnd):
    return {
        "id": f"urn:ngsi-ld:Vehicle:{tenant}:{n}",
        "type": DC + "Vehicle",
        DC + "brand": [{"type": "Property", "value": rnd.choice(BRANDS)}],
        DC + "speed": [{"type": "Property", "value": rnd.randint(0, 130)}],
        DC + "mileage": [{"type": "Property", "value": rnd.randint(0, 400000), "unitCode": "KMT"}],
        DC + "colour": [{"type": "Property", "value": rnd.choice(COLOURS)}],
        LOC: [{"type": "GeoProperty", "value": {
            "type": "Point",
            "coordinates": [round(rnd.uniform(16.8, 22.6), 5), round(rnd.uniform(47.7, 49.6), 5)]}}],
    }


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
    for _, d in rows:
        assert set(d) == {"id", "type", DC + "brand", DC + "speed", DC + "mileage", DC + "colour", LOC}
        assert all(isinstance(v, list) for k, v in d.items() if k not in ("id", "type"))
        lon, lat = d[LOC][0]["value"]["coordinates"]
        assert 16.8 <= lon <= 22.6 and 47.7 <= lat <= 49.6
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
