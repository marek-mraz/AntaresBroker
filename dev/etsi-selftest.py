#!/usr/bin/env python3
"""Self-check for the ETSI resource-sampling pair.

The two pieces with real logic in them: the sampler's /proc parsing, and the
report's sample↔TP interval join. Both fail silently if they break — an empty
CSV column looks exactly like "nothing happened" — so they get one assert-based
check. std-only, no framework.

Run: python3 dev/etsi-selftest.py
"""
import csv
import os
import subprocess
import sys
import tempfile
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import importlib.util

spec = importlib.util.spec_from_file_location("sampler", f"{HERE}/etsi-sampler.py")
sampler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sampler)


def check_proc_parsing():
    """cpu_ticks/rss_mib must read THIS process, and CPU must advance."""
    me = os.getpid()
    rss = sampler.rss_mib(me)
    assert rss and rss > 0, f"rss_mib({me}) returned {rss!r} — /proc parsing broke"

    t0 = sampler.cpu_ticks(me)
    assert t0 is not None, "cpu_ticks returned None for a live process"
    x = 0
    for i in range(3_000_000):  # burn measurable CPU
        x += i
    t1 = sampler.cpu_ticks(me)
    assert t1 > t0, f"cpu ticks did not advance across real work ({t0} -> {t1})"

    assert sampler.cpu_ticks(2**30) is None, "a dead pid must yield None, not raise"
    assert sampler.rss_mib(2**30) is None, "a dead pid must yield None, not raise"
    print(f"  proc parsing ok (self rss {rss:.0f} MiB, ticks {t0}->{t1})")


def check_join():
    """A sample inside a TP gets that TP; one in the gap gets (between tests);
    one outside the suite entirely stays unlabelled."""
    base = datetime(2026, 8, 4, 12, 0, 0)
    t0 = base.timestamp()

    def iso(offset):
        return datetime.fromtimestamp(t0 + offset).strftime("%Y-%m-%dT%H:%M:%S.%f")

    with tempfile.TemporaryDirectory() as tmp:
        os.mkdir(f"{tmp}/DemoSuite")
        with open(f"{tmp}/DemoSuite/output.xml", "w") as fh:
            fh.write(f"""<?xml version="1.0"?>
<robot>
<suite id="s1" name="DemoSuite">
  <test id="s1-t1" name="001_01 First"><status status="PASS" start="{iso(0)}" elapsed="2.0"/></test>
  <test id="s1-t2" name="002_01 Second"><status status="FAIL" start="{iso(10)}" elapsed="2.0">boom went the payload</status></test>
  <status status="FAIL" start="{iso(0)}" elapsed="12.0"/>
</suite>
<statistics><total><stat pass="1" fail="1" skip="0">All Tests</stat></total></statistics>
</robot>""")

        with open(f"{tmp}/resource-samples.csv", "w") as fh:
            fh.write("ts,iso,container,phase,cpu_pct,rss_mib\n")
            for offset in (1, 6, 11, 500):  # in t1, gap, in t2, far outside
                fh.write(f"{t0 + offset:.0f},{iso(offset)[:19]},antares1,DemoSuite,10.0,100.0\n")

        env = {**os.environ, "RESULTS": tmp, "STORE": "memory",
               "MEM_LIMIT_MB": "350", "IMAGE_BYTES": "0"}
        subprocess.run([sys.executable, f"{HERE}/etsi-report.py"],
                       env=env, check=True, capture_output=True)

        got = {int(float(r["ts"]) - t0): r["test"]
               for r in csv.DictReader(open(f"{tmp}/resource-samples.csv"))}
        assert got[1] == "001_01 First", f"in-test sample mislabelled: {got[1]!r}"
        assert got[11] == "002_01 Second", f"in-test sample mislabelled: {got[11]!r}"
        assert got[6] == "(between tests)", f"gap sample mislabelled: {got[6]!r}"
        assert got[500] == "", f"sample outside the run must stay unlabelled: {got[500]!r}"

        fails = list(csv.DictReader(open(f"{tmp}/failures.csv")))
        assert len(fails) == 1 and fails[0]["test"] == "002_01 Second", fails
        assert "boom went the payload" in fails[0]["message"], fails

        rollup = list(csv.DictReader(open(f"{tmp}/resource-by-test.csv")))
        assert any(r["test"] == "001_01 First" and r["samples"] == "1" for r in rollup), rollup

        summary = open(f"{tmp}/run-summary.md").read()
        assert "002_01 Second" in summary, "the failure must reach the summary"
        assert open(f"{tmp}/gate-status.txt").read().strip() == "FAIL", \
            "a failing suite must fail the gate"
        print(f"  join ok ({len(got)} samples labelled, gate FAIL on a red suite)")


if __name__ == "__main__":
    print("etsi sampling self-check")
    check_proc_parsing()
    check_join()
    print("OK")
