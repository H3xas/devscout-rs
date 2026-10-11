//! Integration tests for cargo's nested target roots, `cfg_attr` tests and
//! `#[path]` modules over `fixtures/rust-targets/`, including incremental maps.

// Every test goes through the compiled binary. Every spawned process gets
// HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB pointed at a scratch directory
// outside the mapped root, so no real settings, registry or content store is
// read or written. The fixture tree is copied, never mapped in place.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

// The default-rule files a `#[path]` attribute bypasses: rustc never
// compiles them.
const BYPASSED: [&str; 9] = [
    "src/alias.rs",
    "src/hub.rs",
    "src/deep/hub/spoke.rs",
    "src/bay/pier.rs",
    "src/dock.rs",
    "src/reel/cog.rs",
    "src/gear/cog.rs",
    "src/reel/spool/line.rs",
    "src/spool/thread.rs",
];

const NESTED: [&str; 3] = ["tests/flight", "examples/glide", "benches/lift"];

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "scout-cli-rust-targets-{prefix}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create copy dir");
    for entry in fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn with_package(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let fx = Fixture {
            root: base.join("repo"),
            home: base.join("home"),
        };
        fs::create_dir_all(&fx.home).expect("create home");
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-targets");
        copy_tree(&package, &fx.root);
        let _ = fs::remove_file(fx.root.join("README.md"));
        let init = fx.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .stdin(Stdio::null())
            .output()
            .expect("devscout must run")
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8(self.run(args).stdout).expect("utf-8 stdout")
    }

    fn json_out(&self, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.stdout(args)).expect("json answer")
    }

    fn scout(&self, rel: &str) -> PathBuf {
        self.root.join(".scout").join(rel)
    }

    fn graph(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.scout("graph/graph.json")).unwrap()).unwrap()
    }

    // Every edge, serialized with sorted keys.
    fn edges(&self) -> Vec<serde_json::Value> {
        self.graph()["edges"].as_array().unwrap().clone()
    }

    // Whether `graph.json` holds this edge exactly as written there.
    fn has_edge(&self, raw: &str) -> bool {
        fs::read_to_string(self.scout("graph/graph.json"))
            .unwrap()
            .contains(raw)
    }
}

// `file:line` for each row of one inbound table of a `refs --json` answer.
fn json_rows(answer: &serde_json::Value, table: &str) -> Vec<String> {
    answer["inbound"][table]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| format!("{}:{}", r["file"].as_str().unwrap(), r["line"]))
        .collect()
}

// `file:line` for each row a `refs` text answer prints under a kind.
fn text_rows(text: &str, kind: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.split_whitespace().nth(1) == Some(kind))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

