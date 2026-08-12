#!/usr/bin/env python3
"""docs/spec/ — CIM 009 V1.9.1 split into one file per clause, WITH full text.

This is the conformance ledger (supersedes docs/ics.yaml, deleted 2026-08-10):
each of the ~950 outline sections of gs_cim009v010901p.pdf becomes
docs/spec/<chapter>/<clause>.md — YAML frontmatter for tracking, the clause's
own text as the body. Implementation status is HAND-edited in the
frontmatter; the robot-TP list is GENERATED from the suite fork's [Tags].

    status:  not-implemented | partial | implemented | staged-v1x   (hand)
    evidence, notes:                                                (hand)
    robot:   TPs whose [Tags] cite this clause                      (generated)

Commands:
    python3 dev/spec.py split     # (re)extract all sections from the PDF;
                                  #   existing frontmatter is PRESERVED,
                                  #   only the body + generated fields move
    python3 dev/spec.py robot     # refresh the robot: field from suite tags
    python3 dev/spec.py status    # counts + per-chapter rollup
    python3 dev/spec.py gaps      # leaves with status not-implemented and no TPs
    python3 dev/spec.py check     # ledger integrity gate (exit 1 on violation):
                                  #   every clause file parses, status is in the
                                  #   enum, partial/staged carry notes, robot
                                  #   lists match the suite tags
"""

import re
import sys
from pathlib import Path

import fitz  # pymupdf
import yaml

ROOT = Path(__file__).resolve().parent.parent
PDF = ROOT / "etsi-cim-specs/gs_cim009v010901p.pdf"
SUITE = ROOT / "ngsi-ld-test-suite/TP"
SPEC = ROOT / "docs/spec"

CLAUSE_RE = re.compile(r"^(\d+(?:\.\d+)*|[A-Z]\.\d+(?:\.\d+)*)\s+(.*)$")
ANNEX_RE = re.compile(r"^Annex ([A-Z])\s*\(?(normative|informative)?\)?:?\s*(.*)$")
TAG_RE = re.compile(r"^[1-9]\d*(_[0-9]\d*)+$")
FOOTER_RE = re.compile(r"^(ETSI|\d{1,3}|ETSI GS CIM 009 V1\.9\.1 \(2025-07\))\s*$")

HAND_FIELDS = ("status", "evidence", "notes")
DEFAULTS = {"status": "not-implemented", "evidence": "", "notes": ""}
STATUSES = {"not-implemented", "partial", "implemented", "staged-v1x", "informative"}


def outline():
    """[(clause_or_None, title, page_start, page_end_inclusive)] in document
    order. Unnumbered outline entries (Foreword, History, ...) get no file but
    STAY in the list as cut points — without them the History table on the
    last page would leak into annex I's body."""
    doc = fitz.open(PDF)
    rows = []
    for _level, title, page in doc.get_toc():
        title = " ".join(title.split())
        m = CLAUSE_RE.match(title)
        a = ANNEX_RE.match(title)
        if m:
            rows.append([m.group(1), m.group(2), page])
        elif a:
            rows.append([a.group(1), title, page])
        else:
            rows.append([None, title, page])
    out = []
    for i, (clause, title, ps) in enumerate(rows):
        pe = rows[i + 1][2] if i + 1 < len(rows) else doc.page_count
        out.append((clause, title, ps, max(ps, pe)))
    return out


def section_body(doc, sections, i):
    """The i-th section's own text: its heading up to the next heading."""
    clause, title, ps, pe = sections[i]
    lines = []
    for p in range(ps, pe + 1):
        for line in doc[p - 1].get_text().splitlines():
            if not FOOTER_RE.match(line.strip()):
                lines.append(line.rstrip())
    txt = "\n".join(lines)

    def find_heading(text, c, t):
        # heading appears as "<number> <title-prefix>" in the body text —
        # unnumbered cut points (c=None) match on the bare title
        pat = (re.escape(c) + r"\s+" if c else "") + re.escape(" ".join(t.split()[:4]))
        m = re.search(pat, text)
        return m.start() if m else -1

    start = find_heading(txt, clause, title)
    if start >= 0:
        txt = txt[start:]
    if i + 1 < len(sections):
        nc, nt, nps, _ = sections[i + 1]
        if nps <= pe:  # next heading shares the page span — cut there
            cut = find_heading(txt, nc, nt)
            if cut > 0:
                txt = txt[:cut]
    return txt.strip() + "\n"


def path_for(clause):
    return SPEC / clause.split(".")[0] / f"{clause}.md"


def read_frontmatter(path):
    if not path.exists():
        return {}
    text = path.read_text()
    # frontmatter must OPEN the file — README.md carries example fences that
    # would otherwise parse as a section
    if not text.startswith("---\n"):
        return {}
    parts = text.split("---\n", 2)
    if len(parts) < 3:
        return {}
    return yaml.safe_load(parts[1]) or {}


