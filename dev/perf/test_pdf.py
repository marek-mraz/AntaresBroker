#!/usr/bin/env python3
"""Automated verification for dev/perf/pdf.py report generation."""

import os
import re
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import pdf

SAMPLE_STARTUP = """| store | ready in (median of 5) | RSS after start |
|---|---|---|
| memory | 32 ms | 19 MiB |
| file | 32 ms | 20 MiB |
| postgres | 155 ms | 59 MiB |
"""

SAMPLE_SHAPES = """| store | shape | concurrency | req/s | p99 |
|---|---|---|---|---|
| memory | query | c50 | 16377 | 6.82 ms |
| memory | query | c200 | 17673 | 36.31 ms |
| memory | retrieve | c50 | 28992 | 4.06 ms |
| postgres | query | c50 | 2052 | 28.68 ms |
| postgres | query | c200 | 2090 | 98.78 ms |
| postgres | retrieve | c50 | 5870 | 8.88 ms |
"""

SAMPLE_SATURATE = """| store | shape | knee (rps held) | p99 at knee | first failing stage | cores used | peak threads |
|---|---|---|---|---|---|---|
| postgres | query | 5000 | 2.5 ms | none reached | 2.42 | 37 |
| postgres | write | 5000 | 3.6 ms | none reached | 2.24 | 37 |
"""

SAMPLE_RSS = """| broker RSS peak | 3611 MiB | no ceiling set |
| Postgres RSS peak | 16.17 GiB | no ceiling set |
| broker CPU peak / mean | 28.4 / 3.8 cores | of 32 |
| Postgres CPU peak / mean | 18.0 / 2.8 cores | of 32 |
| host busy peak / mean | 30.5 / 7.6 cores | of 32: saturated when peak ≈ 32 |
| k6 RSS peak / CPU peak | 1285 MiB / 3.6 cores | rig, not the broker |
| sink RSS peak / CPU peak | 196 MiB / 2.7 cores | rig, not the broker |
| mosquitto RSS peak / CPU peak | 4 MiB / 0.0 cores | rig, not the broker |
| samples | 1044 | rss.csv, 1.2 s apart |
"""

SAMPLE_LOAD = """scale 0.01: 1000000 entities / 100 tenants / 10000 subscriptions / 10000 registrations
- entities (1000000, bulk COPY): 318 s
- subscriptions (10000): 5 s
- registrations (10000): 45 s
loaded; sink stats at http://127.0.0.1:9800/stats
"""

SAMPLE_SUBS = """10000 subscriptions over 100 tenants

| class | entities (p=0) | filter (p=0) | fires on | count |
|---|---|---|---|---|
| vehicle-any | [{"type": "Vehicle"}] | {"q": "speed>100"} … p = k // tenants | Vehicle updates with speed > 100+p (all) | 1300 |
| vehicle-cold-attr | [{"type": "Vehicle"}] | {"watchedAttributes": ["brand"], "q": "speed>0"} … p = k // tenants | never (updates touch speed) | 1300 |
| vehicle-high-speed | [{"type": "Vehicle"}] | {"q": "speed>500000000"} … p = k // tenants | Vehicle updates with speed > 5e8 + p·1e6 (about half) | 1300 |
| vehicle-id-tail | [{"type": "Vehicle", "idPattern": ".*0$"}] | {"q": "speed>100"} … p = k // tenants | Vehicle updates on ids ending in p % 10 (a tenth) | 1300 |
| building-any | [{"type": "Building"}] | {"q": "temperature>20"} … p = k // tenants | Building updates with temperature > 20+p (all) | 1200 |
| sensor-any | [{"type": "Sensor"}] | {"q": "value>0"} … p = k // tenants | Sensor updates with value > p (all) | 1200 |
| vehicle-geo-west | [{"type": "Vehicle"}] | {"geoQ": {"geometry": "Polygon", "georel": "within", "coordinates": [[[16.7, 47.6], [18.2499, 47.6], [18.2499, 49.7], [16.7, 49.7], [16.7, 47.6]]]}} … p = k // tenants | Vehicle updates on ids with n % 1000 < 250 + 5p (west of the polygon's edge) | 1200 |
| any-scope | [{"type": "Vehicle"}, {"type": "Building"}, {"type": "Sensor"}] | {"scopeQ": "/region/north/#"} … p = k // tenants | updates on ids whose scope (n % 4) matches SCOPE_Q[p % 4] | 1200 |
"""

