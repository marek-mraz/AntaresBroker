#!/usr/bin/env python3
"""Fold every cargo-nextest JUnit file into ONE unit-test page.

Usage: unit-report-site.py <junit-dir> <out-dir>
  <junit-dir> holds one <config>.xml per feature/store config, written by
  dev/unit-junit.sh (default, no-default-features, all-features, ignored-db,
  timescale-store, ...). The file STEM is the config name on the page.

Writes <out-dir>/:
  index.html      every config's crate×binary rows with pass/fail/skip and
                  the failure messages inline — the stats are ON the page
  ../badge-unit.json  shields.io endpoint schema ("2431 passing" / red)

Deliberately NOT Robot XML: nothing but Robot writes output.xml, JUnit is
what nextest emits natively and what every CI already reads. The ETSI half
keeps its Robot report.html/log.html drill-down and this page links to it —
one page for the numbers, Robot's own HTML for the ETSI detail.
"""
import glob
import html
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

junit = sys.argv[1] if len(sys.argv) > 1 else "unit-junit"
out = sys.argv[2] if len(sys.argv) > 2 else "site/reports/unit"
os.makedirs(out, exist_ok=True)

# how it reads on the page, in the order CI runs them; anything else lands
# after these, alphabetically
PREFERRED = (
    "default ignored-db-api ignored-db-broker "
    "no-default-features all-features timescale-store"
).split()

# dev/unit-junit.sh stamps this — JUnit itself records no commit, so without
# it the page cannot say WHICH revision the failures belong to.
try:
    META = json.load(open(os.path.join(junit, "meta.json")))
except Exception:
    META = {}
SHA = META.get("sha", "")
REPO = META.get("repo", "marek-mraz/AntaresBroker")
BLOB = f"https://github.com/{REPO}/blob/{SHA or 'master'}"

# `panicked at crates/antares-ql/src/parser.rs:88:13` — the ONE place a Rust
# failure says where it happened. It lives in the free-text failure body, so
# it has to be pulled out by hand to become a link.
AT = re.compile(r"([A-Za-z0-9_./-]+\.rs):(\d+)(?::\d+)?")


def suites_of(root):
    """<testsuite> elements, whether the file is wrapped in <testsuites> or not."""
    return root.iter("testsuite") if root.tag == "testsuites" else [root]


def locate(msg):
    """First crates/… source position named in a panic body -> (path, line)."""
    for path, line in AT.findall(msg):
        if path.startswith("crates/") or path.startswith("dev/"):
            return path, line
    return None, None


configs, failures = [], []
for path in sorted(glob.glob(f"{junit}/*.xml")):
    name = os.path.basename(path)[: -len(".xml")]
    try:
        root = ET.parse(path).getroot()
    except Exception as e:  # truncated upload, disk full mid-write
        configs.append((name, [], 0, 1, 0, 0, f"unreadable: {e}"))
        continue
    rows, tp = [], [0, 0, 0, 0]
    for ts in suites_of(root):
        p = f = sk = fl = 0
        for tc in ts.iter("testcase"):
            bad = tc.find("failure")
            if bad is None:
                bad = tc.find("error")
            # a test nextest retried and then PASSED. Counting it green hides
            # the flake; counting it red fails a run that is actually green.
            flaky = tc.find("flakyFailure") is not None or tc.find("rerunFailure") is not None
            if tc.find("skipped") is not None:
                sk += 1
            elif bad is not None:
                f += 1
                msg = (bad.text or bad.get("message") or "").strip()
                failures.append(
                    (name, ts.get("name", "?"), tc.get("name", "?"), msg, locate(msg))
                )
            else:
                p += 1
                fl += 1 if flaky else 0
        rows.append(
            (ts.get("name", "?"), p, f, sk, fl, float(ts.get("time", "0") or 0))
        )
        tp[0] += p
        tp[1] += f
        tp[2] += sk
        tp[3] += fl
    configs.append((name, sorted(rows), tp[0], tp[1], tp[2], tp[3], None))

order = {n: i for i, n in enumerate(PREFERRED)}
configs.sort(key=lambda c: (order.get(c[0], len(PREFERRED)), c[0]))

total_pass = sum(c[2] for c in configs)
total_fail = sum(c[3] for c in configs)
total_skip = sum(c[4] for c in configs)
total_flaky = sum(c[5] for c in configs)
green = bool(configs) and total_fail == 0

json.dump(
    {
        "schemaVersion": 1,
        "label": "unit tests",
        "message": (
            f"{total_pass} passing" if green else f"{total_fail} failing"
        ) if configs else "no recent run",
        "color": "brightgreen" if green else ("red" if configs else "lightgrey"),
    },
    open(os.path.join(out, os.pardir, "badge-unit.json"), "w"),
)

