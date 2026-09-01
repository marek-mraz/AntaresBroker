#!/usr/bin/env python3
"""docs/spec/ — CIM 009 V1.9.1 split into one file per clause, WITH full text.

This is the conformance ledger:
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
    # An absent/empty suite tree would read as 100% robot drift — fail loudly.
    if not SUITE.is_dir():
        sys.exit(f"{SUITE}: test-suite tree missing")
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


def cmd_statements():
    """Per leaf clause: SHALL sentences in the spec text against the Robot
    TPs and code/test anchors the ledger cites. Reveals clauses whose
    normative statements no test asserts; adds no tests itself."""
    rows = []
    metas = [m for m in (read_frontmatter(p) for p in all_sections()) if m]
    leaf = {m["clause"] for m in leaves(metas)}
    by_clause = {m["clause"]: m for m in metas}

    def ancestors(clause):
        parts = clause.split(".")
        return [".".join(parts[:i]) for i in range(len(parts), 0, -1)]

    def robot_tps(clause):
        # a TP tagged with the operation clause covers its sub-clauses
        return {tp for c in ancestors(clause) for tp in (by_clause.get(c, {}).get("robot") or [])}

    def full_title(clause):
        own = by_clause[clause]["title"]
        parent = by_clause.get(".".join(clause.split(".")[:-1]))
        return f"{parent['title']} › {own}" if parent and len(clause.split(".")) > 3 else own
    for path in all_sections():
        meta = read_frontmatter(path)
        if not meta or meta["clause"] not in leaf:
            continue
        if meta.get("status") in (None, "informative", "not-implemented"):
            continue
        body = path.read_text().split("---\n", 2)[2]
        shalls = len(re.findall(r"\bshall\b", body, re.I))
        if not shalls:
            continue
        evidence = str(meta.get("evidence") or "")
        anchors = len(re.findall(r"[\w/]+\.(?:rs|robot|py|sql)\b", evidence))
        rows.append((meta["clause"], full_title(meta["clause"]), shalls, len(robot_tps(meta["clause"])), anchors, meta.get("status")))
    untested = [r for r in rows if r[3] == 0]
    print("| clause | title | SHALL | robot TPs | code/test anchors | status |")
    print("|---|---|---:|---:|---:|---|")
    for c, t, sh, tp, an, st in sorted(rows, key=lambda r: (-r[2], r[0])):
        print(f"| {c} | {t} | {sh} | {tp} | {an} | {st} |")
    print()
    print(f"{len(rows)} leaf clauses carry {sum(r[2] for r in rows)} SHALL statements; "
          f"{len(untested)} of them have no Robot TP ({sum(r[2] for r in untested)} SHALLs), "
          f"{sum(1 for r in rows if r[4] == 0)} cite no code/test anchor.")


# The suites one ETSI cell runs, in the order the chapter tables them.
SUITE_DIRS = [
    ("CommonBehaviours", "TP/NGSI-LD/CommonBehaviours"),
    ("Consumption", "TP/NGSI-LD/ContextInformation/Consumption"),
    ("EntityMap", "TP/NGSI-LD/ContextInformation/EntityMap"),
    ("Provision", "TP/NGSI-LD/ContextInformation/Provision"),
    ("Snapshot", "TP/NGSI-LD/ContextInformation/Snapshot"),
    ("Subscription", "TP/NGSI-LD/ContextInformation/Subscription"),
    ("ContextSource", "TP/NGSI-LD/ContextSource"),
    ("DistributedOperations", "TP/NGSI-LD/DistributedOperations"),
    ("IOP", "IOP_TP"),
    ("jsonldContext", "TP/NGSI-LD/jsonldContext"),
]


def robot_cases(subtree):
    """(cases, of which MQTT) under a suite directory, counted the way a cell
    counts them: a test case is a line at column 0 inside a `*** Test Cases ***`
    section, and `dev/etsi-run.sh` passes `--exclude config_no_temporal`, so a
    case carrying that tag is not one a cell runs. `MQTT=0` adds
    `--exclude *mqtt*`, which is the browser cell — hence the second number."""
    root = ROOT / "ngsi-ld-test-suite" / subtree
    total = mqtt = 0
    for f in sorted(root.rglob("*.robot")):
        insec = False
        pending = None  # None = not in a case; 0/1 = in one, is it MQTT

        def close(pending, total, mqtt):
            return (total + 1, mqtt + pending) if pending is not None else (total, mqtt)

        for line in f.read_text(errors="replace").splitlines():
            st = line.strip()
            if st.startswith("***"):
                total, mqtt = close(pending, total, mqtt)
                pending = None
                insec = st.lower().startswith("*** test case")
            elif not insec:
                continue
            elif st and not line[0].isspace() and not st.startswith("#"):
                total, mqtt = close(pending, total, mqtt)
                pending = 0
            elif pending is not None and st.lower().startswith("[tags]"):
                if "config_no_temporal" in st:
                    pending = None
                elif "mqtt" in st.lower():
                    pending = 1
        total, mqtt = close(pending, total, mqtt)
    return total, mqtt


def chapter_violations():
    """The conformance chapter republishes numbers this tool computes: the
    status counts and the size of the suite. A generated number copied into
    prose drifts in silence — it read 479 implemented with zero partial while
    the ledger held 477 and two, and 666 suite files while the directory held
    667. An operator reads those numbers instead of running the tool."""
    book = ROOT / "docs/src/conformance.md"
    if not book.exists():
        return [f"{book}: the conformance chapter is missing"]
    text = book.read_text(encoding="utf-8")
    out = []

    metas = [m for m in (read_frontmatter(p) for p in all_sections()) if m]
    counts = {"sections": len(metas), "robot-tagged": sum(1 for m in metas if m.get("robot"))}
    for m in metas:
        key = m.get("status", "?")
        counts[key] = counts.get(key, 0) + 1
    block = re.search(r"Current counts.*?```text\n(.*?)```", text, re.S)
    if not block:
        return [f"{book}: no `Current counts` block to check"]
    for line in block.group(1).splitlines():
        parts = line.split()
        if len(parts) != 2:
            continue
        stated, label = (parts[0], parts[1]) if parts[1] == "sections" else (parts[1], parts[0])
        if not stated.isdigit():
            continue
        actual = counts.get(label)
        if actual is None:
            out.append(f"{book}: counts block names {label!r}, which the ledger has none of")
        elif int(stated) != actual:
            out.append(f"{book}: counts block says {label} {stated}, ledger has {actual}")
    for label, actual in sorted(counts.items()):
        if label != "?" and f" {label} " not in block.group(1).replace("\n", " ") + " ":
            out.append(f"{book}: counts block omits {label} ({actual})")

    # The per-suite table and the two totals the chapter (and the README)
    # publish are the project's headline number. Counted here from the tree
    # with the cell's own exclusions, they matched the green run
    # 33486804581 suite for suite: 1803 native, 1791 in the browser cell.
    per_suite = {name: robot_cases(d) for name, d in SUITE_DIRS}
    native = sum(c for c, _ in per_suite.values())
    wasm = native - sum(m for _, m in per_suite.values())
    for name, (cases, _) in per_suite.items():
        row = re.search(rf"^\| {re.escape(name)} \| (\d+) \|", text, re.M)
        if not row:
            out.append(f"{book}: the suite table has no {name} row")
        elif int(row.group(1)) != cases:
            out.append(f"{book}: says {name} runs {row.group(1)} cases, it runs {cases}")
    for stated, actual, what in (
        (set(re.findall(r"is (\d+) test cases", text)), native, "a native cell"),
        (set(re.findall(r"passes at (\d+)/", text)), native, "a native cell"),
        (set(re.findall(r"same (\d+) cases run", text)), native, "a native cell"),
        (set(re.findall(r"runs (\d+) of\b", text)), wasm, "the browser cell"),
        (set(re.findall(r"`wasm-file` at (\d+)/", text)), wasm, "the browser cell"),
    ):
        for n in stated:
            if int(n) != actual:
                out.append(f"{book}: says {n} for {what}, the suites hold {actual}")

    # The README leads with the same two numbers.
    readme = ROOT / "README.md"
    if readme.exists():
        rtext = readme.read_text(encoding="utf-8")
        for pat, actual, what in (
            (r"(\d+)/\d+ ETSI CIM 009", native, "a native cell"),
            (r"and (\d+)/\d+ in the browser", wasm, "the browser cell"),
        ):
            hit = re.search(pat, rtext)
            if not hit:
                out.append(f"README.md: no count for {what} to check")
            elif int(hit.group(1)) != actual:
                out.append(
                    f"README.md: says {hit.group(1)} for {what}, the suites hold {actual}"
                )

    # The maintainer note classifies every chapter into one of the four
    # documentation modes, and is the checklist for whether a new feature
    # needs a how-to. A chapter added to SUMMARY and not to the note is a
    # chapter the gap check silently stops covering.
    summ = ROOT / "docs/src/SUMMARY.md"
    note = ROOT / "docs/book-structure.md"
    if summ.exists() and note.exists():
        stext, part, listed = summ.read_text(encoding="utf-8"), None, {}
        for line in stext.splitlines():
            if line.startswith("# ") and line[2:].strip() != "Summary":
                part = line[2:].strip().lower()
            hit = re.search(r"\]\(([a-z0-9-]+\.md)\)", line)
            if hit:
                # mdBook's prefix chapter precedes every part heading
                listed.setdefault(hit.group(1), part)
        ntext = note.read_text(encoding="utf-8")
        mapped = {}
        for line in ntext.splitlines():
            row = re.match(r"\| (tutorial|how-to|reference|explanation) \| (.+) \|$", line)
            if not row:
                continue
            for cell in row.group(2).split(","):
                chapter = cell.strip().split()[0]
                if chapter in mapped:
                    out.append(
                        f"docs/book-structure.md: {chapter} is classified twice, under "
                        f"{mapped[chapter]} and {row.group(1)}"
                    )
                    continue  # the first row keeps the chapter
                mapped[chapter] = row.group(1)
        for chapter, part in sorted(listed.items()):
            if chapter not in mapped:
                out.append(f"docs/book-structure.md: {chapter} is in SUMMARY but not classified")
            elif part is not None and mapped[chapter] != part:
                out.append(
                    f"docs/book-structure.md: files {chapter} under {mapped[chapter]}, "
                    f"SUMMARY has it under {part}"
                )
        for chapter in sorted(set(mapped) - set(listed)):
            out.append(f"docs/book-structure.md: classifies {chapter}, which SUMMARY does not list")

    files = len(list(SUITE.rglob("*.robot")))
    stated = re.search(r"holds (\d+) `\.robot` files", text)
    if not stated:
        out.append(f"{book}: no suite file count to check")
    elif int(stated.group(1)) != files:
        out.append(f"{book}: says {stated.group(1)} suite files, TP/ holds {files}")

    # The federation chapter republishes the size of the two suites that
    # cover its surface. Same failure as the counts block: prose that was
    # true when it was written and drifts every time a case is added.
    fed = ROOT / "docs/src/federation.md"
    if not fed.exists():
        out.append("docs/src/federation.md: the federation chapter is missing")
        return out
    ftext = fed.read_text(encoding="utf-8")
    dist = robot_cases("TP/NGSI-LD/DistributedOperations")[0]
    iop = robot_cases("IOP_TP")[0]
    pair = re.search(r"\((\d+) \+ (\d+) tests\)", ftext)
    if not pair:
        out.append("docs/src/federation.md: no suite size to check")
    elif (int(pair.group(1)), int(pair.group(2))) != (dist, iop):
        out.append(
            f"docs/src/federation.md: says {pair.group(1)} + {pair.group(2)} tests, "
            f"the suites hold "
            f"{dist} + {iop}"
        )
    # The storage chapter's migration table is a list of files. A migration
    # added without its row leaves an operator reading a schema that is two
    # migrations behind — 0004 dropped a table 0001's row still advertises.
    mig = ROOT / "crates/antares-sql/migrations"
    book2 = ROOT / "docs/src/storage.md"
    if mig.is_dir() and book2.exists():
        on_disk = {f.stem for f in mig.glob("*.sql")}
        listed = set(re.findall(r"^\| `(\d{4}_[a-z0-9_]+)`", book2.read_text(encoding="utf-8"), re.M))
        for name in sorted(on_disk - listed):
            out.append(f"docs/src/storage.md: migration {name} has no row")
        for name in sorted(listed - on_disk):
            out.append(f"docs/src/storage.md: names migration {name}, which does not exist")

    # Same shape one directory over: the runbook enumerates the reference
    # manifests, and a manifest missing from that list is one an operator
    # copying it never applies — networkpolicy.yaml, the deny-by-default
    # ingress, was exactly that.
    k8s = ROOT / "deploy/k8s"
    ops = ROOT / "docs/src/operations.md"
    if k8s.is_dir() and ops.exists():
        otext = ops.read_text(encoding="utf-8")
        for f in sorted(k8s.glob("*.yaml")):
            if f"`{f.name}`" not in otext:
                out.append(f"docs/src/operations.md: manifest {f.name} is not named")

    lone = re.search(r"(\d+)-test IOP tree", ftext)
    if lone and int(lone.group(1)) != iop:
        out.append(
            f"docs/src/federation.md: says a {lone.group(1)}-test IOP tree, "
            f"it holds {iop}"
        )
    return out


def book_fence_violations():
    """CommonMark 4.5: a closing code fence may be followed only by spaces.
    A ``` with prose glued to its right is therefore NOT a close — the block
    stays open and eats the rest of the chapter as code. One such line in the
    getting-started chapter swallowed a paragraph, a heading and two runnable
    snippets, and neither `mdbook build` nor `mdbook test` says a word about
    it: both render the result exactly as written."""
    out = []
    for path in sorted((ROOT / "docs/src").rglob("*.md")):
        opened = None
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            m = re.match(r"^ {0,3}(`{3,})(.*)$", line)
            if not m:
                continue
            ticks, rest = len(m.group(1)), m.group(2)
            if opened is None:
                # An info string cannot contain a backtick, so a line that
                # does is prose, not a fence.
                if "`" not in rest:
                    opened = (ticks, n)
            elif ticks >= opened[0]:
                if rest.strip():
                    out.append(
                        f"{path.relative_to(ROOT)}:{n}: fence carries trailing text "
                        f"{rest.strip()[:40]!r}, so the block opened on line {opened[1]} "
                        f"never closes"
                    )
                    opened = None
                    break
                opened = None
        if opened is not None:
            out.append(f"{path.relative_to(ROOT)}:{opened[1]}: code fence is never closed")
    return out


