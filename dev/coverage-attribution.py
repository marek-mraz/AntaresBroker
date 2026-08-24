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


def main():
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
