import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import ts_parity as tp

PIN = "0" * 40


def corpus(files):
    texts = dict(files)
    return tp.Corpus("/nonexistent", sorted(texts), texts)


def mentions(name, count, test=True):
    files = {f"lib/user{i}.ts": f"import {{ {name} }} from './a';\n{name}();\n" for i in range(count)}
    if test:
        files["lib/a.spec.ts"] = f"{name}();\n"
    return files


class TestPathRule(unittest.TestCase):
    def test_suffixes_and_segment(self):
        for path in ("a/b.spec.ts", "a/b.spec.tsx", "a/b.test.ts", "a/b.test.tsx", "a/__tests__/b.ts",
                     "__tests__/x/b.tsx"):
            self.assertTrue(tp.is_test_path(path), path)

    def test_negatives(self):
        for path in ("a/b.test.js", "a/b.spec.jsx", "tests/b.ts", "a/my__tests__/b.ts",
                     "a/__tests__.ts", "a/b.spec.ts.snap", "a/B.Spec.ts", "a/__Tests__/b.ts"):
            self.assertFalse(tp.is_test_path(path), path)


class ExportReaderTests(unittest.TestCase):
    def test_forms(self):
        text = "\n".join([
            "export function alpha() {}",
            "export async function beta() {}",
            "export function* gamma() {}",
            "export default class Delta {}",
            "export abstract class Epsilon {}",
            "export interface Zeta {}",
            "export type Eta = string;",
            "export const theta = () => 1;",
            "export const iota: Fn = async (x) => x;",
            "export const kappa = 3;",
            "export declare const lambda: number;",
            "export const enum Mu { A }",
            "  export function nested() {}",
            "export { omitted } from './x';",
            "export type { AlsoOmitted } from './x';",
        ])
        found = {d["name"]: d for d in tp.exported_declarations("a.ts", text)}
        self.assertEqual(sorted(found), ["Delta", "Epsilon", "Eta", "Mu", "Zeta", "alpha", "beta", "gamma",
                                         "iota", "kappa", "lambda", "theta"])
        self.assertEqual(found["gamma"]["keyword"], "function")
        self.assertTrue(found["theta"]["functionLike"])
        self.assertTrue(found["iota"]["functionLike"])
        self.assertFalse(found["kappa"]["functionLike"])

    def test_overloads_are_one_declaration(self):
        text = "export function f(a: string): void;\nexport function f(a: number): void;\n"
        self.assertEqual(len(tp.exported_declarations("a.ts", text)), 1)

    def test_slots(self):
        def slot(line, path="a.tsx"):
            return tp.slot_of(tp.exported_declarations(path, line)[0])
        self.assertEqual(slot("export function useThing() {}", "a.ts"), 5)
        self.assertEqual(slot("export const useThing = () => 1;", "a.ts"), 5)
        self.assertEqual(slot("export function Button() {}"), 5)
        self.assertEqual(slot("export const Panel = memo(() => null);"), 5)
        self.assertEqual(slot("export function Button() {}", "a.ts"), 1)
        self.assertEqual(slot("export const Colors = { a: 1 };"), None)
        self.assertEqual(slot("export interface Props {}"), 4)
        self.assertEqual(slot("export type Props = {};"), 4)
        self.assertEqual(slot("export class Store {}", "a.ts"), 1)
        self.assertEqual(slot("export const run = () => 1;", "a.ts"), 1)
        self.assertEqual(slot("export enum Mode { A }", "a.ts"), None)
        self.assertEqual(slot("export const usage = 3;", "a.ts"), None)


