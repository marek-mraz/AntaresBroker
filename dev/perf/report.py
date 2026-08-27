#!/usr/bin/env python3
"""Fold the per-script tables of one perf run into one page and one record.

    python3 dev/perf/report.py results/perf            # writes index.html + perf.json there
    python3 dev/perf/report.py results/perf --readme   # prints the README block

Every script under dev/perf leaves a Markdown table (`*.md`) and the
loader leaves `load.md`; this collects whatever exists (a partial run
still reports), records the commit and host next to them in `perf.json`
(the machine-readable history one run appends to), and renders a plain
HTML page for the Pages report tree.
"""

import html
import json
import os
import platform
import subprocess
import sys

TABLES = [
    ("startup.md", "Startup and idle footprint"),
    ("shapes.md", "Throughput per request shape"),
    ("core-scale.md", "Core scaling"),
    ("saturate.md", "Saturation knee"),
    ("rss.md", "Resident set under load"),
    ("load.md", "Dataset load"),
    ("noise-profile.txt", "Noise profile"),
]


def commit():
    try:
        return subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], text=True).strip()
    except Exception:
        return "unknown"


def md_table(text):
    """Rows of a pipe table as lists of cells; non-table lines are dropped."""
    rows = []
    for line in text.splitlines():
        if line.startswith("|") and not set(line) <= set("|-: "):
            rows.append([c.strip() for c in line.strip("|").split("|")])
    return rows


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "results/perf"
    readme = "--readme" in sys.argv
    record = {"commit": commit(), "host": f"{platform.machine()} {os.cpu_count()} cpus", "tables": {}}
    sections = []
    for name, title in TABLES:
        path = os.path.join(out, name)
        if not os.path.exists(path):
            continue
        text = open(path).read()
        record["tables"][name.split(".")[0]] = md_table(text)
        sections.append((title, text))
    if readme:
        for title, text in sections:
            print(f"### {title}\n\n{text.strip()}\n")
        return
    json.dump(record, open(os.path.join(out, "perf.json"), "w"), indent=1)
    body = [f"<h1>Antares performance run</h1><p>commit {html.escape(record['commit'])}, {html.escape(record['host'])}</p>"]
    for title, text in sections:
        rows = md_table(text)
        body.append(f"<h2>{html.escape(title)}</h2>")
        if rows:
            body.append("<table>" + "".join(
                "<tr>" + "".join(("<th>" if i == 0 else "<td>") + html.escape(c) + ("</th>" if i == 0 else "</td>") for c in r) + "</tr>"
                for i, r in enumerate(rows)) + "</table>")
        else:
            body.append(f"<pre>{html.escape(text)}</pre>")
    for name in sorted(os.listdir(out)):
        if name.endswith(".csv"):
            body.append(f'<p><a href="{html.escape(name)}">{html.escape(name)}</a></p>')
    page = ("<!doctype html><meta charset=utf-8><title>Antares perf</title>"
            "<style>body{font-family:system-ui;max-width:60rem;margin:2rem auto}table{border-collapse:collapse}"
            "td,th{border:1px solid #ccc;padding:.3rem .6rem}</style>" + "".join(body))
    open(os.path.join(out, "index.html"), "w").write(page)
    print(f"{out}/index.html + perf.json: {len(sections)} sections")


if __name__ == "__main__":
    main()
