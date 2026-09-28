"""Reproducible Rust resolver cost and byte-identity workloads.

Generate once at the harness revision, then run unchanged exports at each
revision. The JSONL output includes failed and timed-out attempts.
"""

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


SIZES = (32, 64, 128)
TOPOLOGIES = ("chain", "wide", "diamond", "cycle")
OUTCOMES = ("found", "absent", "ambiguous", "opaque")
NAMESPACES = ("type", "value")
LOCATIONS = ("module", "block")
SHAPES = ("repeat", "distinct")
REPETITIONS = 3
TIMEOUT = 180
RSS_LIMIT = 2 * 1024 ** 3
ROW_RE = re.compile(r"^COST_ROW=(\{.*\})$", re.MULTILINE)
REPO = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rows():
    for topology in TOPOLOGIES:
        for size in SIZES:
            for outcome in OUTCOMES:
                for namespace in NAMESPACES:
                    for location in LOCATIONS:
                        for shape in SHAPES:
                            yield {"family": "glob", "topology": topology, "size": size,
                                   "queries": size, "outcome": outcome, "namespace": namespace,
                                   "location": location, "shape": shape}
    for shape in ("nested", "wide"):
        for size in SIZES:
            yield {"family": "item", "shape": shape, "size": size,
                   "source": f"item/{shape}-{size}.rs"}
    for size in SIZES:
        for namespace in NAMESPACES:
            for location in LOCATIONS:
                yield {"family": "glob", "topology": "chain", "size": size,
                       "queries": size, "outcome": "found", "namespace": namespace,
                       "location": location, "shape": "mutate"}


def row_id(row):
    if row["family"] == "item":
        return f"item:{row['shape']}:{row['size']}"
    return ":".join(str(row[k]) for k in ("family", "topology", "size", "outcome",
                                         "namespace", "location", "shape"))


def item_source(shape, size):
    if shape == "wide":
        return "".join(f"pub struct Item{i};\n" for i in range(size))
    lines = []
    for i in range(size):
        lines.append(f"pub mod m{i} {{\n")
        lines.append(f"pub struct Item{i};\n")
    lines.append("pub trait DeepTrait { fn run(&self); }\n")
    lines.append("pub struct DeepOwner;\n")
    lines.append("impl DeepOwner { pub fn act(&self) {} }\n")
    lines.append('extern "C" { fn foreign_count() -> i32; }\n')
    lines.extend("}\n" for _ in range(size))
    return "".join(lines)


def glob_source(topology, size):
    edges = []
    if topology == "chain":
        edges = [(i, i + 1) for i in range(size)]
    elif topology == "wide":
        edges = [(0, i) for i in range(1, size + 1)]
    elif topology == "diamond":
        edges = [(0, 1), (0, 2)]
        edges += [(i, i + 1) for i in range(1, size)]
        edges += [(i, i + 2) for i in range(1, size - 1)]
    else:
        edges = [(0, 1)] + [(i, i + 1) for i in range(1, size)] + [(size, 1)]
    children = {i: [] for i in range(size + 1)}
    for source, target in edges:
        children[source].append(target)
    lines = ["pub fn outer() { use crate::m0::*; hit(); let _: Hit = Hit;\n",
             "    { use crate::m0::*; hit(); let _: Hit = Hit; missing(); }\n}\n"]
    for i in range(size + 1):
        lines.append(f"pub mod m{i} {{\n")
        for target in children[i]:
            lines.append(f"pub use crate::m{target}::*;\n")
        if i == size:
            lines.extend(("pub fn hit() {}\n", "pub struct Hit;\n"))
        lines.append("}\n")
    return "".join(lines)


def inventory(root):
    return {str(path.relative_to(root)): {"sha256": digest(path), "bytes": path.stat().st_size}
            for path in sorted(root.rglob("*")) if path.is_file()}


