#!/usr/bin/env python3
"""Attribute line coverage to the test kind that produced it.

Takes two lcov tracefiles over the same tree — one from the Rust tests
alone, one from the ETSI Robot suite alone — and buckets every
instrumented line into both / only-unit / only-robot / uncovered.

    python3 dev/coverage-attribution.py <unit.info> <robot.info> <out-dir>

Writes into <out-dir>:
    attribution.txt      per-file table plus the two lists that matter:
                         files whose coverage comes only from the Robot
                         suite (no unit test pins them — regressions there
                         surface only in the slow suite) and files whose
                         coverage comes only from unit tests (surface the
                         conformance suite never reaches).
    uncovered-lines.txt  crate-path:line of code NO test of either kind ran.

Second mode: the per-source coverage table.

    python3 dev/coverage-attribution.py --table <out-dir> \
        unit=<coverage.json> api=<coverage.json> etsi-memory=<coverage.json> ...

Each argument names one test source and the coverage it produced, as an
llvm-cov JSON export or an lcov tracefile (the suffix decides). Writes
<out-dir>/summary.md: one row per crate plus TOTAL, two columns per
source (line % and function %). Coverage is reported per source
and never as one blended number, so a column that drops is visible even when
the total rises.
"""

import sys
from pathlib import Path


def load(path):
    """lcov -> {file: {line: covered}}; duplicate DA records OR together."""
    files = {}
    cur = None
    for raw in open(path):
        line = raw.strip()
        if line.startswith("SF:"):
            name = line[3:]
            if "/crates/" in name:
                name = "crates/" + name.split("/crates/", 1)[1]
            cur = files.setdefault(name, {})
        elif line.startswith("DA:") and cur is not None:
            ln, cnt = line[3:].split(",")[:2]
            ln = int(ln)
            cur[ln] = cur.get(ln, False) or int(cnt) > 0
    return files


def crate_of(filename):
    """The crate a compiled file belongs to, or None for anything outside."""
    if "/crates/" not in filename:
        return None
    return filename.split("/crates/", 1)[1].split("/", 1)[0]


def per_crate_json(json_path):
    """llvm-cov JSON export -> {crate: [lines_total, lines_hit, fns, fns_hit]}."""
    import json

    acc = {}
    data = json.load(open(json_path))
    for export in data.get("data", []):
        for entry in export.get("files", []):
            crate = crate_of(entry.get("filename", ""))
            if crate is None:
                continue
            summary = entry.get("summary", {})
            lines = summary.get("lines", {})
            fns = summary.get("functions", {})
            row = acc.setdefault(crate, [0, 0, 0, 0])
            row[0] += lines.get("count", 0)
            row[1] += lines.get("covered", 0)
            row[2] += fns.get("count", 0)
            row[3] += fns.get("covered", 0)
    return acc


def per_crate_lcov(path):
    """lcov tracefile -> the same shape. The merge job's inputs are tracefiles:
    `lcov -a` unions the cells per line and per function, which per-file
    summary counts cannot do.

    Functions are keyed by the FN record's START LINE, never by its name. A
    tracefile names them mangled, so one generic function appears once per
    instantiation and one closure once per test binary that linked it — and a
    binary that never linked an instantiation reports it as a miss. Keyed by
    name the workspace has about 9 400 "functions" of which half are never
    called; keyed by line it has about 4 900, and the covered share matches
    what `cargo llvm-cov --summary-only` gates on to within a point. The JSON
    path needs none of this: llvm-cov's own summary already counts source
    functions."""
    acc = {}
    crate = None
    seen_lines, seen_fns, fn_line = set(), {}, {}

    def flush():
        if crate is None:
            return
        row = acc.setdefault(crate, [0, 0, 0, 0])
        row[0] += len(seen_lines)
        row[1] += sum(1 for hit in seen_lines_hit.values() if hit)
        row[2] += len(seen_fns)
        row[3] += sum(1 for hit in seen_fns.values() if hit)

    seen_lines_hit = {}
    for raw in open(path):
        line = raw.strip()
        if line.startswith("SF:"):
            flush()
            crate = crate_of(line[3:])
            seen_lines, seen_lines_hit, seen_fns, fn_line = set(), {}, {}, {}
        elif crate is None:
            continue
        elif line.startswith("DA:"):
            ln, cnt = line[3:].split(",")[:2]
            seen_lines.add(ln)
            seen_lines_hit[ln] = seen_lines_hit.get(ln, False) or int(cnt) > 0
        elif line.startswith("FN:"):
            start, name = line[3:].split(",", 1)
            fn_line[name] = start
            seen_fns.setdefault(start, False)
        elif line.startswith("FNDA:"):
            cnt, name = line[5:].split(",", 1)
            start = fn_line.get(name)
            if start is not None:
                seen_fns[start] = seen_fns.get(start, False) or int(cnt) > 0
    flush()
    return acc


def per_crate(path):
    """Read either input format; the suffix decides."""
    return per_crate_json(path) if str(path).endswith(".json") else per_crate_lcov(path)


