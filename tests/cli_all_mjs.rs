//! End-to-end query coverage for a repository whose every source file is
//! `.mjs`, with no TypeScript and no `tsconfig.json`, `jsconfig.json` or
//! `package.json` in the tree.
//!
//! Fixture: `fixtures/all-mjs` (its README lists every declaration, reference
//! form, test file and near miss the repository carries).
//!
//! Each test runs the compiled binary as a subprocess against a fresh copy of
//! the fixture, with `HOME`, `SCOUT_REGISTRY` and `SCOUT_CONTENT_DB` pointed at
//! a fresh temp dir, so every verb after the first reads the persisted graph
//! from a new process rather than a warm one.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!("scout-all-mjs-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/all-mjs")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() && name != "README.md" {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

/// Every copied path, relative to `root`, in sorted order, without the state
/// directory `map` writes under `.scout`.
fn tree_paths(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if entry.file_type().expect("entry file type").is_dir() {
                walk(&path, root, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .expect("path under root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.retain(|path| !path.starts_with(".scout/"));
    out.sort();
    out
}

const APP: &str = "src/app.mjs";
const MONEY: &str = "src/core/money.mjs";
const CORE_INDEX: &str = "src/core/index.mjs";
const FORMAT: &str = "src/util/format.mjs";
const NODE_TEST_SUITE: &str = "src/core/money.test.mjs";
const SPEC_FILE: &str = "src/core/money.spec.mjs";
const PATH_TEST_FILE: &str = "src/core/__tests__/check.mjs";

/// The four files whose names resemble a test path without being one.
const NEAR_MISSES: [&str; 4] = [
    "src/naming/Total.TEST.mjs",
    "src/core/__tests__x/check.mjs",
    "src/case/__Tests__/check.mjs",
    "tests/helpers.mjs",
];

/// The exact copied file set: every source file, and nothing else.
const TREE: [&str; 11] = [
    "src/app.mjs",
    "src/case/__Tests__/check.mjs",
    "src/core/__tests__/check.mjs",
    "src/core/__tests__x/check.mjs",
    "src/core/index.mjs",
    "src/core/money.mjs",
    "src/core/money.spec.mjs",
    "src/core/money.test.mjs",
    "src/naming/Total.TEST.mjs",
    "src/util/format.mjs",
    "tests/helpers.mjs",
];

struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    home: PathBuf,
    init_stdout: String,
    map_stdout: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

impl Fixture {
    fn build() -> Fixture {
        let base = temp_dir();
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root(), &repo);
        let mut fx = Fixture {
            base,
            repo,
            home,
            init_stdout: String::new(),
            map_stdout: String::new(),
        };
        fx.init_stdout = fx.expect_ok(&["init", "--no-hooks", "--no-map"]);
        fx.map_stdout = fx.expect_ok(&["map", "."]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn expect_ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "devscout {args:?} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }

    fn json(&self, args: &[&str]) -> Value {
        let stdout = self.expect_ok(args);
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("devscout {args:?} did not print JSON: {e}\n{stdout}"))
    }

    fn graph(&self) -> Value {
        let text =
            fs::read_to_string(self.repo.join(".scout/graph/graph.json")).expect("read graph.json");
        serde_json::from_str(&text).expect("graph.json is valid JSON")
    }
}

fn id(file: &str, name: &str) -> String {
    format!("{file}#{name}")
}

/// `(kind, from_file, from_line, to_file)` of every edge, in graph order.
fn edge_sites(graph: &Value) -> Vec<(String, String, u64, String)> {
    graph["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap_or_default().to_string(),
                e["from_file"].as_str().unwrap_or_default().to_string(),
                e["from_line"].as_u64().unwrap_or_default(),
                e["to_file"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn has_edge(graph: &Value, kind: &str, from: &str, line: u64, to: &str) -> bool {
    edge_sites(graph)
        .iter()
        .any(|(k, f, l, t)| k == kind && f == from && *l == line && t == to)
}

/// One line per edge, in graph order, carrying the definitions the edge binds:
/// two graphs whose edges sit at the same sites but bind differently are not
/// the same graph.
fn edge_fingerprints(graph: &Value) -> Vec<String> {
    graph["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .map(|e| {
            let bound: Vec<&str> = e
                .get("bound_defs")
                .and_then(Value::as_array)
                .map(|defs| defs.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            format!(
                "{} {}:{} -> {} [{}]",
                e["kind"].as_str().unwrap_or_default(),
                e["from_file"].as_str().unwrap_or_default(),
                e["from_line"].as_u64().unwrap_or_default(),
                e["to_file"].as_str().unwrap_or_default(),
                bound.join(", ")
            )
        })
        .collect()
}

/// The `bound_defs` of the one `import` edge at `from:line -> to`; an edge
/// without the key binds nothing. Panics when no such edge exists.
fn bound_defs(graph: &Value, from: &str, line: u64, to: &str) -> Vec<String> {
    let edge = graph["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .find(|e| {
            e["kind"] == "import"
                && e["from_file"] == from
                && e["from_line"] == line
                && e["to_file"] == to
        })
        .unwrap_or_else(|| panic!("no import edge {from}:{line} -> {to}"));
    edge.get("bound_defs")
        .and_then(Value::as_array)
        .map(|defs| {
            defs.iter()
                .map(|d| d.as_str().expect("bound def is a string").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn def_ids(graph: &Value) -> BTreeSet<String> {
    graph["defs"]
        .as_array()
        .expect("defs array")
        .iter()
        .map(|d| d["id"].as_str().expect("def id").to_string())
        .collect()
}

fn counts(table: &Value) -> (u64, u64, usize) {
    (
        table["total"].as_u64().unwrap(),
        table["dropped"].as_u64().unwrap(),
        table["rows"].as_array().unwrap().len(),
    )
}

/// `(file, line)` of every row in a `refs` table.
fn row_sites(table: &Value) -> Vec<(String, u64)> {
    table["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// The files of the given `tests` rows.
fn test_rows(tests: &Value) -> Vec<(String, Vec<u64>, String, String)> {
    tests["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l.as_u64().unwrap())
                    .collect(),
                r["via"].as_str().unwrap_or_default().to_string(),
                r["why"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn path_row(file: &str, lines: [u64; 2]) -> (String, Vec<u64>, String, String) {
    (
        file.to_string(),
        lines.to_vec(),
        "path".to_string(),
        "test-path".to_string(),
    )
}

/// Every answer the other tests assert: the persisted graph's definitions and
/// binding-carrying edges, then each verb's own output.
fn answers(fx: &Fixture) -> Vec<String> {
    let graph = fx.graph();
    let defs: Vec<String> = def_ids(&graph).into_iter().collect();
    let edges = edge_fingerprints(&graph);
    vec![
        defs.join("\n"),
        edges.join("\n"),
        fx.expect_ok(&["find", "total"]),
        fx.expect_ok(&["find", "Wallet"]),
        fx.expect_ok(&["find", "formatAmount"]),
        fx.expect_ok(&["refs", "total", "--all", "--json"]),
        fx.expect_ok(&["refs", "Wallet", "--all", "--json"]),
        fx.expect_ok(&["read", "total", "--json"]),
        fx.expect_ok(&["impact", MONEY, "--hops", "1", "--json"]),
        fx.expect_ok(&["tests", "total", "--json"]),
    ]
}

fn census(stdout: &str) -> &str {
    stdout
        .lines()
        .find(|line| line.starts_with("languages: "))
        .expect("a languages census line")
}

#[test]
fn all_mjs_ac_1_the_tree_is_pure_mjs() {
    let fx = Fixture::build();

    let paths = tree_paths(&fx.repo);
    if let Some(impure) = paths.iter().find(|p| !p.ends_with(".mjs")) {
        panic!("the copied tree holds a file that is not .mjs: {impure}");
    }
    assert_eq!(paths, TREE, "the fixture's file set moved");

    assert_eq!(
        census(&fx.init_stdout),
        "languages: 11 .mjs (indexed and graphed, narrower edge coverage)"
    );
}

#[test]
fn all_mjs_ac_2_map_reports_the_tree_and_find_locates_its_declarations() {
    let fx = Fixture::build();

    assert!(
        fx.map_stdout.contains(
            "mapped 11 files under . (preserved 0 agent purposes, downgraded 0 changed, \
             11 new, 0 removed, 10 ast signatures)"
        ),
        "{}",
        fx.map_stdout
    );
    assert!(
        fx.map_stdout.contains("(11 defs, 24 edges)"),
        "{}",
        fx.map_stdout
    );

    let graph = fx.graph();
    let expected_defs: BTreeSet<String> = [
        (APP, "summarize"),
        (APP, "makeWallet"),
        ("src/case/__Tests__/check.mjs", "casedSegment"),
        (PATH_TEST_FILE, "pathTotal"),
        ("src/core/__tests__x/check.mjs", "partialSegment"),
        (MONEY, "total"),
        (MONEY, "Wallet"),
        (SPEC_FILE, "checksTotal"),
        (NEAR_MISSES[0], "casedName"),
        (FORMAT, "formatAmount"),
        (NEAR_MISSES[3], "helper"),
    ]
    .into_iter()
    .map(|(file, name)| id(file, name))
    .collect();
    assert_eq!(
        def_ids(&graph),
        expected_defs,
        "the graph's definitions moved"
    );
    assert_eq!(edge_sites(&graph).len(), 24, "the graph's edge count moved");

    assert_eq!(bound_defs(&graph, APP, 1, MONEY), vec![id(MONEY, "total")]);
    assert_eq!(
        bound_defs(&graph, APP, 2, CORE_INDEX),
        Vec::<String>::new(),
        "the barrel binds nothing itself"
    );
    assert_eq!(bound_defs(&graph, APP, 2, MONEY), vec![id(MONEY, "Wallet")]);
    assert_eq!(
        bound_defs(&graph, APP, 3, FORMAT),
        vec![id(FORMAT, "formatAmount")]
    );
    assert!(
        has_edge(&graph, "call", APP, 6, MONEY),
        "the caller's call to total is missing"
    );
    assert!(
        has_edge(&graph, "call", APP, 6, FORMAT),
        "the caller's call to formatAmount is missing"
    );

    let found = fx.expect_ok(&["find", "total"]);
    assert!(
        found.contains("src/core/money.mjs:1: class Wallet; balance | function total"),
        "{found}"
    );
    let found = fx.expect_ok(&["find", "Wallet"]);
    assert!(
        found.contains("src/core/money.mjs:1: class Wallet; balance | function total"),
        "{found}"
    );
    let found = fx.expect_ok(&["find", "formatAmount"]);
    assert!(
        found.contains("src/util/format.mjs:1: function formatAmount"),
        "{found}"
    );
}

#[test]
fn all_mjs_ac_3_refs_and_read_agree_on_the_referrers() {
    let fx = Fixture::build();

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    let inbound = &refs["inbound"];
    assert_eq!(counts(&inbound["call"]), (9, 0, 9));
    assert_eq!(counts(&inbound["import"]), (8, 0, 8));
    assert_eq!(
        row_sites(&inbound["call"]),
        vec![
            (APP.to_string(), 6),
            ("src/case/__Tests__/check.mjs".to_string(), 4),
            (PATH_TEST_FILE.to_string(), 4),
            ("src/core/__tests__x/check.mjs".to_string(), 4),
            (MONEY.to_string(), 11),
            (SPEC_FILE.to_string(), 4),
            (NODE_TEST_SUITE.to_string(), 6),
            (NEAR_MISSES[0].to_string(), 4),
            (NEAR_MISSES[3].to_string(), 4),
        ],
        "the call rows moved"
    );

    let wallet = fx.json(&["refs", "Wallet", "--all", "--json"]);
    assert_eq!(
        row_sites(&wallet["inbound"]["call"]),
        vec![(APP.to_string(), 10), (NODE_TEST_SUITE.to_string(), 10)]
    );

    let read = fx.json(&["read", "total", "--json"]);
    assert_eq!(read["span"]["file"], MONEY);
    assert_eq!(read["span"]["startLine"], 1);
    assert_eq!(read["span"]["endLine"], 3);
    assert_eq!(
        read["span"]["source"],
        "export function total(items) {\n  return items.length;\n}"
    );
    assert_eq!(counts(&read["inbound"]["call"]), (9, 0, 9));
    assert_eq!(counts(&read["inbound"]["import"]), (8, 0, 8));
}

#[test]
fn all_mjs_ac_4_impact_reaches_the_importers_without_the_seed() {
    let fx = Fixture::build();

    let impact = fx.json(&["impact", MONEY, "--hops", "1", "--json"]);
    assert_eq!(impact["totalAffected"], 8);
    assert_eq!(impact["testsAffected"], 3);
    let affected: BTreeSet<String> = impact["rows"]
        .as_array()
        .expect("impact rows")
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_string())
        .collect();
    let expected: BTreeSet<String> = std::iter::once(APP)
        .chain(std::iter::once(SPEC_FILE))
        .chain(std::iter::once(PATH_TEST_FILE))
        .chain(std::iter::once(NODE_TEST_SUITE))
        .chain(NEAR_MISSES)
        .map(str::to_string)
        .collect();
    assert_eq!(affected, expected, "the affected set moved");
    assert!(
        !affected.contains(MONEY),
        "the seed is not one of its own affected files"
    );
}

#[test]
fn all_mjs_ac_5_tests_vouches_the_paths_and_nothing_near_them() {
    let fx = Fixture::build();

    let tests = fx.json(&["tests", "total", "--json"]);
    assert_eq!(
        test_rows(&tests),
        vec![
            path_row(PATH_TEST_FILE, [1, 4]),
            path_row(SPEC_FILE, [1, 4]),
            path_row(NODE_TEST_SUITE, [3, 6]),
        ],
        "the test rows moved"
    );
    assert_eq!(tests["testFileCount"], 3);
    assert_eq!(tests["refCount"], 6);
    assert_eq!(tests["heuristicFileCount"], 0);

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    let referrers = row_sites(&refs["inbound"]["call"]);
    for near_miss in NEAR_MISSES {
        assert!(
            referrers.iter().any(|(file, _)| file == near_miss),
            "{near_miss} must still answer refs: {referrers:?}"
        );
    }
}

#[test]
fn all_mjs_ac_6_a_cached_remap_moves_no_answer() {
    let fx = Fixture::build();
    let before = answers(&fx);

    let remapped = fx.expect_ok(&["map", "."]);
    assert!(
        remapped.contains("0 new, 0 removed") && remapped.contains("graph unchanged"),
        "{remapped}"
    );

    assert_eq!(answers(&fx), before, "an answer moved across the remap");
}