def generate(out):
    out = Path(out).resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"export already exists: {out}")
    out.mkdir(parents=True, exist_ok=True)
    for shape in ("nested", "wide"):
        for size in SIZES:
            path = out / "item" / f"{shape}-{size}.rs"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(item_source(shape, size), encoding="utf-8")
    for topology in TOPOLOGIES:
        for size in SIZES:
            path = out / "corpus" / f"glob-{topology}-{size}" / "src" / "lib.rs"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(glob_source(topology, size), encoding="utf-8")
    for shape in ("nested", "wide"):
        for size in SIZES:
            root = out / "corpus" / f"item-{shape}-{size}"
            root.joinpath("src").mkdir(parents=True, exist_ok=True)
            shutil.copy2(out / "item" / f"{shape}-{size}.rs", root / "src" / "lib.rs")
    shutil.copytree(REPO / "fixtures", out / "corpus" / "fixtures")
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=REPO).split(b"\0")
    source_root = out / "corpus" / "source-snapshot"
    for raw in tracked:
        if not raw:
            continue
        rel = Path(os.fsdecode(raw))
        if rel.suffix not in (".rs", ".cs", ".ts", ".tsx") and rel.name != "Cargo.toml":
            continue
        if rel.parts[0] in ("fixtures", "bench"):
            continue
        target = source_root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO / rel, target)
    source_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO,
                                          text=True).strip()
    manifest = {"schema": 1, "source_revision": source_head, "rows": list(rows()),
                "files": inventory(out)}
    (out / "manifest.json").write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"export": str(out), "rows": len(manifest["rows"]),
                      "files": len(manifest["files"])}))


def read_export(root):
    root = Path(root).resolve()
    manifest = json.loads((root / "manifest.json").read_text())
    expected_rows = list(rows())
    if manifest["rows"] != expected_rows:
        raise ValueError("workload matrix differs from the registered protocol")
    actual = inventory(root)
    actual.pop("manifest.json", None)
    if actual != manifest["files"]:
        raise ValueError("frozen source inventory changed")
    return root, manifest


def command(argv, *, cwd=REPO, env=None, timeout=TIMEOUT):
    started = time.monotonic()
    try:
        result = subprocess.run(argv, cwd=cwd, env=env, text=True, capture_output=True,
                                timeout=timeout, check=False)
        status = "ok" if result.returncode == 0 else "failed"
        return {"status": status, "exit": result.returncode,
                "seconds": time.monotonic() - started, "stdout": result.stdout,
                "stderr": result.stderr}
    except subprocess.TimeoutExpired as error:
        return {"status": "timeout", "exit": None, "seconds": time.monotonic() - started,
                "stdout": (error.stdout or b"").decode(errors="replace") if isinstance(error.stdout, bytes) else error.stdout or "",
                "stderr": (error.stderr or b"").decode(errors="replace") if isinstance(error.stderr, bytes) else error.stderr or ""}


def metered_command(argv, *, cwd=REPO, env=None):
    if platform.system() != "Darwin":
        raise RuntimeError("per-process RSS meter is registered for macOS only")
    result = command(["/usr/bin/time", "-l", *argv], cwd=cwd, env=env)
    match = re.search(r"^\s*(\d+)\s+maximum resident set size$", result["stderr"], re.MULTILINE)
    result["peak_rss_bytes"] = int(match.group(1)) if match else None
    if result["status"] == "ok" and match is None:
        result["status"] = "missing-memory"
    if result["peak_rss_bytes"] is not None and result["peak_rss_bytes"] > RSS_LIMIT:
        result["status"] = "over-memory"
    return result


def test_binary():
    result = command(["cargo", "test", "--lib", "--locked", "--no-run", "--message-format=json"],
                     timeout=TIMEOUT)
    if result["status"] != "ok":
        raise RuntimeError(result["stderr"])
    binaries = []
    for line in result["stdout"].splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if entry.get("reason") == "compiler-artifact" and entry.get("profile", {}).get("test"):
            path = entry.get("executable")
            if path and "lib" in entry.get("target", {}).get("kind", []):
                binaries.append(path)
    if len(binaries) != 1:
        raise RuntimeError(f"expected one library test binary, found {binaries}")
    return Path(binaries[0])


