import json
import os
import shutil
import tempfile
import unittest

import rust_edges
import rust_sites

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = os.path.join(HERE, "rust-edges-sample")
LIB, SHELF = "src/lib.rs", "src/shelf.rs"


def d(id_, name, kind, file, line, end, tests=False):
    out = dict(id=id_, name=name, kind=kind, file=file, line=line, endLine=end)
    if tests:
        out["testMethods"] = [name]
    return out


def n(name, kind, file, line):
    return dict(name=name, kind=kind, file=file, line=line)


def call(file, line, to, member):
    return {"kind": "uses-member", "from_file": file, "from_line": line, "to": to,
            "to_file": to.split("#")[0], "member": member}


def sample_graph(edges):
    return {
        "defs": [
            d("src/lib.rs#shelf", "shelf", "mod", LIB, 1, 1),
            d("src/lib.rs#polish", "polish", "fn", LIB, 3, 5),
            d("src/lib.rs#tests", "tests", "mod", LIB, 7, 19),
            d("src/lib.rs#tests::polish_adds_one", "polish_adds_one", "fn", LIB, 11, 14, True),
            d("src/lib.rs#tests::dust_twice", "dust_twice", "fn", LIB, 16, 18),
            d("src/shelf.rs#Lamp", "Lamp", "struct", SHELF, 1, 1),
            d("src/shelf.rs#dust", "dust", "fn", SHELF, 3, 7),
        ],
        "names": [
            n("shelf", "mod", LIB, 1), n("polish", "fn", LIB, 3), n("tests", "mod", LIB, 8),
            n("polish_adds_one", "fn", LIB, 12), n("dust_twice", "fn", LIB, 16),
            n("Lamp", "struct", SHELF, 1), n("dust", "fn", SHELF, 3),
        ],
        "edges": edges,
    }


GOOD = [call(LIB, 4, "src/shelf.rs#dust", "dust"),
        call(LIB, 13, "src/lib.rs#polish", "polish"),
        call(LIB, 13, "src/shelf.rs#dust", "dust"),
        call(LIB, 17, "src/shelf.rs#dust", "dust"),
        call(LIB, 17, "src/shelf.rs#dust", "dust")]
WRONG = [call(LIB, 4, "src/lib.rs#polish", "polish"),
         call(LIB, 13, "src/lib.rs#polish", "dust")]


def load_sample(graph):
    with open(os.path.join(SAMPLE, "sample.lsif"), encoding="utf-8") as fh:
        lsif = rust_edges.parse_lsif(fh)
    sources = rust_edges.Sources(SAMPLE)
    keys, kinds = rust_edges.def_keys(graph)
    refs, defs, universe = rust_edges.oracle_references(lsif, ["/sample"], sources, keys, kinds)
    return refs, defs, universe, sources, keys