sections = []
# failures FIRST — the page is read when something is red, and scrolling
# past six green config tables to find the one panic is the wrong order.
if failures:
    sections.append("<section><h2>Failures</h2><table>")
    sections.append(
        "<tr><th>Config</th><th>Test</th><th>Where</th><th>Message</th></tr>"
    )
    for cfg, suite, test, msg, (src, line) in failures[:200]:
        short = msg if len(msg) <= 600 else msg[:600] + " …"
        where = (
            f'<a href="{BLOB}/{html.escape(src)}#L{line}"><code>'
            f"{html.escape(os.path.basename(src))}:{line}</code></a>"
            if src
            else "—"
        )
        sections.append(
            f'<tr class="bad"><td>{html.escape(cfg)}</td>'
            f"<td><code>{html.escape(suite)}::{html.escape(test)}</code></td>"
            f"<td>{where}</td>"
            f"<td><pre>{html.escape(short)}</pre></td></tr>"
        )
    sections.append("</table>")
    if len(failures) > 200:
        sections.append(f"<p>… and {len(failures) - 200} more.</p>")
    sections.append("</section>")

for name, rows, p, f, sk, fl, err in configs:
    mark = "✅" if not f and not err else "❌"
    head = f"{p}/{p + f + sk}" + (f" — {err}" if err else "")
    sections.append(
        f'<section><h2>{mark} {html.escape(name)} — {html.escape(head)}</h2><table>'
        "<tr><th>Crate / target</th><th>Pass</th><th>Fail</th><th>Skip</th>"
        "<th>Flaky</th><th>Time</th></tr>"
    )
    for suite, sp, sf, ssk, sfl, secs in rows:
        cls = ' class="bad"' if sf else (' class="warn"' if sfl else "")
        sections.append(
            f"<tr{cls}><td><code>{html.escape(suite)}</code></td><td>{sp}</td>"
            f"<td>{sf}</td><td>{ssk}</td><td>{sfl or ''}</td><td>{secs:.2f}s</td></tr>"
        )
    sections.append("</table></section>")

banner = (
    f"{total_pass} passing, {total_skip} skipped across {len(configs)} configs"
    if green
    else f"{total_fail} FAILING across {len(configs)} configs"
) + (f" — {total_flaky} flaky (passed on retry)" if total_flaky else "")

run = META.get("run")
stamp = (
    f'commit <a href="https://github.com/{REPO}/commit/{SHA}"><code>{SHA[:8]}</code></a>'
    if SHA and SHA != "unknown"
    else "commit unknown"
) + (
    f' · <a href="https://github.com/{REPO}/actions/runs/{run}">CI run</a>'
    if run
    else ""
) + f" · branch <code>{html.escape(META.get('ref', '?'))}</code>"
page = f"""<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Antares — unit &amp; integration tests</title>
<style>
  body {{ font: 15px/1.5 system-ui, sans-serif; margin: 2rem auto; max-width: 60rem; padding: 0 1rem; color: #1a1a1a; background: #fff; }}
  h1 {{ font-size: 1.5rem; }}
  nav {{ margin-bottom: 1rem; font-size: .9rem; }}
  nav a {{ margin-right: 1rem; }}
  .banner {{ padding: .6rem 1rem; border-radius: .5rem; font-weight: 600;
             background: {"#e6f6e6" if green else "#fde8e8"};
             color: {"#176617" if green else "#8f1d1d"}; }}
  table {{ border-collapse: collapse; margin: .5rem 0 1.5rem; width: 100%; }}
  th, td {{ text-align: left; padding: .3rem .7rem; border-bottom: 1px solid #e5e5e5; vertical-align: top; }}
  tr.bad td {{ background: #fdf0f0; }}
  pre {{ margin: 0; white-space: pre-wrap; font-size: .8rem; }}
  footer {{ color: #666; font-size: .85rem; }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #111; color: #ddd; }}
    th, td {{ border-color: #333; }}
    tr.bad td {{ background: #3a1d1d; }}
    a {{ color: #7ab8ff; }}
  }}
</style>
<h1>Antares — unit &amp; integration tests</h1>
<nav><a href="../latest/">ETSI conformance (Robot drill-down) →</a>
<a href="../coverage/">Coverage →</a></nav>
<p class="banner">{html.escape(banner)}</p>
<p>Every cargo-nextest config from the <code>workspace</code> job, one JUnit
file each, folded here. The ETSI conformance suite is a separate run with
Robot's own HTML — follow the link above for its per-suite drill-down.</p>
{"".join(sections)}
<footer>Generated by dev/unit-report-site.py from the <code>unit-junit</code>
bundle. Doctests run in the same job but are gated separately: nextest cannot
execute them, so they are not counted above.</footer>
"""
open(os.path.join(out, "index.html"), "w").write(page)
print(f"site: {out}/index.html  unit: {total_pass}P/{total_fail}F/{total_skip}S")
