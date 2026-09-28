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
import signal
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
LIMITS = {"glob_visits": "2*sum(Vref)+2*Q", "glob_arcs": "2*sum(Eref)+4*Q",
          "copied": "2*(P+R+Q)", "item_ancestry": "2*N"}
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
    process = subprocess.Popen(argv, cwd=cwd, env=env, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
        status = "ok" if process.returncode == 0 else "failed"
        return {"status": status, "exit": process.returncode,
                "seconds": time.monotonic() - started, "stdout": stdout, "stderr": stderr}
    except subprocess.TimeoutExpired:
        stop_group(process)
        stdout, stderr = process.communicate()
        return {"status": "timeout", "exit": None, "seconds": time.monotonic() - started,
                "stdout": stdout, "stderr": stderr}
    except BaseException:
        stop_group(process)
        process.communicate()
        raise


def stop_group(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


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
    try:
        if spec["family"] == "item":
            return cost["awork"] <= 2 * cost["items"]
        q = spec["queries"]
        return (cost["vwork"] <= 2 * cost["vref"] + 2 * q
                and cost["ework"] <= 2 * cost["eref"] + 4 * q
                and cost["copied_entries"] <= 2 * (cost["probes"] + cost["candidates"] + q))
    except (KeyError, TypeError, ValueError):
        return False


def complete(specs, observations):
    expected = {(row_id(spec), repetition) for spec in specs
                for repetition in range(REPETITIONS)}
    seen = set()
    for observation in observations:
        key = (observation.get("row_id"), observation.get("repetition"))
        if key not in expected or key in seen:
            return False
        seen.add(key)
    return seen == expected


def registered_corpora(manifest):
    corpora = {Path(path).parts[1] for path in manifest["files"]
               if len(Path(path).parts) >= 3 and Path(path).parts[0] == "corpus"}
    required = {f"glob-{topology}-{size}" for topology in TOPOLOGIES for size in SIZES}
    required.update(f"item-{shape}-{size}" for shape in ("nested", "wide") for size in SIZES)
    required.update(("fixtures", "source-snapshot"))
    if corpora != required:
        raise ValueError("frozen export corpus set differs from the registered protocol")
    return sorted(corpora)


def protocol_metadata(inputs, manifest):
    return {"export_manifest_sha256": digest(inputs / "manifest.json"),
            "source_revision": manifest["source_revision"],
            "workload_sha256": hashlib.sha256(json.dumps(manifest["rows"], sort_keys=True).encode()).hexdigest(),
            "corpora": registered_corpora(manifest), "repetitions": REPETITIONS,
            "timeout_seconds": TIMEOUT, "rss_limit_bytes": RSS_LIMIT, "limits": LIMITS,
            "profile": "dev"}


def read_evidence(folder, inputs, manifest):
    folder = Path(folder)
    metadata = json.loads((folder / "metadata.json").read_text())
    for key, expected in protocol_metadata(inputs, manifest).items():
        if metadata.get(key) != expected:
            raise ValueError(f"{folder}: protocol metadata differs: {key}")
    for key in ("revision", "rustc", "cargo", "python", "platform"):
        if not isinstance(metadata.get(key), str) or not metadata[key]:
            raise ValueError(f"{folder}: missing build metadata: {key}")
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", metadata["revision"]):
        raise ValueError(f"{folder}: invalid revision")
    for binary, field in (("cost-tests", "test_binary_sha256"),
                          ("devscout", "cli_binary_sha256")):
        if not (folder / binary).is_file() or digest(folder / binary) != metadata.get(field):
            raise ValueError(f"{folder}: binary hash differs: {binary}")
    return metadata


def number(value):
    return type(value) in (int, float) and value >= 0


def validate_resource(row, label):
    if not isinstance(row.get("status"), str) or not row["status"]:
        raise ValueError(f"{label}: missing status")
    if not number(row.get("seconds")):
        raise ValueError(f"{label}: missing elapsed time")
    if row["status"] == "ok":
        if row.get("exit") != 0 or not number(row.get("peak_rss_bytes")):
            raise ValueError(f"{label}: success lacks exit or RSS evidence")
        if row["seconds"] > TIMEOUT or row["peak_rss_bytes"] > RSS_LIMIT:
            raise ValueError(f"{label}: success exceeds resource protocol")


def validate_cost(spec, row):
    label = row_id(spec)
    validate_resource(row, label)
    if type(row.get("bound")) is not bool:
        raise ValueError(f"{label}: missing bound flag")
    cost = row.get("cost")
    if row["status"] != "ok":
        return False
    if not isinstance(cost, dict) or cost.get("family") != spec["family"]:
        raise ValueError(f"{label}: missing or wrong cost family")
    if spec["family"] == "glob":
        for key in ("topology", "size", "queries", "outcome", "namespace", "location", "shape"):
            if cost.get(key) != spec[key]:
                raise ValueError(f"{label}: cost row dimension differs: {key}")
        fields = ("vertices", "arcs", "distinct_keys", "vwork", "ework", "vref", "eref",
                  "probes", "candidates", "copied_entries", "result_bytes",
                  "cache_entries", "cache_payload_bytes")
    else:
        if cost.get("source") != Path(spec["source"]).name:
            raise ValueError(f"{label}: item source differs")
        fields = ("input_bytes", "items", "parent_calls", "awork", "path_bytes",
                  "fragment_bytes")
    if any(type(cost.get(key)) is not int or cost[key] < 0 for key in fields):
        raise ValueError(f"{label}: missing or invalid work counter")
    computed = bound(spec, cost)
    if row["bound"] != computed:
        raise ValueError(f"{label}: stale stored bound flag")
    return computed


def cost_rows(folder, specs):
    observations = [json.loads(line) for line in (Path(folder) / "cost.jsonl").read_text().splitlines()]
    if not complete(specs, observations):
        raise ValueError(f"{folder}: missing, duplicate or unknown workload repetition")
    by_id = {row_id(spec): spec for spec in specs}
    for row in observations:
        validate_cost(by_id[row["row_id"]], row)
    return observations


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
            yield result


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
    metadata = {"revision": head, "test_binary_sha256": digest(test_bin),
                "cli_binary_sha256": digest(cli), "python": sys.version,
                "platform": platform.platform(), "processor": platform.processor(),
                "rustc": command(["rustc", "-Vv"])["stdout"],
                "cargo": command(["cargo", "-V"])["stdout"],
                **protocol_metadata(inputs, manifest)}
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
        for corpus in registered_corpora(manifest):
            for result in map_corpus(out / "devscout", inputs, out, corpus):
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


def compare(baseline, head, inputs):
    baseline, head = Path(baseline), Path(head)
    inputs, manifest = read_export(inputs)
    specs = manifest["rows"]
    before_meta = read_evidence(baseline, inputs, manifest)
    after_meta = read_evidence(head, inputs, manifest)
    for key in ("rustc", "cargo", "platform", "profile"):
        if before_meta[key] != after_meta[key]:
            raise ValueError(f"baseline and head build protocol differs: {key}")
    before = cost_rows(baseline, specs)
    after = cost_rows(head, specs)
    ids = [(r["row_id"], r["repetition"]) for r in before]
    if ids != [(r["row_id"], r["repetition"]) for r in after]:
        raise ValueError("baseline and head row inventories differ")
    by_id = {row_id(spec): spec for spec in specs}
    failed = [r for r in after if r["status"] != "ok" or not bound(by_id[r["row_id"]], r["cost"])]
    report = {"baseline_failing": sum(r["status"] != "ok" or
                  not bound(by_id[r["row_id"]], r["cost"]) for r in before),
              "head_failing": len(failed), "runs": len(after),
              "first_failures": [r["row_id"] for r in failed[:10]]}
    print(json.dumps(report, sort_keys=True))
    if failed:
        raise SystemExit(1)


def map_rows(folder, corpora):
    folder = Path(folder)
    observations = [json.loads(line) for line in (folder / "maps.jsonl").read_text().splitlines()]
    def key(row):
        return (row["corpus"], row["repetition"], row["mode"])
    expected = {(source, rep, mode) for source in corpora
                for rep in range(REPETITIONS) for mode in ("cold", "warm")}
    keys = [key(row) for row in observations]
    if len(keys) != len(expected) or set(keys) != expected:
        raise ValueError("missing or duplicate artifact workload")
    for row in observations:
        validate_resource(row, str(key(row)))
        if row.get("init_status") != "ok" and row["status"] == "ok":
            raise ValueError(f"{key(row)}: successful map after failed init")
        artifact = row.get("artifact")
        if not isinstance(artifact, dict) or not isinstance(artifact.get("files"), dict):
            raise ValueError(f"{key(row)}: missing artifact inventory")
        if row["status"] == "ok":
            files = artifact["files"]
            if not artifact.get("complete") or not {"manifest.json", "graph/graph.json"}.issubset(files):
                raise ValueError(f"{key(row)}: missing required artifact")
            actual = inventory(folder / "artifacts" / row["corpus"] /
                               str(row["repetition"]) / row["mode"])
            if actual != files:
                raise ValueError(f"{key(row)}: artifact inventory differs from saved bytes")
    return {key(row): row for row in observations}


def same_artifact_bytes(left_root, right_root, files):
    return all((left_root / path).read_bytes() == (right_root / path).read_bytes()
               for path in files)


def compare_artifacts(baseline, head, inputs):
    baseline, head = Path(baseline), Path(head)
    inputs, manifest = read_export(inputs)
    before_meta = read_evidence(baseline, inputs, manifest)
    after_meta = read_evidence(head, inputs, manifest)
    for key in ("rustc", "cargo", "platform", "profile"):
        if before_meta[key] != after_meta[key]:
            raise ValueError(f"baseline and head build protocol differs: {key}")
    corpora = registered_corpora(manifest)
    old = map_rows(baseline, corpora)
    new = map_rows(head, corpora)
    expected = {(source, rep, mode) for source in corpora
                for rep in range(REPETITIONS) for mode in ("cold", "warm")}
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
        elif not same_artifact_bytes(baseline / "artifacts" / current_key[0] /
                                     str(current_key[1]) / current_key[2],
                                     head / "artifacts" / current_key[0] /
                                     str(current_key[1]) / current_key[2],
                                     left["artifact"]["files"]):
            failures.append((current_key, "baseline/head raw byte mismatch"))
    for corpus in corpora:
        reference = new[(corpus, 0, "cold")]["artifact"]["files"]
        for repetition in range(REPETITIONS):
            for mode in ("cold", "warm"):
                current = new[(corpus, repetition, mode)]["artifact"]["files"]
                if current != reference or not same_artifact_bytes(
                    head / "artifacts" / corpus / "0" / "cold",
                    head / "artifacts" / corpus / str(repetition) / mode, reference):
                    failures.append(((corpus, repetition, mode), "head cold/warm mismatch"))
    report = {"corpora": len(corpora), "maps": len(new),
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
    comparison.add_argument("--inputs", required=True)
    artifacts = sub.add_parser("artifacts")
    artifacts.add_argument("--baseline", required=True)
    artifacts.add_argument("--head", required=True)
    artifacts.add_argument("--inputs", required=True)
    args = parser.parse_args()
    if args.action == "generate":
        generate(args.out)
    elif args.action == "run":
        run(args.revision, args.inputs, args.out)
    elif args.action == "compare":
        compare(args.baseline, args.head, args.inputs)
    else:
        compare_artifacts(args.baseline, args.head, args.inputs)


if __name__ == "__main__":
    main()
