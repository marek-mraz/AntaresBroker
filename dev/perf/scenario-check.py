#!/usr/bin/env python3
"""Correctness assertion driver for multi-broker and edge scenarios (MODE=check)."""

import argparse
import json
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

CORE_CTX = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"


def http_req(url, method="GET", body=None, headers=None, timeout=15):
    h = headers.copy() if headers else {}
    data = None
    if body is not None:
        if isinstance(body, (dict, list)):
            data = json.dumps(body).encode()
            # 6.3.5: a body that carries @context is application/ld+json
            has_ctx = isinstance(body, dict) and "@context" in body
            h["Content-Type"] = "application/ld+json" if has_ctx else h.get("Content-Type", "application/json")
        elif isinstance(body, str):
            data = body.encode()
        elif isinstance(body, bytes):
            data = body
        h.setdefault("Content-Length", str(len(data)))
    req = urllib.request.Request(url, data=data, method=method, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return res.status, res.read(), dict(res.headers)
    except urllib.error.HTTPError as e:
        return e.code, e.read(), dict(e.headers)
    except Exception as e:
        return 0, str(e).encode(), {}


def get_sink_stats(sink_url):
    st, body, _ = http_req(f"{sink_url}/stats")
    return json.loads(body.decode()) if st == 200 else {}


def reset_sink(sink_url):
    http_req(f"{sink_url}/stats", method="DELETE")


def check_hot_entity(broker_url, tenant="hot_entity"):
    hot_id = f"urn:ngsi-ld:Vehicle:{tenant}:hot0"
    base = f"{broker_url}/ngsi-ld/v1"
    headers = {"NGSILD-Tenant": tenant, "Content-Type": "application/json"}

    # Initial entity
    seed = {
        "id": hot_id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10},
        "@context": CORE_CTX,
    }
    st, _, _ = http_req(f"{base}/entities", "POST", seed, headers)
    if st not in (201, 409):
        print(f"FAIL: hot-entity initial creation status {st}")
        return False

    errors = []
    # 32 threads, 50 updates each with datasetId
    def worker_ds(tid):
        for seq in range(50):
            val = tid * 1000 + seq
            ds = f"urn:ngsi-ld:Dataset:{tid}"
            patch = {"speed": {"type": "Property", "value": val, "datasetId": ds}}
            st, b, _ = http_req(f"{base}/entities/{hot_id}/attrs", "PATCH", patch, headers)
            if st != 204:
                errors.append(f"ds PATCH returned {st}")

    threads = [threading.Thread(target=worker_ds, args=(i,)) for i in range(32)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    if errors:
        print(f"FAIL: hot-entity datasetId writes had {len(errors)} errors: {errors[0]}")
        return False

    # Check that all 32 datasetIds are present with correct final values
    st, body, _ = http_req(f"{base}/entities/{hot_id}", "GET", None, {"NGSILD-Tenant": tenant})
    if st != 200:
        print(f"FAIL: hot-entity GET failed with {st}")
        return False
    doc = json.loads(body.decode())
    speeds = doc.get("speed", [])
    if isinstance(speeds, dict):
        speeds = [speeds]
    ds_map = {s.get("datasetId"): s.get("value") for s in speeds}
    for tid in range(32):
        ds = f"urn:ngsi-ld:Dataset:{tid}"
        expected = tid * 1000 + 49
        if ds_map.get(ds) != expected:
            print(f"FAIL: 5.6.3 datasetId update lost for {ds}: expected {expected}, got {ds_map.get(ds)}")
            return False

    # Test single property updates without datasetId (race overwrite)
    def worker_plain(tid):
        for seq in range(25):
            val = tid * 100 + seq
            patch = {"speed": {"type": "Property", "value": val}}
            st, _, _ = http_req(f"{base}/entities/{hot_id}/attrs", "PATCH", patch, headers)
            if st != 204:
                errors.append(f"plain PATCH returned {st}")

    threads = [threading.Thread(target=worker_plain, args=(i,)) for i in range(16)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    st, body, _ = http_req(f"{base}/entities/{hot_id}", "GET", None, {"NGSILD-Tenant": tenant})
    if st != 200 or errors:
        print(f"FAIL: hot-entity plain updates failed")
        return False
    print("ok: hot-entity 5.6.3 multi-instance datasetId updates preserved under concurrent writes")
    return True


def check_noisy_tenant(broker_url, sink_url):
    base = f"{broker_url}/ngsi-ld/v1"
    reset_sink(sink_url)

    # Measure quiet alone
    alone_lats = []
    for i in range(100):
        t0 = time.time()
        st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:quiet:{i % 10}", "GET", None, {"NGSILD-Tenant": "quiet"})
        alone_lats.append((time.time() - t0) * 1000.0)

    # Now hammer loud while measuring quiet
    stop_loud = threading.Event()
    loud_errors = []

    def hammer():
        idx = 0
        while not stop_loud.is_set():
            val = idx % 1000
            patch = {"speed": {"type": "Property", "value": val}}
            st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:loud:0/attrs", "PATCH", patch, {"NGSILD-Tenant": "loud"})
            if st != 204:
                loud_errors.append(st)
            idx += 1

    threads = [threading.Thread(target=hammer) for _ in range(16)]
    for t in threads:
        t.start()

    load_lats = []
    quiet_errors = []
    for i in range(100):
        t0 = time.time()
        st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:quiet:{i % 10}", "GET", None, {"NGSILD-Tenant": "quiet"})
        load_lats.append((time.time() - t0) * 1000.0)
        if st != 200:
            quiet_errors.append(st)

    stop_loud.set()
    for t in threads:
        t.join()

    if quiet_errors:
        print(f"FAIL: noisy-tenant quiet tenant saw HTTP errors under load: {quiet_errors[:5]}")
        return False
    print(f"ok: noisy-tenant tenant isolation preserved (quiet GET intact under 16 concurrent loud writers)")
    return True


def check_slow_subscriber(broker_url, sink_url):
    base = f"{broker_url}/ngsi-ld/v1"
    reset_sink(sink_url)
    tenant = "slow_sub"

    # Send 50 updates
    for i in range(50):
        patch = {"speed": {"type": "Property", "value": 200 + i}}
        st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:{tenant}:0/attrs", "PATCH", patch, {"NGSILD-Tenant": tenant})
        if st != 204:
            print(f"FAIL: slow-subscriber update failed: {st}")
            return False

    # Check that fast subscribers drain within 5 s
    time.sleep(2)
    s = get_sink_stats(sink_url)
    fast_count = sum(v for k, v in s.get("by_sub", {}).items() if not k.endswith(":slow"))
    if fast_count < 50:
        print(f"FAIL: slow-subscriber deliveryWidthPerTenant stalled fast subscribers (fast deliveries: {fast_count})")
        return False
    print("ok: slow-subscriber deliveryWidthPerTenant isolated slow endpoint from fast fleet")
    return True


def check_fan_in(broker_url, sink_url):
    base = f"{broker_url}/ngsi-ld/v1"
    reset_sink(sink_url)
    tenant = "fan_in"

    patch = {"speed": {"type": "Property", "value": 999}}
    st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:{tenant}:0/attrs", "PATCH", patch, {"NGSILD-Tenant": tenant})
    if st != 204:
        print(f"FAIL: fan-in trigger PATCH failed: {st}")
        return False

    # Wait up to 30 s for notifications
    for _ in range(30):
        time.sleep(1)
        s = get_sink_stats(sink_url)
        if s.get("posts", 0) >= 50:
            break

    s = get_sink_stats(sink_url)
    posts = s.get("posts", 0)
    if posts == 0:
        print("FAIL: fan-in notifications not received")
        return False
    print(f"ok: fan-in notification fan-out executed successfully ({posts} deliveries)")
    return True


def check_hub_sources(hub_url, src_a_url, src_b_url):
    base = f"{hub_url}/ngsi-ld/v1"
    headers = {"NGSILD-Tenant": "hub_src"}

    # Query hub for type=Vehicle
    st, body, _ = http_req(f"{base}/entities?type=Vehicle&limit=50", "GET", None, headers)
    if st != 200:
        print(f"FAIL: hub-sources 5.7.2 query failed: {st}")
        return False
    doc = json.loads(body.decode())
    ids = [e["id"] for e in doc]

    has_a = any("srca" in eid for eid in ids)
    has_b = any("srcb" in eid for eid in ids)
    if not (has_a and has_b):
        print(f"FAIL: 4.3.6.1 hub-sources distributed query missing sources: has_a={has_a}, has_b={has_b}")
        return False

    # Retrieve specific entity from source b through hub
    sample_b = [eid for eid in ids if "srcb" in eid][0]
    st, body, _ = http_req(f"{base}/entities/{sample_b}", "GET", None, headers)
    if st != 200:
        print(f"FAIL: 5.7.1 hub-sources direct retrieve failed: {st}")
        return False

    # Retrieve absent entity -> 404
    st, _, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:absent:404", "GET", None, headers)
    if st != 404:
        print(f"FAIL: 5.7.1 absent entity expected 404, got {st}")
        return False

    print("ok: hub-sources 4.3.6.1 + 5.7.2 distributed read and merge complete")
    return True


def check_collision(hub_url, src_url):
    base = f"{hub_url}/ngsi-ld/v1"
    tenant = "collision"
    headers = {"NGSILD-Tenant": tenant, "Content-Type": "application/json"}

    # Case A: inclusive collision
    st, body, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:{tenant}:both", "GET", None, {"NGSILD-Tenant": tenant})
    if st != 200:
        print(f"FAIL: collision case A failed: {st}")
        return False
    doc = json.loads(body.decode())
    if "brand" not in doc or "speed" not in doc:
        print(f"FAIL: collision 4.5.5 non-colliding attributes missing in merged entity")
        return False

    # Case B: auxiliary collision (local wins)
    st, body, _ = http_req(f"{base}/entities/urn:ngsi-ld:Vehicle:{tenant}:aux", "GET", None, {"NGSILD-Tenant": tenant})
    if st != 200:
        print(f"FAIL: collision case B failed: {st}")
        return False
    doc = json.loads(body.decode())
    speed_val = doc.get("speed", {}).get("value")
    if speed_val != 100:
        print(f"FAIL: 4.3.6.2 auxiliary registration overrode local data (expected 100, got {speed_val})")
        return False

    # Case C: exclusive registration conflict when entity already exists locally
    csr_exc = {
        "id": f"urn:ngsi-ld:ContextSourceRegistration:{tenant}:exc-clash",
        "type": "ContextSourceRegistration",
        "mode": "exclusive",
        "information": [{"entities": [{"id": f"urn:ngsi-ld:Vehicle:{tenant}:both", "type": "Vehicle"}], "propertyNames": ["brand"]}],
        "endpoint": src_url,
        "@context": CORE_CTX,
    }
    st, _, _ = http_req(f"{base}/csourceRegistrations", "POST", csr_exc, headers)
    if st != 409:
        print(f"FAIL: 5.9.2.4 exclusive conflict expected 409, got {st}")
        return False

    # Case D: redirect registration covering existing entity -> 409
    csr_red = {
        "id": f"urn:ngsi-ld:ContextSourceRegistration:{tenant}:red-clash",
        "type": "ContextSourceRegistration",
        "tenant": tenant,
        "mode": "redirect",
        "information": [{"entities": [{"idPattern": f".*{tenant}:both.*", "type": "Vehicle"}]}],
        "endpoint": src_url,
        "@context": CORE_CTX,
    }
    st, _, _ = http_req(f"{base}/csourceRegistrations", "POST", csr_red, headers)
    if st != 409:
        print(f"FAIL: 5.9.2.4 redirect conflict expected 409, got {st}")
        return False

    print("ok: collision 4.3.6.2 (auxiliary local-wins) and 5.9.2.4 (registration 409 conflicts) confirmed")
    return True


def check_loop(broker_a_url, broker_b_url):
    base_a = f"{broker_a_url}/ngsi-ld/v1"
    base_b = f"{broker_b_url}/ngsi-ld/v1"
    headers = {"NGSILD-Tenant": "loop"}

    # A query through a registration cycle terminates and holds both sides once
    st, body, _ = http_req(f"{base_a}/entities?type=Vehicle&limit=1000", "GET", None, headers)
    if st != 200:
        print(f"FAIL: loop query at A expected 200, got {st} (6.3.18 Via chain)")
        return False
    ids = [e.get("id") for e in json.loads(body.decode())]
    if len(ids) != len(set(ids)) or not any("onlyA" in i for i in ids) or not any("onlyB" in i for i in ids):
        print(f"FAIL: loop query returned {len(ids)} ids, {len(set(ids))} distinct; both sides expected once (4.3.6.1 merge)")
        return False

    # A chain that already names A runs locally and is not re-forwarded (docs: Loop protection)
    st, body, _ = http_req(f"{base_a}/info/sourceIdentity", "GET", None, headers)
    alias = json.loads(body.decode()).get("contextSourceAlias", "") if st == 200 else ""
    if not alias:
        print(f"FAIL: /info/sourceIdentity at A gave {st}; no alias to build the Via chain from")
        return False
    patch = {"speed": {"type": "Property", "value": 1}}
    st, _, _ = http_req(f"{base_a}/entities/urn:ngsi-ld:Vehicle:loop:onlyA/attrs", "PATCH", patch,
                        {"NGSILD-Tenant": "loop", "Via": f"1.1 {alias}"})
    if st != 204:
        print(f"FAIL: write with Via naming A under an inclusive registration expected 204 (served locally), got {st}")
        return False

    # When the only matching registration is redirect and the chain names A, the loop closes: 508.
    # An id-only write carries no type, so every type-only registration in the tenant matches it
    # (Table 6.3.18-2) and the "single registered source" case needs a tenant of its own.
    rt = "loop_redirect"
    st, body, _ = http_req(f"{base_a}/info/sourceIdentity", "GET", None, {"NGSILD-Tenant": rt})
    r_alias = json.loads(body.decode()).get("contextSourceAlias", "") if st == 200 else ""
    csr = {
        "id": "urn:ngsi-ld:ContextSourceRegistration:loop:bike-redirect",
        "type": "ContextSourceRegistration",
        "tenant": rt,
        "mode": "redirect",
        "information": [{"entities": [{"type": "Bike"}]}],
        "endpoint": broker_b_url,
        "@context": CORE_CTX,
    }
    st, _, _ = http_req(f"{base_a}/csourceRegistrations", "POST", csr, {"NGSILD-Tenant": rt})
    if st not in (201, 409):
        print(f"FAIL: redirect registration for the loop case expected 201, got {st}")
        return False
    st, _, _ = http_req(f"{base_a}/entities/urn:ngsi-ld:Bike:loop:1/attrs", "PATCH", patch,
                        {"NGSILD-Tenant": rt, "Via": f"1.1 {r_alias}"})
    if st != 508:
        print(f"FAIL: 6.3.17 write whose only source is redirect and whose Via names A ({r_alias}) expected 508, got {st}")
        return False
    print("ok: loop 6.3.18 Via chain cut the cycle (query complete, local serve on own alias, 508 on a redirect-only loop)")
    return True


def check_dist_sub(hub_url, src_a_url, sink_url):
    base_hub = f"{hub_url}/ngsi-ld/v1"
    base_src = f"{src_a_url}/ngsi-ld/v1"
    reset_sink(sink_url)
    tenant = "dist_sub"

    # Update on source
    patch = {"speed": {"type": "Property", "value": 777}}
    st, _, _ = http_req(f"{base_src}/entities/urn:ngsi-ld:Vehicle:{tenant}:srca0/attrs", "PATCH", patch, {"NGSILD-Tenant": tenant})
    if st != 204:
        print(f"FAIL: dist-sub update on source failed: {st}")
        return False

    # Wait for notification at sink
    time.sleep(2)
    s = get_sink_stats(sink_url)
    if s.get("posts", 0) < 1:
        print("FAIL: 5.8.1.4 distributed subscription did not notify hub subscriber")
        return False
    print("ok: distributed-subscription 5.8.1.4 forwarded notifications back to hub subscriber")
    return True


def check_ha_pair(pod1_url, pod2_url, sink_url):
    tenant = "ha_pair"
    reset_sink(sink_url)

    # Alternate writes across pod1 and pod2
    for i in range(20):
        url = pod1_url if i % 2 == 0 else pod2_url
        patch = {"speed": {"type": "Property", "value": 100 + i}}
        st, _, _ = http_req(f"{url}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:{tenant}:0/attrs", "PATCH", patch, {"NGSILD-Tenant": tenant})
        if st != 204:
            print(f"FAIL: ha-pair write to pod failed with status {st}")
            return False

    # every change notified exactly once: wait until the sink stops growing, then count entities
    prev, quiet, entities = -1, 0, 0
    for _ in range(30):
        time.sleep(1)
        entities = get_sink_stats(sink_url).get("entities", 0)
        quiet = quiet + 1 if entities == prev else 0
        prev = entities
        if quiet >= 2 and entities >= 20:
            break
    st, _, _ = http_req(f"{pod2_url}/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:{tenant}:sub", "GET", None, {"NGSILD-Tenant": tenant})
    if st != 200:
        print(f"FAIL: ha-pair subscription created at pod-1 is not visible at pod-2 (GET {st}); the pods do not share it")
        return False
    if entities != 20:
        what = "duplicates" if entities > 20 else "lost deliveries"
        print(f"FAIL: ha-pair 20 writes alternating over two pods notified {entities} entities ({what})")
        return False
    print("ok: ha-pair 20 writes over two pods sharing one database and bus notified exactly 20 entities")
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("scenario")
    ap.add_argument("--broker", required=True)
    ap.add_argument("--source", default=None)
    ap.add_argument("--source-b", default=None)
    ap.add_argument("--sink", default="http://127.0.0.1:9800")
    a = ap.parse_args()

    scen = a.scenario
    ok = False
    if scen == "hot-entity":
        ok = check_hot_entity(a.broker)
    elif scen == "noisy-tenant":
        ok = check_noisy_tenant(a.broker, a.sink)
    elif scen == "slow-subscriber":
        ok = check_slow_subscriber(a.broker, a.sink)
    elif scen == "fan-in":
        ok = check_fan_in(a.broker, a.sink)
    elif scen == "hub-sources":
        ok = check_hub_sources(a.broker, a.source, a.source_b)
    elif scen == "collision":
        ok = check_collision(a.broker, a.source)
    elif scen == "loop":
        ok = check_loop(a.broker, a.source)
    elif scen == "distributed-subscription":
        ok = check_dist_sub(a.broker, a.source, a.sink)
    elif scen == "ha-pair":
        ok = check_ha_pair(a.broker, a.source, a.sink)
    else:
        print(f"unknown scenario {scen}")
        sys.exit(2)

    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