class LsifTests(unittest.TestCase):
    def test_sample_parses_into_references_with_targets(self):
        refs, _, universe, _, _ = load_sample(sample_graph([]))
        self.assertEqual(universe, [LIB, SHELF])
        dust = [r for r in refs if r["name"] == "dust"]
        self.assertEqual([(r["file"], r["line"]) for r in dust],
                         [(LIB, 4), (LIB, 13), (LIB, 17), (LIB, 17)])
        self.assertTrue(all(r["targets"] == ["src/shelf.rs#dust"] for r in dust))
        u32 = [r for r in refs if r["name"] == "u32"]
        self.assertTrue(u32 and all(r["status"] == "external" for r in u32))
        local = [r for r in refs if r["name"] == "n"]
        self.assertTrue(local and all(r["status"] == "not a recorded item" for r in local))

    def test_variant_shapes_parse_alike(self):
        lines = [
            '{"id":"d","type":"vertex","label":"document","uri":"file:///r/a.rs"}',
            '{"id":"x","type":"vertex","label":"range","start":{"line":0,"character":3},'
            '"end":{"line":0,"character":4}}',
            '{"id":"y","type":"vertex","label":"range","start":{"line":1,"character":0},'
            '"end":{"line":1,"character":1}}',
            '{"id":"s","type":"vertex","label":"resultSet"}',
            '{"id":"e1","type":"edge","label":"next","outV":"x","inV":"s"}',
            '{"id":"e2","type":"edge","label":"next","outV":"y","inV":"s"}',
            '{"id":"dr","type":"vertex","label":"definitionResult"}',
            '{"id":"e3","type":"edge","label":"textDocument/definition","outV":"s","inV":"dr"}',
            '{"id":"e4","type":"edge","label":"item","outV":"dr","inVs":["x"],"shard":"d"}',
            '{"id":"e5","type":"edge","label":"contains","outV":"d","inVs":["y"]}',
        ]
        lsif = rust_edges.parse_lsif(lines)
        self.assertEqual(lsif.definitions("y"), ["x"])
        self.assertEqual(lsif.range_doc["x"], "d")
        self.assertEqual(rust_edges.uri_to_rel("file:///r/a%20b.rs", ["/r"]), "a b.rs")
        self.assertIsNone(rust_edges.uri_to_rel("file:///elsewhere/a.rs", ["/r"]))

    def test_item_shaped_definitions_map_and_misses_are_listed(self):
        _, defs, _, sources, keys = load_sample(sample_graph([]))
        mapped, missed = rust_edges.definition_mapping(defs, keys, sources)
        self.assertEqual(mapped, 6)
        self.assertEqual([(m["file"], m["line"], m["name"]) for m in missed],
                         [(SHELF, 10, "glow")])


