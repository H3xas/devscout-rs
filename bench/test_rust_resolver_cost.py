import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest import mock

import rust_resolver_cost as cost


def evidence_folders(root):
    inputs = root / "inputs"
    corpora = {f"glob-{topology}-{size}" for topology in cost.TOPOLOGIES for size in cost.SIZES}
    corpora.update(f"item-{shape}-{size}" for shape in ("nested", "wide") for size in cost.SIZES)
    corpora.update(("fixtures", "source-snapshot"))
    for corpus in corpora:
        source = inputs / "corpus" / corpus / "src" / "lib.rs"
        source.parent.mkdir(parents=True)
        source.write_text("pub struct Demo;\n")
    manifest = {"schema": 1, "source_revision": "a" * 40,
                "rows": list(cost.rows()), "files": cost.inventory(inputs)}
    (inputs / "manifest.json").write_text(json.dumps(manifest))
    metadata = cost.protocol_metadata(inputs, manifest) | {
        "revision": "b" * 40, "rustc": "rustc test", "cargo": "cargo test",
        "python": "python test", "platform": "test platform"}
    folders = []
    for name in ("baseline", "head"):
        folder = root / name
        folder.mkdir()
        for binary in ("cost-tests", "devscout"):
            (folder / binary).write_bytes(binary.encode())
        data = metadata | {"test_binary_sha256": cost.digest(folder / "cost-tests"),
                           "cli_binary_sha256": cost.digest(folder / "devscout")}
        (folder / "metadata.json").write_text(json.dumps(data))
        folders.append(folder)
    return inputs, *folders


def passing_cost(spec):
    if spec["family"] == "item":
        return {"family": "item", "source": Path(spec["source"]).name,
                "input_bytes": 1, "items": 1, "parent_calls": 0,
                "awork": 0, "path_bytes": 1, "fragment_bytes": 1}
    return {**spec, "vertices": 1, "arcs": 0, "distinct_keys": 1,
            "vwork": 0, "ework": 0, "vref": 0, "eref": 0,
            "probes": 0, "candidates": 0, "copied_entries": 0,
            "result_bytes": 0, "cache_entries": 0, "cache_payload_bytes": 0}


def passing_rows():
    return [{"row_id": cost.row_id(spec), "repetition": rep,
             "status": "ok", "exit": 0, "peak_rss_bytes": 1024,
             "seconds": 0.01, "bound": True, "cost": passing_cost(spec)}
            for spec in cost.rows() for rep in range(cost.REPETITIONS)]


def write_passing_maps(folder, corpora):
    rows = []
    for corpus in corpora:
        for repetition in range(cost.REPETITIONS):
            for mode in ("cold", "warm"):
                artifacts = folder / "artifacts" / corpus / str(repetition) / mode
                graph = artifacts / "graph" / "graph.json"
                graph.parent.mkdir(parents=True)
                graph.write_bytes(b"{}\n")
                (artifacts / "manifest.json").write_bytes(b"{}\n")
                rows.append({"corpus": corpus, "repetition": repetition, "mode": mode,
                             "status": "ok", "init_status": "ok", "exit": 0,
                             "seconds": 0.01, "peak_rss_bytes": 1024,
                             "stdout": "graph rebuilt" if mode == "cold" else "graph unchanged",
                             "pre_cold": {"source_files": 1, "manifest_exists": False,
                                          "graph_exists": False, "index_state_exists": False,
                                          "content_db_exists": False},
                             "artifact": {"files": cost.inventory(artifacts), "complete": True}})
    (folder / "maps.jsonl").write_text("\n".join(map(json.dumps, rows)))