SAMPLE_CSR = """10000 registrations over 100 tenants

| class | type | mode, operations | extra | count |
|---|---|---|---|---|
| vehicle-inclusive | Vehicle | inclusive, ["federationOps"] | {} | 1300 |
| building-exclusive | Building | exclusive, ["retrieveOps"] | {} | 1300 |
| sensor-redirect | Sensor | redirect, ["queryEntity", "retrieveEntity"] | {} | 1300 |
| vehicle-auxiliary-csf | Vehicle | auxiliary, ["queryEntity"] | {"sourceType": {"type": "Property", "value": "archive"}} | 1300 |
| building-with-headers | Building | inclusive, ["queryEntity"] | {"contextSourceInfo": [{"key": "X-Perf-Source", "value": "csr"}], "observationInterval": {"startAt": "2020-01-01T00:00:00Z"}} | 1200 |
| sensor-expiring | Sensor | inclusive, ["queryEntity"] | {"expiresAt": "2099-01-01T00:00:00Z"} | 1200 |
| vehicle-geo-west | Vehicle | inclusive, ["queryEntity"] | {"location": "west_polygon(p)"} | 1200 |
| building-scope | Building | inclusive, ["queryEntity"] | {"scopes": "[/region/north | /region/south by p % 2]"} | 1200 |
"""

SAMPLE_FIRE = """In the broker: 1378624 entities, 10000 subscriptions, 10000 registrations over 101 tenants.

| rate (rps) | updates | deletes | reads | failed ops (conn/4xx/5xx) | entity notifications due | delivered | delivered % | subscriptions that fired | notification POSTs | POSTs/s | quiet after (s) | dropped by broker | dead letters | PATCH p99 (ms) | GET p99 (ms) | broker cores | host busy cores |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 100 | 5400 | 601 | 1201 | 0 (0/0/0) | 96710 | 96710 | 100.0 | 6010 | 96710 | 1640.0 | 0 | 0 | 0 | 11.7 | 11.1 | 1.3 | 2.6 |
| 200 | 10800 | 600 | 2400 | 0 (0/0/0) | 193747 | 193747 | 100.0 | 6010 | 193747 | 3257.4 | 0 | 0 | 0 | 13.1 | 11.8 | 2.6 | 5.2 |
| 500 | 27000 | 1800 | 6001 | 0 (0/0/0) | 482137 | 482137 | 100.0 | 6010 | 482077 | 8060.9 | 0 | 0 | 0 | 17.5 | 16.0 | 7.2 | 14.7 |
| 1000 | 54000 | 3001 | 12000 | 0 (0/0/0) | 966544 | 325915 | 33.7 | 6010 | 287845 | 4709.9 | 1 | 38068 | 0 | 95.0 | 70.8 | 12.7 | 20.8 |

Limit: 500 rps (the last rate that delivered 99% with no failed operation).
"""

SAMPLE_FIRE_CLASSES = """Per class (api-load.py SUB_CLASSES; k6-fire.js evaluates the same rule for the count due):

| rate (rps) | class | due | delivered | delivered % |
|---|---|---|---|---|
| 100 | vehicle-any | 21060 | 21060 | 100.0 |
| 100 | vehicle-cold-attr | 0 | 0 | 0.0 |
| 100 | vehicle-high-speed | 9590 | 9590 | 100.0 |
| 100 | vehicle-id-tail | 2340 | 2340 | 100.0 |
| 100 | building-any | 19440 | 19440 | 100.0 |
| 100 | sensor-any | 19440 | 19440 | 100.0 |
| 100 | vehicle-geo-west | 8640 | 8640 | 100.0 |
| 100 | any-scope | 16200 | 16200 | 100.0 |
| 200 | vehicle-any | 42120 | 42120 | 100.0 |
| 200 | vehicle-cold-attr | 0 | 0 | 0.0 |
| 200 | vehicle-high-speed | 19507 | 19507 | 100.0 |
| 200 | vehicle-id-tail | 4680 | 4680 | 100.0 |
| 200 | building-any | 38880 | 38880 | 100.0 |
| 200 | sensor-any | 38880 | 38880 | 100.0 |
| 200 | vehicle-geo-west | 17280 | 17280 | 100.0 |
| 200 | any-scope | 32400 | 32400 | 100.0 |
| 500 | vehicle-any | 105300 | 105300 | 100.0 |
| 500 | vehicle-cold-attr | 0 | 0 | 0.0 |
| 500 | vehicle-high-speed | 46537 | 46537 | 100.0 |
| 500 | vehicle-id-tail | 11700 | 11700 | 100.0 |
| 500 | building-any | 97200 | 97200 | 100.0 |
| 500 | sensor-any | 97200 | 97200 | 100.0 |
| 500 | vehicle-geo-west | 43200 | 43200 | 100.0 |
| 500 | any-scope | 81000 | 81000 | 100.0 |
| 1000 | vehicle-any | 210600 | 71396 | 33.9 |
| 1000 | vehicle-cold-attr | 0 | 0 | 0.0 |
| 1000 | vehicle-high-speed | 95344 | 31624 | 33.2 |
| 1000 | vehicle-id-tail | 23400 | 7938 | 33.9 |
| 1000 | building-any | 194400 | 65784 | 33.8 |
| 1000 | sensor-any | 194400 | 65892 | 33.9 |
| 1000 | vehicle-geo-west | 86400 | 28309 | 32.8 |
| 1000 | any-scope | 162000 | 54972 | 33.9 |
"""