class SiteTests(unittest.TestCase):
    def classes(self, source):
        idx = rust_sites.Index(source)
        return [(t.text, idx.contexts[k], rust_sites.shape(idx.tokens, k))
                for k, t in enumerate(idx.tokens) if t.kind == "ident"]

    def cls(self, source, name, nth=0):
        return [c for c in self.classes(source) if c[0] == name][nth][1:]

    def test_contexts(self):
        src = ("use a::{b, c::d};\n#[derive(Clone)]\nstruct S;\n"
               "macro_rules! m { ($x:expr) => { f($x) } }\n"
               "fn g() { assert_eq!(h(1), 2); k(); }\n")
        self.assertEqual(self.cls(src, "d"), ("use tree", "other value reference"))
        self.assertEqual(self.cls(src, "Clone")[0], "attribute")
        self.assertEqual(self.cls(src, "f")[0], "macro_rules body")
        self.assertEqual(self.cls(src, "h"), ("macro argument", "path call"))
        self.assertEqual(self.cls(src, "k"), ("plain", "path call"))
        self.assertEqual(self.cls(src, "assert_eq"), ("plain", "macro name"))

    def test_shapes(self):
        src = ("fn g(x: Lamp) -> Vec<Shade> { x.glow(); x.wick; let y = Lamp { a: 1 }; "
               "m::f::<u8>(); Lamp::new(); if Z > 1 {} }\n")
        self.assertEqual(self.cls(src, "Lamp", 0)[1], "type position")
        self.assertEqual(self.cls(src, "Shade")[1], "type position")
        self.assertEqual(self.cls(src, "glow")[1], "method call")
        self.assertEqual(self.cls(src, "wick")[1], "other value reference")
        self.assertEqual(self.cls(src, "Lamp", 1)[1], "struct literal")
        self.assertEqual(self.cls(src, "m")[1], "path qualifier")
        self.assertEqual(self.cls(src, "f")[1], "path call")
        self.assertEqual(self.cls(src, "Lamp", 2)[1], "path qualifier")
        self.assertEqual(self.cls(src, "new")[1], "path call")
        self.assertEqual(self.cls(src, "Z")[1], "other value reference")
        refs = "fn h(s: &str) { k(&rel, *count); }\n"
        self.assertEqual(self.cls(refs, "str")[1], "type position")
        self.assertEqual(self.cls(refs, "rel")[1], "other value reference")
        self.assertEqual(self.cls(refs, "count")[1], "other value reference")

    def test_lexer_skips_literals_and_nested_comments(self):
        src = ('let s = r#"{ f( }"#; let c = \'{\'; fn l<\'a>(x: &\'a str) {}\n'
               "/* outer /* inner f( */ still g( */ h();\n")
        names = [c[0] for c in self.classes(src)]
        self.assertNotIn("f", names)
        self.assertNotIn("g", names)
        self.assertIn("h", names)
        idx = rust_sites.Index(src)
        self.assertEqual([t.text for t in idx.tokens if t.kind == "lifetime"], ["'a", "'a"])

    def test_operators_are_their_own_shape(self):
        idx = rust_sites.Index("fn g() { if a == b[0] { h()?; } }\n")
        eq = [t for t in idx.tokens if t.text == "=="][0]
        self.assertEqual(idx.site(eq.line, eq.col), ("==", "plain", "operator"))
        q = [t for t in idx.tokens if t.text == "?"][0]
        self.assertEqual(idx.site(q.line, q.col)[2], "operator")
        zero = [t for t in idx.tokens if t.text == "0"][0]
        self.assertEqual(idx.site(zero.line, zero.col)[2], "literal")
        self.assertEqual(idx.site(eq.line, eq.col - 1), ("", "plain", "whitespace"))

    def test_columns_count_utf16_units(self):
        idx = rust_sites.Index('let s = "\U0001F600é"; f();\n')
        f = [t for t in idx.tokens if t.text == "f"][0]
        self.assertEqual((f.line, f.col), (0, 15))
        self.assertEqual(idx.site(0, 15), ("f", "plain", "path call"))

    def test_test_scan_and_mods(self):
        src = ("mod a;\n#[cfg(test)]\nmod tests {\n    mod b;\n    #[test]\n"
               "    #[should_panic]\n    fn t1() {\n        #[test]\n        fn inner() {}\n"
               "    }\n    #[tokio::test]\n    async fn t2() {}\n    #[test_case]\n"
               "    fn not_one() {}\n}\nmacro_rules! m { () => { #[test] fn gen() {} } }\n")
        tests, mods = rust_sites.scan_items(src)
        self.assertEqual([(t["name"], t["nameLine"], t["startLine"], t["endLine"], t["modPath"])
                          for t in tests],
                         [("t1", 7, 5, 10, ["tests"]), ("t2", 12, 11, 12, ["tests"])])
        self.assertEqual([(m["name"], m["modPath"], m["inline"]) for m in mods],
                         [("a", [], False), ("tests", [], True), ("b", ["tests"], False)])