// The files of a `tests --json` answer with their test defs.
fn test_rows(answer: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    answer["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["testDefs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| d.as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn nested_targets_call_the_library_by_name_and_their_siblings_resolve() {
    let fx = Fixture::with_package("nested");
    for (target, fly, kite, sibling) in [
        ("tests/flight", 7, 8, "check"),
        ("examples/glide", 4, 5, "trim"),
        ("benches/lift", 4, 5, "pull"),
    ] {
        for want in [
            format!(
                r#"{{"kind":"uses-member","from_file":"{target}/main.rs","from_line":{fly},"to":"src/lib.rs#fly","to_file":"src/lib.rs","member":"fly"}}"#
            ),
            format!(
                r#"{{"kind":"uses-type","from_file":"{target}/main.rs","from_line":{kite},"to":"src/lib.rs#Kite","to_file":"src/lib.rs"}}"#
            ),
            format!(
                r#"{{"kind":"uses-member","from_file":"{target}/main.rs","from_line":{kite},"to":"src/lib.rs#Kite::new","to_file":"src/lib.rs","member":"new"}}"#
            ),
            format!(
                r#"{{"kind":"uses-member","from_file":"{target}/main.rs","from_line":{},"to":"{target}/part.rs#{sibling}","to_file":"{target}/part.rs","member":"{sibling}"}}"#,
                kite + 1
            ),
        ] {
            assert!(fx.has_edge(&want), "{want} missing");
        }
    }
    assert!(fx.has_edge(
        r#"{"kind":"uses-member","from_file":"tests/flight/part.rs","from_line":2,"to":"tests/flight/main.rs#helper","to_file":"tests/flight/main.rs","member":"helper"}"#
    ));

    let nested_calls = vec![
        "benches/lift/main.rs:4",
        "examples/glide/main.rs:4",
        "tests/flight/main.rs:7",
        "tests/flight/part.rs:3",
    ];
    let text = fx.stdout(&["refs", "fly"]);
    let rows = text_rows(&text, "uses-member");
    assert_eq!(rows[5..].to_vec(), nested_calls, "{text}");
    let answer = fx.json_out(&["refs", "fly", "--json"]);
    assert_eq!(
        json_rows(&answer, "uses-member")[5..].to_vec(),
        nested_calls
    );

    let tests = test_rows(&fx.json_out(&["tests", "fly", "--json"]));
    assert!(tests.contains(&(
        "tests/flight/main.rs".to_string(),
        vec!["tests/flight/main.rs#flight_reaches_the_library".to_string()]
    )));
}

#[test]
fn cfg_attr_tests_carry_their_names_and_are_counted() {
    let fx = Fixture::with_package("cfg-attr");
    let graph = fx.graph();
    let defs = graph["defs"].as_array().unwrap();
    let names = [
        "flies_under_test",
        "flies_under_any",
        "flies_nested",
        "flies_ignored",
    ];
    for name in names {
        let id = format!("src/checks.rs#{name}");
        let def = defs.iter().find(|d| d["id"] == id.as_str()).unwrap();
        assert_eq!(def["testMethods"], serde_json::json!([name]), "{id}");
    }
    let plain = defs
        .iter()
        .find(|d| d["id"] == "src/checks.rs#not_a_test")
        .unwrap();
    assert!(plain.get("testMethods").is_none());
    let marked = defs
        .iter()
        .filter(|d| d.get("testMethods").is_some())
        .count();
    assert_eq!(graph["stats"]["test_def_count"], marked);
    assert_eq!(marked, 20);

    let tests = test_rows(&fx.json_out(&["tests", "fly", "--json"]));
    let checks = tests
        .iter()
        .find(|(file, _)| file == "src/checks.rs")
        .expect("src/checks.rs is a test file");
    let want: Vec<String> = names.iter().map(|n| format!("src/checks.rs#{n}")).collect();
    assert_eq!(checks.1, want);
}

#[test]
fn path_modules_reach_the_named_file_and_no_bypassed_default_file() {
    let fx = Fixture::with_package("path");
    for (line, to, member) in [
        (29, "src/real.rs", "f"),
        (30, "src/deep/hub.rs", "center"),
        (31, "src/deep/spoke.rs", "spin"),
        (32, "src/bay/dock.rs", "moor"),
    ] {
        let want = format!(
            r#"{{"kind":"uses-member","from_file":"src/lib.rs","from_line":{line},"to":"{to}#{member}","to_file":"{to}","member":"{member}"}}"#
        );
        assert!(fx.has_edge(&want), "{want} missing");
    }
    for (line, to, member) in [
        (10, "gear/cog.rs", "turn"),
        (11, "src/reel/spool/thread.rs", "wind"),
    ] {
        let want = format!(
            r#"{{"kind":"uses-member","from_file":"src/reel.rs","from_line":{line},"to":"{to}#{member}","to_file":"{to}","member":"{member}"}}"#
        );
        assert!(fx.has_edge(&want), "{want} missing");
    }
    for edge in fx.edges() {
        let from = edge["from_file"].as_str().unwrap();
        if BYPASSED.contains(&from) {
            continue;
        }
        let to_file = edge["to_file"].as_str().unwrap_or("");
        assert!(!BYPASSED.contains(&to_file), "{edge}");
    }
    let alias = fx.json_out(&["refs", "src/alias.rs#f", "--json"]);
    assert_eq!(
        json_rows(&alias, "uses-member"),
        vec!["src/alias.rs:5"],
        "only the uncompiled file's own call"
    );
}

#[test]
fn an_unchanged_map_reuses_everything() {
    let fx = Fixture::with_package("reuse");
    let artifacts = [
        "graph/graph.json",
        "manifest.json",
        "graph/fragments-v32.json",
        "graph/fragments-index-v32.json",
        "graph/rust-crates.json",
    ];
    let before: Vec<Vec<u8>> = artifacts
        .iter()
        .map(|a| fs::read(fx.scout(a)).expect("artifact present"))
        .collect();
    let again = fx.stdout(&["map", "."]);
    assert!(again.contains("graph unchanged"), "{again}");
    for (a, bytes) in artifacts.iter().zip(before) {
        assert_eq!(fs::read(fx.scout(a)).unwrap(), bytes, "{a} changed");
    }
}

#[test]
fn editing_a_path_literal_moves_that_modules_edges_to_the_new_file() {
    let fx = Fixture::with_package("path-edit");
    let lib = fx.root.join("src/lib.rs");
    let body = fs::read_to_string(&lib).unwrap();
    fs::write(
        &lib,
        body.replace("#[path = \"real.rs\"]", "#[path = \"alias.rs\"]"),
    )
    .unwrap();
    let map = fx.stdout(&["map", "."]);
    assert!(map.contains("graph rebuilt"), "{map}");
    assert!(fx.has_edge(
        r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":29,"to":"src/alias.rs#f","to_file":"src/alias.rs","member":"f"}"#
    ));
    for edge in fx.edges() {
        if edge["to_file"] == "src/real.rs" {
            assert_eq!(edge["from_file"], "src/real.rs", "{edge}");
        }
    }
}

#[test]
fn adding_a_nested_test_target_adds_its_root() {
    let fx = Fixture::with_package("add-root");
    let path = fx.root.join("tests/climb/main.rs");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "#[test]\nfn climbs() {\n    rust_targets::fly();\n}\n",
    )
    .unwrap();
    let map = fx.stdout(&["map", "."]);
    assert!(map.contains("graph rebuilt"), "{map}");
    let crates = fs::read_to_string(fx.scout("graph/rust-crates.json")).unwrap();
    assert!(
        crates.contains(r#"{"file":"tests/climb/main.rs","kind":"test"}"#),
        "{crates}"
    );
    for target in NESTED {
        assert!(
            crates.contains(&format!("\"{target}/main.rs\"")),
            "{crates}"
        );
    }
    let answer = fx.json_out(&["refs", "fly", "--json"]);
    assert!(json_rows(&answer, "uses-member").contains(&"tests/climb/main.rs:3".to_string()));
}