def observe(test_bin, spec, inputs):
    env = os.environ.copy()
    if spec["family"] == "glob":
        env["RESOLVER_COST_GLOB"] = ",".join(str(spec[k]) for k in
            ("topology", "size", "queries", "outcome", "namespace", "location", "shape"))
        name = "rsgraph::edge_tests::cost::generated_glob_cost_row"
    else:
        env["RESOLVER_COST_ITEM"] = str(inputs / spec["source"])
        name = "extract::tests::rs_fragment_cost::generated_item_cost_row"
    result = metered_command([str(test_bin), name, "--exact", "--nocapture"], env=env)
    matches = ROW_RE.findall(result["stdout"])
    if result["status"] == "ok" and len(matches) != 1:
        result["status"] = "missing-counter"
    result["cost"] = json.loads(matches[0]) if len(matches) == 1 else None
    result["row_id"] = row_id(spec)
    result["stdout"] = result["stdout"][-2048:]
    result["stderr"] = result["stderr"][-2048:]
    return result


def bound(spec, cost):
    if cost is None:
        return False
    if spec["family"] == "item":
        return cost["awork"] <= 2 * cost["items"]
    q = spec["queries"]
    return (cost["vwork"] <= 2 * cost["vref"] + 2 * q
            and cost["ework"] <= 2 * cost["eref"] + 4 * q
            and cost["copied_entries"] <= 2 * (cost["probes"] + cost["candidates"] + q))


def complete(specs, observations):
    expected = {row_id(spec) for spec in specs}
    counts = {key: 0 for key in expected}
    for observation in observations:
        key = observation["row_id"]
        if key not in counts:
            return False
        counts[key] += 1
    return all(count == REPETITIONS for count in counts.values())


def artifact_inventory(root, destination):
    paths = [root / ".scout" / "manifest.json"]
    graph = root / ".scout" / "graph"
    if graph.exists():
        paths.extend(path for path in graph.rglob("*") if path.is_file())
    required = {"manifest.json", "graph/graph.json"}
    found = {}
    for path in sorted(paths):
        if not path.is_file():
            continue
        rel = str(path.relative_to(root / ".scout"))
        target = destination / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        found[rel] = {"sha256": digest(path), "bytes": path.stat().st_size}
    return {"files": found, "complete": required.issubset(found)}


def map_scratch_path(inputs):
    scratch = Path(tempfile.gettempdir()).resolve() / (
        "devscout-rust-cost-" + digest(inputs / "manifest.json")[:16])
    for parent in (scratch, *scratch.parents):
        if (parent / ".git").exists() or (parent / ".scout").exists():
            raise ValueError(f"map scratch has a repository ancestor: {parent}")
    return scratch