class ScoreTests(unittest.TestCase):
    def test_precision_recall_and_multiset_matching(self):
        edges = GOOD + WRONG
        refs, _, universe, _, _ = load_sample(sample_graph(edges))
        scores = rust_edges.call_scores(refs, edges)
        self.assertEqual((scores["population"], scores["recalled"]), (6, 5))
        self.assertEqual((scores["edges"], scores["matched"]), (7, 5))
        self.assertEqual((scores["recall"], scores["precision"]), (0.8333, 0.7143))

    def test_verdicts(self):
        graph = sample_graph(GOOD + WRONG)
        refs, _, _, _, _ = load_sample(graph)
        defs = {x["id"]: x for x in graph["defs"]}
        judged = rust_edges.verdicts(GOOD + WRONG, refs, defs)
        self.assertEqual([j["verdict"] for j in judged],
                         ["match"] * 5 + ["unconfirmed", "wrong-target"])
        self.assertEqual(judged[-1]["oracleTargets"], ["src/shelf.rs#dust"])
        amb = {"kind": "ambiguous", "origin": "uses-member", "from_file": LIB, "from_line": 4,
               "raw": "shelf::dust", "candidates": [{"id": "src/shelf.rs#dust", "file": SHELF}],
               "candidate_count": 2}
        self.assertTrue(rust_edges.verdicts([amb], refs, defs)[0]["oracleAmongCandidates"])

    def test_test_pairs(self):
        edges = GOOD + WRONG
        graph = sample_graph(edges)
        refs, _, universe, sources, keys = load_sample(graph)
        o_tests = rust_edges.oracle_tests(universe, sources, keys)
        self.assertEqual([t["id"] for t in o_tests], ["src/lib.rs#tests::polish_adds_one"])
        scores = rust_edges.test_scores(refs, edges, o_tests,
                                        rust_edges.devscout_tests(graph, universe))
        self.assertEqual((scores["oraclePairs"], scores["devscoutPairs"], scores["matched"]),
                         (3, 2, 2))
        self.assertEqual(scores["missedPairs"],
                         [["src/lib.rs#tests::polish_adds_one", "src/lib.rs#tests::dust_twice"]])

    def test_compiler_list_against_the_scan(self):
        listed = rust_edges.parse_test_list(
            "   Compiling sample v0.1.0\n     Running unittests src/lib.rs (target/x)\n"
            "tests::polish_adds_one: test\ntests::made_by_macro: test\n\n"
            "2 tests, 0 benchmarks\n")
        graph = sample_graph([])
        _, _, universe, sources, keys = load_sample(graph)
        o_tests = rust_edges.oracle_tests(universe, sources, keys)
        mods = {rel: rust_sites.scan_items(sources.index(rel).source)[1] for rel in universe}
        out = rust_edges.compare_test_lists(
            listed, o_tests, mods, lambda p: os.path.isfile(os.path.join(SAMPLE, p)))
        self.assertEqual(out[LIB]["compilerOnly"], ["tests::made_by_macro"])
        self.assertEqual(out[LIB]["scanOnly"], [])

    def test_confirm_rows(self):
        graph = sample_graph([])
        refs, _, universe, sources, keys = load_sample(graph)
        o_tests = rust_edges.oracle_tests(universe, sources, keys)
        answer = {"id": "src/shelf.rs#dust", "inbound": {"uses-member": {"rows": [
            {"file": LIB, "line": 4}, {"file": LIB, "line": 3}]}}}
        self.assertEqual([r["confirmed"] for r in rust_edges.confirm_rows(answer, refs, o_tests)],
                         [True, False])
        tests = {"symbol": "src/shelf.rs#dust", "rows": [{"file": LIB, "lines": [13, 17]}]}
        self.assertEqual([r["confirmed"] for r in rust_edges.confirm_rows(tests, refs, o_tests)],
                         [True])


def test_names(source):
    return [t["name"] for t in rust_sites.scan_items(source)[0]]


def mod_facts(source):
    return [(m["name"], m["modPath"], m["inline"], m["path"], m["unread"])
            for m in rust_sites.scan_items(source)[1]]


def place(files, roots):
    mods = {rel: rust_sites.scan_items(src)[1] for rel, src in files.items()}
    return rust_edges.module_paths(roots, mods, lambda p: p in files)


class CfgAttrTestScanTests(unittest.TestCase):
    def test_a_test_attribute_carried_by_cfg_attr_marks_a_test(self):
        src = ("#[cfg_attr(test, test)]\nfn a() {}\n"
               "#[cfg_attr(any(test, doc), test)]\nfn b() {}\n"
               "#[cfg_attr(test, tokio::test)]\nasync fn c() {}\n"
               "#[cfg_attr(test, cfg_attr(all(), test))]\nfn d() {}\n"
               "#[cfg_attr(test, test, ignore)]\nfn e() {}\n"
               "#[cfg_attr(test, ignore, test(x),)]\nfn f() {}\n")
        self.assertEqual(test_names(src), ["a", "b", "c", "d", "e", "f"])

    def test_other_cfg_attr_forms_mark_no_test(self):
        src = ("#[cfg_attr(test, derive(Debug))]\nfn a() {}\n"
               "#[cfg_attr(test, allow(dead_code))]\nfn b() {}\n"
               "#[cfg_attr(test, test_case)]\nfn c() {}\n"
               "#[cfg_attr]\nfn d() {}\n#[cfg_attr(test)]\nfn e() {}\n"
               "#[cfg_attr(test, )]\nfn f() {}\n#[cfg_attr = \"x\"]\nfn g() {}\n"
               "#[cfg_attr(test test)]\nfn h() {}\n#[cfg_attr(test, test test)]\nfn i() {}\n"
               "#[cfg_attr(test, test = \"x\")]\nfn j() {}\n#[cfg_attr(test,, test)]\nfn k() {}\n"
               "#[test]\nfn plain() {}\n")
        self.assertEqual(test_names(src), ["plain"])