class SeedRuleTests(unittest.TestCase):
    def build(self, extra=None):
        files = {}
        for name in ("fnA", "fnB", "fnC", "fnD"):
            files[f"lib/{name}.ts"] = f"export function {name}() {{}}\n"
            files.update({f"use/{name}{k}": v for k, v in mentions(name, 5).items()})
        files["lib/model.ts"] = "export interface Model {}\n"
        files.update({f"m/{k}": v for k, v in mentions("Model", 5).items()})
        files["lib/useThing.ts"] = "export function useThing() {}\n"
        files.update({f"h/{k}": v for k, v in mentions("useThing", 5).items()})
        files.update(extra or {})
        return corpus(files)

    def test_five_seeds_by_lowest_rank(self):
        selection = tp.select_seeds(self.build(), PIN)
        names = [seed["name"] for seed in selection["seeds"]]
        functions = sorted(["fnA", "fnB", "fnC", "fnD"], key=lambda n: tp.seed_rank(PIN, n))[:3]
        self.assertEqual(names, functions + ["Model", "useThing"])
        self.assertEqual(selection["declaringFiles"], sorted({f"lib/{functions[0]}.ts", "lib/model.ts"}))
        self.assertEqual(selection["diffFiles"], sorted({f"lib/{functions[1]}.ts", f"lib/{functions[2]}.ts",
                                                         "lib/useThing.ts"}))

    def test_duplicate_declaration_rejected(self):
        selection = tp.select_seeds(self.build({"other/fnA.ts": "export const fnA = () => 1;\n"}), PIN)
        self.assertNotIn("fnA", [seed["name"] for seed in selection["seeds"]])
        self.assertGreater(selection["rejected"]["name declared by more than one file"], 0)

    def test_needs_a_test_mention_and_count_range(self):
        extra = {"lib/lonely.ts": "export function lonely() {}\n"}
        extra.update({f"x/{k}": v for k, v in mentions("lonely", 6, test=False).items()})
        extra["lib/rare.ts"] = "export function rare() {}\n"
        extra["y/rare.spec.ts"] = "rare();\n"
        selection = tp.select_seeds(self.build(extra), PIN)
        self.assertNotIn("lonely", selection["pools"]["1"])
        self.assertNotIn("rare", selection["pools"]["1"])

    def test_test_and_declaration_files_never_seed(self):
        extra = {"lib/only.spec.ts": "export function onlySpec() {}\n",
                 "lib/types.d.ts": "export declare function ambient(): void;\n"}
        extra.update({f"z/{k}": v for k, v in mentions("onlySpec", 5).items()})
        extra.update({f"w/{k}": v for k, v in mentions("ambient", 5).items()})
        pools = tp.select_seeds(self.build(extra), PIN)["pools"]
        self.assertNotIn("onlySpec", pools["1"])
        self.assertNotIn("ambient", pools["1"])

    def test_fewer_than_five_is_reported(self):
        files = {"lib/a.ts": "export function a1() {}\n"}
        files.update(mentions("a1", 5))
        self.assertEqual(len(tp.select_seeds(corpus(files), PIN)["seeds"]), 1)


class TruthTests(unittest.TestCase):
    def test_truth_excludes_declaring_files_and_collects_target_names(self):
        files = {"lib/a.ts": "export function seed() {}\nexport const helper = 1;\n",
                 "u/one.ts": "seed();\n", "u/two.ts": "helper;\n"}
        seeds = {"pin": PIN, "seeds": [{"slot": 1, "name": "seed", "file": "lib/a.ts"}],
                 "declaringFiles": ["lib/a.ts"], "diffFiles": []}
        c = corpus(files)
        truth = tp.build_truth(c, seeds, lambda name: {p: [1] for p in c.word_files(name)})
        self.assertEqual(sorted(truth["names"]), ["helper", "seed"])
        self.assertEqual(sorted(truth["names"]["seed"]["files"]), ["u/one.ts"])
        self.assertEqual(truth["targets"]["declaring:lib/a.ts"]["names"], ["helper", "seed"])
        sealed = tp.seal_truth(truth, {}, {})
        self.assertEqual(tp.impact_truth(sealed, "declaring:lib/a.ts"), ["u/one.ts", "u/two.ts"])

    def test_classifier(self):
        self.assertEqual(tp.classify_file("// uses seed here\nx();\n", "seed"), "comment only")
        self.assertEqual(tp.classify_file("/* seed\n seed */\n", "seed"), "comment only")
        self.assertEqual(tp.classify_file("log('seed');\n// seed\n", "seed"), "string only")
        self.assertEqual(tp.classify_file("const s = `a ${seed()} b`;\n", "seed"), None)
        self.assertEqual(tp.classify_file("const s = `a seed b`;\n", "seed"), "string only")
        self.assertEqual(tp.classify_file("seed();\n", "seed"), None)
        self.assertEqual(tp.classify_file("function seed() {}\nseed();\n", "seed"), "other declaration")
        self.assertEqual(tp.classify_file("x.seed = 1;\nimport { seed } from './a';\n", "seed"), None)
        self.assertEqual(tp.classify_file("// function seed() {}\nseed();\n", "seed"), None)

    def test_seal_requires_a_decision_per_flag(self):
        truth = {"pin": PIN, "targets": {}, "names": {"s": {"declaredIn": ["a.ts"], "files": {"b.ts": [1], "c.ts": [2]}}}}
        flags = {"s": {"b.ts": "comment only"}}
        with self.assertRaises(SystemExit):
            tp.seal_truth(truth, flags, {})
        with self.assertRaises(SystemExit):
            tp.seal_truth(truth, {}, {"s": {"b.ts": {"remove": True, "reason": "comment only"}}})
        sealed = tp.seal_truth(truth, flags, {"s": {"b.ts": {"remove": True, "reason": "comment only"}}})
        self.assertEqual(sealed["names"]["s"]["files"], ["c.ts"])
        self.assertEqual(sealed["removed"], [{"name": "s", "file": "b.ts", "reason": "comment only"}])
        kept = tp.seal_truth(truth, flags, {"s": {"b.ts": {"remove": False, "reason": "real use"}}})
        self.assertEqual(kept["names"]["s"]["files"], ["b.ts", "c.ts"])
        self.assertEqual(len(kept["kept"]), 1)


