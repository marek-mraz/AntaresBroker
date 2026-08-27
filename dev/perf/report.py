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


def pdf(out, record, sections):
    """report.pdf: every table plus the RSS/CPU timeline and the delivery
    curve, from rss.csv and fire.md. matplotlib only; absent → no PDF."""
    try:
        import csv
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        from matplotlib.backends.backend_pdf import PdfPages
    except ImportError:
        print("matplotlib missing: no report.pdf")
        return
    with PdfPages(os.path.join(out, "report.pdf")) as pp:
        fig = plt.figure(figsize=(8.27, 11.69))
        fig.text(0.5, 0.9, "Antares performance run", ha="center", fontsize=20)
        fig.text(0.5, 0.86, f"commit {record['commit']}, {record['host']}", ha="center", fontsize=11)
        fig.text(0.1, 0.8, "\n".join(f"- {t}" for t, _ in sections), fontsize=10, va="top")
        pp.savefig(fig); plt.close(fig)
        rss = os.path.join(out, "rss.csv")
        if os.path.exists(rss):
            rows = list(csv.DictReader(open(rss)))
            if rows:
                t0 = int(rows[0]["t"]); t = [int(r["t"]) - t0 for r in rows]
                f = lambda k: [float(r.get(k) or 0) for r in rows]
                fig, ax = plt.subplots(3, 1, figsize=(8.27, 11.69), sharex=True)
                ax[0].plot(t, [v / 1024 for v in f("broker_kib")], label="broker RSS (MiB)")
                ax[0].plot(t, [v / 1024 / 1024 * 1024 for v in f("postgres_kib")], label="postgres RSS (MiB)")
                ax[0].set_ylabel("MiB"); ax[0].legend(); ax[0].set_title("Resident set")
                ax[1].plot(t, [v / 100 for v in f("broker_cpu_pct")], label="broker cores")
                ax[1].plot(t, [v / 100 for v in f("postgres_cpu_pct")], label="postgres cores")
                ax[1].set_ylabel("cores"); ax[1].legend(); ax[1].set_title("CPU per process (1 = one core)")
                cores = float(rows[0].get("host_cores") or 0)
                ax[2].plot(t, f("host_busy_cores"), label="host busy cores")
                if cores:
                    ax[2].axhline(cores, color="red", ls="--", label=f"{cores:.0f} cores = saturated")
                ax[2].set_ylabel("cores"); ax[2].set_xlabel("seconds since the broker started"); ax[2].legend()
                ax[2].set_title("Whole machine")
                pp.savefig(fig); plt.close(fig)
        fire = os.path.join(out, "fire.md")
        if os.path.exists(fire):
            rows = md_table(open(fire).read())
            if len(rows) > 1:
                head, data = rows[0], rows[1:]
                col = lambda name: [r[head.index(name)] for r in data if name in head]
                try:
                    rates = [int(x) for x in col("rate (rps)")]
                    pct = [float(x) for x in col("delivered %")]
                    fig, ax = plt.subplots(figsize=(8.27, 5))
                    ax.plot(rates, pct, marker="o"); ax.set_ylim(0, 105)
                    ax.set_xlabel("update rate (rps)"); ax.set_ylabel("notifications delivered %")
                    ax.set_title("Subscriptions under the update stream"); ax.grid(True)
                    pp.savefig(fig); plt.close(fig)
                except (ValueError, IndexError):
                    pass
        for title, text in sections:
            rows = md_table(text)
            fig = plt.figure(figsize=(11.69, 8.27))
            fig.text(0.02, 0.96, title, fontsize=14, va="top")
            if rows:
                w = len(rows[0])
                cells = [([c[:60] for c in r] + [""] * w)[:w] for r in rows[1:]] or [[""] * w]
                tbl = fig.add_axes([0.02, 0.02, 0.96, 0.9]); tbl.axis("off")
                tab = tbl.table(cellText=cells, colLabels=rows[0], loc="upper left", cellLoc="left")
                tab.auto_set_font_size(False); tab.set_fontsize(6); tab.scale(1, 1.2)
            else:
                fig.text(0.02, 0.9, text[:4000], fontsize=7, va="top", family="monospace")
            pp.savefig(fig); plt.close(fig)
    print(f"{out}/report.pdf")


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
        if name.endswith((".csv", ".pdf", ".json")):
            body.append(f'<p><a href="{html.escape(name)}">{html.escape(name)}</a></p>')
    page = ("<!doctype html><meta charset=utf-8><title>Antares perf</title>"
            "<style>body{font-family:system-ui;max-width:60rem;margin:2rem auto}table{border-collapse:collapse}"
            "td,th{border:1px solid #ccc;padding:.3rem .6rem}</style>" + "".join(body))
    open(os.path.join(out, "index.html"), "w").write(page)
    print(f"{out}/index.html + perf.json: {len(sections)} sections")
    pdf(out, record, sections)


if __name__ == "__main__":
    main()