class PathAttributeScanTests(unittest.TestCase):
    def test_the_first_path_attribute_of_a_declaration_is_recorded(self):
        src = ('#[path = "x/real.rs"]\nmod a;\n#[path = r"raw.rs"]\npub mod b;\n'
               '#[path = r#"hash.rs"#]\nmod c;\n#[path = "one.rs"]\n#[path = "two.rs"]\nmod d;\n'
               "#[allow(dead_code)]\nmod e;\n")
        self.assertEqual(mod_facts(src), [
            ("a", [], False, "x/real.rs", False), ("b", [], False, "raw.rs", False),
            ("c", [], False, "hash.rs", False), ("d", [], False, "one.rs", False),
            ("e", [], False, None, False)])

    def test_a_value_that_is_not_a_plain_literal_or_comes_through_cfg_attr_is_unread(self):
        src = ('#[path = "a\\\\b.rs"]\nmod a;\n#[path = b"b.rs"]\nmod b;\n'
               '#[path = concat!("c", ".rs")]\nmod c;\n'
               '#[cfg_attr(unix, path = "u.rs")]\n#[path = "d.rs"]\nmod d;\n'
               '#[path = "e.rs"]\n#[cfg_attr(unix, path = "u.rs")]\nmod e;\n')
        self.assertEqual([(m[0], m[3], m[4]) for m in mod_facts(src)], [
            ("a", None, True), ("b", None, True), ("c", None, True), ("d", None, True),
            ("e", "e.rs", False)])

    def test_children_of_an_inline_module_with_a_path_attribute_are_unread(self):
        src = ('#[path = "elsewhere"]\nmod outer {\n    mod inner {\n        mod deep;\n    }\n'
               '    #[path = "x.rs"]\n    mod near;\n}\n'
               '#[cfg_attr(unix, path = "p")]\nmod other {\n    mod c;\n}\n'
               "mod plain {\n    mod kid;\n}\n")
        self.assertEqual(mod_facts(src), [
            ("outer", [], True, None, False), ("inner", ["outer"], True, None, False),
            ("deep", ["outer", "inner"], False, None, True),
            ("near", ["outer"], False, None, True),
            ("other", [], True, None, False), ("c", ["other"], False, None, True),
            ("plain", [], True, None, False), ("kid", ["plain"], False, None, False)])


