"""HSL HFP vp-events -> NGSI-LD batch upserts against Antares.

Subscribes to mqtt.hsl.fi (TLS 8883), buffers the latest VP message per
vehicle, and every FLUSH_MS posts one /entityOperations/upsert?options=update
batch with expiresAt = now + TTL_SECS on every entity. Prints a stats line
every 10 s (consumed rate, batch size, upsert latency, HTTP errors).
"""

import json
import os
import ssl
import threading
import time
from datetime import datetime, timedelta, timezone

import paho.mqtt.client as mqtt
import requests

BROKER = os.environ.get("BROKER", "http://localhost:9090/ngsi-ld/v1")
MQTT_HOST = os.environ.get("MQTT_HOST", "mqtt.hsl.fi")
MQTT_TOPIC = os.environ.get("MQTT_TOPIC", "/hfp/v2/journey/ongoing/vp/#")
TTL_SECS = int(os.environ.get("TTL_SECS", "180"))
# 4.22: per-instance attribute expiresAt -> each observation leaves the
# temporal history ATTR_TTL_SECS after ingest (rolling window, no cron).
ATTR_TTL_SECS = int(os.environ.get("ATTR_TTL_SECS", "600"))
FLUSH_MS = int(os.environ.get("FLUSH_MS", "1000"))

pending = {}  # entity id -> entity doc (latest wins)
lock = threading.Lock()
stats = {"msgs": 0, "bad": 0, "batches": 0, "upserted": 0,
         "http_err": 0, "lat_ms": []}


def prop(value, observed_at=None):
    p = {"type": "Property", "value": value}
    if observed_at:
        p["observedAt"] = observed_at
    return p


def on_message(_client, _userdata, msg):
    try:
        parts = msg.topic.split("/")
        # /hfp/v2/journey/ongoing/vp/<mode>/<oper>/<veh>/...
        mode, oper, veh = parts[6], parts[7], parts[8]
        vp = json.loads(msg.payload)["VP"]
        lat, lng, tst = vp.get("lat"), vp.get("long"), vp.get("tst")
        if lat is None or lng is None or not tst:
            stats["bad"] += 1
            return
        ent = {
            "id": f"urn:ngsi-ld:Vehicle:HSL:{oper}-{veh}",
            "type": "Vehicle",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [lng, lat]},
                "observedAt": tst,
            },
            "transportMode": prop(mode),
        }
        for attr, key in [("speed", "spd"), ("heading", "hdg"),
                          ("delay", "dl"), ("route", "route"),
                          ("occupancy", "occu")]:
            if vp.get(key) is not None:
                ent[attr] = prop(vp[key], tst)
        with lock:
            pending[ent["id"]] = ent
            stats["msgs"] += 1
    except Exception:
        stats["bad"] += 1


def flusher():
    sess = requests.Session()
    while True:
        time.sleep(FLUSH_MS / 1000)
        with lock:
            batch = list(pending.values())
            pending.clear()
        if not batch:
            continue
        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=TTL_SECS)).strftime("%Y-%m-%dT%H:%M:%SZ")
        attr_expires = (now + timedelta(seconds=ATTR_TTL_SECS)).strftime("%Y-%m-%dT%H:%M:%SZ")
        for e in batch:
            e["expiresAt"] = expires
            for v in e.values():
                if isinstance(v, dict) and "type" in v:
                    v["expiresAt"] = attr_expires
        t0 = time.monotonic()
        try:
            r = sess.post(f"{BROKER}/entityOperations/upsert?options=update",
                          json=batch,
                          headers={"Content-Type": "application/json"},
                          timeout=30)
            ms = (time.monotonic() - t0) * 1000
            stats["batches"] += 1
            stats["lat_ms"].append(ms)
            if r.status_code in (201, 204, 207):
                stats["upserted"] += len(batch)
                if r.status_code == 207:
                    stats["http_err"] += 1
                    print(f"207 partial: {r.text[:300]}")
            else:
                stats["http_err"] += 1
                print(f"HTTP {r.status_code}: {r.text[:300]}")
        except Exception as exc:
            stats["http_err"] += 1
            print(f"upsert failed: {exc}")


def reporter():
    last_msgs = 0
    while True:
        time.sleep(10)
        lat = sorted(stats["lat_ms"][-100:])
        p50 = lat[len(lat) // 2] if lat else 0
        p95 = lat[int(len(lat) * 0.95)] if lat else 0
        rate = (stats["msgs"] - last_msgs) / 10
        last_msgs = stats["msgs"]
        print(f"STATS msgs={stats['msgs']} rate={rate:.0f}/s "
              f"upserted={stats['upserted']} batches={stats['batches']} "
              f"lat_p50={p50:.0f}ms lat_p95={p95:.0f}ms "
              f"http_err={stats['http_err']} bad={stats['bad']}", flush=True)


client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2)
client.tls_set(cert_reqs=ssl.CERT_REQUIRED)
client.on_message = on_message
client.on_connect = lambda c, u, f, rc, p=None: (
    print(f"MQTT connected rc={rc}, subscribing {MQTT_TOPIC}", flush=True),
    c.subscribe(MQTT_TOPIC),
)
client.connect(MQTT_HOST, 8883)
threading.Thread(target=flusher, daemon=True).start()
threading.Thread(target=reporter, daemon=True).start()
client.loop_forever()
