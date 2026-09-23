import copy
import unittest

from extension_evidence import Evidence, admission_failures, answer_stats, candidates, summarize


def edge(target="Lib.Extensions", member="Apply", tier="ext"):
    return dict(kind="uses-member", from_file="Caller.cs", from_line=7,
                to=target, to_file="Extensions.cs", member=member,
                heuristic=tier != "precise", tier=tier)


def graph(edges):
    return dict(defs=[dict(id="Lib.Extensions", file="Extensions.cs", methods=["Apply"])], edges=edges)


def record(member="Apply", target="Lib.Extensions", **values):
    return dict(file="Caller.cs", startLine=7, member=member, target=target,
                targetKind="class", external=False, shape="access", **values)


def row(file, heuristic=False):
    return dict(file=file, heuristic=heuristic)


class ExtensionEvidenceTests(unittest.TestCase):
    def test_guess_never_admitted(self):
        self.assertEqual(candidates(graph([edge(tier="guess")]))[0], [])

    def test_mixed_site_rejected_even_when_one_edge_is_extension(self):
        self.assertEqual(candidates(graph([edge(), edge(target="Other", tier="guess")]))[0], [])

    def test_different_site_in_same_row_does_not_promote_guess(self):
        guess = edge(target="Other", tier="guess")
        guess["from_line"] = 8
        self.assertEqual(candidates(graph([edge(), guess]))[0], [0])

    def test_missing_declaration_and_duplicate_definition_rejected(self):
        missing = graph([edge(member="Missing")])
        self.assertEqual(candidates(missing)[0], [])
        duplicate = graph([edge()])
        duplicate["defs"] *= 2
        self.assertEqual(candidates(duplicate)[0], [])

    def test_candidate_selection_does_not_relabel_graph(self):
        value = graph([edge()])
        before = copy.deepcopy(value)
        self.assertEqual(candidates(value)[0], [0])
        self.assertEqual(value, before)

    def test_same_line_other_member_cannot_vouch_for_edge(self):
        ev = Evidence(graph([edge()]), [record(member="Different")], [])
        self.assertEqual(ev.stats([0])["fp"], 1)

    def test_recall_counts_oracle_records_and_union_deduplicates_overlap(self):
        ev = Evidence(graph([edge(), edge(tier="precise")]),
                      [record(), record(receiverKind="ident")], [])
        self.assertEqual(ev.stats([0, 1])["recall_denominator"], 2)
        self.assertEqual(ev.stats([0, 1])["recall_hits"], 2)
        self.assertEqual(ev.cohort()["ext_overlap_with_precise"], 2)

    def test_failed_and_unlisted_units_are_outside_scored_universe(self):
        units = [dict(status="ok", files=["Extensions.cs"]),
                 dict(status="failed", files=["Caller.cs"])]
        ev = Evidence(graph([edge()]), [record()], units)
        self.assertEqual(ev.stats([])["recall_denominator"], 0)
        self.assertEqual(ev.edges, {})

    def test_answer_precision_scores_rows_instead_of_edge_precision(self):
        stats = answer_stats([row("Correct"), row("Wrong"), row("Wrong")], {"Correct"})
        self.assertEqual((stats["tp"], stats["fp"], stats["precision"]), (1, 1, 0.5))

    def test_displacement_and_heuristic_rows_remain_visible(self):
        answers = [dict(seed="Seed", oracle=dict(rows=[row("Known")]),
                        baseline=dict(rows=[row("Known")]),
                        candidate=dict(rows=[row("Guess", True)]))]
        result = summarize(answers)["cohorts"]
        self.assertEqual(result["candidate"]["fp"], 1)
        self.assertEqual(result["displaced"]["tp"], 1)
        self.assertIsNone(result["candidate_asserted"]["precision"])

    def test_oracle_replaces_member_predictions_but_keeps_context_fixed(self):
        context = dict(kind="inherits", from_file="Caller.cs", to="Lib.Extensions")
        ev = Evidence(graph([edge(), context]), [record()], [])
        baseline, oracle = ev.replay_graphs()
        self.assertTrue(baseline["edges"][0]["heuristic"])
        self.assertIn(context, oracle["edges"])
        self.assertFalse(any(e.get("heuristic") for e in oracle["edges"]))

    def test_correct_edges_cannot_hide_a_whole_answer_regression(self):
        edges = dict(candidate=dict(edges=100, tp=100), combined=dict(edges=200, tp=200))
        answers = dict(cohorts=dict(baseline=dict(rows=1000, tp=990),
                                   candidate=dict(rows=2000, tp=1960, below_floor_answers=0),
                                   added=dict(rows=1000, tp=970)))
        self.assertIn("whole-answer precision decreases", admission_failures(edges, answers))

    def test_high_micro_precision_cannot_hide_a_failing_answer(self):
        edges = dict(candidate=dict(edges=100, tp=100), combined=dict(edges=200, tp=200))
        answers = dict(cohorts=dict(baseline=dict(rows=1000, tp=970),
                                   candidate=dict(rows=2000, tp=1980, below_floor_answers=1),
                                   added=dict(rows=1000, tp=990)))
        self.assertIn("individual candidate answers fall below 0.95", admission_failures(edges, answers))

    def test_suggestions_cannot_hide_an_asserted_answer_regression(self):
        edges = dict(candidate=dict(edges=100, tp=100), combined=dict(edges=200, tp=200))
        answers = dict(cohorts=dict(baseline=dict(rows=1000, tp=960),
                                   candidate=dict(rows=2000, tp=1940, below_floor_answers=0),
                                   baseline_asserted=dict(rows=100, tp=100),
                                   candidate_asserted=dict(rows=200, tp=196, below_floor_answers=0),
                                   added=dict(rows=1000, tp=980)))
        self.assertIn("asserted affected-answer precision decreases", admission_failures(edges, answers))


if __name__ == "__main__":
    unittest.main()
