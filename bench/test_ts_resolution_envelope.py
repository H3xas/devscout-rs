import json
import tempfile
import unittest
from pathlib import Path

from ts_resolution_envelope import (REFUSED, classify, config, declared_mode, mode_for,
                                    package, pnpm_patterns, read_sites, scope_for, score, strip_jsonc,
                                    valid_patterns, write_sites)


class ReaderTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def put(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def test_package_accepts_maps_and_refuses_first_bad_field(self):
        path = self.put("package.json", json.dumps({"name": "x", "type": "module",
                         "exports": {".": {"import": "./main.js", "default": None}},
                         "workspaces": {"packages": ["packages/*"]}}))
        self.assertIsInstance(package(path)[0], dict)
        path.write_text('{"name":"", "type":"other"}')
        self.assertEqual(package(path), (REFUSED, "name"))
        path.write_text('{"exports": {".": "./x", "import": "./y"}}')
        self.assertEqual(package(path), (REFUSED, "exports"))
        path.write_text('{"imports": {"#x": true}}')
        self.assertEqual(package(path), (REFUSED, "imports"))
        path.write_text('{"workspaces": ["packages/?"]}')
        self.assertEqual(package(path), (REFUSED, "workspaces"))

    def test_package_rejects_bom_comment_and_non_finite(self):
        path = self.put("package.json", "\ufeff{}")
        self.assertEqual(package(path), (REFUSED, "document"))
        path.write_text('{/* comment */ "name":"x"}')
        self.assertEqual(package(path), (REFUSED, "document"))
        path.write_text('{"value":NaN}')
        self.assertEqual(package(path), (REFUSED, "document"))

    def test_jsonc_keeps_comment_tokens_in_strings(self):
        self.assertEqual(json.loads(strip_jsonc('{"url":"//example/*x*/", // note\n"v":1,}')),
                         {"url": "//example/*x*/", "v": 1})
        self.assertEqual(config(self.put("tsconfig.json", '{"compilerOptions":{"module":"Node20",}}'))[1], "")
        self.assertEqual(config(self.put("tsconfig.json", '{"compilerOptions":{"composite":"yes"}}')),
                         (REFUSED, "composite"))

    def test_mode_chain_and_refusal(self):
        self.put("tsconfig.base.json", '{"compilerOptions":{"moduleResolution":"bundler"}}')
        self.put("packages/lib/tsconfig.json", '{"extends":"../../tsconfig.base.json", "compilerOptions":{}}')
        self.assertEqual(mode_for(self.root, "packages/lib/src/main.ts"), "bundler")
        self.put("packages/lib/tsconfig.json", '{"compilerOptions":{"moduleResolution":"classic"}}')
        self.assertEqual(mode_for(self.root, "packages/lib/src/main.ts"), "classic")
        self.put("packages/lib/tsconfig.json", '{"compilerOptions":{"moduleResolution":7}}')
        self.assertEqual(mode_for(self.root, "packages/lib/src/main.ts"), "bundler")
        self.assertEqual(declared_mode({"module": "node20"}), "node16")
        self.assertEqual(declared_mode({"module": "preserve"}), "bundler")

    def test_pnpm_patterns_and_refusals(self):
        path = self.put("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n  - \"!packages/test\"\nother: true\n")
        self.assertEqual(pnpm_patterns(path), (["packages/*", "!packages/test"], ""))
        path.write_text("packages: ['packages/*', 'packages/**']\n")
        self.assertEqual(pnpm_patterns(path), (["packages/*", "packages/**"], ""))
        path.write_text("packages:\n  - packages/[abc]\n")
        self.assertEqual(pnpm_patterns(path), (REFUSED, "packages"))
        self.assertFalse(valid_patterns(["packages/?"]))


class ClassificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "tsconfig.json").write_text('{"compilerOptions":{"moduleResolution":"nodenext"}}')
        (self.root / "src").mkdir()
        (self.root / "src/main.ts").write_text("import './target.js';\n")

    def row(self, **changes):
        row = dict(fixture="fixture", file="src/main.ts", line=1, specifier="./target.js",
                   target="src/target.ts", form="import", mode="nodenext", link="", mapped="")
        row.update(changes)
        return row

    def test_precedence_nonliteral_then_mode_then_json(self):
        row = self.row(form="dynamic-non-literal", specifier="(non-literal)", mode="classic")
        self.assertEqual(scope_for(self.root, row), "non-literal-import")
        self.assertEqual(scope_for(self.root, self.row(mode="node10", specifier="./data.json")),
                         "mode-not-declared")
        self.assertEqual(scope_for(self.root, self.row(specifier="./data.json")), "json-module")

    def test_types_versions_and_refused_configuration(self):
        package_path = self.root / "package.json"
        package_path.write_text('{"name":"x","typesVersions":{"*":{}}}')
        self.assertEqual(scope_for(self.root, self.row(link=".")), "types-versions")
        package_path.write_text('{"type":"module",')
        self.assertEqual(scope_for(self.root, self.row()), "refused-configuration")

    def test_no_exclusion_for_accepted_scope(self):
        (self.root / "package.json").write_text('{"type":"commonjs"}')
        self.assertEqual(scope_for(self.root, self.row()), "")

    def test_pnpm_default_ignore_after_accepted_inputs(self):
        (self.root / "package.json").write_text('{"type":"commonjs"}')
        (self.root / "pnpm-workspace.yaml").write_text("packages:\n  - packages/**\n")
        member = self.root / "packages/tests/lib"
        member.mkdir(parents=True)
        (member / "package.json").write_text('{"name":"lib"}')
        self.assertEqual(scope_for(self.root, self.row(link="packages/tests/lib")),
                         "pnpm-default-ignore")
        self.assertEqual(scope_for(self.root, self.row(link="packages/lib")), "")

    def test_classified_file_keeps_order_and_excluded_rows(self):
        rows = [dict(self.row(), scope=""), dict(self.row(line=2, form="dynamic-non-literal",
                specifier="(non-literal)", target=""), scope="non-literal-import")]
        path = self.root / "classified.tsv"
        write_sites(path, rows)
        self.assertEqual(len(path.read_text().splitlines()), 2)

    def test_unsafe_source_is_refused(self):
        with self.assertRaisesRegex(ValueError, "site source unavailable"):
            scope_for(self.root, self.row(file="../outside.ts"))

    def test_score_excludes_sites_but_lists_their_actual_edges(self):
        registration = self.root / "registration"
        registration.mkdir()
        (registration / "manifest.json").write_text('{"sha256":{}}')
        (registration / "fixtures.json").write_text('[{"id":"fixture","mode":"nodenext",'
                                                    '"minimum_truth_resolvable":1}]')
        classified = self.root / "classified.tsv"
        write_sites(classified, [dict(self.row(), scope=""),
                                 dict(self.row(line=2, specifier="./else.js", target="src/else.ts"),
                                      scope="refused-configuration")])
        graph = self.root / "graph.json"
        graph.write_text(json.dumps({"edges": [
            {"kind": "import", "from_file": "src/main.ts", "from_line": 1,
             "target": "./target.js", "to_file": "src/target.ts"},
            {"kind": "import", "from_file": "src/main.ts", "from_line": 2,
             "target": "./else.js", "to_file": "src/wrong.ts"}]}))
        result = score(registration, "fixture", "head", classified, graph)
        self.assertEqual(result["counts"]["agree"], 1)
        self.assertEqual(result["counts"]["wrong-target"], 0)
        self.assertEqual(result["truth_resolvable"], 1)
        self.assertEqual(result["outside_envelope"]["counts"], {"refused-configuration": 1})
        self.assertEqual(result["outside_envelope"]["sites"][0]["actual"], ["src/wrong.ts"])
        (registration / "fixtures.json").write_text('[{"id":"fixture","mode":"nodenext",'
                                                    '"minimum_truth_resolvable":2}]')
        with self.assertRaisesRegex(ValueError, "registered size"):
            score(registration, "fixture", "head", classified, graph)

    def test_invalid_classified_row_fails_closed(self):
        path = self.root / "invalid.tsv"
        path.write_text("fixture\t../escape.ts\t1\t./x\t\timport\tnodenext\t\t\t\n")
        with self.assertRaisesRegex(ValueError, "unsafe file"):
            read_sites(path, True)


if __name__ == "__main__":
    unittest.main()