@contextmanager
def map_scratch_lock(inputs):
    lock_path = Path(str(map_scratch_path(inputs)) + ".lock")
    with lock_path.open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise BlockingIOError(f"map scratch already in use: {lock_path}") from error
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def map_corpus(cli, inputs, out, corpus):
    source = inputs / "corpus" / corpus
    scratch = map_scratch_path(inputs)
    observations = []
    for repetition in range(REPETITIONS):
        if scratch.exists():
            if scratch.is_symlink() or not (scratch / ".resolver-cost-owned").exists():
                raise ValueError(f"refusing to replace unowned scratch: {scratch}")
            shutil.rmtree(scratch)
        scratch.mkdir()
        (scratch / ".resolver-cost-owned").write_text("generated benchmark scratch\n")
        started = time.monotonic()
        root = scratch / "repo"
        shutil.copytree(source, root)
        home = scratch / "home"
        home.mkdir()
        copy_seconds = time.monotonic() - started
        env = os.environ.copy()
        env.update({"HOME": str(home), "SCOUT_REGISTRY": str(home / "repos.json"),
                    "SCOUT_CONTENT_DB": str(home / "content.db")})
        init = metered_command([str(cli), "init", "--no-hooks"], cwd=root, env=env)
        for mode in ("cold", "warm"):
            result = (metered_command([str(cli), "map", "."], cwd=root, env=env)
                      if init["status"] == "ok" else
                      {"status": "skipped-after-init-failure", "exit": None, "seconds": 0,
                       "peak_rss_bytes": None, "stdout": "", "stderr": init["stderr"]})
            destination = out / "artifacts" / corpus / str(repetition) / mode
            artifact = artifact_inventory(root, destination) if result["status"] == "ok" else \
                {"files": {}, "complete": False}
            result.update({"corpus": corpus, "repetition": repetition, "mode": mode,
                           "copy_seconds": copy_seconds, "init_status": init["status"],
                           "artifact": artifact})
            result["stdout"] = result["stdout"][-2048:]
            result["stderr"] = result["stderr"][-2048:]
            observations.append(result)
    return observations


