import json
from pathlib import Path
import tempfile
import unittest

import rust_resolver_cost as cost


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
        observed = [{"row_id": cost.row_id(spec)} for spec in specs
                    for _ in range(cost.REPETITIONS)]
        self.assertTrue(cost.complete(specs, observed))
        self.assertFalse(cost.complete(specs, observed[:-1]))
        self.assertFalse(cost.complete(specs, observed + observed[:1]))
        self.assertFalse(cost.complete(specs, observed + [{"row_id": "unexpected"}]))


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
            base = Path(temp) / "baseline"
            head = Path(temp) / "head"
            for folder in (base, head):
                folder.mkdir()
                folder.joinpath("metadata.json").write_text(json.dumps({"export_manifest_sha256": "same"}))
            rows = [{"row_id": cost.row_id(spec), "repetition": rep,
                     "status": "ok", "bound": True} for spec in cost.rows()
                    for rep in range(cost.REPETITIONS)]
            base.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            rows[0]["status"] = "timeout"
            head.joinpath("cost.jsonl").write_text("\n".join(map(json.dumps, rows)))
            with self.assertRaises(SystemExit):
                cost.compare(base, head)


if __name__ == "__main__":
    unittest.main()