class GeneratorTests(unittest.TestCase):
    def test_rows_are_unique_and_generation_is_deterministic(self):
        rows = list(cost.rows())
        self.assertEqual(len(rows), len({cost.row_id(row) for row in rows}))
        self.assertEqual({row["size"] for row in rows}, {32, 64, 128})
        with tempfile.TemporaryDirectory() as temp:
            first, second = Path(temp) / "first", Path(temp) / "second"
            cost.generate(first)
            cost.generate(second)
            self.assertEqual(cost.read_export(first)[1], cost.read_export(second)[1])

    def test_missing_and_duplicate_rows_fail_closed(self):
        specs = list(cost.rows())
        observed = [{"row_id": cost.row_id(spec), "repetition": rep} for spec in specs
                    for rep in range(cost.REPETITIONS)]
        self.assertTrue(cost.complete(specs, observed))
        self.assertFalse(cost.complete(specs, observed[:-1]))
        self.assertFalse(cost.complete(specs, observed + observed[:1]))
        self.assertFalse(cost.complete(specs, observed + [{"row_id": "unexpected"}]))
        replacement = observed.copy()
        replacement[1] = replacement[0]
        self.assertFalse(cost.complete(specs, replacement))

    def test_map_scratch_is_outside_the_source_export_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "manifest.json").write_text("frozen\n")
            scratch = cost.map_scratch_path(root)
            self.assertFalse(scratch.is_relative_to(cost.REPO))
            self.assertFalse(scratch.is_relative_to(root))

    def test_map_scratch_rejects_concurrent_writer(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "manifest.json").write_text("frozen\n")
            with cost.map_scratch_lock(root):
                with self.assertRaises(BlockingIOError):
                    with cost.map_scratch_lock(root):
                        self.fail("second writer acquired map scratch")

    def test_real_cli_map_corpus_measures_first_build_then_reuse(self):
        build = cost.command(["cargo", "build", "--locked", "--bin", "devscout"],
                             timeout=cost.TIMEOUT)
        self.assertEqual(build["status"], "ok", build["stderr"])
        cli = cost.REPO / "target" / "debug" / "devscout"
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs = root / "inputs"
            source = inputs / "corpus" / "tiny" / "src" / "lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub struct Present;\n", encoding="utf-8")
            (inputs / "manifest.json").write_text("tiny frozen corpus\n")
            out = root / "out"
            out.mkdir()
            original = cost.metered_command
            checked = []
            map_calls = 0

            def observed(argv, *, cwd, env):
                nonlocal map_calls
                if argv[1:3] == ["map", "."]:
                    if map_calls % 2 == 0:
                        checked.append(cwd)
                        self.assertTrue((cwd / "src" / "lib.rs").stat().st_size)
                        self.assertFalse((cwd / ".scout" / "manifest.json").exists())
                        self.assertFalse((cwd / ".scout" / "graph").exists())
                        self.assertFalse((cwd / ".scout" / "index-state.json").exists())
                        self.assertFalse(Path(env["SCOUT_CONTENT_DB"]).exists())
                    map_calls += 1
                return original(argv, cwd=cwd, env=env)

            with cost.map_scratch_lock(inputs), mock.patch.object(
                    cost, "metered_command", side_effect=observed):
                rows = list(cost.map_corpus(cli, inputs, out, "tiny"))
            self.assertEqual(len(checked), cost.REPETITIONS)
            self.assertEqual(len(rows), cost.REPETITIONS * 2)
            for row in rows:
                self.assertEqual(row["status"], "ok", row["stderr"])
                self.assertTrue(row["artifact"]["complete"])
                self.assertIn("graph rebuilt" if row["mode"] == "cold" else
                              "graph unchanged", row["stdout"])

    def test_failed_init_is_retained_and_does_not_invoke_map(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs = root / "inputs"
            source = inputs / "corpus" / "tiny" / "src" / "lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub struct Present;\n")
            (inputs / "manifest.json").write_text("failed setup corpus\n")
            out = root / "out"
            out.mkdir()
            failure = {"status": "failed", "exit": 9, "seconds": 0.25,
                       "peak_rss_bytes": 2048, "stdout": "partial setup",
                       "stderr": "setup failed"}
            with cost.map_scratch_lock(inputs), mock.patch.object(
                    cost, "metered_command", return_value=failure) as invoked:
                rows = list(cost.map_corpus(Path("/unused-cli"), inputs, out, "tiny"))
            self.assertEqual(invoked.call_count, cost.REPETITIONS)
            self.assertEqual(len(rows), cost.REPETITIONS * 2)
            for row in rows:
                self.assertEqual(row["status"], "skipped-after-init-failure")
                self.assertEqual(row["init_status"], "failed")
                self.assertEqual(row["init_exit"], 9)
                self.assertEqual(row["init_stderr"], "setup failed")
                self.assertEqual(row["init_stdout"], "partial setup")

    def test_setup_that_populates_graph_cannot_be_measured_as_cold(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs = root / "inputs"
            source = inputs / "corpus" / "tiny" / "src" / "lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub struct Present;\n")
            (inputs / "manifest.json").write_text("contaminated setup corpus\n")
            out = root / "out"
            out.mkdir()

            def contaminated(argv, *, cwd, env):
                self.assertEqual(argv[1], "init", "map must not run after contaminated init")
                graph = cwd / ".scout" / "graph"
                graph.mkdir(parents=True)
                (graph / "graph.json").write_text("{}\n")
                return {"status": "ok", "exit": 0, "seconds": 0.25,
                        "peak_rss_bytes": 2048, "stdout": "graph rebuilt", "stderr": ""}

            with cost.map_scratch_lock(inputs), mock.patch.object(
                    cost, "metered_command", side_effect=contaminated) as invoked:
                rows = list(cost.map_corpus(Path("/unused-cli"), inputs, out, "tiny"))
            self.assertEqual(invoked.call_count, cost.REPETITIONS)
            self.assertEqual(len(rows), cost.REPETITIONS * 2)
            for row in rows:
                self.assertEqual(row["status"], "skipped-after-setup-contamination")
                self.assertTrue(row["pre_cold"]["graph_exists"])


