"""Checks scoring classes and registration guards for module resolution."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from ts_resolution import direct_edges, score, verify_manifest


class ResolutionScoringTests(unittest.TestCase):
    def test_every_class_and_threshold(self):
        truth = [{"file": "a.ts", "line": i, "specifier": str(i),
                  "truth": value} for i, value in enumerate(["a.ts", "b.ts", "c.ts", "", ""], 1)]
        graph = {"edges": [{"kind": "import", "from_file": "a.ts", "from_line": line,
                            "target": str(line), "to_file": target}
                           for line, target in [(1, "a.ts"), (3, "wrong.ts"), (5, "extra.ts")]]}
        result = score(truth, graph)
        self.assertEqual(result["counts"], {"agree": 1, "agree-unresolved": 1, "miss": 1,
                                            "wrong-target": 1, "spurious": 1})
        self.assertFalse(result["gate_met"])

    def test_via_edges_do_not_change_direct_target(self):
        graph = {"edges": [{"kind": "import", "from_file": "a.ts", "from_line": 1,
                            "target": "x", "to_file": "b.ts", "via": "barrel.ts"}]}
        self.assertEqual(direct_edges(graph), {})

    def test_changed_registered_input_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "input").write_text("original")
            digest = hashlib.sha256(b"original").hexdigest()
            (root / "manifest.json").write_text(json.dumps({"sha256": {"input": digest}}))
            verify_manifest(root)
            (root / "input").write_text("changed")
            with self.assertRaisesRegex(ValueError, "changed registered input"):
                verify_manifest(root)


if __name__ == "__main__":
    unittest.main()
