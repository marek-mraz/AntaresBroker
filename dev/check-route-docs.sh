#!/usr/bin/env bash
# Gate: every route the broker serves OUTSIDE the NGSI-LD API root has a
# heading in the book. The NGSI-LD routes are the conformance ledger's
# (`docs/spec/`, `dev/spec.py`); these are the ones no clause describes, so
# a chapter is the only place they can be written down.
#
# Routes are read from the router rather than from a list kept by hand: the
# `Admin` surface's own `PATHS` array with the methods its `router` mounts,
# and every other literal `.route(...)` that is not inside the `let api =
# ...` statement nested under the API root. A surface an addon mounts under
# `/x` is the addon's to document; the `/x` contract itself is a heading here.
set -euo pipefail
cd "$(dirname "$0")/.."

python3 - <<'PY'
import re, sys, pathlib

SRC = pathlib.Path("crates/antares-api/src/lib.rs")
lib = SRC.read_text()
METHODS = ("get", "post", "put", "patch", "delete", "head", "options")


def statement(src, start):
    """(from, to) of the statement beginning at `start`, up to its own `;`."""
    i = src.index(start)
    depth = 0
    for j in range(i, len(src)):
        c = src[j]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ";" and depth == 0:
            return i, j
    raise SystemExit(f"{SRC}: unterminated statement at {start!r}")


def calls(src):
    """Every `.route(a, b)` as (offset, first argument, rest)."""
    for m in re.finditer(r"\.route\(", src):
        depth, k = 1, m.end()
        while depth:
            if src[k] in "([{":
                depth += 1
            elif src[k] in ")]}":
                depth -= 1
            k += 1
        args = src[m.end() : k - 1]
        # the top-level comma between the path and the method chain
        depth, cut = 0, None
        for n, c in enumerate(args):
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == "," and depth == 0:
                cut = n
                break
        if cut is None:
            raise SystemExit(f"{SRC}: .route with one argument at offset {m.start()}")
        yield m.start(), args[:cut].strip(), args[cut + 1 :]


def methods_of(chain):
    return [m.upper() for m in METHODS if re.search(rf"\b{m}\(", chain)]


routes = set()

# 1. The admin surface: PATHS gives the paths, its own router the methods.
a, b = statement(lib, "const PATHS:")
paths = re.findall(r'"(/[^"]*)"', lib[a:b])
a, b = statement(lib, "let [health_p")
names = re.split(r"\s*,\s*", lib[a:b].split("[", 1)[1].split("]", 1)[0])
by_name = dict(zip(names, paths, strict=True))
a, b = statement(lib, "let [health_p")
admin_from = lib.index("fn router(&self, _st: AppState) -> Router<AppState> {")
for off, arg, chain in calls(lib):
    if off > admin_from and arg in by_name:
        routes.update((m, "/q" + by_name[arg]) for m in methods_of(chain))

# 2. Every literal route outside the API nest — a spec resource is the
#    ledger's, not the book's.
api_from, api_to = statement(lib, "let api = Router::new()")
for off, arg, chain in calls(lib):
    if api_from <= off <= api_to or not arg.startswith('"'):
        continue
    routes.update((m, arg.strip('"')) for m in methods_of(chain))

if not routes:
    raise SystemExit(f"{SRC}: no routes parsed — the gate would pass vacuously")

book = "\n".join(p.read_text() for p in sorted(pathlib.Path("docs/src").glob("*.md")))
headings = set(re.findall(r"^#{2,6}\s+([A-Z]+)\s+(\S+)\s*$", book, re.M))

missing = sorted(r for r in routes if r not in headings)
for method, path in missing:
    print(f"UNDOCUMENTED route: {method} {path} (add a heading to docs/src/)")
if missing:
    sys.exit(1)
print(f"route docs check: OK ({len(routes)} non-NGSI-LD routes documented)")
PY
