#!/usr/bin/env python3
"""Rank functions by complexity x churn x (missing) coverage.

usage: code-radar.py complexity.csv [coverage.json]

complexity.csv is lizard's --csv output; coverage.json is the llvm-cov JSON
export (optional — without it every function counts as uncovered-unknown and
the rank is complexity x churn). Churn = commits touching the file since the
repo's first commit. Score = CCN * log2(churn + 1) * (2 if uncovered else 1).
"""
import csv
import json
import math
import subprocess
import sys
from collections import Counter

def churn():
    out = subprocess.run(
        ["git", "log", "--format=", "--name-only", "--", "crates"],
        capture_output=True, text=True, check=True).stdout
    return Counter(l for l in out.splitlines() if l.endswith(".rs"))

def covered(path):
    """function name -> execution count, from llvm-cov's export."""
    if not path:
        return None
    data = json.load(open(path))
    counts = {}
    for d in data.get("data", []):
        for f in d.get("functions", []):
            counts[f["name"]] = max(counts.get(f["name"], 0), f.get("count", 0))
    return counts

def main():
    cov = covered(sys.argv[2] if len(sys.argv) > 2 else "")
    ch = churn()
    rows = []
    for r in csv.reader(open(sys.argv[1])):
        nloc, ccn, _tok, _par, _len, _loc, file, name, _sig, start, _end = r[:11]
        if "/tests/" in file or file.endswith("/tests.rs"):
            continue
        ccn = int(ccn)
        c = ch.get(file, 0)
        # mangled names carry the function name as a path segment
        if cov is None:
            unc, label = True, "n/a"
        else:
            unc = not any(k.endswith("::" + name) or k == name for k in cov if name in k)
            label = "uncov" if unc else "cov"
        score = ccn * math.log2(c + 1) * (2 if unc else 1)
        rows.append((score, ccn, int(nloc), c, label, file, name, start))
    rows.sort(reverse=True)
    print("coverage: %s" % ("none supplied — rank is complexity x churn" if cov is None else sys.argv[2]))
    print("%7s %4s %5s %5s %-5s  %s" % ("score", "CCN", "NLOC", "churn", "cov", "function"))
    for s, ccn, nloc, c, u, f, n, st in rows[:200]:
        print("%7.1f %4d %5d %5d %-5s  %s:%s %s" % (s, ccn, nloc, c, u, f, st, n))

if __name__ == "__main__":
    main()
