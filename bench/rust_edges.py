"""Score devscout's Rust edges against rust-analyzer LSIF output. Standard library only.

    rust_edges.py run --repo R --commit C --devscout BIN --label L --scratch DIR [--out DIR]
    rust_edges.py score --out DIR --source EXPORT
    rust_edges.py confirm --out DIR --source EXPORT ROWS.json...

`run` exports the commit with `git archive` into a scratch directory outside every git work
tree, runs `rust-analyzer lsif` and `cargo test -- --list` there offline, maps the export with
the given devscout build, saves every output under `--out`, then scores. `score` works from the
saved files only. `confirm` checks each row of a saved `refs --json` or `tests --json` answer
against the LSIF references.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from collections import Counter, defaultdict
from urllib.parse import unquote, urlparse

import rust_sites

THRESHOLDS = {"call_precision": 0.98, "call_recall": 0.85,
              "test_precision": 0.98, "test_recall": 0.85}
CALL_CONTEXTS = ("plain", "macro argument")
RUST_EDGE_KINDS = ("uses-member", "uses-type", "implements", "ambiguous")
_WANTED_LABELS = ('"document"', '"range"', '"next"', '"textDocument/definition"', '"item"',
                  '"contains"')


# ---------------------------------------------------------------------------
# LSIF
# ---------------------------------------------------------------------------

class Lsif:
    """The parts of an LSIF dump that tie a reference range to its definition ranges."""

    def __init__(self):
        self.docs = {}
        self.ranges = {}
        self.range_doc = {}
        self.next = {}
        self.definition = {}
        self.items = defaultdict(list)

    def definitions(self, range_id):
        """Definition range ids of a range, following `next` to its result set."""
        seen = set()
        node = range_id
        while node not in seen:
            seen.add(node)
            result = self.definition.get(node)
            if result is not None:
                return self.items.get(result, [])
            node = self.next.get(node)
            if node is None:
                break
        return []


def _ids(edge):
    if "inVs" in edge:
        return [str(v) for v in edge["inVs"]]
    return [str(edge["inV"])] if "inV" in edge else []


def parse_lsif(lines):
    """Parses LSIF JSON lines; tolerates `document` or `shard`, `inV` or `inVs`, any id type."""
    out = Lsif()
    for raw in lines:
        if not any(label in raw for label in _WANTED_LABELS):
            continue
        obj = json.loads(raw)
        oid = str(obj.get("id"))
        label = obj.get("label")
        if obj.get("type") == "vertex":
            if label == "document":
                out.docs[oid] = obj.get("uri", "")
            elif label == "range":
                s, e = obj["start"], obj["end"]
                out.ranges[oid] = (s["line"], s["character"], e["line"], e["character"])
            continue
        outv = str(obj.get("outV"))
        targets = _ids(obj)
        if label == "contains":
            for v in targets:
                out.range_doc.setdefault(v, outv)
        elif label == "next" and targets:
            out.next[outv] = targets[0]
        elif label == "textDocument/definition" and targets:
            out.definition[outv] = targets[0]
        elif label == "item":
            doc = obj.get("document", obj.get("shard"))
            out.items[outv].extend(targets)
            if doc is not None:
                for v in targets:
                    out.range_doc.setdefault(v, str(doc))
    return out


def uri_to_rel(uri, roots):
    """Repository-relative path of a `file://` URI under one of `roots`, else None."""
    path = unquote(urlparse(uri).path) if uri.startswith("file:") else uri
    for root in roots:
        prefix = root.rstrip("/") + "/"
        if path.startswith(prefix):
            return path[len(prefix):]
    return None


# ---------------------------------------------------------------------------
# The join: oracle references against devscout defs
# ---------------------------------------------------------------------------

class Sources:
    """Lazily lexed export files."""

    def __init__(self, root):
        self.root = root
        self.cache = {}

    def index(self, rel):
        if rel not in self.cache:
            try:
                with open(os.path.join(self.root, rel), encoding="utf-8") as fh:
                    self.cache[rel] = rust_sites.Index(fh.read())
            except (OSError, UnicodeDecodeError):
                self.cache[rel] = None
        return self.cache[rel]

    def site(self, rel, line, col):
        idx = self.index(rel)
        return idx.site(line, col) if idx else None


def def_keys(graph):
    """`(file, name, name line) -> def id` for every recorded Rust item, and def kinds."""
    spans = defaultdict(list)
    kinds = {}
    for d in graph.get("defs", []):
        if d["file"].endswith(".rs"):
            spans[(d["file"], d["name"])].append(d)
            kinds[d["id"]] = d["kind"]
    keys = {}
    for n in graph.get("names", []):
        if not n["file"].endswith(".rs"):
            continue
        for d in spans.get((n["file"], n["name"]), []):
            if d["line"] <= n["line"] <= d.get("endLine", d["line"]):
                keys[(n["file"], n["name"], n["line"])] = d["id"]
                break
    return keys, kinds


def _target_class(kind):
    if kind == "fn":
        return "recorded fn"
    if kind in ("struct", "enum", "union", "trait", "type"):
        return "recorded type"
    return "recorded other"


def oracle_references(lsif, roots, sources, keys, kinds):
    """Every LSIF reference range in the universe, with its site class and mapped target."""
    doc_rel = {d: uri_to_rel(u, roots) for d, u in lsif.docs.items()}
    universe = sorted({r for r in doc_rel.values() if r and r.endswith(".rs")})
    refs, definitions = [], []
    for rid, (sl, sc, _, _) in lsif.ranges.items():
        rel = doc_rel.get(lsif.range_doc.get(rid))
        if not rel or not rel.endswith(".rs"):
            continue
        defs = lsif.definitions(rid)
        site = sources.site(rel, sl, sc)
        if rid in defs:
            definitions.append((rel, sl, sc, site))
            continue
        if not defs:
            continue
        targets, external = [], False
        for d in defs:
            drel = doc_rel.get(lsif.range_doc.get(d))
            if drel is None or d not in lsif.ranges:
                external = True
                continue
            dl, dc, _, _ = lsif.ranges[d]
            dsite = sources.site(drel, dl, dc)
            target = keys.get((drel, dsite[0], dl + 1)) if dsite else None
            if target and target not in targets:
                targets.append(target)
        if targets:
            status = _target_class(kinds.get(targets[0], ""))
        else:
            status = "external" if external else "not a recorded item"
        name, context, shape = site if site else ("", "unlexed", "unlexed")
        refs.append({"file": rel, "line": sl + 1, "col": sc, "name": name, "context": context,
                     "shape": shape, "targets": sorted(targets), "status": status})
    refs.sort(key=lambda r: (r["file"], r["line"], r["col"]))
    return refs, definitions, universe


def in_population(ref):
    return (ref["shape"] == "path call" and ref["context"] in CALL_CONTEXTS
            and ref["status"] == "recorded fn")


def definition_mapping(definitions, keys, sources):
    """Share of item-shaped LSIF definitions that map to a devscout def, with the misses."""
    item_words = {"fn", "struct", "enum", "union", "trait", "mod", "const", "static", "type"}
    mapped, missed = 0, []
    for rel, line, col, site in definitions:
        idx = sources.index(rel)
        if not idx or not site:
            continue
        k = idx.by_pos.get((line, col))
        if k is None:
            continue
        prev = idx.tokens[k - 1].text if k else ""
        before = idx.tokens[k - 2].text if k and k > 1 else ""
        if prev not in item_words and not (prev == "!" and before == "macro_rules"):
            continue
        if keys.get((rel, site[0], line + 1)):
            mapped += 1
        else:
            missed.append({"file": rel, "line": line + 1, "name": site[0], "keyword": prev,
                           "context": site[1]})
    return mapped, missed


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def rust_edges(graph, universe):
    keep = set(universe)
    return [e for e in graph.get("edges", [])
            if e.get("kind") in RUST_EDGE_KINDS and e.get("from_file") in keep]


def _edge_name(edge, graph_defs):
    if edge["kind"] == "uses-member" and edge.get("member"):
        return edge["member"]
    d = graph_defs.get(edge.get("to"))
    return d["name"] if d else ""


def verdicts(edges, refs, graph_defs):
    """One verdict per edge: `match`, `wrong-target` or `unconfirmed`."""
    oracle = Counter()
    at_line = defaultdict(list)
    for r in refs:
        for t in r["targets"]:
            oracle[(r["file"], r["line"], t)] += 1
        at_line[(r["file"], r["line"])].append(r)
    used = Counter()
    out = []
    for e in edges:
        if e["kind"] == "ambiguous":
            name = e["raw"].split("::")[-1]
            cands = {c["id"] for c in e["candidates"]}
            hits = [r for r in at_line[(e["from_file"], e["from_line"])] if r["name"] == name]
            among = any(set(r["targets"]) & cands for r in hits)
            out.append(dict(e, verdict="ambiguous", oracleAmongCandidates=among))
            continue
        key = (e["from_file"], e["from_line"], e["to"])
        name = _edge_name(e, graph_defs)
        if used[key] < oracle[key]:
            used[key] += 1
            out.append(dict(e, verdict="match"))
            continue
        same = [r for r in at_line[(e["from_file"], e["from_line"])] if r["name"] == name]
        verdict = "wrong-target" if same else "unconfirmed"
        sites = sorted({(r["context"], r["shape"], r["status"]) for r in same})
        out.append(dict(e, verdict=verdict, oracleSites=[list(s) for s in sites],
                        oracleTargets=sorted({t for r in same for t in r["targets"]})))
    return out


def call_scores(refs, edges):
    population = Counter((r["file"], r["line"], r["targets"][0])
                         for r in refs if in_population(r))
    calls = Counter((e["from_file"], e["from_line"], e["to"])
                    for e in edges if e["kind"] == "uses-member")
    oracle = Counter()
    for r in refs:
        for t in r["targets"]:
            oracle[(r["file"], r["line"], t)] += 1
    recalled = sum(min(n, calls[k]) for k, n in population.items())
    matched = sum(min(n, oracle[k]) for k, n in calls.items())
    pop_n, edge_n = sum(population.values()), sum(calls.values())
    return {"population": pop_n, "edges": edge_n, "recalled": recalled, "matched": matched,
            "recall": ratio(recalled, pop_n), "precision": ratio(matched, edge_n)}


def ratio(num, den):
    return round(num / den, 4) if den else None


def oracle_tests(universe, sources, keys):
    tests = []
    for rel in universe:
        idx = sources.index(rel)
        if not idx:
            continue
        found, _ = rust_sites.scan_items(idx.source)
        for t in found:
            tid = keys.get((rel, t["name"], t["nameLine"]))
            tests.append(dict(t, file=rel, id=tid or "%s@%d" % (rel, t["nameLine"])))
    return tests


def devscout_tests(graph, universe):
    keep = set(universe)
    return [d for d in graph.get("defs", []) if d.get("testMethods") and d["file"] in keep]


def _within(spans, file, line):
    return [tid for (f, lo, hi, tid) in spans.get(file, []) if f == file and lo <= line <= hi]


def test_scores(refs, edges, o_tests, d_tests):
    o_spans, d_spans = defaultdict(list), defaultdict(list)
    for t in o_tests:
        o_spans[t["file"]].append((t["file"], t["startLine"], t["endLine"], t["id"]))
    for d in d_tests:
        d_spans[d["file"]].append((d["file"], d["line"], d.get("endLine", d["line"]), d["id"]))
    o_pairs = {(tid, r["targets"][0]) for r in refs if in_population(r)
               for tid in _within(o_spans, r["file"], r["line"])}
    d_pairs = {(tid, e["to"]) for e in edges if e["kind"] == "uses-member"
               for tid in _within(d_spans, e["from_file"], e["from_line"])}
    both = o_pairs & d_pairs
    o_ids = {t["id"] for t in o_tests}
    d_ids = {d["id"] for d in d_tests}
    return {"oraclePairs": len(o_pairs), "devscoutPairs": len(d_pairs), "matched": len(both),
            "recall": ratio(len(both), len(o_pairs)), "precision": ratio(len(both), len(d_pairs)),
            "oracleTests": len(o_ids), "devscoutTests": len(d_ids),
            "oracleOnlyTests": sorted(o_ids - d_ids), "devscoutOnlyTests": sorted(d_ids - o_ids),
            "missedPairs": sorted(map(list, o_pairs - d_pairs)),
            "extraPairs": sorted(map(list, d_pairs - o_pairs))}


# ---------------------------------------------------------------------------
# The compiler's test list against the script's own scan
# ---------------------------------------------------------------------------

_RUNNING = re.compile(r"^\s*Running (?:unittests )?(\S+)")
_LISTED = re.compile(r"^(\S+): test$")


def parse_test_list(text):
    """`cargo test -- --list` output (stdout and stderr together) -> target file -> names."""
    targets = defaultdict(set)
    current = None
    for line in text.splitlines():
        m = _RUNNING.match(line)
        if m:
            current = m.group(1)
            targets[current]
            continue
        m = _LISTED.match(line.strip())
        if m and current:
            targets[current].add(m.group(1))
    return targets


def module_paths(roots, mods_by_file, exists):
    """File -> (target root, module path) by `mod` declarations, first position wins."""
    placed = {}
    for root in roots:
        queue = [(root, [])]
        while queue:
            rel, path = queue.pop(0)
            if rel in placed:
                continue
            placed[rel] = (root, path)
            base = os.path.dirname(rel)
            stem = os.path.splitext(os.path.basename(rel))[0]
            owns_dir = rel == root or stem == "mod"
            for m in mods_by_file.get(rel, []):
                if m["inline"]:
                    continue
                parts = ([] if owns_dir else [stem]) + m["modPath"]
                d = os.path.join(base, *parts) if parts else base
                cands = [c for c in (os.path.join(d, m["name"] + ".rs"),
                                     os.path.join(d, m["name"], "mod.rs")) if exists(c)]
                if len(cands) == 1:
                    queue.append((cands[0], path + m["modPath"] + [m["name"]]))
    return placed


def compare_test_lists(listed, o_tests, mods_by_file, exists):
    roots = sorted(listed)
    placed = module_paths(roots, mods_by_file, exists)
    scanned = defaultdict(set)
    for t in o_tests:
        if t["file"] in placed:
            root, path = placed[t["file"]]
            scanned[root].add("::".join(path + t["modPath"] + [t["name"]]))
    out = {}
    for root in roots:
        c, s = listed[root], scanned[root]
        out[root] = {"compiler": len(c), "scan": len(s), "compilerOnly": sorted(c - s),
                     "scanOnly": sorted(s - c)}
    return out


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def score(out_dir, source):
    with open(os.path.join(out_dir, "graph.json"), encoding="utf-8") as fh:
        graph = json.load(fh)
    with open(os.path.join(out_dir, "dump.lsif"), encoding="utf-8") as fh:
        lsif = parse_lsif(fh)
    run = _load_json(os.path.join(out_dir, "run.json")) or {}
    roots = sorted({source, os.path.realpath(source)} | set(run.get("exportRoots", [])))
    sources = Sources(source)
    keys, kinds = def_keys(graph)
    refs, definitions, universe = oracle_references(lsif, roots, sources, keys, kinds)
    edges = rust_edges(graph, universe)
    graph_defs = {d["id"]: d for d in graph.get("defs", [])}
    judged = verdicts(edges, refs, graph_defs)
    o_tests = oracle_tests(universe, sources, keys)
    d_tests = devscout_tests(graph, universe)
    mapped, missed = definition_mapping(definitions, keys, sources)
    mods = {rel: rust_sites.scan_items(sources.index(rel).source)[1]
            for rel in universe if sources.index(rel)}
    listed_path = os.path.join(out_dir, "test-list.txt")
    listed = parse_test_list(open(listed_path, encoding="utf-8").read()) \
        if os.path.exists(listed_path) else {}
    classes = Counter((r["context"], r["shape"], r["status"]) for r in refs)
    report = {
        "label": run.get("label"), "commit": run.get("commit"),
        "rustAnalyzer": run.get("rustAnalyzer"), "universeFiles": len(universe),
        "oracleReferences": len(refs),
        "sites": [{"context": c, "shape": s, "target": t, "count": n}
                  for (c, s, t), n in sorted(classes.items())],
        "macroArgumentPathCalls": sum(1 for r in refs if in_population(r)
                                      and r["context"] == "macro argument"),
        "definitions": {"itemShaped": mapped + len(missed), "mapped": mapped,
                        "unmapped": missed},
        "calls": call_scores(refs, edges),
        "tests": test_scores(refs, edges, o_tests, d_tests),
        "compilerTests": compare_test_lists(listed, o_tests, mods,
                                            lambda p: os.path.isfile(os.path.join(source, p))),
        "edgesByKind": dict(Counter(e["kind"] for e in edges)),
        "verdicts": dict(Counter((j["kind"] + " " + j["verdict"]) for j in judged)),
        "ambiguousWithOracleTargetAmongCandidates":
            sum(1 for j in judged if j.get("oracleAmongCandidates")),
        "contradicted": [j for j in judged if j["verdict"] in ("wrong-target", "unconfirmed")],
    }
    report["thresholds"] = check_thresholds(report)
    _write_json(os.path.join(out_dir, "report.json"), report)
    with open(os.path.join(out_dir, "edges.jsonl"), "w", encoding="utf-8") as fh:
        for j in judged:
            fh.write(json.dumps(j, sort_keys=True) + "\n")
    with open(os.path.join(out_dir, "references.jsonl"), "w", encoding="utf-8") as fh:
        for r in refs:
            fh.write(json.dumps(r, sort_keys=True) + "\n")
    return report


def check_thresholds(report):
    got = {"call_precision": report["calls"]["precision"],
           "call_recall": report["calls"]["recall"],
           "test_precision": report["tests"]["precision"],
           "test_recall": report["tests"]["recall"]}
    return {k: {"value": v, "min": THRESHOLDS[k], "met": v is not None and v >= THRESHOLDS[k]}
            for k, v in got.items()}


def summary_lines(report):
    c, t = report["calls"], report["tests"]
    lines = [
        "label %s commit %s" % (report["label"], report["commit"]),
        "rust-analyzer %s" % report["rustAnalyzer"],
        "universe files %d, oracle references %d" % (report["universeFiles"],
                                                    report["oracleReferences"]),
        "definitions: %d item-shaped, %d mapped" % (report["definitions"]["itemShaped"],
                                                   report["definitions"]["mapped"]),
        "call population %d (macro argument %d); edges %d; recall %s; precision %s" % (
            c["population"], report["macroArgumentPathCalls"], c["edges"], c["recall"],
            c["precision"]),
        "test pairs oracle %d devscout %d matched %d; recall %s; precision %s" % (
            t["oraclePairs"], t["devscoutPairs"], t["matched"], t["recall"], t["precision"]),
        "tests oracle %d devscout %d" % (t["oracleTests"], t["devscoutTests"]),
    ]
    for k, v in report["thresholds"].items():
        lines.append("threshold %s %s >= %s: %s" % (k, v["value"], v["min"],
                                                     "met" if v["met"] else "missed"))
    return lines


# ---------------------------------------------------------------------------
# confirm
# ---------------------------------------------------------------------------

def confirm_rows(answer, refs, o_tests):
    """Rows of a `refs --json` or `tests --json` answer, each with `confirmed`."""
    at = defaultdict(set)
    for r in refs:
        for t in r["targets"]:
            at[(r["file"], r["line"])].add(t)
    test_files = {t["file"] for t in o_tests}
    rows = []
    if "symbol" in answer:
        target = answer["symbol"]
        for row in answer.get("rows", []):
            ok = row["file"] in test_files and all(
                target in at[(row["file"], line)] for line in row["lines"])
            rows.append({"table": "tests", "file": row["file"], "lines": row["lines"],
                         "confirmed": ok})
        return rows
    target = answer.get("id")
    for table, content in answer.get("inbound", {}).items():
        for row in content.get("rows", []):
            rows.append({"table": table, "file": row["file"], "line": row["line"],
                         "confirmed": target in at[(row["file"], row["line"])]})
    return rows


# ---------------------------------------------------------------------------
# run
# ---------------------------------------------------------------------------

def inside_git_tree(path):
    path = os.path.realpath(path)
    while True:
        if os.path.exists(os.path.join(path, ".git")):
            return True
        parent = os.path.dirname(path)
        if parent == path:
            return False
        path = parent


def _run(cmd, cwd, env, stdout_path, stderr_path=None):
    start = time.time()
    with open(stdout_path, "w", encoding="utf-8") as out:
        err = open(stderr_path, "w", encoding="utf-8") if stderr_path else subprocess.STDOUT
        try:
            code = subprocess.call(cmd, cwd=cwd, env=env, stdout=out, stderr=err)
        finally:
            if stderr_path:
                err.close()
    return {"cmd": cmd, "exit": code, "seconds": round(time.time() - start, 1)}


def run(args):
    scratch = os.path.realpath(args.scratch)
    if inside_git_tree(scratch):
        sys.exit("refusing %s: it is inside a git work tree" % scratch)
    export = os.path.join(scratch, "export")
    if os.path.exists(export):
        sys.exit("refusing %s: the export directory already exists" % export)
    out_dir = args.out or os.path.join(os.path.dirname(os.path.abspath(__file__)), "out",
                                       "rust-edges", args.label)
    os.makedirs(out_dir, exist_ok=True)
    os.makedirs(export)
    archive = subprocess.run(["git", "-C", args.repo, "archive", args.commit],
                             check=True, capture_output=True).stdout
    subprocess.run(["tar", "-x", "-C", export], input=archive, check=True)
    home = os.path.join(scratch, "home")
    os.makedirs(home, exist_ok=True)
    env = dict(os.environ, CARGO_NET_OFFLINE="true",
               CARGO_TARGET_DIR=os.path.join(scratch, "target"), HOME=home,
               SCOUT_REGISTRY=os.path.join(home, "repos.json"),
               SCOUT_CONTENT_DB=os.path.join(home, "content.db"))
    ra = args.rust_analyzer
    version = subprocess.run([ra, "--version"], capture_output=True, text=True,
                             env=env).stdout.strip()
    steps = {
        "lsif": _run([ra, "lsif", export], export, env, os.path.join(out_dir, "dump.lsif"),
                     os.path.join(out_dir, "rust-analyzer.log")),
        "testList": _run(["cargo", "test", "--offline", "--all-targets", "--", "--list"],
                         export, env, os.path.join(out_dir, "test-list.txt")),
        "map": _run([os.path.abspath(args.devscout), "init", "--no-hooks"], export, env,
                    os.path.join(out_dir, "map.log")),
    }
    shutil.copy(os.path.join(export, ".scout", "graph", "graph.json"),
                os.path.join(out_dir, "graph.json"))
    _write_json(os.path.join(out_dir, "run.json"), {
        "label": args.label, "commit": args.commit, "rustAnalyzer": version,
        "export": export, "exportRoots": [export, os.path.realpath(export)], "steps": steps})
    failed = [k for k, v in steps.items() if v["exit"] != 0]
    if failed:
        sys.exit("step failed: %s" % ", ".join(failed))
    return score(out_dir, export)


def _load_json(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except OSError:
        return None


def _write_json(path, value):
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(value, fh, indent=1, sort_keys=True)
        fh.write("\n")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--repo", required=True)
    r.add_argument("--commit", required=True)
    r.add_argument("--devscout", required=True)
    r.add_argument("--label", required=True)
    r.add_argument("--scratch", required=True)
    r.add_argument("--out")
    r.add_argument("--rust-analyzer", default="rust-analyzer")
    s = sub.add_parser("score")
    s.add_argument("--out", required=True)
    s.add_argument("--source", required=True)
    c = sub.add_parser("confirm")
    c.add_argument("--out", required=True)
    c.add_argument("--source", required=True)
    c.add_argument("rows", nargs="+")
    args = p.parse_args(argv)
    if args.cmd == "run":
        report = run(args)
    elif args.cmd == "score":
        report = score(args.out, args.source)
    else:
        return confirm(args)
    print("\n".join(summary_lines(report)))
    return 0


def confirm(args):
    with open(os.path.join(args.out, "references.jsonl"), encoding="utf-8") as fh:
        refs = [json.loads(line) for line in fh]
    with open(os.path.join(args.out, "graph.json"), encoding="utf-8") as fh:
        graph = json.load(fh)
    keys, _ = def_keys(graph)
    sources = Sources(args.source)
    universe = sorted({r["file"] for r in refs})
    o_tests = oracle_tests(universe, sources, keys)
    status = 0
    for path in args.rows:
        with open(path, encoding="utf-8") as fh:
            rows = confirm_rows(json.load(fh), refs, o_tests)
        bad = [r for r in rows if not r["confirmed"]]
        print("%s: %d rows, %d confirmed" % (path, len(rows), len(rows) - len(bad)))
        for r in bad:
            print("  unconfirmed %s" % json.dumps(r, sort_keys=True))
        status = status or (1 if bad or not rows else 0)
    return status


if __name__ == "__main__":
    sys.exit(main())
