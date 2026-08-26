#!/usr/bin/env python3
"""Render the ETSI matrix results as a static site.

Usage: etsi-report-site.py <cells-dir> <out-dir>
  <cells-dir> is the ETSI-matrix-results bundle layout: one
  ETSI-cell-<store>/ per store (run-summary.md, gate-status.txt,
  failures.csv, per-suite robot report.html/log.html). Cells come from the
  STORES env when set (a listed-but-missing cell renders red); otherwise
  they are AUTO-DISCOVERED from the bundle, so the Pages fold renders
  whatever the newest matrix actually ran — a 3-cell bundle from before the
  six-cell matrix still renders green instead of inventing missing cells.

Writes <out-dir>/:
  index.html      per-store pass/total + per-suite rows, each linking the
                  store's Robot report/log for that suite — the stats are
                  ON the page, nothing to download
  badge.json      shields.io endpoint schema ("ETSI 6×1652 green" / red)
  badge-<cell>.json  one endpoint badge PER CELL ("1652/1652", 08-15b item 6)
  <store>/<suite>/report.html|log.html   copied from the bundle
"""
import glob
import html
import json
import os
import re
import shutil
import sys

cells = sys.argv[1] if len(sys.argv) > 1 else "cells"
out = sys.argv[2] if len(sys.argv) > 2 else "site/reports/latest"
os.makedirs(out, exist_ok=True)

PREFERRED = "memory file postgres timescale postgres-nats timescale-nats".split()
if os.environ.get("STORES"):
    STORES = os.environ["STORES"].split()
else:
    found = [
        os.path.basename(d)[len("ETSI-cell-"):]
        for d in glob.glob(f"{cells}/ETSI-cell-*")
        if os.path.isdir(d)
    ]
    STORES = [s for s in PREFERRED if s in found] + sorted(
        s for s in found if s not in PREFERRED
    )
    if not STORES:  # empty/absent bundle: keep the historical shape (renders red)
        STORES = ["file", "postgres", "timescale"]


def find(d, name):
    hits = sorted(glob.glob(f"{d}/**/{name}", recursive=True))
    return hits[0] if hits else None


stores = []
for st in STORES:
    d = f"{cells}/ETSI-cell-{st}"
    gate = "MISSING"
    p = find(d, "gate-status.txt")
    if p:
        gate = open(p).read().strip()
    suites = []
    p = find(d, "run-summary.md")
    summary_dir = os.path.dirname(p) if p else d
    if p:
        for name, ok, fail, skip in re.findall(
            r"^\| (?!Suite)(\S+) \| (\d+) \| (\d+) \| (\d+) \|$", open(p).read(), re.M
        ):
            # the robot artifacts for this suite live next to run-summary.md
            link = None
            for f in ("report.html", "log.html"):
                src = find(os.path.join(summary_dir, name), f)
                if src:
                    dst = os.path.join(out, st, name, f)
                    os.makedirs(os.path.dirname(dst), exist_ok=True)
                    shutil.copyfile(src, dst)
                    if link is None:
                        link = f"{st}/{name}/{f}"
            suites.append((name, int(ok), int(fail), int(skip), link))
    stores.append((st, gate, suites))

all_green = bool(stores) and all(g == "PASS" for _, g, _ in stores)
totals = [sum(ok + f + sk for _, ok, f, sk, _ in su) for _, _, su in stores]
if all_green and totals and len(set(totals)) == 1:
    message = f"{len(stores)}×{totals[0]} green"
elif all_green:
    message = "green"
else:
    red = [st for st, g, _ in stores if g != "PASS"]
    message = "red: " + ",".join(red)
json.dump(
    {
        "schemaVersion": 1,
        "label": "ETSI CIM 009",
        "message": message,
        "color": "brightgreen" if all_green else "red",
    },
    open(os.path.join(out, os.pardir, "badge.json"), "w"),
)

# 08-15b item 6: one shields endpoint badge PER CELL next to the combined
# one — the README's live "file 1652/1652" row reads these.
for st, gate, suites in stores:
    ok = sum(o for _, o, _, _, _ in suites)
    tot = sum(o + f + sk for _, o, f, sk, _ in suites)
    green = gate == "PASS"
    json.dump(
        {
            "schemaVersion": 1,
            "label": st,
            "message": f"{ok}/{tot}" if tot else gate.lower(),
            "color": "brightgreen" if green else "red",
        },
        open(os.path.join(out, os.pardir, f"badge-{st}.json"), "w"),
    )

rows = []
for st, gate, suites in stores:
    ok = sum(o for _, o, _, _, _ in suites)
    tot = sum(o + f + sk for _, o, f, sk, _ in suites)
    mark = "✅" if gate == "PASS" else "❌"
    rows.append(
        f'<section><h2>{mark} {html.escape(st)} — {ok}/{tot} '
        f"(gate {html.escape(gate)})</h2><table>"
        "<tr><th>Suite</th><th>Pass</th><th>Fail</th><th>Skip</th><th>Robot</th></tr>"
    )
    for name, o, f, sk, link in suites:
        robot = f'<a href="{html.escape(link)}">report</a>' if link else "—"
        cls = ' class="bad"' if f else ""
        rows.append(
            f"<tr{cls}><td>{html.escape(name)}</td><td>{o}</td>"
            f"<td>{f}</td><td>{sk}</td><td>{robot}</td></tr>"
        )
    rows.append("</table></section>")

banner = "All stores green" if all_green else "RED cells — see below"
page = f"""<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Antares — ETSI CIM 009 conformance</title>
<style>
  body {{ font: 15px/1.5 system-ui, sans-serif; margin: 2rem auto; max-width: 60rem; padding: 0 1rem; color: #1a1a1a; background: #fff; }}
  h1 {{ font-size: 1.5rem; }}
  nav {{ margin-bottom: 1rem; font-size: .9rem; }}
  nav a {{ margin-right: 1rem; }}
  .banner {{ padding: .6rem 1rem; border-radius: .5rem; font-weight: 600;
             background: {"#e6f6e6" if all_green else "#fde8e8"};
             color: {"#176617" if all_green else "#8f1d1d"}; }}
  table {{ border-collapse: collapse; margin: .5rem 0 1.5rem; width: 100%; }}
  th, td {{ text-align: left; padding: .3rem .7rem; border-bottom: 1px solid #e5e5e5; }}
  tr.bad td {{ background: #fdf0f0; }}
  footer {{ color: #666; font-size: .85rem; }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #111; color: #ddd; }}
    th, td {{ border-color: #333; }}
    tr.bad td {{ background: #3a1d1d; }}
    a {{ color: #7ab8ff; }}
  }}
</style>
<h1>Antares — ETSI NGSI-LD (CIM 009 V1.9.1) conformance</h1>
<nav><a href="../unit/">Unit &amp; integration tests →</a>
<a href="../coverage/">Coverage →</a></nav>
<p class="banner">{banner}</p>
<p>One full run of every ETSI Robot suite per store mode; each suite row
links Robot's own drill-down for that run. Produced by the
<code>etsi-matrix</code> workflow.</p>
{"".join(rows)}
<footer>Generated by dev/etsi-report-site.py from the ETSI-matrix-results
bundle.</footer>
"""
open(os.path.join(out, "index.html"), "w").write(page)
print(f"site: {out}/index.html  badge: {message}")
sys.exit(0 if all_green else 0)