SAMPLE_FED = """Federated queries over 10000 registrations (every source is the sink, answering empty).

| rate (rps) | queries | failed (conn/4xx/5xx) | with a source warning | GET p99 (ms) | source calls | calls per query | broker cores | host busy cores |
|---|---|---|---|---|---|---|---|---|
| 50 | 1501 | 0 (0/0/0) | 0 | 436.6 | 51038 | 34.00 | 1.4 | 3.8 |
| 100 | 3001 | 0 (0/0/0) | 0 | 448.2 | 102038 | 34.00 | 3.1 | 7.3 |
| 200 | 5783 | 0 (0/0/0) | 0 | 2357.9 | 197256 | 34.11 | 6.5 | 15.0 |
| 500 | 9498 | 0 (0/0/0) | 0 | 27476.1 | 322950 | 34.00 | 6.7 | 14.8 |
"""

SAMPLE_MANIFEST = """# scale-weekly results — what each file is
Rig: broker, postgres, mosquitto, sink, k6.
- rss.csv — 1 Hz sampler
- rss.md — RSS and CPU peaks
- load.md — dataset sizes and load wall times
- shapes.md — req/s + p99 per request shape
- fire.md — subscriptions under update stream
- fed.md — federated queries
- scenarios/ — edge and topology scenario tables
- report.pdf — narrated report
- perf.json + index.html — folded report
"""

SAMPLE_VERDICTS = """| scenario | verdict | limit or key number | note |
|---|---|---|---|
| hot-entity | PASS | 1000 rps | 5.6.3 multi-instance datasetId updates preserved |
| noisy-tenant | PASS | 500 rps | quiet tenant unaffected by loud flood |
| slow-subscriber | PASS | 200 rps | deliveryWidthPerTenant isolated slow endpoint |
| fan-in | PASS | 100 rps | 50 000 notifs/s delivered |
| hub-sources | PASS | 200 rps | federated reads and merges complete |
| collision | PASS | 409 Conflict | 4.3.6.2 aux local-wins and 5.9.2.4 conflicts |
| loop | PASS | 508 Loop Detected | 6.3.17 loop cut by Via header |
| distributed-subscription | PASS | 100 rps | 5.8.1.4 remote changes notified to hub |
| ha-pair | PASS | zero duplicates | writes across pods notified exactly once |
"""

SAMPLE_HOT_ENTITY = """Contention on 1 hot entity vs spread over 1000 entities.

| rate | spread | req/s | p99 | 409/5xx | notes |
|---|---|---|---|---|---|
| 100 | 1 hot entity | 100 | 2.1 ms | 0 | row lock contention |
| 200 | 1 hot entity | 200 | 3.5 ms | 0 | row lock contention |
"""

SAMPLE_HEALTH = """{"changesDropped":38068,"commit":"799f38b","deadLetters":0,"limits":{"changeQueue":1024,"deliveryWidth":64,"maxBatchItems":1000,"maxBodyBytes":4194304,"maxUriBytes":8192},"memory":{"allocatedBytes":119026344,"residentBytes":3189354496},"status":"UP","store":"postgres","version":"0.1.0"}
"""


