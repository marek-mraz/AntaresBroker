import json, glob, re, collections, os
D="/home/dev/cargo-target/doc"
CLAUSE=re.compile(r'\b(?:[4-7]\.\d+(?:\.\d+)*|[A-C]\.\d+(?:\.\d+)*|RFC\s?\d+|Table\s+\d)')
BANNED=re.compile(r'deep-analysis|tasks\.md|error\.md|claude\.md|CLAUDE\.md|audit|§|\b[CBNPSXY]\d{1,2}[a-z]?\b')
graph={}; typeuse=collections.Counter()
tot=collections.Counter()
for f in sorted(glob.glob(f"{D}/antares_*.json")):
    j=json.load(open(f)); crate=os.path.basename(f)[:-5]
    idx=j["index"]; paths=j["paths"]; ext=j["external_crates"]
    deps={v["name"] for v in ext.values() if v["name"].startswith("antares")}
    graph[crate]=deps
    # external antares items referenced (via paths with crate_id != 0)
    used=collections.Counter()
    for pid,p in paths.items():
        c=ext.get(str(p["crate_id"]),{}).get("name")
        if c and c.startswith("antares"): used[(c,"::".join(p["path"]))]+=1
    # count references in the index
    s=json.dumps(idx)
    refs=collections.Counter()
    for (c,path) in used:
        refs[(c,path)]=s.count(f'"{path.split("::")[-1]}"')
    print(f"\n=== {crate}  deps={sorted(deps)}")
    print("  top external antares items:", [f"{c[0]}::{c[1].split('::')[-1]}({n})" for c,n in refs.most_common(8)])
    kinds=collections.Counter(); undoc=[]; nocl=[]; banned=[]
    for id_,it in idx.items():
        if it.get("crate_id",0)!=0: continue
        k=list(it["inner"].keys())[0]; kinds[k]+=1
        vis=it["visibility"]; name=it.get("name"); doc=it.get("docs") or ""
        span=it.get("span") or {}; loc=f'{span.get("filename","?")}:{span.get("begin",["?"])[0]}'
        if k in("function","struct","enum","trait","module") and name and not name.startswith("__"):
            if vis=="public" and not doc.strip(): undoc.append((k,name,loc))
            if k=="function" and vis=="public" and doc and not CLAUSE.search(doc): nocl.append((name,loc,doc.split("\n")[0][:70]))
        if doc and BANNED.search(doc): banned.append((name,loc,BANNED.search(doc).group(0)))
    tot.update(kinds)
    print("  kinds:",dict(kinds.most_common(8)))
    print(f"  undocumented PUBLIC items: {len(undoc)}")
    for u in undoc[:25]: print("    ",*u)
    if len(undoc)>25: print("     ...",len(undoc)-25,"more")
    print(f"  public fns documented w/o any clause/RFC citation: {len(nocl)}")
    for u in nocl[:15]: print("    ",*u)
    if len(nocl)>15: print("     ...",len(nocl)-15,"more")
    print(f"  docs with banned internal refs (hygiene 0.5.1): {len(banned)}")
    for u in banned[:20]: print("    ",*u)
print("\n=== dependency graph")
for c,d in graph.items(): print(f"  {c} -> {sorted(d)}")
# cycles / layering
import itertools
for a,b in itertools.permutations(graph,2):
    if b in graph[a] and a in graph[b]: print("  CYCLE",a,b)
print("totals",dict(tot))
