#!/usr/bin/env python3
"""Turn one ETSI run's raw output into the reviewable artifacts.

Reads   $RESULTS/<suite>/output.xml  (Robot Framework 7 XML)
        $RESULTS/resource-samples.csv (dev/etsi-sampler.py, 1 Hz)

Writes  $RESULTS/resource-samples.csv  same rows + a `test` column: every
                                       sample is attributed to the TP that was
                                       running when it was taken, so a spike is
                                       traceable to a test, not just a suite
        $RESULTS/resource-by-test.csv  per test × container rollup
                                       (samples, cpu avg/peak, rss avg/peak)
        $RESULTS/failures.csv          EVERY failing TP with its message —
                                       the markdown only shows the first 50
        $RESULTS/run-summary.md        the human view, incl. the spike tables
        $RESULTS/gate-status.txt       PASS/FAIL — suites green AND RSS ≤ limit

The sample↔test join is a post-processing step on timestamps rather than a
runtime hook: Robot already records `start`/`elapsed` per test, so labelling
costs nothing at run time and cannot perturb what it measures.
"""
import bisect
import collections
import csv
import glob
import os
import xml.etree.ElementTree as ET
from datetime import datetime

RESULTS = os.environ["RESULTS"]
STORE = os.environ.get("STORE", "?")
LIMIT = float(os.environ.get("MEM_LIMIT_MB", "350"))
IMAGE_MB = int(os.environ.get("IMAGE_BYTES", "0")) / 1024 / 1024
SAMPLES = f"{RESULTS}/resource-samples.csv"


def epoch(stamp):
    """Robot timestamp -> epoch seconds (local tz, same box as the sampler)."""
    if not stamp:
        return None
    for fmt in ("%Y-%m-%dT%H:%M:%S.%f", "%Y%m%d %H:%M:%S.%f"):
        try:
            return datetime.strptime(stamp, fmt).timestamp()
        except ValueError:
            continue
    return None


# ---------------------------------------------------------------- suites
suites, failures, intervals, suite_spans = [], [], [], []
total_pass = total_fail = 0

for path in sorted(glob.glob(f"{RESULTS}/*/output.xml")):
    name = path.split("/")[-2]
    try:
        root = ET.parse(path).getroot()
    except Exception as e:
        suites.append((name, "—", "—", f"unreadable: {e}"))
        total_fail += 1
        continue
    stat = root.find("./statistics/total/stat")
    p, f, s = (int(stat.get(k, "0")) for k in ("pass", "fail", "skip")) if stat is not None else (0, 0, 0)
    suites.append((name, p, f, s))
    total_pass += p
    total_fail += f + s

    def span(elem):
        """(start, end) epoch of a <test>/<suite> status, or (None, None)."""
        st = elem.find("status")
        if st is None:
            return None, None
        # RF7: start + elapsed. RF<=6: starttime + endtime.
        s = epoch(st.get("start") or st.get("starttime"))
        if st.get("elapsed") is not None and s is not None:
            return s, s + float(st.get("elapsed"))
        return s, epoch(st.get("endtime"))

    for suite_el in root.iter("suite"):
        s, e = span(suite_el)
        if s is not None and e is not None:
            suite_spans.append((s, e, name))

    for test in root.iter("test"):
        st = test.find("status")
        if st is None:
            continue
        msg = (st.text or "").strip()
        start, end = span(test)
        if start is not None and end is not None:
            intervals.append((start, end, name, test.get("name") or "?"))
        if st.get("status") == "FAIL" and "exit-on-failure" not in msg:
            failures.append({
                "suite": name,
                "test": test.get("name") or "?",
                "tags": ";".join(t.text or "" for t in test.iter("tag")),
                "start": st.get("start") or st.get("starttime") or "",
                "elapsed_s": st.get("elapsed") or "",
                "message": " ".join(msg.split()),
            })

intervals.sort()
starts = [i[0] for i in intervals]
# Widest span per suite — the fallback for samples that land between TPs.
suite_bounds = {}
for s, e, n in suite_spans:
    lo, hi = suite_bounds.get(n, (s, e))
    suite_bounds[n] = (min(lo, s), max(hi, e))


def label(ts):
    """Which (suite, test) was running at this epoch second.

    ETSI TPs are short and a 1 Hz sample often lands in a gap — suite setup,
    the between-suite broker reset, teardown. Those samples fall back to
    `(between tests)` inside their suite rather than going unlabelled, because
    a spike during a reset is exactly the kind of thing worth seeing.
    """
    idx = bisect.bisect_right(starts, ts) - 1
    if idx >= 0:
        start, end, suite, test = intervals[idx]
        # +1 s slack: samples are stamped to the second, tests to the microsecond
        if ts <= end + 1:
            return suite, test
    for name, (lo, hi) in suite_bounds.items():
        if lo - 1 <= ts <= hi + 1:
            return name, "(between tests)"
    return "", ""


with open(f"{RESULTS}/failures.csv", "w", newline="") as fh:
    w = csv.DictWriter(fh, ["suite", "test", "tags", "start", "elapsed_s", "message"])
    w.writeheader()
    w.writerows(failures)

# ------------------------------------------------------------- resources
rows = []
try:
    with open(SAMPLES) as fh:
        for r in csv.DictReader(fh):
            try:
                r["ts"] = float(r["ts"])
            except (TypeError, ValueError):
                continue
            r["cpu"] = float(r["cpu_pct"]) if r.get("cpu_pct") else None
            r["rss"] = float(r["rss_mib"]) if r.get("rss_mib") else None
            suite, test = label(r["ts"])
            r["suite"], r["test"] = suite, test
            rows.append(r)
