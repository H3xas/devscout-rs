"""The module resolution renderer is deterministic on stored scores."""

import unittest

from ts_resolution_report import render


class ResolutionReportTests(unittest.TestCase):
    def test_same_scores_render_same_bytes(self):
        scores = [{"fixture": "sample", "mode": "node10", "arm": "head",
                   "counts": {"agree": 100, "agree-unresolved": 2, "miss": 0,
                              "wrong-target": 0, "spurious": 0},
                   "truth_resolvable": 100, "gate_met": True, "disagreements": []}]
        first = render(scores, "2026-01-01", "base", "head")
        self.assertEqual(first, render(scores, "2026-01-01", "base", "head"))
        self.assertIn("| node10 | head | 100/100 | 0 | 0 | met |", first)


if __name__ == "__main__":
    unittest.main()
