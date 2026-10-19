//! CLI coverage for the `.mjs`/`.cjs`/`.mts`/`.cts` module suffixes: the files
//! they let a map discover, the imports they resolve to, the queries a
//! repository mixing every module suffix answers, and the JavaScript
//! test-path rule.
//!
//! Fixture: `fixtures/module-extensions` (its README lists every declaration,
//! reference form and near miss the repository carries).
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

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-module-extensions-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/module-extensions")
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

const APP: &str = "src/app.mjs";
const MONEY: &str = "src/core/money.mjs";
const CORE_INDEX: &str = "src/core/index.mjs";
const FORMAT: &str = "src/util/format.mjs";
const TOTALS: &str = "src/legacy/totals.cjs";
const REPORT: &str = "src/legacy/report.cjs";
const SCHEMA: &str = "src/typed/schema.mts";
const SETTINGS: &str = "src/typed/settings.cts";
const TOOL_MTS: &str = "src/build/tool.mts";
const TOOL_MJS: &str = "src/build/tool.mjs";
const THING_MJS: &str = "src/dual/thing.mjs";
const THING_MTS: &str = "src/dual/thing.mts";
const EMITTED_MTS: &str = "src/dual/emitted.mts";
const FLAGGED_CTS: &str = "src/dual/flagged.cts";
const PICK: &str = "src/dual/pick.mjs";
const BROKEN: &str = "src/broken/missing.mjs";

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
    fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
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

    fn graph_path(&self) -> PathBuf {
        self.repo.join(".scout/graph/graph.json")
    }

    fn graph(&self) -> Value {
        let text = fs::read_to_string(self.graph_path()).expect("read graph.json");
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

fn def_ids(graph: &Value) -> BTreeSet<String> {
    graph["defs"]
        .as_array()
        .expect("defs array")
        .iter()
        .map(|d| d["id"].as_str().expect("def id").to_string())
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

fn counts(table: &Value) -> (u64, u64, usize) {
    (
        table["total"].as_u64().unwrap(),
        table["dropped"].as_u64().unwrap(),
        table["rows"].as_array().unwrap().len(),
    )
}

#[test]
fn module_extensions_ac_1_discovers_every_module_suffix_and_parses_it() {
    let fx = Fixture::build("discovery");
    let graph = fx.graph();
    let defs = def_ids(&graph);

    for (file, name) in [
        (MONEY, "total"),
        (TOTALS, "grandTotal"),
        (SCHEMA, "parseRow"),
        (SETTINGS, "readSettings"),
    ] {
        assert!(
            defs.contains(&id(file, name)),
            "no definition {name} from {file}: {defs:?}"
        );
    }
    for excluded in ["src/skip/upper.MJS", "src/skip/notes.txt"] {
        assert!(
            !defs.iter().any(|d| d.starts_with(excluded)),
            "{excluded} contributed a definition"
        );
    }

    let text = fx.expect_ok(&["find", "readSettings"]);
    assert!(
        text.contains("src/typed/settings.cts:1: function readSettings"),
        "{text}"
    );
    let text = fx.expect_ok(&["find", "grandTotal"]);
    assert!(
        text.contains("src/legacy/totals.cjs:1: function grandTotal"),
        "{text}"
    );
    let text = fx.expect_ok(&["find", "parseRow"]);
    assert!(
        text.contains("src/typed/schema.mts:1: function parseRow"),
        "{text}"
    );

    let remapped = fx.expect_ok(&["map", "."]);
    assert!(
        remapped.contains("0 new, 0 removed") && remapped.contains("graph unchanged"),
        "{remapped}"
    );
    assert_eq!(def_ids(&fx.graph()), defs, "the persisted graph moved");
}

#[test]
fn module_extensions_ac_2_resolves_relative_imports_to_the_new_suffixes() {
    let fx = Fixture::build("imports");
    let graph = fx.graph();

    assert_eq!(bound_defs(&graph, APP, 1, MONEY), vec![id(MONEY, "total")]);
    assert_eq!(bound_defs(&graph, APP, 2, CORE_INDEX), Vec::<String>::new());
    assert_eq!(bound_defs(&graph, APP, 2, MONEY), vec![id(MONEY, "Wallet")]);
    assert_eq!(
        bound_defs(&graph, APP, 3, FORMAT),
        vec![id(FORMAT, "formatAmount")]
    );
    assert_eq!(
        bound_defs(&graph, REPORT, 1, TOTALS),
        vec![id(TOTALS, "grandTotal")]
    );

    assert_eq!(
        bound_defs(&graph, APP, 4, TOOL_MTS),
        vec![id(TOOL_MTS, "tool")]
    );
    assert!(
        !edge_sites(&graph).iter().any(|(_, _, _, t)| t == TOOL_MJS),
        "the built sibling was chosen over the source"
    );

    assert_eq!(
        bound_defs(&graph, PICK, 1, THING_MJS),
        vec![id(THING_MJS, "thing")]
    );
    assert!(
        !edge_sites(&graph).iter().any(|(_, _, _, t)| t == THING_MTS),
        "the compiled-from source was chosen over the literal file"
    );
    assert_eq!(
        bound_defs(&graph, PICK, 2, EMITTED_MTS),
        vec![id(EMITTED_MTS, "emitted")]
    );
    assert_eq!(
        bound_defs(&graph, PICK, 3, FLAGGED_CTS),
        vec![id(FLAGGED_CTS, "flagged")]
    );

    assert!(
        !edge_sites(&graph).iter().any(|(_, f, _, _)| f == BROKEN),
        "an unresolvable specifier produced an edge"
    );
    assert_eq!(graph["stats"]["ts"]["external_import_count"], 3);
    assert_eq!(graph["stats"]["ts"]["unresolved_ref_count"], 4);

    let kinds = &graph["stats"]["edges_by_kind"];
    assert_eq!(
        (kinds["import"].as_u64(), kinds["call"].as_u64()),
        (Some(26), Some(26)),
        "edges_by_kind: {kinds:#?}"
    );
    for (kind, from, line, to) in [
        ("import", APP, 5, SCHEMA),
        ("call", APP, 8, MONEY),
        ("call", APP, 12, MONEY),
        ("call", APP, 16, TOOL_MTS),
        ("call", APP, 20, SCHEMA),
        ("call", PICK, 6, THING_MJS),
        ("call", PICK, 6, EMITTED_MTS),
        ("call", PICK, 6, FLAGGED_CTS),
        ("call", REPORT, 4, TOTALS),
    ] {
        assert!(
            has_edge(&graph, kind, from, line, to),
            "no {kind} edge {from}:{line} -> {to} in {:#?}",
            edge_sites(&graph)
        );
    }
}

#[test]
fn module_extensions_ac_3_answers_every_query_on_the_mixed_module_repository() {
    let fx = Fixture::build("mixed");

    assert!(
        fx.map_stdout.contains("mapped 31 files"),
        "{}",
        fx.map_stdout
    );
    assert!(
        fx.map_stdout.contains("(34 defs, 52 edges)"),
        "{}",
        fx.map_stdout
    );

    let found = fx.expect_ok(&["find", "total"]);
    assert!(
        found.contains("src/core/money.mjs:1: class Wallet; balance | function total"),
        "{found}"
    );

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    let inbound = &refs["inbound"];
    assert_eq!(counts(&inbound["call"]), (17, 0, 17));
    assert_eq!(counts(&inbound["import"]), (16, 0, 16));
    let call_files: Vec<&str> = inbound["call"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert!(call_files.contains(&MONEY), "{call_files:?}");
    assert!(
        call_files.contains(&"src/core/money.test.mjs"),
        "{call_files:?}"
    );
    assert!(
        call_files.contains(&"src/naming/Total.TEST.mjs"),
        "a name the path rule rejects is still a referrer: {call_files:?}"
    );

    let wallet = fx.json(&["refs", "Wallet", "--all", "--json"]);
    let wallet_calls: Vec<(String, u64)> = wallet["inbound"]["call"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        wallet_calls,
        vec![
            (APP.to_string(), 12),
            ("src/core/money.test.mjs".to_string(), 10)
        ]
    );

    let read = fx.json(&["read", "total", "--json"]);
    assert_eq!(read["span"]["file"], MONEY);
    assert_eq!(read["span"]["startLine"], 1);
    assert_eq!(read["span"]["endLine"], 3);
    assert_eq!(
        read["span"]["source"],
        "export function total(items) {\n  return items.length;\n}"
    );
    assert_eq!(counts(&read["inbound"]["call"]), (17, 2, 15));
    assert_eq!(counts(&read["inbound"]["import"]), (16, 1, 15));

    let impact = fx.json(&["impact", MONEY, "--hops", "1", "--json"]);
    assert_eq!(impact["totalAffected"], 16);
    assert_eq!(impact["testsAffected"], 11);
}

/// `(file, lines, via, why)` of every `tests` row.
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

#[test]
fn module_extensions_ac_4_vouches_javascript_test_paths_and_nothing_near_them() {
    let fx = Fixture::build("test-paths");
    let tests = fx.json(&["tests", "total", "--json"]);
    let expected = vec![
        path_row("src/core/__tests__/deep/check.cjs", [1, 4]),
        path_row("src/core/__tests__/deep/check.js", [1, 4]),
        path_row("src/core/__tests__/deep/check.mjs", [1, 4]),
        path_row("src/core/money.spec.cjs", [1, 4]),
        path_row("src/core/money.spec.js", [1, 4]),
        path_row("src/core/money.spec.mjs", [1, 4]),
        path_row("src/core/money.test.cjs", [1, 4]),
        path_row("src/core/money.test.js", [1, 4]),
        path_row("src/core/money.test.jsx", [1, 4]),
        path_row("src/core/money.test.mjs", [3, 6]),
        path_row("src/core/money.test.mts", [1, 4]),
    ];
    assert_eq!(test_rows(&tests), expected);
    assert_eq!(tests["testFileCount"], 11);
    assert_eq!(tests["refCount"], 22);
    assert_eq!(tests["heuristicFileCount"], 0);

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    let referrers: Vec<String> = refs["inbound"]["call"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_string())
        .collect();
    for not_a_test in [
        "src/naming/Total.TEST.mjs",
        "src/core/__tests__x/check.mjs",
        "src/case/__Tests__/check.mjs",
        "tests/helpers.js",
    ] {
        assert!(
            referrers.contains(&not_a_test.to_string()),
            "{not_a_test} must still answer refs: {referrers:?}"
        );
    }

    let impact = fx.json(&["impact", MONEY, "--hops", "1", "--json"]);
    assert_eq!(impact["testsAffected"], 11);
    let affected: Vec<String> = impact["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_string())
        .collect();
    for file in ["src/core/money.test.mjs", "tests/helpers.js"] {
        assert!(affected.contains(&file.to_string()), "{affected:?}");
    }
    assert!(
        !affected.contains(&MONEY.to_string()),
        "a seed is not its own affected file: {affected:?}"
    );

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    assert_eq!(refs["inbound"]["call"]["total"], 17);
    assert_eq!(refs["inbound"]["import"]["total"], 16);
}

#[test]
fn module_extensions_ac_5_reports_the_suffixes_and_discloses_them() {
    let fx = Fixture::build("reporting");
    assert!(
        fx.init_stdout.contains(
            "languages: 5 .cjs, 2 .cts, 4 .js, 1 .jsx, 14 .mjs, 5 .mts \
             (indexed and graphed, narrower edge coverage)"
        ),
        "{}",
        fx.init_stdout
    );
    assert!(
        !fx.init_stdout.contains(".MJS") && !fx.init_stdout.contains(".txt"),
        "{}",
        fx.init_stdout
    );

    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("README");
    let section = readme
        .split("## Language support")
        .nth(1)
        .expect("a Language support section")
        .split("\n## ")
        .next()
        .expect("the section body");
    assert!(
        section.contains(".mjs") && section.contains(".cjs"),
        "the module suffixes are undisclosed: {section}"
    );
    assert!(
        section.contains(".mts") && section.contains(".cts"),
        "the TypeScript module suffixes are undisclosed: {section}"
    );
    assert!(
        section.contains("not measured"),
        "the disclosure claims more than the tool measures: {section}"
    );
}
