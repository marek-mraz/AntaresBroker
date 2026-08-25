#!/usr/bin/env python3
# Parse robot output.xml files -> a failures-only report.
# For every FAILED test it emits: the requirements (spec tags + doc),
# what is wrong (the assertion message: expected vs actual), and the
# HTTP request/response that was under test.
import datetime
import sys, glob, xml.etree.ElementTree as ET, os

def collect_msgs(el, out):
    for child in el:
        if child.tag == 'msg':
            out.append(child.text or '')
        else:
            collect_msgs(child, out)
    return out

def expected_diff_block(msgs):
    """The ETSI comparison keywords log a full unified diff right after a
    'Dictionary comparison failed with ->' / 'comparison failed with ->' marker.
    In that block '-' lines are the EXPECTED reference body and '+' lines are the
    ACTUAL response. Return that block so the report can show the expected body."""
    for i, m in enumerate(msgs):
        s = (m or '').strip()
        if s.endswith('comparison failed with ->') and i + 1 < len(msgs):
            nxt = msgs[i + 1]
            if nxt and nxt.strip():
                return nxt
    return None

def expected_var_dumps(msgs):
    """Fallback: Robot logs resolved reference bodies as '${...expectation...} = {...}'
    or '${temporal_entity_representation} = {...}'. Grab the largest such dump."""
    best = None
    for m in msgs:
        s = (m or '').strip()
        if ' = ' in s and (s.startswith('${')) and ('{' in s or '[' in s):
            name = s.split(' = ', 1)[0]
            if any(k in name.lower() for k in ('expect', 'representation', 'expected')):
                val = s.split(' = ', 1)[1]
                if best is None or len(val) > len(best):
                    best = val
    return best

def reqresp_pairs(msgs):
    """Return list of (request_json, response_json) blocks logged by the suite's Output keyword."""
    pairs, cur = [], {}
    label = None
    for m in msgs:
        s = (m or '').strip()
        if s == 'Request ->':
            label = 'req'; continue
        if s == 'Response ->':
            label = 'resp'; continue
        if label and s.startswith('{'):
            cur[label] = s
            if 'req' in cur and 'resp' in cur:
                pairs.append((cur.get('req'), cur.get('resp'))); cur = {}
            label = None
    return pairs

# Full CI matrix — these are the result-dir names dev/etsi-serial.sh produces (keep in sync with the
# suite loop there). The "left out" footer flags any of these that has no parseable output.xml, so it
# must use the SAME names the runner writes (ContextInformation-* grouping), not the old flat ETSI
# folder names — otherwise it falsely reports every suite as "not run".
EXPECTED_SUITES = [
    "CommonBehaviours",
    "ContextInformation-Consumption", "ContextInformation-Provision",
    "ContextInformation-Subscription",
    "ContextSource", "jsonldContext",
    "DistributedOperations", "IOP",
]
LEFT_OUT_REASON = {
}

def main(results_glob, outfile):
    files = sorted(glob.glob(results_glob))
    # First pass counts pass/fail per suite so the report can lead with totals.
    counts = []  # (suite, npass, nfail)
    for xmlf in files:
        try:
            root = ET.parse(xmlf).getroot()
        except Exception:
            continue
        suite_name = os.path.basename(os.path.dirname(xmlf)).replace('results-', '')
        tot = root.find('./statistics/total')
        if tot is not None and tot.findall('stat'):
            s = tot.findall('stat')[-1]
            counts.append((suite_name, int(s.get('pass')), int(s.get('fail'))))
    total_fail = sum(c[2] for c in counts)
    total_pass = sum(c[1] for c in counts)

    with open(outfile, 'w') as f:
        f.write(f"# {datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n")
        f.write("# ETSI NGSI-LD — Failing tests report\n\n")
        f.write(f"## ❌ Errors: {total_fail}   (✅ passing: {total_pass})\n\n")
        f.write("| Suite | Pass | **Fail** |\n|---|---|---|\n")
        for suite_name, np, nf in counts:
            f.write(f"| {suite_name} | {np} | **{nf}** |\n")
        f.write(f"| **TOTAL** | **{total_pass}** | **{total_fail}** |\n\n")
        ran = {c[0] for c in counts}
        left_out = [s for s in EXPECTED_SUITES if s not in ran]
        if left_out:
            f.write("### Suites left out (no results)\n\n")
            for s in left_out:
                f.write(f"- **{s}** — {LEFT_OUT_REASON.get(s, 'not run')}\n")
            f.write("\n")
        f.write("Below: only FAILED tests. Each entry has the requirement (spec clauses), what is "
                "wrong (expected vs actual), and the HTTP request/response under test.\n\n")
        for xmlf in files:
            try:
                root = ET.parse(xmlf).getroot()
            except Exception as e:
                f.write(f"## {xmlf}: PARSE ERROR {e}\n\n"); continue
            suite_name = os.path.basename(os.path.dirname(xmlf)).replace('results-', '')
            fails = [t for t in root.iter('test')
                     if (t.find('status') is not None and t.find('status').get('status') == 'FAIL')]
            if not fails:
                continue
            f.write(f"\n## {suite_name}  ({len(fails)} failing)\n\n")
            for t in fails:
                st = t.find('status')
                name = t.get('name')
                tags = [tg.text for tg in t.findall('tag')]
                doc = t.find('doc').text if t.find('doc') is not None else ''
                msg = (st.text or '').strip()
                f.write(f"### {name}\n")
                if doc: f.write(f"- **Requirement (doc):** {doc}\n")
                if tags: f.write(f"- **Spec clauses / tags:** {', '.join(tags)}\n")
                f.write(f"- **What is wrong (expected vs actual):**\n\n  ```\n  {msg}\n  ```\n")
                all_msgs = collect_msgs(t, [])
                diff = expected_diff_block(all_msgs)
                if diff:
                    f.write("- **Expected vs actual body (`-` = expected reference, `+` = actual response):**\n\n"
                            "```diff\n" + diff[:4000] + "\n```\n")
                else:
                    exp = expected_var_dumps(all_msgs)
                    if exp:
                        f.write("- **Expected body (reference):**\n\n```json\n" + exp[:3000] + "\n```\n")
                pairs = reqresp_pairs(all_msgs)
                if pairs:
                    req, resp = pairs[-1]  # the request under assertion (last one)
                    if len(pairs) > 1:
                        f.write(f"- _(test also issued {len(pairs)-1} setup request(s) before this one)_\n")
                    f.write("- **Request under test:**\n\n```json\n" + (req or '')[:2000] + "\n```\n")
                    f.write("- **Actual response:**\n\n```json\n" + (resp or '')[:2000] + "\n```\n")
                else:
                    f.write("- _(no Request/Response block logged for this test)_\n")
                f.write("\n")
        f.write(f"\n---\n**Total failing tests: {total_fail}**\n")
    print(f"Wrote {outfile}: {total_fail} failing tests across {len(files)} result file(s)")

if __name__ == '__main__':
    g = sys.argv[1] if len(sys.argv) > 1 else 'results-*/output.xml'
    o = sys.argv[2] if len(sys.argv) > 2 else 'etsi-failures.md'
    main(g, o)
