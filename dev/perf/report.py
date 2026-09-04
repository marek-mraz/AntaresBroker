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
    ("rss.md", "Resident set and CPU under load"),
    ("load.md", "Dataset load"),
    ("subs.md", "Subscription classes"),
    ("csr.md", "Context source registration classes"),
    ("fire.md", "Subscriptions under an update stream"),
    ("fire-classes.md", "Notifications due and delivered per subscription class"),
    ("fed.md", "Federated queries over the registrations"),
    ("noise-profile.txt", "Noise profile"),
]

# What each table measures, printed under its title so the numbers read
# without the scripts open.
LEGEND = {
    "shapes.md": (
        "query = GET /entities?type=Vehicle&limit=20; retrieve = GET /entities/{id}. "
        "c50 / c200 = that many closed-loop clients, each sending the next request as soon as the "
        "previous one answers; req/s is what the broker sustained at that concurrency. "
        "postgres rows run on the main broker over the LOADED dataset (tenant t7), memory rows on a "
        "fresh in-memory broker holding the 100 seeded entities."
    ),
    "saturate.md": (
        "Open-loop arrival rate stepped up by STEP rps per stage on a FRESH broker in the default "
        "tenant (100 seeded entities, not the loaded dataset); query = the shapes query, write = "
        "POST /entities. knee = the last stage whose p99 stayed under P99_MS and error rate under "
        "ERR; 'none reached' = every stage held, the ceiling is above STAGES*STEP."
    ),
    "rss.md": (
        "1 Hz samples from rss.sh: RSS and CPU per service (100 % = one core); the rig's own "
        "processes (k6, sink, mosquitto) share the machine with the broker and Postgres."
    ),
    "fire.md": (
        "Updates + deletes streamed at the rate over the loaded entities with every subscription "
        "live; due = notifications the classes' rules say must fire, delivered = what the sink "
        "received; dropped by broker = the change queue was full (antares_notification_changes_dropped_total)."
    ),
    "fed.md": (
        "GET /entities fanned out over the matching registrations, every source answering empty at "
        "the sink; calls per query = how many sources one query dialled."
    ),
}


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
        legend = LEGEND.get(name)
        sections.append((title, f"{legend}\n\n{text}" if legend else text))
    if readme:
        for title, text in sections:
            print(f"### {title}\n\n{text.strip()}\n")
        return
    json.dump(record, open(os.path.join(out, "perf.json"), "w"), indent=1)
    body = [f"<h1>Antares performance run</h1><p>commit {html.escape(record['commit'])}, {html.escape(record['host'])}</p>"]
    for title, text in sections:
        rows = md_table(text)
        body.append(f"<h2>{html.escape(title)}</h2>")
        prose = " ".join(l for l in text.splitlines() if l.strip() and not l.startswith("|"))
        if rows and prose:
            body.append(f"<p>{html.escape(prose)}</p>")
        if rows:
            body.append("<table>" + "".join(
                "<tr>" + "".join(("<th>" if i == 0 else "<td>") + html.escape(c) + ("</th>" if i == 0 else "</td>") for c in r) + "</tr>"
                for i, r in enumerate(rows)) + "</table>")
        else:
            body.append(f"<pre>{html.escape(text)}</pre>")
    for name in sorted(os.listdir(out)):
        if name.endswith((".csv", ".pdf", ".json")):
            body.append(f'<p><a href="{html.escape(name)}">{html.escape(name)}</a></p>')
    page = ("<!doctype html><meta charset=utf-8><title>Antares perf</title>"
            "<style>body{font-family:system-ui;max-width:60rem;margin:2rem auto}table{border-collapse:collapse}"
            "td,th{border:1px solid #ccc;padding:.3rem .6rem}</style>" + "".join(body))
    open(os.path.join(out, "index.html"), "w").write(page)
    print(f"{out}/index.html + perf.json: {len(sections)} sections")
    sys.path.insert(0, os.path.dirname(__file__))
    try:
        import pdf
        pdf.build(out, record)
    except Exception as e:
        print(f"pdf generation skipped: {e}")


if __name__ == "__main__":
    main()
