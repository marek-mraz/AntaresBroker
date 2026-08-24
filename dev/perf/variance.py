#!/usr/bin/env python3
"""Noise profile from repeated same-commit benchmark passes.

    python3 dev/perf/variance.py results/perf/summary-*.json

Reads k6 --summary-export files from N identical runs and prints, per
metric, median / Q1 / Q3 / IQR and the outlier fence Q3 + 3*IQR. That
fence — each benchmark judged against its OWN history — is the gate shape
to adopt later; no gate exists until this profile is measured on the
pinned dedicated-vCPU instance with N >= 10.
"""

import json
import statistics
import sys

METRICS = ["http_req_duration", "http_req_failed", "iterations"]
STATS = ["med", "p(95)", "p(99)"]


def main():
    runs = [json.load(open(p)) for p in sys.argv[1:]]
    if len(runs) < 2:
        sys.exit("need at least 2 summary files")
    print(f"passes: {len(runs)}")
    for metric in METRICS:
        for stat in STATS:
            vals = []
            for r in runs:
                m = r.get("metrics", {}).get(metric, {})
                if stat in m:
                    vals.append(float(m[stat]))
                elif stat == "med" and "rate" in m:
                    vals.append(float(m["rate"]))
            if len(vals) < 2:
                continue
            vals.sort()
            q = statistics.quantiles(vals, n=4)
            q1, med, q3 = q[0], q[1], q[2]
            iqr = q3 - q1
            print(
                f"{metric}.{stat}: median={med:.3f} q1={q1:.3f} "
                f"q3={q3:.3f} iqr={iqr:.3f} fence(Q3+3*IQR)={q3 + 3 * iqr:.3f}"
            )


if __name__ == "__main__":
    main()