class BoundTests(unittest.TestCase):
    def test_numeric_limits_and_missing_counters(self):
        glob = {"family": "glob", "queries": 128}
        passing = {"vwork": 320, "vref": 32, "ework": 400, "eref": 16,
                   "copied_entries": 1000, "probes": 300, "candidates": 100}
        self.assertTrue(cost.bound(glob, passing))
        for key in ("vwork", "ework", "copied_entries"):
            failing = passing.copy()
            failing[key] = 100_000
            self.assertFalse(cost.bound(glob, failing))
        self.assertFalse(cost.bound(glob, None))
        self.assertTrue(cost.bound({"family": "item"}, {"awork": 20, "items": 10}))
        self.assertFalse(cost.bound({"family": "item"}, {"awork": 21, "items": 10}))

    def test_timeout_and_failure_cannot_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, head = evidence_folders(Path(temp))
            rows = passing_rows()
            base.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            rows[0]["status"] = "timeout"
            head.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(SystemExit):
                cost.compare(base, head, inputs)

    def test_missing_cost_counters_and_stale_pass_flags_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, head = evidence_folders(Path(temp))
            rows = passing_rows()
            base.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            rows[0]["cost"] = None
            head.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.compare(base, head, inputs)
            rows[0]["cost"] = passing_cost(list(cost.rows())[0])
            rows[0]["cost"]["vwork"] = 100_000
            head.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.compare(base, head, inputs)

    def test_empty_artifact_inventories_do_not_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, head = evidence_folders(Path(temp))
            for folder in (base, head):
                folder.joinpath("maps.jsonl").write_text("")
            with self.assertRaises(ValueError):
                cost.compare_artifacts(base, head, inputs)

    def test_warm_only_row_cannot_claim_cold_artifact(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, _ = evidence_folders(Path(temp))
            corpora = cost.registered_corpora(cost.read_export(inputs)[1])
            write_passing_maps(base, corpora)
            rows = [json.loads(line) for line in (base / "maps.jsonl").read_text().splitlines()]
            rows[0]["stdout"] = "graph unchanged"
            (base / "maps.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaisesRegex(ValueError, "contradicts cold/warm"):
                cost.map_rows(base, corpora)

    def test_missing_family_and_replacement_duplicate_cannot_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            _, base, _ = evidence_folders(Path(temp))
            specs = list(cost.rows())
            rows = passing_rows()
            base.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, [
                row for row in rows if not row["row_id"].startswith("item:")])))
            with self.assertRaises(ValueError):
                cost.cost_rows(base, specs)
            rows[1] = rows[0]
            base.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.cost_rows(base, specs)

    def test_missing_corpus_and_replacement_duplicate_cannot_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            _, base, _ = evidence_folders(Path(temp))
            rows = [{"corpus": "demo", "repetition": rep, "mode": mode}
                    for rep in range(cost.REPETITIONS) for mode in ("cold", "warm")]
            base.joinpath("maps.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.map_rows(base, ["demo", "another"])
            rows[1] = rows[0]
            base.joinpath("maps.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.map_rows(base, ["demo"])

    def test_protocol_metadata_and_resource_evidence_are_required(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, head = evidence_folders(Path(temp))
            rows = passing_rows()
            for folder in (base, head):
                folder.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            metadata = json.loads((head / "metadata.json").read_text())
            metadata["timeout_seconds"] += 1
            (head / "metadata.json").write_text(json.dumps(metadata))
            with self.assertRaises(ValueError):
                cost.compare(base, head, inputs)
            metadata["timeout_seconds"] -= 1
            (head / "metadata.json").write_text(json.dumps(metadata))
            rows[0]["peak_rss_bytes"] = None
            head.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(ValueError):
                cost.compare(base, head, inputs)

    def test_complete_cost_and_artifact_comparisons_use_saved_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            inputs, base, head = evidence_folders(Path(temp))
            rows = passing_rows()
            corpora = cost.registered_corpora(cost.read_export(inputs)[1])
            for folder in (base, head):
                folder.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
                write_passing_maps(folder, corpora)
            cost.compare(base, head, inputs)
            cost.compare_artifacts(base, head, inputs)
            changed = head / "artifacts" / corpora[0] / "0" / "cold" / "graph" / "graph.json"
            changed.write_bytes(b"{\"changed\":true}\n")
            with self.assertRaises(ValueError):
                cost.compare_artifacts(base, head, inputs)

    def test_timeout_reaps_metered_descendant_before_return(self):
        with tempfile.TemporaryDirectory() as temp:
            marker = Path(temp) / "late-write"
            script = ("import pathlib,time; time.sleep(0.25); "
                      "pathlib.Path(__import__('sys').argv[1]).write_text('escaped')")
            result = cost.command(["/usr/bin/time", "-l", sys.executable, "-c", script,
                                   str(marker)], timeout=0.05)
            self.assertEqual(result["status"], "timeout")
            time.sleep(0.35)
            self.assertFalse(marker.exists(), "timed-out descendant wrote after return")


if __name__ == "__main__":
    unittest.main()