except OSError:
    pass

if rows:  # rewrite the samples file with the test attribution folded in
    with open(SAMPLES, "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["ts", "iso", "container", "phase", "test", "cpu_pct", "rss_mib"])
        for r in rows:
            w.writerow([
                int(r["ts"]), r["iso"], r["container"], r["phase"], r["test"],
                r["cpu_pct"], r["rss_mib"],
            ])

per_container = collections.defaultdict(lambda: {"cpu": [], "rss": []})
per_test = collections.defaultdict(lambda: {"cpu": [], "rss": []})
for r in rows:
    for bucket in (per_container[r["container"]],
                   per_test[(r["phase"], r["test"], r["container"])]):
        if r["cpu"] is not None:
            bucket["cpu"].append(r["cpu"])
        if r["rss"] is not None:
            bucket["rss"].append(r["rss"])

avg = lambda xs: sum(xs) / len(xs) if xs else 0.0

with open(f"{RESULTS}/resource-by-test.csv", "w", newline="") as fh:
    w = csv.writer(fh)
    w.writerow(["phase", "test", "container", "samples",
                "cpu_avg_pct", "cpu_peak_pct", "rss_avg_mib", "rss_peak_mib"])
    for (phase, test, container), d in sorted(per_test.items()):
        w.writerow([
            phase, test, container, max(len(d["cpu"]), len(d["rss"])),
            f"{avg(d['cpu']):.1f}", f"{max(d['cpu'], default=0):.1f}",
            f"{avg(d['rss']):.1f}", f"{max(d['rss'], default=0):.1f}",
        ])

# ------------------------------------------------------------------ gate
peaks = {n: max(d["rss"], default=0) for n, d in per_container.items()}
# Some daemons (DinD without a delegated memory cgroup) report nothing — say
# so rather than letting an unmeasured gate pass silently; CI runners report
# real values and DO enforce the limit.
measurable = any(p > 0 for p in peaks.values())
mem_ok = bool(peaks) and (not measurable or all(p <= LIMIT for p in peaks.values()))
gate = "PASS" if mem_ok and total_fail == 0 and total_pass > 0 else "FAIL"
open(f"{RESULTS}/gate-status.txt", "w").write(gate + "\n")

# --------------------------------------------------------------- summary
def spikes(key, unit, n=10):
    hot = sorted((r for r in rows if r[key] is not None),
                 key=lambda r: r[key], reverse=True)[:n]
    if not hot:
        return ""
    out = [f"\n### Top {len(hot)} {key.upper()} samples — where the spike happened",
           "",
           f"| Time | {key.upper()} | Broker | Suite | Test |",
           "|---|---|---|---|---|"]
    for r in hot:
        out.append(f"| {r['iso']} | {r[key]:.0f}{unit} | {r['container']} | "
                   f"{r['phase'] or '—'} | {r['test'] or '—'} |")
    return "\n".join(out) + "\n"


with open(f"{RESULTS}/run-summary.md", "w") as out:
    out.write(f"## ETSI results — store: `{STORE}`\n\n")
    out.write(f"**{total_pass} passed, {total_fail} failed/skipped — gate {gate}** · "
              f"image {IMAGE_MB:.0f} MB · peak RSS limit {LIMIT:.0f} MiB · "
              f"{len(rows)} resource samples @1 Hz\n\n")
    if os.environ.get("BACKED_NOTE"):
        out.write(f"> ⚠️ {os.environ['BACKED_NOTE']}\n\n")
    if peaks and not measurable:
        out.write("> ⚠️ RSS not measurable on this docker daemon — the memory "
                  "gate was not evaluated in this run\n\n")

    out.write("| Suite | Pass | Fail | Skip |\n|---|---|---|---|\n")
    for name, p, f, s in suites:
        out.write(f"| {name} | {p} | {f} | {s} |\n")

    out.write("\n### Downloads (run artifacts)\n\n"
              "| File | What |\n|---|---|\n"
              "| `resource-samples.csv` | every 1 Hz CPU/RSS sample, labelled with suite + test |\n"
              "| `resource-by-test.csv` | per test × broker rollup (avg/peak CPU and RSS) |\n"
              "| `failures.csv` | every failing TP with its full message |\n"
              "| `<suite>/log.html` | Robot's own drill-down |\n")

    if failures:
        out.write(f"\n### Failures ({len(failures)}) — first 50, full list in `failures.csv`\n\n")
        for f in failures[:50]:
            out.write(f"- **{f['suite']} / {f['test']}**: {f['message'][:200]}\n")

    out.write("\n### Broker resources\n\n")
    out.write("| Broker | Samples | CPU avg | CPU peak | RSS avg | RSS peak |\n|---|---|---|---|---|---|\n")
    for name in sorted(per_container):
        c, m = per_container[name]["cpu"], per_container[name]["rss"]
        out.write(f"| {name} | {max(len(c), len(m))} | {avg(c):.1f}% | {max(c, default=0):.1f}% "
                  f"| {avg(m):.0f} MiB | {max(m, default=0):.0f} MiB |\n")

    out.write(spikes("rss", " MiB"))
    out.write(spikes("cpu", "%"))

    if not mem_ok and peaks:
        worst = max(peaks, key=peaks.get)
        out.write(f"\n**memory gate: {worst} peaked at {peaks[worst]:.0f} MiB "
                  f"vs limit {LIMIT:.0f} MiB**\n")

print(open(f"{RESULTS}/run-summary.md").read())
