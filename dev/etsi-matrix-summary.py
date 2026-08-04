#!/usr/bin/env python3
"""Concat store x suite cell results into 4 per-store tables (same
| Suite | Pass | Fail | Skip | shape as run-summary.md) and gate:
exit nonzero unless ALL cells are green.

Usage: etsi-matrix-summary.py <cells-dir>
  <cells-dir> holds one ETSI-cell-<store>-<suite>/ dir per matrix cell
  (CI: downloaded artifacts; local: written by dev/etsi-local.sh).
Appends to $GITHUB_STEP_SUMMARY when set (CI); always prints to stdout.
"""
import glob
import os
import re
import sys

STORES = ["memory", "file", "postgres", "timescale"]
SUITES = ["CommonBehaviours", "Consumption", "Provision", "Subscription",
          "ContextSource", "jsonldContext", "DistributedOperations", "IOP"]

cells_dir = sys.argv[1] if len(sys.argv) > 1 else "cells"
red, out_md = [], []
for st in STORES:
    out_md.append(f"## ETSI results — store: `{st}`\n")
    out_md.append("| Suite | Gate | Pass | Fail | Skip |\n|---|---|---|---|---|")
    for su in SUITES:
        d = f"{cells_dir}/ETSI-cell-{st}-{su}"
        gate, p, f, s = "MISSING", "—", "—", "—"
        try:
            gate = open(glob.glob(f"{d}/**/gate-status.txt", recursive=True)[0]).read().strip()
        except (IndexError, OSError):
            pass
        # sum the suite rows of the cell's run-summary table (one suite ran,
        # so the sum is that suite's counts whatever the results dir is named)
        try:
            md = open(glob.glob(f"{d}/**/run-summary.md", recursive=True)[0]).read()
            rows = re.findall(r"^\| (?!Suite)(\S+) \| (\d+) \| (\d+) \| (\d+) \|$", md, re.M)
            if rows:
                p, f, s = (sum(int(r[i]) for r in rows) for i in (1, 2, 3))
        except (IndexError, OSError):
            pass
        mark = "✅" if gate == "PASS" else "❌"
        if gate != "PASS":
            red.append(f"{st} × {su}: {gate}")
        out_md.append(f"| {su} | {mark} {gate} | {p} | {f} | {s} |")
    out_md.append("")

report = "\n".join(out_md) + "\n"
if red:
    report += "\n**Red cells:**\n" + "".join(f"- {r}\n" for r in red)
print(report)
if os.environ.get("GITHUB_STEP_SUMMARY"):
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as out:
        out.write(report)
sys.exit(f"{len(red)} red cell(s)" if red else 0)
