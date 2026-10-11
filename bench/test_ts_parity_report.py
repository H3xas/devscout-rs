import copy
import json
import tempfile
import unittest
from pathlib import Path

import ts_parity as tp
import ts_parity_report as report

PIN = "0" * 40
BASE = "a" * 40
HEAD = "b" * 40
REPO = Path(__file__).resolve().parents[1]

PREREGISTRATION = """# Pre-registration

## Predictions

| Arm | Metric | Prediction | Falsified if |
| --- | --- | --- | --- |
| `devscout-head` | refs recall | 0.65 | pooled recall below 0.55 or above 0.75 |
| `crg` | tests recall | ≤ 0.5 | pooled recall above 0.5 |

## Where peers win
"""

TEMPLATES = ("devscout-base\tsetup\tsetup\t{bin} init --no-hooks\n"
             "devscout-base\trefs\tseed\t{bin} refs {seed} --all --json\n"
             "devscout-base\ttests\tseed\t{bin} tests {seed} --json\n"
             "devscout-base\timpact\tfile\t{bin} impact {file} --hops 1 --json\n"
             "devscout-head\tsetup\tsetup\t{bin} init --no-hooks\n"
             "devscout-head\trefs\tseed\t{bin} refs {seed} --all --json\n"
             "devscout-head\ttests\tseed\t{bin} tests {seed} --json\n"
             "devscout-head\timpact\tfile\t{bin} impact {file} --hops 1 --json\n"
             "crg\tsetup\tsetup\t{bin} build --repo {corpus}\n"
             "crg\trefs\tseed\t{bin} query callers_of {seed} --repo {corpus}\n"
             "crg\ttests\tseed\t{bin} query tests_for {seed} --repo {corpus}\n"
             "crg\timpact\tfiles\t{bin} impact --files {files} --depth 1 --repo {corpus}\n")


def scored_bench(root):
    """A registered bench with one seed, three stored arm runs, and both score files."""
    root = Path(tp.canonical(root))
    bench = root / "bench" / "truth" / "typescript"
    bench.mkdir(parents=True)
    (root / "bench" / "corpus.lock").write_text(
        f'[typescript]\nrepo = "owner/corpus"\nsha = "{PIN}"\nlicense = "MIT"\n')
    seeds = {"pin": PIN, "seeds": [{"slot": 1, "name": "a", "file": "d/a.ts", "keyword": "function"}],
             "declaringFiles": ["d/a.ts"], "diffFiles": ["d/b.ts"]}
    targets = {"declaring:d/a.ts": {"files": ["d/a.ts"], "names": ["a"]},
               "diff": {"files": ["d/b.ts"], "names": ["a"]}}
    files = {"templates.tsv": TEMPLATES, "seeds.json": json.dumps(seeds), "preregistration.md": PREREGISTRATION,
             "truth-sealed.json": json.dumps({"names": {"a": {"files": ["x.spec.ts", "y.ts"]}},
                                              "targets": targets, "removed": []}),
             "truth-raw.json": json.dumps({"names": {"a": {"files": {"x.spec.ts": [1], "y.ts": [1]}}},
                                           "targets": targets})}
    for name, text in files.items():
        (bench / name).write_text(text)
    (root / "bench" / "ts_parity.py").write_text("")
    files["../../ts_parity.py"] = ""
    (bench / "manifest.json").write_text(json.dumps({"files": {n: tp.sha256_file(bench / n) for n in files}}))
    corpus = root / "nx-crg"
    (corpus / "d").mkdir(parents=True)
    (corpus / "x.spec.ts").write_text("import { a } from '../d/a';\na();\n")
    (corpus / "y.ts").write_text("import { a } from '@pkg/a';\na();\n")
    payloads = {"devscout-head": {"refs": {"inbound": {"call": {"rows": [{"file": "x.spec.ts"}]}}},
                                  "tests": {"rows": [{"file": "x.spec.ts"}]},
                                  "impact": {"totalAffected": 1, "rows": [{"file": "x.spec.ts"}], "dropped": 0}},
                "crg": {"refs": {"status": "ok", "results": [{"file_path": str(corpus / "y.ts")}]},
                        "tests": {"status": "not_found", "results": []},
                        "impact": {"status": "ok", "impacted_files": [str(corpus / "y.ts")], "impacted_nodes": [{}],
                                   "total_impacted": 3, "nodes_omitted": 2, "truncated": True}}}
    specs = []
    for arm in ("devscout-base", "devscout-head", "crg"):
        out = root / f"out-{arm}"
        out.mkdir()
        log = []
        for number, (task, target, argv) in enumerate(tp.invocations(tp.read_templates(bench / "templates.tsv"),
                                                                      arm, seeds, f"/bin/{arm}", str(corpus))):
            payload = payloads.get(arm, {}).get(task, {"status": "ok", "results": [], "rows": [], "inbound": {}})
            (out / f"{number}.out").write_text(json.dumps(payload))
            log.append({"n": number, "task": task, "target": target, "argv": argv, "exit": 0,
                        "seconds": 1.25, "stdout": f"{number}.out"})
        (out / "runs.json").write_text(json.dumps({"arm": arm, "corpusHead": PIN, "runs": log}))
        specs.append(f"{arm}={out}")
    tp.cmd_score([str(bench), str(corpus), str(root / "scores.json"), *specs])
    after = tp.read_json(root / "scores.json")
    before = copy.deepcopy(after)
    for arm in ("devscout-base", "devscout-head", "crg"):
        del before[arm]["evidence"]
    return bench, before, after


class RenderTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.bench, self.before, self.after = scored_bench(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def render(self, before=None, after=None, date="2026-10-11", base=BASE, head=HEAD):
        return report.render(self.bench, before or self.before, after or self.after, date, base, head)

    def test_rendering_the_same_inputs_twice_writes_the_same_bytes(self):
        out = Path(self.dir.name, "one.md")
        args = ["ts_parity_report.py", str(self.bench), str(Path(self.dir.name, "before.json")),
                str(Path(self.dir.name, "scores.json")), str(out), "2026-10-11", BASE, HEAD]
        tp.write_json(args[2], self.before)
        self.assertEqual(report.main(args), 0)
        first = out.read_bytes()
        self.assertEqual(report.main(args), 0)
        self.assertEqual(out.read_bytes(), first)
        self.assertEqual(first.decode("utf-8"), self.render())

    def test_scores_lacking_a_registered_arm_or_evidence_are_refused(self):
        missing = copy.deepcopy(self.after)
        del missing["crg"]
        bare = copy.deepcopy(self.after)
        del bare["devscout-head"]["evidence"]
        for before, after in ((missing, self.after), (self.before, missing), (self.before, bare)):
            with self.assertRaises(SystemExit):
                self.render(before=before, after=after)

    def test_a_changed_registered_input_is_refused_and_a_changed_code_entry_is_reported(self):
        manifest = tp.read_json(self.bench / "manifest.json")
        (self.bench / "scorer.py").write_text("")
        manifest["files"]["scorer.py"] = "0" * 64
        tp.write_json(self.bench / "manifest.json", manifest)
        self.assertIn("| `scorer.py` | code | `0000000000000000` | differs |", self.render())
        (self.bench / "seeds.json").write_text((self.bench / "seeds.json").read_text() + " ")
        with self.assertRaises(SystemExit):
            self.render()

    def test_a_manifest_without_the_pre_registered_scorer_is_refused(self):
        manifest = tp.read_json(self.bench / "manifest.json")
        del manifest["files"]["../../ts_parity.py"]
        tp.write_json(self.bench / "manifest.json", manifest)
        with self.assertRaises(SystemExit):
            self.render()

    def test_a_malformed_date_or_commit_is_refused(self):
        for args in ({"date": "11/10/2026"}, {"base": "abc"}, {"head": "B" * 40}):
            with self.assertRaises(SystemExit):
                self.render(**args)

    def test_every_section_reads_its_figures_from_the_scores(self):
        text = self.render()
        self.assertIn("**Gate verdict (`devscout-head`): missed.**", text)
        self.assertIn("| pooled `refs` recall | ≥ 0.9 | **1/2 (0.500)** | **missed** |", text)
        self.assertIn("| `crg` | `setup` | 1 | 0 ×1 | n/a |", text)
        self.assertIn("| `crg` | `tests` | 1 | 0 ×1 | not_found ×1 |", text)
        self.assertIn("| `crg` | diff list | 1 | nodes | 3 | 1 | 2 | yes |", text)
        self.assertIn("| `devscout-head` | refs recall | 0.65 | pooled recall below 0.55 or above 0.75 "
                      "| **1/2 (0.500)** | **falsified** |", text)
        self.assertIn("| `crg` | tests | `a` | `x.spec.ts` | no row for any target |", text)
        self.assertIn("| `devscout-head` | refs | `a` | `y.ts` | package-name import naming the seed |", text)
        self.assertIn("every truth file of the target (2)", text)
        self.assertIn("**Registered figures and gate checks equal before and after: 48 of 48.**", text)

    def test_a_figure_the_scorers_disagree_on_is_disclosed(self):
        changed = copy.deepcopy(self.before)
        changed["crg"]["sealed"]["refs"]["perSeed"]["a"]["found"] = 0
        text = self.render(before=changed)
        self.assertIn("| crg refs recall, sealed, a | 0/2 (0.500) | 1/2 (0.500) | **differs** |", text)
        self.assertIn("equal before and after: 47 of 48.", text)

    def test_a_registered_prediction_without_a_rule_is_refused(self):
        text = (self.bench / "preregistration.md").read_text().replace("| refs recall |", "| refs reach |")
        (self.bench / "preregistration.md").write_text(text)
        manifest = tp.read_json(self.bench / "manifest.json")
        manifest["files"]["preregistration.md"] = tp.sha256_file(self.bench / "preregistration.md")
        tp.write_json(self.bench / "manifest.json", manifest)
        with self.assertRaises(SystemExit):
            self.render()


class PredictionRuleTests(unittest.TestCase):
    def test_every_pre_registered_prediction_has_a_rule(self):
        preregistration = (REPO / "bench" / "truth" / "typescript" / "preregistration.md").read_text()
        registered = {(arm, metric) for arm, metric, _, _ in report.predictions_table(preregistration)}
        self.assertEqual(registered, set(report.PREDICTION_RULES))

    def test_a_recall_prediction_is_falsified_outside_its_band(self):
        rule = report.PREDICTION_RULES[("devscout-head", "refs recall")]
        for pooled, falsified in ((0.54, True), (0.55, False), (0.75, False), (0.76, True), (None, True)):
            sealed = {"refs": {"pooled": pooled, "found": 0, "truth": 0, "perSeed": {}}}
            self.assertEqual(rule(sealed, None)[1], falsified, pooled)


if __name__ == "__main__":
    unittest.main()