class ScoringTests(unittest.TestCase):
    seeds = {"pin": PIN, "seeds": [{"slot": 1, "name": "a", "file": "d/a.ts"},
                                   {"slot": 4, "name": "b", "file": "d/b.ts"}],
             "declaringFiles": ["d/a.ts", "d/b.ts"], "diffFiles": []}
    sealed = {"names": {"a": {"files": ["x.ts", "y.spec.ts"]}, "b": {"files": ["z.ts"]}},
              "targets": {"declaring:d/a.ts": {"files": ["d/a.ts"], "names": ["a"]},
                          "declaring:d/b.ts": {"files": ["d/b.ts"], "names": ["b"]},
                          "diff": {"files": [], "names": []}}}

    def test_pooled_recall_and_precision(self):
        answers = {("refs", "a"): ["x.ts", "q.ts"], ("refs", "b"): ["z.ts"], ("tests", "a"): ["y.spec.ts"],
                   ("impact", "declaring:d/a.ts"): ["x.ts", "d/a.ts", "w.ts", "k.js"]}
        result = tp.score(self.sealed, self.seeds, answers)
        self.assertEqual(result["refs"]["pooled"], round(2 / 3, 4))
        self.assertEqual(result["refs"]["perSeed"]["a"]["extra"], ["q.ts"])
        self.assertEqual(result["tests"]["pooled"], 1.0)
        block = result["impact"]["impact"]["declaring:d/a.ts"]
        self.assertEqual((block["rows"], block["correct"], block["precision"]), (2, 1, 0.5))
        self.assertEqual(block["outsideUniverse"], ["k.js"])
        self.assertTrue(result["impact"]["impact"]["declaring:d/b.ts"]["failed"])
        self.assertNotIn("failed", block)

    def test_no_rows_is_not_a_precision(self):
        answers = {("impact", "declaring:d/a.ts"): []}
        block = tp.score(self.sealed, self.seeds, answers)["impact"]["impact"]["declaring:d/a.ts"]
        self.assertIsNone(block["precision"])
        self.assertFalse(tp.gate(tp.score(self.sealed, self.seeds, answers), self.seeds)[
            "impact precision >= 0.5 on declaring:d/a.ts"])

    def test_importer_widening_is_reported_apart(self):
        answers = {("refs", "a"): ["x.ts"], ("refs-importers", "a"): ["y.spec.ts"]}
        result = tp.score(self.sealed, self.seeds, answers)
        self.assertEqual(result["refs"]["perSeed"]["a"]["found"], 1)
        self.assertEqual(result["refsWithFileImporters"]["perSeed"]["a"]["found"], 2)

    def test_peers_win_names_every_leader(self):
        one = tp.score(self.sealed, self.seeds, {("refs", "a"): ["x.ts"]})
        two = tp.score(self.sealed, self.seeds, {("refs", "a"): ["x.ts", "y.spec.ts"], ("refs", "b"): ["z.ts"]})
        cells = {label: leaders for label, leaders, _ in tp.peers_win({"one": one, "two": two})}
        self.assertEqual(cells["refs recall (pooled)"], ["two"])
        self.assertEqual(cells["tests recall, b"], [])

    def test_answer_readers(self):
        refs = {"inbound": {"call": {"rows": [{"file": "a.ts"}]}, "import": {"rows": [{"file": "b.ts"}, {"file": "a.ts"}]}}}
        self.assertEqual(tp.devscout_files("refs", refs), ["a.ts", "b.ts"])
        self.assertEqual(tp.devscout_files("impact", {"rows": [{"file": "c.ts"}]}), ["c.ts"])
        crg = {"results": [{"file_path": "/r/a.ts"}, {"file_path": "/r/a.ts"}, {"file": "/r/b.ts"}]}
        self.assertEqual(tp.crg_files("refs", crg, "/r"), ["a.ts", "b.ts"])
        self.assertEqual(tp.crg_files("impact", {"impacted_files": ["/r/x.ts"]}, "/r"), ["x.ts"])


