#!/usr/bin/env python3
"""Fold the per-store ETSI results into ONE bundle + ONE report and gate.

Usage: etsi-matrix-summary.py <cells-dir>
  <cells-dir> holds one ETSI-cell-<store>/ dir per store job — each the
  results of ONE pipeline run over ALL suites (CI: downloaded artifacts;
  local: written by dev/etsi-local.sh). Expected stores come from the
  STORES env (default "file postgres timescale"); a missing dir is a
  red cell, not a silent skip.

Writes <cells-dir>/_combined/:
  all-resource-samples.csv  EVERY 1 Hz CPU/RSS sample from EVERY store run,
                            each row prefixed with its store (`phase` already
                            names the suite, `test` the TP under test)
  all-failures.csv          every failing TP from every store: store, suite,
                            test, the keyword that ran (top step → failing
                            call), the FAIL log (expected vs got), message
  matrix-summary.md         the report below, as a file

Prints the report to stdout (the workflow appends it to the step summary
together with the bundle download link). Exit: nonzero unless ALL stores
green.
"""
import csv
import glob
import os
import re
import sys
from collections import defaultdict

STORES = (os.environ.get("STORES") or "file postgres timescale").split()
SAMPLE_COLS = ["run", "ts", "iso", "container", "phase", "test", "cpu_pct", "rss_mib"]
FAIL_COLS = ["run", "suite", "test", "tags", "start", "elapsed_s",
             "keyword", "fail_log", "message"]

cells_dir = sys.argv[1] if len(sys.argv) > 1 else "cells"
comb = os.path.join(cells_dir, "_combined")
os.makedirs(comb, exist_ok=True)


def find(d, name):
    hits = glob.glob(f"{d}/**/{name}", recursive=True)
    return hits[0] if hits else None


red, out_md, all_failures = [], [], []
samples_out = open(f"{comb}/all-resource-samples.csv", "w", newline="")
samples_w = csv.DictWriter(samples_out, SAMPLE_COLS, extrasaction="ignore")
samples_w.writeheader()

for st in STORES:
    d = f"{cells_dir}/ETSI-cell-{st}"
    gate = "MISSING"
    path = find(d, "gate-status.txt")
    if path:
        gate = open(path).read().strip()
    mark = "✅" if gate == "PASS" else "❌"
    if gate != "PASS":
        red.append(f"{st}: {gate}")

    # per-suite pass/fail/skip rows straight from the store's run-summary
    suite_rows = []
    path = find(d, "run-summary.md")
    if path:
        suite_rows = re.findall(r"^\| (?!Suite)(\S+) \| (\d+) \| (\d+) \| (\d+) \|$",
                                open(path).read(), re.M)

    # per-second samples: fold into the combined file; roll up per suite
    # via the sampler's phase label (the suite under test at that second)
    res = defaultdict(lambda: {"rss": [], "cpu": []})
    path = find(d, "resource-samples.csv")
    if path:
        for r in csv.DictReader(open(path)):
            r["run"] = st
            samples_w.writerow(r)
            bucket = res[r.get("phase") or "?"]
            try:
                bucket["rss"].append(float(r["rss_mib"]))
            except (KeyError, TypeError, ValueError):
                pass
            try:
                bucket["cpu"].append(float(r["cpu_pct"]))
            except (KeyError, TypeError, ValueError):
                pass

    path = find(d, "failures.csv")
    if path:
        for r in csv.DictReader(open(path)):
            r["run"] = st
            all_failures.append(r)

    out_md.append(f"## ETSI results — store: `{st}` — gate {mark} {gate}\n")
    out_md.append("| Suite | Pass | Fail | Skip | RSS avg | RSS peak | CPU peak | Samples |")
    out_md.append("|---|---|---|---|---|---|---|---|")
    for name, p, f, s in suite_rows:
        rss, cpu = res[name]["rss"], res[name]["cpu"]
        rss_avg = f"{sum(rss) / len(rss):.0f} MiB" if rss else "—"
        rss_peak = f"{max(rss):.0f} MiB" if rss else "—"
        cpu_peak = f"{max(cpu):.0f}%" if cpu else "—"
        out_md.append(f"| {name} | {p} | {f} | {s} "
                      f"| {rss_avg} | {rss_peak} | {cpu_peak} | {max(len(rss), len(cpu))} |")
    if suite_rows:
        tp, tf, ts = (sum(int(row[i]) for row in suite_rows) for i in (1, 2, 3))
        out_md.append(f"| **Total** | **{tp}/{tp + tf + ts}** | {tf} | {ts} | | | | |")
    else:
        out_md.append("| _no results produced_ | — | — | — | — | — | — | — |")
    out_md.append("")

samples_out.close()
with open(f"{comb}/all-failures.csv", "w", newline="") as fh:
    w = csv.DictWriter(fh, FAIL_COLS, extrasaction="ignore")
    w.writeheader()
    w.writerows(all_failures)

# ---- the detailed error list (full data lives in all-failures.csv) ----
out_md.append(f"## All failures — {len(all_failures)} across the matrix\n")
if not all_failures:
    out_md.append("None. 🎉\n")
SHOW = 200
for i, fl in enumerate(all_failures[:SHOW], 1):
    detail = fl.get("fail_log") or fl.get("message") or ""
    out_md.append(f"**{i}. `{fl['run']} × {fl.get('suite', '?')}` — {fl.get('test', '?')}**")
    if fl.get("keyword"):
        out_md.append(f"- ran: `{fl['keyword'][:300]}`")
    out_md.append(f"- error: {detail[:600]}")
    out_md.append("")
if len(all_failures) > SHOW:
    out_md.append(f"…and {len(all_failures) - SHOW} more — see `all-failures.csv` in the bundle.\n")
if red:
    out_md.append("**Red stores:**\n" + "".join(f"- {r}\n" for r in red))

report = "\n".join(out_md) + "\n"
open(f"{comb}/matrix-summary.md", "w").write(report)
print(report)
sys.exit(f"{len(red)} red store(s)" if red else 0)