class PathPlacementTests(unittest.TestCase):
    def test_a_file_level_path_skips_the_default_file(self):
        files = {"src/lib.rs": '#[path = "real.rs"]\nmod alias;\n',
                 "src/real.rs": "", "src/alias.rs": ""}
        placed = place(files, ["src/lib.rs"])
        self.assertEqual(placed["src/real.rs"], ("src/lib.rs", ["alias"]))
        self.assertNotIn("src/alias.rs", placed)

    def test_dot_dot_reaches_a_sibling_directory_and_a_non_mod_rs_file_uses_its_directory(self):
        files = {"src/lib.rs": "mod shelf;\n",
                 "src/shelf.rs": '#[path = "../gear/cog.rs"]\nmod cog;\n',
                 "gear/cog.rs": "", "src/shelf/cog.rs": "", "src/gear/cog.rs": ""}
        placed = place(files, ["src/lib.rs"])
        self.assertEqual(placed["gear/cog.rs"], ("src/lib.rs", ["shelf", "cog"]))
        self.assertNotIn("src/shelf/cog.rs", placed)
        self.assertNotIn("src/gear/cog.rs", placed)

    def test_inside_inline_modules_the_default_folder_is_the_base(self):
        files = {"src/lib.rs": 'mod bay {\n    #[path = "in.rs"]\n    mod a;\n}\nmod side;\n',
                 "src/bay/in.rs": "", "src/in.rs": "",
                 "src/side.rs": 'mod bay {\n    #[path = "in.rs"]\n    mod b;\n}\n',
                 "src/side/bay/in.rs": "", "src/bay/b.rs": ""}
        placed = place(files, ["src/lib.rs"])
        self.assertEqual(placed["src/bay/in.rs"], ("src/lib.rs", ["bay", "a"]))
        self.assertEqual(placed["src/side/bay/in.rs"], ("src/lib.rs", ["side", "bay", "b"]))
        self.assertNotIn("src/in.rs", placed)

    def test_a_file_reached_by_path_owns_its_directory_and_the_first_attribute_wins(self):
        files = {"src/lib.rs": '#[path = "deep/hub.rs"]\n#[path = "x.rs"]\nmod hub;\n',
                 "src/deep/hub.rs": "mod spoke;\n", "src/deep/spoke.rs": "",
                 "src/deep/hub/spoke.rs": "", "src/x.rs": ""}
        placed = place(files, ["src/lib.rs"])
        self.assertEqual(placed["src/deep/spoke.rs"], ("src/lib.rs", ["hub", "spoke"]))
        self.assertNotIn("src/deep/hub/spoke.rs", placed)
        self.assertNotIn("src/x.rs", placed)

    def test_unread_absolute_and_root_leaving_paths_reach_nothing(self):
        files = {"lib.rs": ('#[path = "/abs.rs"]\nmod a;\n#[path = "../up.rs"]\nmod b;\n'
                            '#[path = "gone.rs"]\nmod c;\n#[cfg_attr(unix, path = "d.rs")]\nmod d;\n'
                            '#[path = "a.rs/"]\nmod e;\n#[path = "b.rs/."]\nmod f;\n'),
                 "a.rs": "", "b.rs": "", "c.rs": "", "d.rs": ""}
        self.assertEqual(sorted(place(files, ["lib.rs"])), ["lib.rs"])

    def test_a_nested_target_root_places_its_sibling(self):
        files = {"tests/flow/main.rs": "mod part;\n", "tests/flow/part.rs": "",
                 "tests/part.rs": ""}
        placed = place(files, ["tests/flow/main.rs"])
        self.assertEqual(placed["tests/flow/part.rs"], ("tests/flow/main.rs", ["part"]))
        self.assertNotIn("tests/part.rs", placed)


class MissedCallTests(unittest.TestCase):
    def test_every_population_reference_not_fully_recalled_is_listed(self):
        edges = GOOD + WRONG
        refs, _, _, _, _ = load_sample(sample_graph(edges))
        self.assertEqual(rust_edges.call_scores(refs, edges)["missed"],
                         [[LIB, 13, "src/lib.rs#tests::dust_twice"]])
        recalled = GOOD + [call(LIB, 13, "src/lib.rs#tests::dust_twice", "dust_twice")]
        self.assertEqual(rust_edges.call_scores(refs, recalled)["missed"], [])


class RunGuardTests(unittest.TestCase):
    def test_scratch_inside_a_git_tree_is_refused(self):
        base = tempfile.mkdtemp()
        try:
            os.makedirs(os.path.join(base, ".git"))
            inner = os.path.join(base, "deep", "scratch")
            os.makedirs(inner)
            self.assertTrue(rust_edges.inside_git_tree(inner))
            with self.assertRaises(SystemExit):
                rust_edges.main(["run", "--repo", base, "--commit", "HEAD", "--devscout", "x",
                                 "--label", "t", "--scratch", inner])
        finally:
            shutil.rmtree(base)


if __name__ == "__main__":
    unittest.main()
