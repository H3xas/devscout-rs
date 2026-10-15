import json
import tempfile
import unittest
from pathlib import Path

from ts_resolution_envelope_report import render


class ReportTests(unittest.TestCase):
    def score(self, arm):
        return {"fixture": "example", "mode": "node16", "arm": arm,
                "counts": {"agree": 100, "agree-unresolved": 1, "miss": 0,
                           "wrong-target": 0, "spurious": 0},
                "truth_resolvable": 100, "gate_met": True, "disagreements": [],
                "outside_envelope": {"counts": {"refused-configuration": 1},
                    "sites": [{"file": "src/main.ts", "line": 3, "specifier": "./x",
                               "form": "import", "sub_class": "refused-configuration",
                               "truth": "src/x.ts", "actual": []}]}}

    def test_render_is_stable_and_lists_prior_result_exclusions_and_synthetic(self):
        scores = [self.score("head"), self.score("base")]
        synthetic = [{"arm": "head", "rows": 2, "matching": 2, "mismatches": []},
                     {"arm": "base", "rows": 2, "matching": 1,
                      "mismatches": [{"case": "example", "file": "src/main.ts", "line": 1,
                                      "specifier": "./x", "expected": "src/x.ts", "actual": []}]}]
        first = render(scores, synthetic, "2026-10-15", "base-ref", "head-ref")
        second = render(list(reversed(scores)), list(reversed(synthetic)),
                        "2026-10-15", "base-ref", "head-ref")
        self.assertEqual(first, second)
        for phrase in ("2026-10-15-typescript-module-resolution.md", "## Outside the scoring set",
                       "refused-configuration", "src/main.ts:3", "## Synthetic cases",
                       "head: 2/2 matching", "base: 1/2 matching"):
            self.assertIn(phrase, first)


if __name__ == "__main__":
    unittest.main()
