#!/usr/bin/env python3
"""Fold the store × suite cell results into ONE bundle + ONE report and gate.

Usage: etsi-matrix-summary.py <cells-dir>
  <cells-dir> holds one ETSI-cell-<store>-<suite>/ dir per matrix cell
  (CI: downloaded artifacts; local: written by dev/etsi-local.sh).

Writes <cells-dir>/_combined/:
  all-resource-samples.csv  EVERY 1 Hz CPU/RSS sample from EVERY run, each row
                            prefixed with the run it came from
  all-failures.csv          every failing TP from every run: run, test, the
                            keyword that ran (top step → failing call), the
                            FAIL log (expected vs got) and the message
  matrix-summary.md         the report below, as a file

Prints the report to stdout (the workflow appends it to the step summary
together with the bundle download link). Exit: nonzero unless ALL cells green.
"""
import csv
import glob
import os
import re
import sys

STORES = ["memory", "file", "postgres", "timescale"]
SUITES = ["CommonBehaviours", "Consumption", "Provision", "Subscription",
          "ContextSource", "jsonldContext", "DistributedOperations", "IOP"]
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
    out_md.append(f"## ETSI results — store: `{st}`\n")
    out_md.append("| Suite | Gate | Pass | Fail | Skip | RSS avg | RSS peak | CPU peak | Samples |")
    out_md.append("|---|---|---|---|---|---|---|---|---|")
    for su in SUITES:
        d = f"{cells_dir}/ETSI-cell-{st}-{su}"
        run = f"{st} × {su}"
        gate, p, f, s = "MISSING", "—", "—", "—"
        path = find(d, "gate-status.txt")
        if path:
            gate = open(path).read().strip()
        # sum the suite rows of the cell's run-summary table (one suite ran,
        # so the sum is that suite's counts whatever the results dir is named)
        path = find(d, "run-summary.md")
        if path:
            rows = re.findall(r"^\| (?!Suite)(\S+) \| (\d+) \| (\d+) \| (\d+) \|$",
                              open(path).read(), re.M)
            if rows:
                p, f, s = (sum(int(r[i]) for r in rows) for i in (1, 2, 3))
        # per-second samples: fold into the combined file, roll up for the table
        rss, cpu = [], []
        path = find(d, "resource-samples.csv")
        if path:
            for r in csv.DictReader(open(path)):
                r["run"] = run
                samples_w.writerow(r)
                try:
                    rss.append(float(r["rss_mib"]))
                except (KeyError, TypeError, ValueError):
                    pass
                try:
                    cpu.append(float(r["cpu_pct"]))
                except (KeyError, TypeError, ValueError):
                    pass
        rss_avg = f"{sum(rss) / len(rss):.0f} MiB" if rss else "—"
        rss_peak = f"{max(rss):.0f} MiB" if rss else "—"
        cpu_peak = f"{max(cpu):.0f}%" if cpu else "—"
        # failures: fold into the combined list
        path = find(d, "failures.csv")
        if path:
            for r in csv.DictReader(open(path)):
                r["run"] = run
                all_failures.append(r)
        mark = "✅" if gate == "PASS" else "❌"
        if gate != "PASS":
            red.append(f"{st} × {su}: {gate}")
        out_md.append(f"| {su} | {mark} {gate} | {p} | {f} | {s} "
                      f"| {rss_avg} | {rss_peak} | {cpu_peak} | {len(rss)} |")
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
    out_md.append(f"**{i}. `{fl['run']}` — {fl.get('test', '?')}**")
    if fl.get("keyword"):
        out_md.append(f"- ran: `{fl['keyword'][:300]}`")
    out_md.append(f"- error: {detail[:600]}")
    out_md.append("")
if len(all_failures) > SHOW:
    out_md.append(f"…and {len(all_failures) - SHOW} more — see `all-failures.csv` in the bundle.\n")
if red:
    out_md.append("**Red cells:**\n" + "".join(f"- {r}\n" for r in red))

report = "\n".join(out_md) + "\n"
open(f"{comb}/matrix-summary.md", "w").write(report)
print(report)
sys.exit(f"{len(red)} red cell(s)" if red else 0)