def book_error_title_violations():
    """An error body in the book names a `title`, and 6.3.6 makes that member
    the error type — a reader matches on it. The set the broker can actually
    emit is closed: `NgsiError::kind()` plus the few statuses built by hand
    (508 has no variant). A prettified title is a body no client will ever
    see: the book printed "Bad Request Data" where the broker sends
    "BadRequestData"."""
    src = ROOT / "crates"
    err = (src / "antares-model/src/error.rs").read_text(encoding="utf-8")
    body = re.search(r"fn kind\(&self\).*?\n    \}", err, re.S)
    if not body:
        return ["crates/antares-model/src/error.rs: kind() no longer parses"]
    known = set(re.findall(r'=>\s*"([^"]+)"', body.group(0)))
    for path in src.rglob("*.rs"):
        known.update(re.findall(r'"title"\s*:\s*"([^"]+)"', path.read_text(encoding="utf-8")))
    out = []
    # The book only: docs/spec/ transcribes the standard, whose own examples
    # carry the spec's titles rather than this broker's.
    for path in sorted((ROOT / "docs/src").rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for n, line in enumerate(text.splitlines(), 1):
            for title in re.findall(r'"title"\s*:\s*"([^"]+)"', line):
                if title not in known:
                    out.append(
                        f"{path.relative_to(ROOT)}:{n}: error title {title!r} is one "
                        f"no broker response carries"
                    )
    return out


WORD_NUMBERS = {
    w: str(i)
    for i, w in enumerate(
        "zero one two three four five six seven eight nine ten eleven twelve".split()
    )
}


def suite_count_violations():
    """`dev/etsi-suites.sh` is the runner's own list of suites, and every cell
    runs all of them plus the IOP step. Prose that states that number instead
    of deriving it goes stale the moment a suite is added: two were added to
    SERIAL_ALL while five comments and table rows still promised eight, so the
    workflow that runs ten advertised eight to everyone reading it. The same
    list is what SUITE_DIRS counts cases from, and a suite in one and not the
    other drops out of the headline totals without a word."""
    runner = ROOT / "dev/etsi-suites.sh"
    if not runner.exists():
        return [f"{runner}: the suite list is missing"]
    hit = re.search(r'^SERIAL_ALL="([^"]*)"', runner.read_text(encoding="utf-8"), re.M)
    if not hit:
        return [f"{runner}: no SERIAL_ALL to read the suite list from"]
    serial, out = hit.group(1).split(), []
    listed = {d.removeprefix("TP/NGSI-LD/") for _, d in SUITE_DIRS if d != "IOP_TP"}
    for name in sorted(set(serial) - listed):
        out.append(f"dev/spec.py: SUITE_DIRS has no entry for the suite {name}")
    for name in sorted(listed - set(serial)):
        out.append(f"dev/spec.py: SUITE_DIRS counts {name}, which no cell runs")
    total = len(serial) + 1  # the serial suites, then IOP against all five brokers
    scan = [ROOT / "README.md", ROOT / "ARCHITECTURE.md", ROOT / "CONTRIBUTING.md"]
    scan += sorted((ROOT / "docs/src").rglob("*.md"))
    scan += sorted((ROOT / ".github/workflows").glob("*.yml"))
    for path in scan:
        if not path.exists():
            continue
        # Whole text, not line by line: prose wraps, and the count that drifted
        # in CONTRIBUTING sat on the line above the word `suites`.
        text = path.read_text(encoding="utf-8")
        for m in re.finditer(r"\b([a-z]+|\d+)\s+suites\b", text):
            said = WORD_NUMBERS.get(m.group(1), m.group(1))
            if said.isdigit() and int(said) != total:
                line = text.count("\n", 0, m.start()) + 1
                out.append(
                    f"{path.relative_to(ROOT)}:{line}: says {m.group(1)} suites, "
                    f"a cell runs {total}"
                )
    return out


def robot_recipe_violations():
    """The suite ships upstream's compose addresses in `resources/variables.py`:
    the broker is `scorpio1`, the notification mock and the two context-source
    mocks are `172.28.0.18`. Both runners rewrite all five before every run, a
    bare `robot` invocation rewrites none, so a published recipe that omits one
    points its TPs at a host the box cannot resolve — and `Start Local Server`
    on an address the box does not own never comes up at all. A documented
    recipe is runnable only if it overrides everything the runner does."""
    run = ROOT / "dev/etsi-run.sh"
    if not run.exists():
        return [f"{run}: the suite runner is missing"]
    names = re.findall(r'sed -i "s\|\^(\w+) = ', run.read_text(encoding="utf-8"))
    if not names:
        return [f"{run}: no variables.py overrides to compare a recipe against"]
    out = []
    for path in [ROOT / "CONTRIBUTING.md", *sorted((ROOT / "docs/src").rglob("*.md"))]:
        if not path.exists():
            continue
        blocks = path.read_text(encoding="utf-8").split("```")
        for block in blocks[1::2]:
            if "robot --variable" not in block:
                continue
            for name in names:
                if f"--variable {name}:" not in block:
                    out.append(
                        f"{path.relative_to(ROOT)}: a robot recipe leaves {name} at the "
                        f"suite's compose default, which the runner overrides"
                    )
    return out


def architecture_size_violations():
    """ARCHITECTURE.md sizes every crate and every `antares-api` module so a
    reader can tell where the weight sits before opening anything. Typed by
    hand they drift with the code and stop meaning anything: the sql crate was
    published at 2 900 lines while it held 9 415, and the store crate at half
    its size, which sends a reader to the wrong file for the biggest change in
    the workspace. Rounding is the author's business — the check only refuses a
    number that is no longer the right size."""
    doc = ROOT / "ARCHITECTURE.md"
    if not doc.exists():
        return [f"{doc}: the code map is missing"]
    out, tolerance = [], 0.10

    def lines_of(paths):
        n = 0
        for f in paths:
            n += len(f.read_bytes().split(b"\n")) - 1
        return n

    sizes = {}
    for d in sorted((ROOT / "crates").iterdir()):
        if (d / "src").is_dir():
            sizes[d.name] = lines_of(sorted((d / "src").rglob("*.rs")))
    for f in sorted((ROOT / "crates/antares-api/src").glob("*.rs")):
        sizes[f.name] = lines_of([f])

    for line in doc.read_text(encoding="utf-8").splitlines():
        row = re.match(r"\| `([a-z_0-9.-]+)` \| ([\d ]+) \|", line)
        if not row or row.group(1) not in sizes:
            continue
        name, stated = row.group(1), int(row.group(2).replace(" ", ""))
        actual = sizes[name]
        if abs(stated - actual) > max(tolerance * actual, 2):
            out.append(
                f"ARCHITECTURE.md: sizes {name} at {stated} lines, it holds {actual}"
            )
    for name in sorted(sizes):
        if name.endswith(".rs") and name not in ("geo.rs", "qeval.rs", "regexcache.rs"):
            if not re.search(rf"^\| `{re.escape(name)}` \|", doc.read_text(encoding="utf-8"), re.M):
                out.append(f"ARCHITECTURE.md: the module table has no row for {name}")
    return out


def shared_crate_violations():
    """The shared-crates chapter names the crates a gateway can take on its own,
    and the workspace workflow is what proves each one still builds, tests and
    documents alone without naming the broker or a storage backend. The two
    lists are one claim: a crate published in the chapter and missing from the
    matrix is a promise nothing checks."""
    book = ROOT / "docs/src/shared-crates.md"
    wf = ROOT / ".github/workflows/workspace.yml"
    if not book.exists() or not wf.exists():
        return [f"{book if not book.exists() else wf}: missing"]
    published = set(re.findall(r"^\| `(antares-[a-z]+)` \|", book.read_text(encoding="utf-8"), re.M))
    gated = re.search(r"crate: \[([^\]]*)\]", wf.read_text(encoding="utf-8"))
    if not gated:
        return [f"{wf}: no shared-crate matrix to compare the chapter against"]
    gated = {c.strip() for c in gated.group(1).split(",") if c.strip()}
    out = []
    for name in sorted(published - gated):
        out.append(f"docs/src/shared-crates.md publishes {name}, which the standalone matrix omits")
    for name in sorted(gated - published):
        out.append(f"workspace.yml gates {name} standalone, which the shared-crates chapter omits")
    return out


# ADR-0018 names the four references that select a channel or a tool rather
# than a version; pinning them would freeze the resolution, not the action.
UNVERSIONED_USES = {
    "dtolnay/rust-toolchain@stable",
    "dtolnay/rust-toolchain@nightly",
    "dtolnay/rust-toolchain@master",
    "taiki-e/install-action@nextest",
}


def workflow_pin_violations():
    """ADR-0018 states what every workflow may run: a `uses:` naming a path in
    this repository or carrying a version tag, a `run:` step that never asks a
    release API which version is newest, and a `permissions:` block on every
    job. Its confirmation was a grep somebody had to remember to run — which is
    how `advisories.yml` came to resolve `releases/latest` in a job holding
    `issues: write`. The decision is checkable, so it is checked."""
    out = []
    for path in sorted((ROOT / ".github").rglob("*.yml")):
        rel = path.relative_to(ROOT)
        text = path.read_text(encoding="utf-8")
        for n, line in enumerate(text.splitlines(), 1):
            hit = re.search(r"uses:\s*(\S+)", line)
            if hit:
                ref = hit.group(1)
                if ref.startswith("./"):
                    pass
                elif ref in UNVERSIONED_USES:
                    pass
                elif not re.search(r"@(v[\d.]+|[0-9a-f]{40})$", ref):
                    out.append(f"{rel}:{n}: `uses: {ref}` names neither a version nor a commit")
            if "releases/latest" in line:
                out.append(f"{rel}:{n}: asks a release API which version is newest")
        try:
            doc = yaml.safe_load(text)
        except yaml.YAMLError as e:
            out.append(f"{rel}: does not parse: {e}")
            continue
        if not isinstance(doc, dict) or "jobs" not in doc:
            continue
        for name, job in (doc.get("jobs") or {}).items():
            if not isinstance(job, dict) or "uses" in job:
                continue  # a called workflow declares its own
            if "permissions" not in job and "permissions" not in doc:
                out.append(f"{rel}: job {name} runs without a permissions block")
    return out


def wrapped_literal_violations():
    """A Rust string literal wrapped across source lines WITHOUT a trailing `\\`
    keeps every space of the next line''s indentation. The message still
    compiles, still reads correctly in the source, and ships the indentation to
    whoever receives it: two ProblemDetails bodies told a client its
    contextSourceInfo pair "is not a valid HTTP header<30 spaces>(RFC 7230)".
    Nothing in a message a client reads needs three spaces in a row."""
    out = []
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for m in re.finditer(r'"([^"\\]|\\.)*"', line):
                if re.search(r"\S {3,}\S", m.group(0)):
                    out.append(
                        f"{path.relative_to(ROOT)}:{n}: a string literal carries a run of spaces "
                        f"— a line wrapped without its `\\`"
                    )
    return out


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

    errors.extend(chapter_violations())
    errors.extend(book_fence_violations())
    errors.extend(suite_count_violations())
    errors.extend(robot_recipe_violations())
    errors.extend(architecture_size_violations())
    errors.extend(shared_crate_violations())
    errors.extend(workflow_pin_violations())
    errors.extend(wrapped_literal_violations())
    errors.extend(book_error_title_violations())

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
        "statements": cmd_statements,
    }.get(cmd, cmd_status)()
