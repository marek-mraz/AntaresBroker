#!/usr/bin/env bash
# Duplication gate: token clones, structural duplicate candidates and
# unused surface, measured the same way locally and in CI, ratcheted
# against dev/dup-baseline.json so the numbers only ever go down.
#
#   dev/dup-check.sh            report + gate (exit 1 above the baseline)
#   dev/dup-check.sh --update   rewrite the baseline from this run
#
# Outputs land under results/dup/:
#   jscpd/jscpd-report.json     token clones (tests excluded), 60-token window
#   signatures.txt|json         same-signature / same-name / same-field candidates
#   dead-surface.txt            rustc dead_code + cargo machete unused deps
#   summary.md                  the table the job summary shows
#
# Needs: node (npx jscpd), nightly rustdoc, cargo machete, python3.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
OUT=results/dup
DOC=${DOC_DIR:-$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/doc}
BASE=dev/dup-baseline.json
mkdir -p "$OUT"

echo "== token clones (jscpd) =="
npm_config_ignore_scripts=true npx -y jscpd@4.3.0 crates --min-tokens 60 --min-lines 8 --max-size 4mb --max-lines 20000 --format rust \
  --ignore "**/tests/**,**/target/**" --reporters json --output "$OUT/jscpd" --silent >/dev/null 2>&1 \
  || { echo "jscpd failed"; exit 2; }
# jscpd cannot see #[cfg(test)] modules; drop clone pairs where both sides
# start inside the file's `#[cfg(test)] mod` block.
python3 - "$OUT" <<'EOF'
import json, sys
out = sys.argv[1]
rep = json.load(open(f"{out}/jscpd/jscpd-report.json"))
first_test = {}
def test_start(path):
    if path not in first_test:
        first_test[path] = 10**9
        lines = open(path, errors="replace").read().splitlines()
        for i, line in enumerate(lines, 1):
            if line.strip() == "#[cfg(test)]" and i < len(lines) and lines[i].lstrip().startswith("mod "):
                first_test[path] = i; break
    return first_test[path]
keep = []
for d in rep["duplicates"]:
    a, b = d["firstFile"], d["secondFile"]
    if a["start"] >= test_start(a["name"]) and b["start"] >= test_start(b["name"]):
        continue
    keep.append(d)
rep["duplicates"] = keep
rep["statistics"]["prod_clones"] = len(keep)
rep["statistics"]["prod_clone_lines"] = sum(d["lines"] for d in keep)
json.dump(rep, open(f"{out}/jscpd/jscpd-report.json", "w"), indent=1)
pairs = {}
for d in keep:
    k = tuple(sorted((d["firstFile"]["name"].split("crates/")[-1],
                       d["secondFile"]["name"].split("crates/")[-1])))
    pairs[k] = pairs.get(k, 0) + d["lines"]
with open(f"{out}/clones.txt", "w") as f:
    f.write(f"production clones: {len(keep)}, lines: {rep['statistics']['prod_clone_lines']}\n\n")
    for k, v in sorted(pairs.items(), key=lambda x: -x[1]):
        f.write(f"{v:5} lines  {k[0]}  <->  {k[1]}\n")
    f.write("\n")
    for d in sorted(keep, key=lambda d: -d["lines"]):
        f.write(f"{d['lines']:4}  {d['firstFile']['name'].split('crates/')[-1]}:{d['firstFile']['start']}"
                f"  <->  {d['secondFile']['name'].split('crates/')[-1]}:{d['secondFile']['start']}\n")
EOF
head -1 "$OUT/clones.txt"

echo "== structural candidates (rustdoc JSON) =="
if [ -z "${SKIP_RUSTDOC:-}" ]; then
  RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items" \
    cargo +nightly doc --workspace --no-deps -q 2>"$OUT/rustdoc.log" \
    || { echo "rustdoc json failed, see $OUT/rustdoc.log"; exit 2; }
fi
python3 dev/dup-signatures.py "$DOC" > "$OUT/signatures.txt"
python3 dev/dup-signatures.py "$DOC" --json > "$OUT/signatures.json"
head -1 "$OUT/signatures.txt"

echo "== dead surface =="
{ cargo build --workspace -q --message-format short 2>&1 | /usr/bin/grep -a "dead_code\|never used\|never read" || true
  cargo machete 2>/dev/null | /usr/bin/grep -a -- "-- " || true
} > "$OUT/dead-surface.txt"
echo "  $(wc -l < "$OUT/dead-surface.txt") items"

python3 - "$OUT" "$BASE" "${1:-}" <<'EOF'
import json, sys, os
out, base, mode = sys.argv[1:4]
rep = json.load(open(f"{out}/jscpd/jscpd-report.json"))["statistics"]
sig = json.load(open(f"{out}/signatures.json"))["counts"]
dead = sum(1 for _ in open(f"{out}/dead-surface.txt"))
now = {
    "clone_lines": rep["prod_clone_lines"],
    "clones": rep["prod_clones"],
    "same_signature": sig["same_signature"],
    "same_name": sig["same_name"],
    "same_fields": sig["same_fields"],
    "dead_surface": dead,
}
old = json.load(open(base)) if os.path.exists(base) else {}
rows = ["| metric | baseline | now | |", "|---|---|---|---|"]
worse = []
for k, v in now.items():
    b = old.get(k)
    flag = "" if b is None or v <= b else "UP"
    if flag: worse.append(k)
    rows.append(f"| {k} | {'-' if b is None else b} | {v} | {flag} |")
md = "\n".join(rows) + "\n"
open(f"{out}/summary.md", "w").write(md)
print(md)
if mode == "--update":
    json.dump(now, open(base, "w"), indent=1); open(base, "a").write("\n")
    print(f"baseline written to {base}")
elif worse:
    print(f"duplication gate: {', '.join(worse)} above baseline — merge, delete, or justify and run --update")
    sys.exit(1)
else:
    print("duplication gate: OK")
EOF
