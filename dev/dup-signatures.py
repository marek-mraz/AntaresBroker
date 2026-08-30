#!/usr/bin/env python3
"""Structural duplicate candidates from rustdoc JSON.

usage: dup-signatures.py <doc-dir> [--json]

<doc-dir> holds the antares_*.json files that
`cargo +nightly doc` writes with
`RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items"`.
Test modules are not compiled into rustdoc output, so every hit is
production code.

Three lists, each a candidate set for a human to merge, delete or keep:

  same-signature  functions with identical (parameter types, return type)
                  and a name that differs only by a prefix such as
                  parse_/to_/from_/as_/make_/build_/new_/get_ or by crate
  same-name       one function name defined in two or more crates
  same-fields     structs with the same field names and types

Prints a text report; --json prints the same as one object.
"""
import glob
import json
import os
import re
import sys
from collections import defaultdict

PRIMITIVE = re.compile(r"^[&(\[\]*mut ,)]*(str|String|Value|bool|u\d+|i\d+|usize|f\d+|IpAddr|\(\))[&(\[\]*, )]*$")
PREFIX = re.compile(r"^(parse|to|from|as|make|build|new|get|read|load|render|with|into|try)_")
SKIP_NAMES = {"new", "default", "fmt", "clone", "drop", "from", "into", "eq", "hash",
              "deserialize", "serialize", "try_from", "as_ref", "borrow", "next",
              "poll", "call", "layer", "visit", "schemes", "name"}


def ty(t):
    """Render a rustdoc Type tree to a stable string (ids stripped)."""
    if isinstance(t, dict):
        if "resolved_path" in t:
            p = t["resolved_path"]
            args = p.get("args") or {}
            inner = ""
            ab = args.get("angle_bracketed") if isinstance(args, dict) else None
            if ab and ab.get("args"):
                inner = "<" + ",".join(ty(a.get("type", a)) for a in ab["args"]) + ">"
            return p["path"].split("::")[-1] + inner
        if "primitive" in t:
            return t["primitive"]
        if "generic" in t:
            return t["generic"]
        if "borrowed_ref" in t:
            b = t["borrowed_ref"]
            return "&" + ("mut " if b.get("is_mutable") else "") + ty(b["type"])
        if "slice" in t:
            return "[" + ty(t["slice"]) + "]"
        if "array" in t:
            return "[" + ty(t["array"]["type"]) + ";" + str(t["array"]["len"]) + "]"
        if "tuple" in t:
            return "(" + ",".join(ty(x) for x in t["tuple"]) + ")"
        if "impl_trait" in t:
            return "impl " + "+".join(ty(b.get("trait_bound", {}).get("trait", b)) for b in t["impl_trait"])
        if "dyn_trait" in t:
            return "dyn " + "+".join(ty(b["trait"]) for b in t["dyn_trait"]["traits"])
        if "raw_pointer" in t:
            return "*" + ty(t["raw_pointer"]["type"])
        if "qualified_path" in t:
            q = t["qualified_path"]
            return ty(q["self_type"]) + "::" + q["name"]
        if "function_pointer" in t:
            return "fn"
        if "path" in t:  # trait ref inside bounds
            return t["path"].split("::")[-1]
        return json.dumps(t, sort_keys=True)
    if t is None:
        return "()"
    return str(t)