def table(out_dir, sources):
    """Render the per-source table. `sources` is [(name, json_path), ...]."""
    measured = []
    for name, path in sources:
        if Path(path).is_file():
            measured.append((name, per_crate(path)))
        else:
            measured.append((name, {}))

    crates = sorted({c for _, acc in measured for c in acc})
    head = "| crate |" + "".join(f" {n} lines | {n} fn |" for n, _ in measured)
    rule = "|---|" + "---|" * (2 * len(measured))

    def cell(row, hit_i, tot_i):
        total = row[tot_i]
        return "-" if not total else f"{100.0 * row[hit_i] / total:.1f}%"

    body = []
    for crate in crates:
        cells = []
        for _, acc in measured:
            row = acc.get(crate)
            cells += ["-", "-"] if row is None else [cell(row, 1, 0), cell(row, 3, 2)]
        body.append(f"| {crate} |" + "".join(f" {c} |" for c in cells))

    totals = []
    for _, acc in measured:
        agg = [0, 0, 0, 0]
        for row in acc.values():
            for i in range(4):
                agg[i] += row[i]
        totals += [cell(agg, 1, 0), cell(agg, 3, 2)]
    body.append("| **TOTAL** |" + "".join(f" **{c}** |" for c in totals))

    md = "\n".join([head, rule] + body) + "\n"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.md").write_text(md)
    print(md, end="")


def selftest():
    """One generic function, two instantiations, one of them never called:
    keyed by name that reads as 2 functions and 50 % covered, keyed by start
    line as the 1 source function it is, covered."""
    import tempfile

    tracefile = (
        "SF:/w/crates/antares-model/src/x.rs\n"
        "FN:7,_ZN1f17hAAAE\nFN:7,_ZN1f17hBBBE\nFNDA:3,_ZN1f17hAAAE\n"
        "FNDA:0,_ZN1f17hBBBE\nDA:7,3\nDA:8,0\nend_of_record\n"
    )
    with tempfile.NamedTemporaryFile("w", suffix=".info", delete=False) as fh:
        fh.write(tracefile)
        path = fh.name
    row = per_crate_lcov(path)["antares-model"]
    Path(path).unlink()
    assert row == [2, 1, 1, 1], row
    print("selftest ok")


def main():
    if sys.argv[1:2] == ["--selftest"]:
        selftest()
        return
    if sys.argv[1:2] == ["--table"]:
        out_dir = Path(sys.argv[2])
        sources = []
        for arg in sys.argv[3:]:
            name, _, path = arg.partition("=")
            sources.append((name, path))
        table(out_dir, sources)
        return
    unit_path, robot_path, out_dir = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    unit, robot = load(unit_path), load(robot_path)
    out_dir.mkdir(parents=True, exist_ok=True)

    rows = []  # (file, pct, both, only_unit, only_robot, uncovered)
    uncovered_lines = []
    for f in sorted(set(unit) | set(robot)):
        u, r = unit.get(f, {}), robot.get(f, {})
        both = only_u = only_r = unc = 0
        for ln in sorted(set(u) | set(r)):
            cu, cr = u.get(ln, False), r.get(ln, False)
            if cu and cr:
                both += 1
            elif cu:
                only_u += 1
            elif cr:
                only_r += 1
            else:
                unc += 1
                uncovered_lines.append(f"{f}:{ln}")
        total = both + only_u + only_r + unc
        pct = 100.0 * (both + only_u + only_r) / total if total else 0.0
        rows.append((f, pct, both, only_u, only_r, unc))

    t_both = sum(r[2] for r in rows)
    t_u = sum(r[3] for r in rows)
    t_r = sum(r[4] for r in rows)
    t_unc = sum(r[5] for r in rows)
    robot_only_files = [r for r in rows if r[2] + r[3] == 0 and r[4] > 0]
    unit_only_files = [r for r in rows if r[2] + r[4] == 0 and r[3] > 0]

    with open(out_dir / "attribution.txt", "w") as out:
        total = t_both + t_u + t_r + t_unc
        out.write(
            f"lines: {total} | both {t_both} | only-unit {t_u} "
            f"| only-robot {t_r} | uncovered {t_unc}\n\n"
        )
        out.write(
            f"files covered ONLY by the Robot suite ({len(robot_only_files)})"
            " — no unit test pins them:\n"
        )
        for f, _, _, _, only_r, _ in sorted(
            robot_only_files, key=lambda r: -r[4]
        ):
            out.write(f"  {f}  ({only_r} lines)\n")
        out.write(
            f"\nfiles covered ONLY by unit tests ({len(unit_only_files)})"
            " — the conformance suite never reaches them:\n"
        )
        for f, _, _, only_u, _, _ in sorted(
            unit_only_files, key=lambda r: -r[3]
        ):
            out.write(f"  {f}  ({only_u} lines)\n")
        out.write("\nfile  line%  both  only-unit  only-robot  uncovered\n")
        for f, pct, both, only_u, only_r, unc in rows:
            out.write(f"{f}  {pct:.1f}%  {both}  {only_u}  {only_r}  {unc}\n")

    (out_dir / "uncovered-lines.txt").write_text(
        "\n".join(uncovered_lines) + ("\n" if uncovered_lines else "")
    )


if __name__ == "__main__":
    main()