def generate_rss_csv() -> str:
    lines = ["t,broker_kib,postgres_kib,broker_cpu_pct,postgres_cpu_pct,host_busy_cores,host_cores,k6_kib,k6_cpu_pct,sink_kib,sink_cpu_pct,mqtt_kib,mqtt_cpu_pct,phase"]
    t0 = 1788541449
    for i in range(25):
        lines.append(f"{t0 + i},{22000 + i * 50},{800000 + i * 1000},{15.0 + (i % 5)},{20.0 + (i % 8)},{2.5 + (i % 3)},32,500,2.0,200,1.0,4000,0.1,test_phase")
    return "\n".join(lines) + "\n"


class TestPdfReport(unittest.TestCase):

    def test_full_report_build(self):
        tmp_dir = tempfile.mkdtemp()
        try:
            p = Path(tmp_dir)
            (p / "startup.md").write_text(SAMPLE_STARTUP)
            (p / "shapes.md").write_text(SAMPLE_SHAPES)
            (p / "saturate.md").write_text(SAMPLE_SATURATE)
            (p / "rss.md").write_text(SAMPLE_RSS)
            (p / "load.md").write_text(SAMPLE_LOAD)
            (p / "subs.md").write_text(SAMPLE_SUBS)
            (p / "csr.md").write_text(SAMPLE_CSR)
            (p / "fire.md").write_text(SAMPLE_FIRE)
            (p / "fire-classes.md").write_text(SAMPLE_FIRE_CLASSES)
            (p / "fed.md").write_text(SAMPLE_FED)
            (p / "MANIFEST.md").write_text(SAMPLE_MANIFEST)
            (p / "health-final.json").write_text(SAMPLE_HEALTH)
            (p / "rss.csv").write_text(generate_rss_csv())

            scen_dir = p / "scenarios"
            scen_dir.mkdir(parents=True, exist_ok=True)
            (scen_dir / "verdicts.md").write_text(SAMPLE_VERDICTS)
            (scen_dir / "hot-entity.md").write_text(SAMPLE_HOT_ENTITY)

            record = {
                "commit": "799f38b",
                "host": "x86_64 32 cpus (ccx53)",
                "tables": {
                    "startup": pdf.md_table(SAMPLE_STARTUP),
                    "shapes": pdf.md_table(SAMPLE_SHAPES),
                    "saturate": pdf.md_table(SAMPLE_SATURATE),
                    "rss": pdf.md_table(SAMPLE_RSS),
                    "subs": pdf.md_table(SAMPLE_SUBS),
                    "csr": pdf.md_table(SAMPLE_CSR),
                    "fire": pdf.md_table(SAMPLE_FIRE),
                    "fire-classes": pdf.md_table(SAMPLE_FIRE_CLASSES),
                    "fed": pdf.md_table(SAMPLE_FED),
                    "verdicts": pdf.md_table(SAMPLE_VERDICTS),
                    "hot-entity": pdf.md_table(SAMPLE_HOT_ENTITY),
                }
            }

            pdf_path = pdf.build(tmp_dir, record)
            self.assertIsNotNone(pdf_path)
            self.assertTrue(os.path.exists(pdf_path))
            file_size = os.path.getsize(pdf_path)
            self.assertGreater(file_size, 40 * 1024, f"PDF file size {file_size} should be > 40 KB")

            content = open(pdf_path, "rb").read()
            pages = re.findall(rb"/Type\s*/Page\b", content)
            page_count = len(pages)
            self.assertGreaterEqual(page_count, 12, f"Expected at least 12 pages, got {page_count}")
        finally:
            shutil.rmtree(tmp_dir)

    def test_empty_dir_build(self):
        tmp_dir = tempfile.mkdtemp()
        try:
            record = {"commit": "none", "host": "empty", "tables": {}}
            pdf_path = pdf.build(tmp_dir, record)
            self.assertIsNotNone(pdf_path)
            self.assertTrue(os.path.exists(pdf_path))
            self.assertGreater(os.path.getsize(pdf_path), 1000)
        finally:
            shutil.rmtree(tmp_dir)

    def test_startup_only_build(self):
        tmp_dir = tempfile.mkdtemp()
        try:
            p = Path(tmp_dir)
            (p / "startup.md").write_text(SAMPLE_STARTUP)
            record = {
                "commit": "799f38b",
                "host": "test-host",
                "tables": {
                    "startup": pdf.md_table(SAMPLE_STARTUP),
                }
            }
            pdf_path = pdf.build(tmp_dir, record)
            self.assertIsNotNone(pdf_path)
            self.assertTrue(os.path.exists(pdf_path))
            self.assertGreater(os.path.getsize(pdf_path), 1000)
        finally:
            shutil.rmtree(tmp_dir)


if __name__ == "__main__":
    unittest.main()
