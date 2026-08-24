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
    python3 dev/spec.py ics       # render docs/src/conformance-ics.md — the
                                  #   completed ETSI GS CIM 029 V2.1.1 annex A
                                  #   ICS pro forma, support column derived
                                  #   from this ledger (see dev/cim029-proforma.yaml)
    python3 dev/spec.py check     # ledger integrity gate (exit 1 on violation):
                                  #   every clause file parses, status is in the
                                  #   enum, partial/staged carry notes, robot
                                  #   lists match the suite tags, and the
                                  #   committed ICS matches a fresh render
"""

import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
PDF = ROOT / "etsi-cim-specs/gs_cim009v010901p.pdf"
SUITE = ROOT / "ngsi-ld-test-suite/TP"
SPEC = ROOT / "docs/spec"
PROFORMA = ROOT / "dev/cim029-proforma.yaml"
ICS_OUT = ROOT / "docs/src/conformance-ics.md"

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
    import fitz  # pymupdf — only split touches the PDF

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
    # An absent/empty submodule would read as 100% robot drift — fail loudly.
    if not SUITE.is_dir():
        sys.exit(f"{SUITE}: test-suite submodule not checked out (git submodule update --init)")
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
    import fitz  # pymupdf — only split touches the PDF

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


def ledger():
    """clause -> frontmatter, for every section file that parses."""
    out = {}
    for path in all_sections():
        meta = read_frontmatter(path)
        if meta and meta.get("clause"):
            out[meta["clause"]] = meta
    return out


def proforma():
    return yaml.safe_load(PROFORMA.read_text())


def scored_clauses(row):
    """The clauses a row is scored against.

    CIM 029 V2.1.1 cites CIM 009 V1.6.1; the ledger carries V1.9.1. Where a
    published citation did not survive renumbering, `clauses_v191` names the
    V1.9.1 clause and is scored instead. The published `clauses` are what the
    rendered Clauses column shows, so the table stays as in Annex A.
    """
    return row.get("clauses_v191") or row["clauses"]


def citation_leaves(clause, led):
    """The ledger clauses that carry the implementation of a citation.

    A cited clause is often a heading whose substance lives in its
    sub-clauses (5.6.1 "Create Entity" is marked informative and delegates to
    5.6.1.1 onwards). Scoring the heading alone would report support for a
    resource whose sub-clauses are unimplemented, so a citation resolves to
    the leaves beneath it whenever it has any.
    """
    kids = [c for c in led if c.startswith(clause + ".")]
    if not kids:
        return [clause]
    return [c for c in kids if not any(o != c and o.startswith(c + ".") for o in kids)]


def row_support(row, led):
    """(support, deciding) for one ICS row.

    ISO/IEC 9646-7 allows only Y or N in the support column, so a row whose
    clauses cannot all be shown implemented is N. A leaf marked informative
    has nothing to implement and does not hold a row down. A cited clause
    absent from the ledger is reported separately: a renumbered clause is a
    mapping defect, never evidence of support.
    """
    deciding = []
    supported = True
    for clause in scored_clauses(row):
        if clause not in led:
            deciding.append((clause, "ABSENT"))
            supported = False
            continue
        leaves_ = citation_leaves(clause, led)
        bad = [c for c in leaves_ if led[c].get("status") not in ("implemented", "informative")]
        if bad:
            supported = False
            worst = sorted({led[c].get("status", "?") for c in bad})
            deciding.append((clause, f"{'/'.join(worst)} in {len(bad)}/{len(leaves_)} leaves"))
        elif len(leaves_) == 1:
            deciding.append((clause, "implemented"))
        else:
            deciding.append((clause, f"implemented (all {len(leaves_)} sub-clauses)"))
    return ("Y" if supported else "N"), deciding


def unresolved_clauses(led):
    """[(table, item, clause)] cited by the pro forma but absent from the ledger."""
    missing = []
    for table in proforma()["tables"]:
        for row in table["rows"]:
            for clause in scored_clauses(row):
                if clause not in led:
                    missing.append((table["id"], row["item"], clause))
    return missing


def render_ics():
    """(text, items, supported, unresolved) — the completed ICS pro forma.

    Annex A.0 grants the right to reproduce the pro forma and publish it
    completed. Clause 4 requires the result to be technically equivalent to
    Annex A and to preserve the numbering and ordering of its items, so the
    tables are emitted in file order with their published item numbers and
    the six published columns. The support column is derived from the
    ledger, never hand-written; the evidence behind each verdict is carried
    in a separate annex so the pro forma tables stay as published.
    """
    pf = proforma()
    led = ledger()
    ident = pf["identification"]
    version = "unknown"
    for line in (ROOT / "Cargo.toml").read_text().splitlines():
        if line.startswith("version"):
            version = line.split('"')[1]
            break

    verdicts = {}
    out = [
        "# NGSI-LD Implementation Conformance Statement",
        "",
        "Completed ICS pro forma of **ETSI GS CIM 029 V2.1.1 (2025-07)**,",
        "Annex A (normative), whose clause A.0 grants the right to reproduce the",
        "pro forma and to publish it completed.",
        "",
        "Generated by `dev/spec.py ics` from the clause ledger in `docs/spec/`.",
        "The support column is derived, never hand-written: a row is `Y` only",
        "when every CIM 009 clause it cites is recorded as implemented, with",
        "the deciding clauses listed in annex ANT-1 below.",
        "",
        "> **Version note.** CIM 029 V2.1.1 cites ETSI GS CIM 009 **V1.6.1**,",
        "> while this implementation targets **V1.9.1** and the ledger carries",
        "> that text. The pro forma's own feature tables already run ahead of",
        "> its normative reference. It is therefore a conformance scaffold, not",
        "> a complete checklist of V1.9.1 behaviour.",
        "",
        "## A.2 Identification of the implementation",
        "",
        f"- IUT name: {ident['iut_name']}",
        f"- IUT version: {version}",
        f"- SUT hardware configuration: {ident['sut_hardware'] or '(not stated)'}",
        f"- SUT operating system: {ident['sut_operating_system'] or '(not stated)'}",
        f"- Product supplier: {ident['supplier_name'] or '(not stated)'}",
        f"- Supplier address: {ident['supplier_address'] or '(not stated)'}",
        f"- Supplier e-mail: {ident['supplier_email'] or '(not stated)'}",
        f"- ICS contact person: {ident['contact_name'] or '(not stated)'}",
        f"- Contact e-mail: {ident['contact_email'] or '(not stated)'}",
        "",
        "## A.3 Identification of the reference specifications",
        "",
        "This ICS pro forma applies to ETSI GS CIM 009.",
        "",
        "## A.4 Global statement of conformance",
        "",
    ]
    declared = str(pf.get("global_statement_of_conformance", "unset"))
    if declared.lower() in ("yes", "no"):
        out += [f"Are all mandatory capabilities implemented? **{declared}**", ""]
    else:
        out += [
            "Are all mandatory capabilities implemented? **(not declared)**",
            "",
            "This answer is a supplier declaration and is deliberately not",
            "derived: the A.5.1 tables carry no mandatory/optional status",
            "column, and several features are optional by architecture. Set",
            "`global_statement_of_conformance` in `dev/cim029-proforma.yaml`",
            "to publish a signed claim.",
            "",
        ]

    section = None
    for table in pf["tables"]:
        head = "A.5.1 Features" if table["id"].startswith("A.5.1") else "A.5.2 API Operation"
        if head != section:
            section = head
            out += [f"## {head}", ""]
        out += [
            f"### {table['id']} {table['title']}",
            "",
            "| Item | Feature | Subfeature | Clauses | Mnemonic | Support |",
            "|---|---|---|---|---|---|",
        ]
        for row in table["rows"]:
            support, deciding = row_support(row, led)
            verdicts[(table["id"], row["item"])] = (row, support, deciding)
            clauses = "; ".join(row["clauses"])
            out.append(
                f"| {row['item']} | {row['feature']} | {row.get('subfeature', '')} "
                f"| {clauses} | {row['mnemonic']} | {support} |"
            )
        out.append("")

    out += ["## A.6 Mnemonics for PICS", "", "| Mnemonic | PICS Item |", "|---|---|"]
    seen = {}
    for (tid, item), (row, _s, _d) in verdicts.items():
        seen.setdefault(row["mnemonic"], []).append(f"{tid}/{item}")
    for mnemonic in sorted(seen):
        out.append(f"| {mnemonic} | {', '.join(seen[mnemonic])} |")
    out.append("")

    out += [
        "## Annex ANT-1: evidence behind each verdict",
        "",
        "Not part of the ETSI pro forma. Every row above with the ledger",
        "status of each clause it cites, and the Robot test purposes tagged",
        "against those clauses.",
        "",
        "| Item | Mnemonic | Support | Clause statuses | Robot TPs |",
        "|---|---|---|---|---|",
    ]
    for (tid, item), (row, support, deciding) in verdicts.items():
        statuses = "; ".join(f"{c}: {s}" for c, s in deciding)
        tps = sorted({t for c, _ in deciding for t in led.get(c, {}).get("robot", [])})
        shown = ", ".join(tps[:6]) + (" …" if len(tps) > 6 else "") or "—"
        out.append(f"| {tid}/{item} | {row['mnemonic']} | {support} | {statuses} | {shown} |")
    out.append("")

    missing = unresolved_clauses(led)
    if missing:
        out += [
            "## Annex ANT-2: pro forma clauses absent from the ledger",
            "",
            "Cited by CIM 029 V2.1.1 against CIM 009 V1.6.1 but not present in",
            "the V1.9.1 ledger — a renumbering to map, not a missing feature.",
            "These rows are scored `N` until mapped.",
            "",
            "| Item | Clause |",
            "|---|---|",
        ] + [f"| {t}/{i} | {c} |" for t, i, c in missing] + [""]

    ys = sum(1 for _k, (_r, s, _d) in verdicts.items() if s == "Y")
    return "\n".join(out), len(verdicts), ys, missing


def cmd_ics():
    text, items, supported, missing = render_ics()
    ICS_OUT.write_text(text)
    print(f"{ICS_OUT}: {items} items, {supported} Y / {items - supported} N")
    print(f"unresolved clause references: {len(missing)}")


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

    # The ICS is generated from the ledger it is published beside; a stale one
    # is a conformance claim that no longer matches the evidence.
    text, _items, _supported, missing = render_ics()
    for tid, item, clause in missing:
        errors.append(f"{PROFORMA.name}: {tid}/{item} cites {clause}, absent from the ledger")
    if not ICS_OUT.exists():
        errors.append(f"{ICS_OUT}: missing — run `dev/spec.py ics`")
    elif ICS_OUT.read_text() != text:
        errors.append(f"{ICS_OUT}: stale — run `dev/spec.py ics` and commit the result")

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
        "ics": cmd_ics,
        "check": cmd_check,
    }.get(cmd, cmd_status)()
