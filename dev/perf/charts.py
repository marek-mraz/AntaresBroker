#!/usr/bin/env python3
"""Per-service CPU and memory charts from rss.csv (dev/perf/rss.sh), one
PNG per dimension with the run's phases shaded, so a bad number can be
placed in the stage that produced it.

    python3 dev/perf/charts.py results/perf      # -> results/perf/cpu.png, memory.png
"""
import csv
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

out = Path(sys.argv[1] if len(sys.argv) > 1 else "results/perf")
rows = list(csv.DictReader(open(out / "rss.csv")))
if not rows:
    sys.exit("rss.csv is empty")
t0 = int(rows[0]["t"])
t = [(int(r["t"]) - t0) / 60 for r in rows]
num = lambda k: [float(r.get(k) or 0) for r in rows]
services = [("broker", "broker"), ("postgres", "postgres"), ("k6", "k6"), ("sink", "sink"), ("mosquitto", "mqtt")]
colors = {"broker": "#1f77b4", "postgres": "#d62728", "k6": "#2ca02c", "sink": "#ff7f0e", "mosquitto": "#9467bd"}

# phase spans: contiguous runs of the same phase label
spans, start, cur = [], 0, rows[0].get("phase", "")
for i, r in enumerate(rows):
    if r.get("phase", "") != cur:
        spans.append((t[start], t[i], cur))
        start, cur = i, r.get("phase", "")
spans.append((t[start], t[-1], cur))


def shade(ax):
    for j, (a, b, label) in enumerate(spans):
        if not label:
            continue
        ax.axvspan(a, b, color="0.9" if j % 2 else "0.95", lw=0)
        ax.text((a + b) / 2, ax.get_ylim()[1], label, rotation=90, va="top", ha="center", fontsize=6, color="0.4")


def chart(name, ylabel, series, hline=None):
    fig, ax = plt.subplots(figsize=(14, 5))
    for label, ys in series:
        if any(ys):
            ax.plot(t, ys, label=label, lw=1, color=colors.get(label))
    if hline:
        ax.axhline(hline[0], ls="--", lw=0.8, color="0.3", label=hline[1])
    ax.set_xlabel("minutes since sampler start")
    ax.set_ylabel(ylabel)
    ax.margins(x=0)
    shade(ax)
    ax.legend(loc="upper left", fontsize=8)
    fig.tight_layout()
    fig.savefig(out / f"{name}.png", dpi=110)


cores = int(float(rows[0].get("host_cores") or 0))
chart("cpu", f"cores busy (of {cores})",
      [(n, [v / 100 for v in num(f"{k}_cpu_pct")]) for n, k in services] + [("host busy", num("host_busy_cores"))])
chart("memory", "RSS (MiB)",
      [(n, [v / 1024 for v in num(f"{k}_kib")]) for n, k in services],
      hline=(500, "broker budget 500 MiB"))
# saturation: broker and Postgres against the core count, host busy as the ground
fig, ax = plt.subplots(figsize=(14, 5.5))
ax.fill_between(t, num("host_busy_cores"), color="0.85", lw=0, label="host busy (all processes)")
ax.plot(t, [v / 100 for v in num("broker_cpu_pct")], color=colors["broker"], lw=1.4, label="broker")
ax.plot(t, [v / 100 for v in num("postgres_cpu_pct")], color=colors["postgres"], lw=1.4, label="postgres")
ax.axhline(cores, ls="--", lw=0.8, color="0.3")
ax.text(t[-1], cores, f" {cores} cores = saturated", va="center", fontsize=8)
ax.set_ylim(0, cores * 1.08)
ax.set_xlabel("minutes since sampler start")
ax.set_ylabel("cores busy")
ax.margins(x=0)
shade(ax)
ax.legend(loc="upper left", fontsize=8)
fig.tight_layout()
fig.savefig(out / "cpu-saturation.png", dpi=110)
print(f"charts: {out/'cpu.png'} {out/'memory.png'} {out/'cpu-saturation.png'}; phases: {[s[2] for s in spans if s[2]]}")