def load(doc_dir):
    fns, structs = [], []
    for f in sorted(glob.glob(os.path.join(doc_dir, "antares_*.json"))):
        crate = os.path.basename(f)[:-5]
        j = json.load(open(f))
        # methods of `impl Trait for T` blocks: the trait dictates the
        # signature, so equal signatures there are not duplicates
        trait_impl_items = set()
        for it in j["index"].values():
            imp = it["inner"].get("impl")
            if imp and imp.get("trait"):
                trait_impl_items.update(str(i) for i in imp["items"])
        for id_, it in j["index"].items():
            if id_ in trait_impl_items:
                continue
            if it.get("crate_id", 0) != 0 or not it.get("name"):
                continue
            span = it.get("span") or {}
            loc = f'{span.get("filename", "?")}:{span.get("begin", ["?"])[0]}'
            inner = it["inner"]
            if "function" in inner:
                sig = inner["function"]["sig"]
                params = tuple(ty(p[1]) for p in sig["inputs"] if p[0] != "self")
                has_self = any(p[0] == "self" for p in sig["inputs"])
                fns.append(dict(crate=crate, name=it["name"], loc=loc, params=params,
                                ret=ty(sig.get("output")), has_self=has_self,
                                nloc=(span.get("end", [0])[0] or 0) - (span.get("begin", [0])[0] or 0) + 1))
            elif "struct" in inner:
                kind = inner["struct"]["kind"]
                if isinstance(kind, dict) and "plain" in kind:
                    ids = kind["plain"]["fields"]
                    fields = tuple(sorted(
                        (j["index"][str(i)]["name"], ty(j["index"][str(i)]["inner"]["struct_field"]))
                        for i in ids if str(i) in j["index"]))
                    if len(fields) >= 2:
                        structs.append(dict(crate=crate, name=it["name"], loc=loc, fields=fields))
    return fns, structs


def report(doc_dir):
    fns, structs = load(doc_dir)
    out = {"same_signature": [], "same_name": [], "same_fields": []}

    by_sig = defaultdict(list)
    for f in fns:
        if f["name"] in SKIP_NAMES or not f["params"] or f["nloc"] < 8:
            continue
        by_sig[(f["params"], f["ret"], f["has_self"])].append(f)
    for (params, ret, _), group in by_sig.items():
        stems = {PREFIX.sub("", g["name"]) for g in group}
        crates = {g["crate"] for g in group}
        # a duplicate candidate: two functions share a name stem under different
        # prefixes, or the same signature is implemented in more than one crate
        generic = all(PRIMITIVE.match(p) for p in params) and PRIMITIVE.match(ret)
        if len(group) >= 2 and (len(stems) < len(group) or (len(crates) > 1 and not generic)):
            out["same_signature"].append(dict(
                signature=f'({", ".join(params)}) -> {ret}',
                functions=[f'{g["crate"]}::{g["name"]} {g["loc"]} ({g["nloc"]} lines)' for g in group]))

    by_name = defaultdict(list)
    for f in fns:
        if f["name"] in SKIP_NAMES or f["has_self"] or f["nloc"] < 8:
            continue
        by_name[f["name"]].append(f)
    for name, group in by_name.items():
        crates = {g["crate"] for g in group}
        if len(crates) > 1:
            out["same_name"].append(dict(
                name=name, functions=[f'{g["crate"]} {g["loc"]} ({g["nloc"]} lines)' for g in group]))

    by_fields = defaultdict(list)
    for s in structs:
        by_fields[s["fields"]].append(s)
    for fields, group in by_fields.items():
        if len(group) > 1:
            out["same_fields"].append(dict(
                fields=[f"{n}: {t}" for n, t in fields],
                structs=[f'{g["crate"]}::{g["name"]} {g["loc"]}' for g in group]))

    for k in out:
        out[k].sort(key=lambda e: json.dumps(e))
    out["counts"] = {k: len(v) for k, v in out.items()}
    out["counts"]["functions_scanned"] = len(fns)
    return out


def main():
    doc_dir = sys.argv[1]
    out = report(doc_dir)
    if "--json" in sys.argv:
        print(json.dumps(out, indent=1))
        return
    print(f'functions scanned: {out["counts"]["functions_scanned"]}')
    for key, label, field in (("same_signature", "same signature, prefix-only name difference", "functions"),
                              ("same_name", "same name in more than one crate", "functions"),
                              ("same_fields", "structs with identical fields", "structs")):
        print(f'\n== {label}: {out["counts"][key]} ==')
        for e in out[key]:
            head = e.get("signature") or e.get("name") or ", ".join(e["fields"])
            print(f"  {head}")
            for x in e[field]:
                print(f"      {x}")


if __name__ == "__main__":
    main()