def run(revision, inputs, out):
    inputs, manifest = read_export(inputs)
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    if not head.startswith(revision) or len(revision) < 12:
        raise ValueError(f"worktree HEAD {head} does not match requested revision {revision}")
    test_bin = test_binary()
    build = command(["cargo", "build", "--locked", "--bin", "devscout"], timeout=TIMEOUT)
    if build["status"] != "ok":
        raise RuntimeError(build["stderr"])
    cli = REPO / "target" / "debug" / "devscout"
    shutil.copy2(test_bin, out / "cost-tests")
    shutil.copy2(cli, out / "devscout")
    metadata = {"revision": head, "export_manifest_sha256": digest(inputs / "manifest.json"),
                "source_revision": manifest["source_revision"], "test_binary_sha256": digest(test_bin),
                "cli_binary_sha256": digest(cli), "python": sys.version,
                "platform": platform.platform(), "processor": platform.processor(),
                "rustc": command(["rustc", "-Vv"])["stdout"],
                "cargo": command(["cargo", "-V"])["stdout"],
                "profile": "dev", "repetitions": REPETITIONS, "timeout_seconds": TIMEOUT,
                "rss_limit_bytes": RSS_LIMIT, "limits": {"glob_visits": "2*sum(Vref)+2*Q",
                    "glob_arcs": "2*sum(Eref)+4*Q", "copied": "2*(P+R+Q)",
                    "item_ancestry": "2*N"}}
    (out / "metadata.json").write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n")
    observed = []
    with (out / "cost.jsonl").open("w") as stream:
        for spec in manifest["rows"]:
            for repetition in range(REPETITIONS):
                result = observe(out / "cost-tests", spec, inputs)
                result.update({"repetition": repetition, "bound": bound(spec, result["cost"])})
                observed.append(result)
                stream.write(json.dumps(result, sort_keys=True) + "\n")
                stream.flush()
    maps = []
    with map_scratch_lock(inputs), (out / "maps.jsonl").open("w") as stream:
        for source in sorted((inputs / "corpus").iterdir()):
            if not source.is_dir():
                continue
            for result in map_corpus(out / "devscout", inputs, out, source.name):
                maps.append(result)
                stream.write(json.dumps(result, sort_keys=True) + "\n")
                stream.flush()
    summary = {"complete": complete(manifest["rows"], observed),
               "runs": len(observed), "failed": sum(r["status"] != "ok" for r in observed),
               "over_bound": sum(not r["bound"] for r in observed),
               "maps": len(maps), "map_failures": sum(r["status"] != "ok" or
                   not r["artifact"]["complete"] for r in maps),
               "median_seconds": {row_id(spec): statistics.median(r["seconds"] for r in observed
                   if r["row_id"] == row_id(spec)) for spec in manifest["rows"]}}
    (out / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    print(json.dumps(summary | {"revision": head}, sort_keys=True))


def compare(baseline, head):
    baseline, head = Path(baseline), Path(head)
    before = [json.loads(line) for line in (baseline / "cost.jsonl").read_text().splitlines()]
    after = [json.loads(line) for line in (head / "cost.jsonl").read_text().splitlines()]
    specs = list(rows())
    ids = [(r["row_id"], r["repetition"]) for r in before]
    if ids != [(r["row_id"], r["repetition"]) for r in after]:
        raise ValueError("baseline and head row inventories differ")
    if not complete(specs, before) or not complete(specs, after):
        raise ValueError("missing or duplicate workload row")
    before_meta = json.loads((baseline / "metadata.json").read_text())
    after_meta = json.loads((head / "metadata.json").read_text())
    if before_meta["export_manifest_sha256"] != after_meta["export_manifest_sha256"]:
        raise ValueError("source export manifests differ")
    failed = [r for r in after if r["status"] != "ok" or not r["bound"]]
    report = {"baseline_failing": sum(r["status"] != "ok" or not r["bound"] for r in before),
              "head_failing": len(failed), "runs": len(after),
              "first_failures": [r["row_id"] for r in failed[:10]]}
    print(json.dumps(report, sort_keys=True))
    if failed:
        raise SystemExit(1)


def compare_artifacts(baseline, head):
    before = [json.loads(line) for line in (Path(baseline) / "maps.jsonl").read_text().splitlines()]
    after = [json.loads(line) for line in (Path(head) / "maps.jsonl").read_text().splitlines()]
    def key(row):
        return (row["corpus"], row["repetition"], row["mode"])
    expected = {(source, rep, mode) for source in {row["corpus"] for row in before}
                for rep in range(REPETITIONS) for mode in ("cold", "warm")}
    if {key(row) for row in before} != expected or {key(row) for row in after} != expected:
        raise ValueError("missing or duplicate artifact workload")
    old = {key(row): row for row in before}
    new = {key(row): row for row in after}
    failures = []
    for current_key in sorted(expected):
        left = old[current_key]
        right = new[current_key]
        if left["status"] != "ok" or right["status"] != "ok":
            failures.append((current_key, "process failure"))
        elif not left["artifact"]["complete"] or not right["artifact"]["complete"]:
            failures.append((current_key, "missing required artifact"))
        elif left["artifact"]["files"] != right["artifact"]["files"]:
            failures.append((current_key, "baseline/head byte mismatch"))
    for corpus in {row["corpus"] for row in after}:
        reference = new[(corpus, 0, "cold")]["artifact"]["files"]
        for repetition in range(REPETITIONS):
            for mode in ("cold", "warm"):
                if new[(corpus, repetition, mode)]["artifact"]["files"] != reference:
                    failures.append(((corpus, repetition, mode), "head cold/warm mismatch"))
    report = {"corpora": len({row["corpus"] for row in after}), "maps": len(after),
              "failures": len(failures), "first_failures": failures[:20]}
    print(json.dumps(report, sort_keys=True))
    if failures:
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("--out", required=True)
    execute = sub.add_parser("run")
    for flag in ("revision", "inputs", "out"):
        execute.add_argument(f"--{flag}", required=True)
    comparison = sub.add_parser("compare")
    comparison.add_argument("--baseline", required=True)
    comparison.add_argument("--head", required=True)
    artifacts = sub.add_parser("artifacts")
    artifacts.add_argument("--baseline", required=True)
    artifacts.add_argument("--head", required=True)
    args = parser.parse_args()
    if args.action == "generate":
        generate(args.out)
    elif args.action == "run":
        run(args.revision, args.inputs, args.out)
    elif args.action == "compare":
        compare(args.baseline, args.head)
    else:
        compare_artifacts(args.baseline, args.head)


if __name__ == "__main__":
    main()
