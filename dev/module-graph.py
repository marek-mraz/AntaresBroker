#!/usr/bin/env python3
"""Module graph per crate from the source, and the cycle gate.

    dev/module-graph.py            report + gate: exit 1 when a crate's
                                   largest strongly connected component
                                   grew past dev/module-baseline.json
    dev/module-graph.py --update   rewrite the baseline from this run
    dev/module-graph.py -v         also print every edge
    dev/module-graph.py --self-test

A module is a top-level `src/<name>.rs` or `src/<name>/` (the root file
counts as `lib` or `main`); an edge is a `crate::<name>` reference in the
module's source, brace imports included. `cargo modules` records call
edges and misses field types, so a struct that only holds another
module's type is invisible there; the text is the honest graph.
"""
import json, os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINE = os.path.join(ROOT, "dev", "module-baseline.json")
REF = re.compile(r"\bcrate::(\{[^}]*\}|\w+)")


def refs(text):
    for m in REF.finditer(text):
        body = m.group(1)
        if body.startswith("{"):
            for item in body[1:-1].split(","):
                head = item.strip().split("::")[0].split(" ")[0]
                if head and head != "self":
                    yield head
        else:
            yield body


def crate_graph(src):
    """{module: set(modules it references)} for one crate's src/ dir."""
    files = {}
    for entry in sorted(os.listdir(src)):
        path = os.path.join(src, entry)
        if entry.endswith(".rs"):
            name = {"lib.rs": "lib", "main.rs": "main"}.get(entry, entry[:-3])
            files.setdefault(name, []).append(path)
        elif os.path.isdir(path):
            for d, _, fs in os.walk(path):
                files.setdefault(entry, []).extend(os.path.join(d, f) for f in fs if f.endswith(".rs"))
    edges = {}
    for mod, paths in files.items():
        text = "".join(open(p, errors="replace").read() for p in paths)
        edges[mod] = {r for r in refs(text) if r in files and r != mod}
    return edges


def sccs(edges):
    """Tarjan: list of components, each a sorted list of module names."""
    index, low, stack, on, out, counter = {}, {}, [], set(), [], [0]

    def visit(v):
        index[v] = low[v] = counter[0]; counter[0] += 1
        stack.append(v); on.add(v)
        for w in edges.get(v, ()):
            if w not in index:
                visit(w); low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop(); on.discard(w); comp.append(w)
                if w == v: break
            out.append(sorted(comp))

    for v in sorted(edges):
        if v not in index: visit(v)
    return out


def self_test():
    g = {"a": {"b"}, "b": {"c"}, "c": {"a", "d"}, "d": set(), "e": {"a"}}
    assert max(sccs(g), key=len) == ["a", "b", "c"]
    assert sccs({"x": set()}) == [["x"]]
    assert set(refs("use crate::{a, b::c, self}; crate::d::e(); crate::a")) == {"a", "b", "d"}
    print("self-test ok")


def main(argv):
    if "--self-test" in argv:
        return self_test()
    verbose, update = "-v" in argv, "--update" in argv
    base = json.load(open(BASELINE)) if os.path.exists(BASELINE) else {}
    result, red = {}, []
    crates = os.path.join(ROOT, "crates")
    for crate in sorted(os.listdir(crates)):
        src = os.path.join(crates, crate, "src")
        if not os.path.isdir(src): continue
        edges = crate_graph(src)
        big = max(sccs(edges), key=len)
        result[crate] = len(big)
        n_edges = sum(len(v) for v in edges.values())
        cap = base.get(crate, 1)
        mark = "" if len(big) <= cap else f"  <- above the baseline {cap}"
        print(f"{crate}: {len(edges)} modules, {n_edges} edges, largest cycle {len(big)}{mark}")
        if len(big) > 1: print("  cycle: " + " ".join(big))
        if verbose:
            for m in sorted(edges):
                for t in sorted(edges[m]): print(f"  {m} -> {t}")
        if len(big) > cap: red.append(crate)
    if update:
        json.dump(result, open(BASELINE, "w"), indent=2, sort_keys=True); open(BASELINE, "a").write("\n")
        print(f"baseline written: {BASELINE}")
        return 0
    if red:
        print("module graph gate: cycle grew in " + ", ".join(red))
        return 1
    print("module graph gate: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