class GuardTests(unittest.TestCase):
    def test_env(self):
        self.assertEqual(tp.env_problems({"HOME": "/scratch/h"}, "/home/example"), [])
        self.assertTrue(tp.env_problems({}, "/home/example"))
        self.assertTrue(tp.env_problems({"HOME": "/home/example"}, "/home/example"))
        self.assertTrue(tp.env_problems({"HOME": "/home/example/x"}, "/home/example"))
        self.assertTrue(tp.env_problems({"HOME": "/s", "OPENAI_API_KEY": "x"}, "/home/example"))
        self.assertTrue(tp.env_problems({"HOME": "/s", "UV_INDEX_URL": "x"}, "/home/example"))
        self.assertTrue(tp.env_problems({"HOME": "/s", "PIP_EXTRA_INDEX_URL": "x"}, "/home/example"))

    def test_requirements(self):
        good = ("a==1.0 \\\n    --hash=sha256:" + "0" * 64 + "\n    # via b\n"
                "c==2 ; sys_platform == 'win32' \\\n    --hash=sha256:" + "1" * 64 + "\n")
        self.assertEqual(tp.requirement_problems(good), [])
        for bad in ("a>=1.0 --hash=sha256:" + "0" * 64, "a==1.0",
                    "a[extra]==1.0 --hash=sha256:" + "0" * 64,
                    "a @ https://example.invalid/a.whl --hash=sha256:" + "0" * 64,
                    "git+https://example.invalid/a.git",
                    "a==1.0 --index-url https://example.invalid --hash=sha256:" + "0" * 64,
                    "--extra-index-url https://example.invalid", ""):
            self.assertTrue(tp.requirement_problems(bad), bad)

    def test_cache(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(tp.cache_problems(root), [])
            self.assertEqual(tp.cache_problems(os.path.join(root, "absent")), [])
            Path(root, "x").write_text("x")
            self.assertTrue(tp.cache_problems(root))
        self.assertTrue(tp.cache_problems(""))

    def test_identity(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(len(tp.identity_problems(root, tp.PEER_VERSION)), 3)
            Path(root, "skills.py").write_text("")
            Path(root, "uninstall.py").write_text("")
            Path(root, "eval").mkdir()
            self.assertEqual(tp.identity_problems(root, tp.PEER_VERSION), [])
            self.assertTrue(tp.identity_problems(root, "2.3.8"))

    def test_corpus(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            clone = Path(root, "clone")
            clone.mkdir()
            self.assertEqual(tp.corpus_problems(clone, PIN, PIN, root, "/home/example"), [])
            self.assertTrue(tp.corpus_problems(clone, PIN, "1" * 40, root, "/home/example"))
            Path(root, "Cargo.toml").write_text('[package]\nname = "devscout-rs"\n')
            self.assertTrue(tp.corpus_problems(clone, PIN, PIN, root, "/home/example"))

    def test_corpus_is_bound_to_the_scratch_root_and_kept_out_of_home(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            scratch, elsewhere = Path(root, "scratch"), Path(root, "elsewhere")
            scratch.mkdir()
            elsewhere.mkdir()
            self.assertTrue(tp.corpus_problems(elsewhere, PIN, PIN, str(scratch), "/home/example"))
            inside_home = scratch / "clone"
            inside_home.mkdir()
            self.assertTrue(tp.corpus_problems(inside_home, PIN, PIN, str(scratch), str(scratch)))

    def test_arm_env(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            env = tp.arm_env("crg", root)
            self.assertEqual(tp.arm_env_problems("crg", env, root, "/home/example"), [])
            self.assertTrue(tp.arm_env_problems("crg", dict(env, CRG_DATA_DIR="/elsewhere"), root, "/home/example"))
            self.assertTrue(tp.arm_env_problems("crg", {k: v for k, v in env.items() if k != "CRG_DATA_DIR"},
                                                root, "/home/example"))
            self.assertTrue(tp.arm_env_problems("crg", env, root, root))
            dev = tp.arm_env("devscout-base", root)
            self.assertEqual(tp.arm_env_problems("devscout-base", dev, root, "/home/example"), [])
            self.assertNotIn("CRG_DATA_DIR", dev)

    def test_relative_state_paths_are_refused(self):
        # The child runs in the corpus, so a relative state path would resolve there, not here.
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            env = dict(tp.arm_env("crg", root), HOME="crg-home")
            self.assertTrue(tp.arm_env_problems("crg", env, root, "/home/example"))
            self.assertTrue(tp.arm_env_problems("crg", tp.arm_env("crg", root), "rel", "/home/example"))

    def test_arm_env_is_absolute_for_a_relative_scratch_root(self):
        env = tp.arm_env("crg", "rel")
        for key in ("HOME", "CRG_DATA_DIR"):
            self.assertTrue(os.path.isabs(env[key]), env[key])
            self.assertEqual(env[key], tp.canonical(env[key]))

    def test_scratch_root_inside_a_checkout_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            Path(root, "Cargo.toml").write_text('[package]\nname = "devscout-rs"\n')
            scratch = str(Path(root, "scratch"))
            self.assertTrue(tp.arm_env_problems("crg", tp.arm_env("crg", scratch), scratch, "/home/example"))

    def test_install_paths(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            cache, venv = str(Path(root, "cache")), str(Path(root, "venv"))
            self.assertEqual(tp.install_path_problems(cache, venv, "/home/example"), [])
            self.assertTrue(tp.install_path_problems("cache", venv, "/home/example"))
            self.assertTrue(tp.install_path_problems(cache, venv, root))
            Path(root, "Cargo.toml").write_text('[package]\nname = "devscout-rs"\n')
            self.assertTrue(tp.install_path_problems(cache, venv, "/home/example"))

    def test_answer_paths_match_the_corpus_root_however_it_is_spelled(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            corpus = Path(root, "corpus")
            (corpus / "src").mkdir(parents=True)
            alias = Path(root, "alias")
            alias.symlink_to(corpus)
            self.assertEqual(tp.relative(str(alias / "src" / "a.ts"), str(corpus)), "src/a.ts")
            self.assertEqual(tp.relative(str(corpus / "src" / "a.ts"), str(alias)), "src/a.ts")
            self.assertEqual(tp.relative("src/a.ts", str(corpus)), "src/a.ts")
            with self.assertRaises(SystemExit):
                tp.relative(str(Path(root, "other", "a.ts")), str(corpus))

    def test_templates(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root, "templates.tsv")
            path.write_text("crg\trefs\tseed\t{bin} query callers_of {seed} --repo {corpus}\n"
                            "devscout-base\trefs\tseed\t{bin} refs {seed} --all --json\n")
            rows = tp.read_templates(path)
            digest = tp.sha256_file(path)
            self.assertEqual(tp.template_problems(rows, digest, path), [])
            self.assertTrue(tp.template_problems(rows, "0" * 64, path))
            for verb in ("visualize", "install", "serve"):
                bad = [dict(rows[0], argv=["{bin}", verb])]
                self.assertTrue(tp.template_problems(bad, digest, path), verb)
            self.assertTrue(tp.template_problems([dict(rows[1], argv=["{bin}", "map"])], digest, path))

    def test_invocations(self):
        rows = [{"arm": "crg", "task": "impact", "target": "files", "argv": ["{bin}", "impact", "--files", "{files}", "--depth", "1"]},
                {"arm": "crg", "task": "refs", "target": "seed", "argv": ["{bin}", "query", "callers_of", "{seed}"]},
                {"arm": "devscout-base", "task": "impact", "target": "file", "argv": ["{bin}", "impact", "{file}"]}]
        seeds = {"seeds": [{"name": "a", "file": "d/a.ts"}], "declaringFiles": ["d/a.ts"], "diffFiles": ["d/b.ts", "d/c.ts"]}
        crg = tp.invocations(rows, "crg", seeds, "crg-bin", "/c")
        self.assertIn(("impact", "diff", ["crg-bin", "impact", "--files", "d/b.ts", "d/c.ts", "--depth", "1"]), crg)
        self.assertIn(("refs", "a", ["crg-bin", "query", "callers_of", "d/a.ts::a"]), crg)
        dev = tp.invocations(rows, "devscout-base", seeds, "ds", "/c")
        self.assertEqual([target for _, target, _ in dev], ["declaring:d/a.ts|d/a.ts", "diff|d/b.ts", "diff|d/c.ts"])

    def test_load_answers_unions_per_target(self):
        with tempfile.TemporaryDirectory() as root:
            Path(root, "0.out").write_text(json.dumps({"rows": [{"file": "x.ts"}]}))
            Path(root, "1.out").write_text(json.dumps({"rows": [{"file": "y.ts"}]}))
            Path(root, "2.out").write_text("not json")
            runs = [{"task": "impact", "target": "diff|a.ts", "exit": 0, "stdout": "0.out"},
                    {"task": "impact", "target": "diff|b.ts", "exit": 0, "stdout": "1.out"},
                    {"task": "refs", "target": "s", "exit": 0, "stdout": "2.out"}]
            Path(root, "runs.json").write_text(json.dumps({"arm": "devscout-head", "runs": runs}))
            answers = tp.load_answers(root)
            self.assertEqual(answers[("impact", "diff")], ["x.ts", "y.ts"])
            self.assertIsNone(answers[("refs", "s")])


def stored_run(root, arm, runs):
    """Write one arm's runs.json and stdout files; each run is (task, target, exit, stdout)."""
    log = []
    for number, (task, target, code, stdout) in enumerate(runs):
        Path(root, f"{number}.out").write_text(stdout if isinstance(stdout, str) else json.dumps(stdout))
        log.append({"task": task, "target": target, "exit": code, "stdout": f"{number}.out"})
    Path(root, "runs.json").write_text(json.dumps({"arm": arm, "runs": log}))


class RegisteredTargetTests(unittest.TestCase):
    seeds = ScoringTests.seeds
    sealed = ScoringTests.sealed
    gated = ["impact precision >= 0.5 on declaring:d/a.ts", "impact precision >= 0.5 on declaring:d/b.ts",
             "impact precision >= 0.5 on diff"]

    def test_a_missing_impact_answer_fails_its_check(self):
        answers = {("refs", "a"): ["x.ts", "y.spec.ts"], ("refs", "b"): ["z.ts"], ("tests", "a"): ["y.spec.ts"],
                   ("impact", "declaring:d/a.ts"): ["x.ts"]}
        checks = tp.gate(tp.score(self.sealed, self.seeds, answers), self.seeds)
        for label in self.gated:
            self.assertIn(label, checks)
        self.assertTrue(checks["impact precision >= 0.5 on declaring:d/a.ts"])
        self.assertFalse(checks["impact precision >= 0.5 on declaring:d/b.ts"])
        self.assertFalse(checks["impact precision >= 0.5 on diff"])

    def test_a_failed_run_answers_nothing_and_fails_its_target(self):
        with tempfile.TemporaryDirectory() as root:
            ok = {"rows": [{"file": "x.ts"}]}
            stored_run(root, "devscout-head", [
                ("impact", "diff|a.ts", 0, ok),
                ("impact", "diff|b.ts", 1, ok),
                ("impact", "declaring:d/a.ts|d/a.ts", 3, {"rows": []}),
                ("refs", "a", 0, "not json"),
                ("refs", "b", 2, {"inbound": {"call": {"rows": [{"file": "z.ts"}]}}}),
            ])
            answers = tp.load_answers(root)
        self.assertIsNone(answers[("impact", "diff")])
        self.assertEqual(answers[("impact", "declaring:d/a.ts")], [])
        self.assertIsNone(answers[("refs", "a")])
        self.assertIsNone(answers[("refs", "b")])
        result = tp.score(self.sealed, self.seeds, answers)
        self.assertTrue(result["impact"]["impact"]["diff"]["failed"])
        self.assertTrue(result["refs"]["perSeed"]["b"]["failed"])
        self.assertEqual(result["refs"]["perSeed"]["b"]["found"], 0)
        self.assertFalse(tp.gate(result, self.seeds)["impact precision >= 0.5 on diff"])

    def test_the_peer_answers_only_with_exit_zero_and_a_known_status(self):
        with tempfile.TemporaryDirectory() as root:
            row = {"status": "ok", "results": [{"file_path": "z.ts"}]}
            stored_run(root, "crg", [
                ("refs", "a", 3, row),
                ("refs", "b", 0, {"status": "error", "results": [{"file_path": "z.ts"}]}),
                ("tests", "a", 0, {"status": "not_found", "summary": "No node"}),
                ("tests", "b", 0, row),
            ])
            answers = tp.load_answers(root, root)
        self.assertIsNone(answers[("refs", "a")])
        self.assertIsNone(answers[("refs", "b")])
        self.assertEqual(answers[("tests", "a")], [])
        self.assertEqual(answers[("tests", "b")], ["z.ts"])

    def test_a_failed_seed_fails_the_recall_check_whatever_the_pool(self):
        sealed = {"names": {"a": {"files": ["x.ts"]}, "b": {"files": [f"f{i}.ts" for i in range(9)]}},
                  "targets": self.sealed["targets"]}
        answers = {("refs", "a"): None, ("refs", "b"): [f"f{i}.ts" for i in range(9)] + ["x.ts"]}
        result = tp.score(sealed, self.seeds, answers)
        self.assertEqual(result["refs"]["pooled"], 0.9)
        self.assertFalse(tp.gate(result, self.seeds)["refs recall >= 0.9"])

    def test_a_registered_seed_without_an_answer_is_failed(self):
        result = tp.score(self.sealed, self.seeds, {("refs", "a"): ["x.ts", "y.spec.ts"]})
        self.assertTrue(result["refs"]["perSeed"]["b"]["failed"])
        self.assertNotIn("failed", result["refs"]["perSeed"]["a"])


def fake_peer(root, version=tp.PEER_VERSION, identity=True, interpreter=None):
    """A virtual environment holding a stand-in distribution of the peer and its entry script."""
    venv = Path(root, "venv")
    subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(venv)], check=True)
    site = next(venv.glob("lib/python*/site-packages"))
    package = site / tp.PEER_MODULE
    package.mkdir()
    if identity:
        for name in tp.PEER_IDENTITY_FILES:
            (package / name).write_text("")
        for name in tp.PEER_IDENTITY_DIRS:
            (package / name).mkdir()
    info = site / f"{tp.PEER_MODULE}-{version}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(f"Metadata-Version: 2.1\nName: {tp.PEER_PACKAGE}\nVersion: {version}\n")
    binary = venv / "bin" / tp.PEER_PACKAGE
    binary.write_text(f"#!{interpreter or venv / 'bin' / 'python'}\nfrom code_review_graph.cli import main\n")
    binary.chmod(0o755)
    return str(binary)


def bench_dir(root, seeds):
    bench = Path(root, "bench")
    bench.mkdir()
    (bench / "templates.tsv").write_text("crg\tsetup\tsetup\t{bin} build --repo {corpus}\n"
                                        "devscout-base\tsetup\tsetup\t{bin} init --no-hooks\n")
    for name, value in (("seeds.json", seeds), ("truth-sealed.json", {"names": {}}), ("truth-raw.json", {"names": {}})):
        (bench / name).write_text(json.dumps(value))
    names = ("templates.tsv", "seeds.json", "truth-sealed.json", "truth-raw.json")
    (bench / "manifest.json").write_text(json.dumps({"files": {n: tp.sha256_file(bench / n) for n in names}}))
    return bench


class ConsumedGuardTests(unittest.TestCase):
    seeds = {"pin": PIN, "seeds": [], "declaringFiles": [], "diffFiles": []}

    def test_an_upstream_peer_install_passes(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(tp.peer_binary_problems(fake_peer(tp.canonical(root))), [])

    def test_a_peer_without_the_upstream_files_is_refused_at_run_time(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertTrue(tp.peer_binary_problems(fake_peer(tp.canonical(root), identity=False)))

    def test_another_peer_version_is_refused_at_run_time(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertTrue(tp.peer_binary_problems(fake_peer(tp.canonical(root), version="2.3.8")))

    def test_a_binary_outside_the_checked_environment_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            fake_peer(root)
            loose = Path(root, tp.PEER_PACKAGE)
            loose.write_text("#!/bin/sh\n")
            self.assertTrue(tp.peer_binary_problems(str(loose)))
            self.assertTrue(tp.peer_binary_problems(str(Path(root, "venv", "bin", "python"))))

    def test_an_entry_script_bound_to_another_interpreter_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertTrue(tp.peer_binary_problems(fake_peer(tp.canonical(root), interpreter="/usr/bin/python3")))

    def test_the_runner_rechecks_the_peer_and_the_seeds(self):
        with tempfile.TemporaryDirectory() as root:
            root = tp.canonical(root)
            bench = bench_dir(root, self.seeds)
            scratch = Path(root, "scratch")
            corpus = scratch / "corpus"
            corpus.mkdir(parents=True)
            good = fake_peer(root)

            def problems(arm, binary):
                return tp.arm_problems(arm, binary, str(corpus), str(scratch), bench, PIN, "/home/example")

            self.assertEqual(problems("crg", good), [])
            self.assertEqual(problems("devscout-base", "/usr/bin/true"), [])
            self.assertTrue(problems("crg", "/usr/bin/true"))
            (bench / "seeds.json").write_text(json.dumps(dict(self.seeds, diffFiles=["x.ts"])))
            self.assertTrue(problems("devscout-base", "/usr/bin/true"))

    def test_the_scorer_rechecks_seeds_and_truth(self):
        with tempfile.TemporaryDirectory() as root:
            bench = bench_dir(tp.canonical(root), self.seeds)
            self.assertEqual(tp.manifest_problems(bench, tp.SCORE_INPUTS), [])
            for name in tp.SCORE_INPUTS:
                original = (bench / name).read_text()
                (bench / name).write_text(original + " ")
                self.assertTrue(tp.manifest_problems(bench, tp.SCORE_INPUTS), name)
                with self.assertRaises(SystemExit):
                    tp.cmd_score([str(bench), root, str(Path(root, "scores.json"))])
                (bench / name).write_text(original)


REPO = Path(__file__).resolve().parents[1]
BENCH_COMMAND_FILES = ("docs/benchmarks/methodology.md", "bench/ts-parity.sh",
                       "bench/peers/code-review-graph/install.sh")


def python_commands(text):
    """(line, options) for every python3 line that runs a script or a module."""
    found = []
    for line in text.splitlines():
        tokens = line.replace('"', " ").split()
        if "python3" not in tokens:
            continue
        options = []
        for token in tokens[tokens.index("python3") + 1:]:
            if not token.startswith("-"):
                if token.endswith(".py"):
                    found.append((line, options))
                break
            options.append(token)
            if token == "-m":
                found.append((line, options))
                break
    return found


class BytecodeTests(unittest.TestCase):
    def test_documented_bench_commands_write_no_bytecode(self):
        commands = []
        for name in BENCH_COMMAND_FILES:
            commands.extend(python_commands((REPO / name).read_text(encoding="utf-8")))
        self.assertGreaterEqual(len(commands), 5)
        for line, options in commands:
            self.assertIn("-B", options, line)


if __name__ == "__main__":
    unittest.main()