def write_section(path, meta, body):
    path.parent.mkdir(parents=True, exist_ok=True)
    fm = yaml.safe_dump(meta, allow_unicode=True, sort_keys=False, width=100)
    path.write_text(f"---\n{fm}---\n\n{body}")


def robot_map():
    mapping = {}
    for f in SUITE.rglob("*.robot"):
        for line in f.read_text(errors="replace").splitlines():
            if "[Tags]" not in line:
                continue
            for tag in line.split():
                if TAG_RE.match(tag):
                    mapping.setdefault(tag.replace("_", "."), set()).add(f.stem)
    return {c: sorted(s) for c, s in mapping.items()}


def cmd_split():
    doc = fitz.open(PDF)
    sections = outline()
    tps = robot_map()
    seen = set()
    for i, (clause, title, ps, pe) in enumerate(sections):
        if clause is None or clause in seen:
            continue  # cut points get no file; outline duplicates: first wins
        seen.add(clause)
        path = path_for(clause)
        old = read_frontmatter(path)
        meta = {
            "clause": clause,
            "title": title,
            "pages": f"{ps}-{pe}" if pe != ps else str(ps),
            **{k: old.get(k, DEFAULTS[k]) for k in HAND_FIELDS},
            "robot": tps.get(clause, []),
        }
        write_section(path, meta, section_body(doc, sections, i))
    print(f"{SPEC}: {len(seen)} sections")


def all_sections():
    # numeric-aware sort that never compares int to str: (0, n) vs (1, token)
    def key(p):
        return [(0, int(x)) if x.isdigit() else (1, x) for x in p.stem.split(".")]

    return sorted(SPEC.rglob("*.md"), key=key)


def cmd_robot():
    tps = robot_map()
    changed = 0
    for path in all_sections():
        meta = read_frontmatter(path)
        if not meta:
            continue
        want = tps.get(meta["clause"], [])
        if meta.get("robot") != want:
            body = path.read_text().split("---\n", 2)[2].lstrip("\n")
            meta["robot"] = want
            write_section(path, meta, body)
            changed += 1
    print(f"robot fields refreshed: {changed} files changed")


def leaves(metas):
    clauses = {m["clause"] for m in metas}
    return [m for m in metas if not any(c != m["clause"] and c.startswith(m["clause"] + ".") for c in clauses)]


def cmd_status():
    metas = [read_frontmatter(p) for p in all_sections()]
    metas = [m for m in metas if m]
    by = {}
    for m in metas:
        by.setdefault(m.get("status", "?"), []).append(m)
    total = len(metas)
    print(f"{total} sections")
    for s, ms in sorted(by.items()):
        print(f"  {s:16} {len(ms):4}")
    with_tp = sum(1 for m in metas if m.get("robot"))
    print(f"  robot-tagged     {with_tp:4}")


def cmd_gaps():
    metas = [m for m in (read_frontmatter(p) for p in all_sections()) if m]
    for m in leaves(metas):
        if m.get("status") == "not-implemented" and not m.get("robot"):
            print(f"{m['clause']:14} {m['title']}  (p.{m['pages']})")


def cmd_check():
    """Ledger integrity gate. A clause file that stops parsing would silently
    DROP OUT of every other command's count — this is the command that makes
    that (and status typos, robot drift) a loud CI failure instead."""
    errors = []
    tps = robot_map()
    n = 0
    for path in all_sections():
        if path.name == "README.md":
            continue
        n += 1
        try:
            meta = read_frontmatter(path)
        except yaml.YAMLError as e:
            errors.append(f"{path}: frontmatter does not parse: {e}")
            continue
        if not meta:
            errors.append(f"{path}: no frontmatter — excluded from all counts")
            continue
        status = meta.get("status")
        if status not in STATUSES:
            errors.append(f"{path}: status {status!r} not in {sorted(STATUSES)}")
        if status in ("partial", "staged-v1x") and not meta.get("notes"):
            errors.append(f"{path}: {status} without notes naming the gap/posture")
        if status == "implemented" and not meta.get("evidence"):
            errors.append(f"{path}: implemented without evidence")
        if meta.get("robot") != tps.get(meta.get("clause"), []):
            errors.append(f"{path}: robot list drifted — run `dev/spec.py robot`")
    for e in errors:
        print(f"CHECK {e}")
    print(f"check: {n} files, {len(errors)} violations")
    if errors:
        sys.exit(1)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "status"
    {
        "split": cmd_split,
        "robot": cmd_robot,
        "status": cmd_status,
        "gaps": cmd_gaps,
        "check": cmd_check,
    }.get(cmd, cmd_status)()
