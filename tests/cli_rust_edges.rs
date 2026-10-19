//! Integration tests for Rust edges: `refs`, `tests` and `impact` over
//! `fixtures/rust-edges/`, ambiguity, incremental reuse and manifest edits.

// Every test goes through the compiled binary. Every spawned process gets
// HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB pointed at a scratch directory
// outside the mapped root, so no real settings, registry or content store is
// read or written. The fixture tree is copied, never mapped in place.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "scout-cli-rust-edges-{prefix}-{}-{n}",
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
    fn empty(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&home).expect("create home");
        Fixture { root, home }
    }

    fn with_package(prefix: &str) -> Fixture {
        let fx = Fixture::empty(prefix);
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-edges");
        copy_tree(&package, &fx.root);
        let _ = fs::remove_file(fx.root.join("README.md"));
        fx.init();
        fx
    }

    fn init(&self) {
        let init = self.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "init failed: {init:?}");
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        fs::write(path, body).expect("write file");
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

    fn edges(&self) -> Vec<String> {
        let graph: serde_json::Value =
            serde_json::from_slice(&fs::read(self.scout("graph/graph.json")).unwrap()).unwrap();
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.to_string())
            .collect()
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

#[test]
fn refs_lists_calls_type_uses_and_the_implementation() {
    let fx = Fixture::with_package("refs");
    let trim = fx.stdout(&["refs", "trim"]);
    let calls = vec!["src/lamp.rs:10", "src/lib.rs:44", "checks/rim.rs:12"];
    assert_eq!(text_rows(&trim, "uses-member"), calls, "{trim}");
    assert_eq!(
        json_rows(&fx.json_out(&["refs", "trim", "--json"]), "uses-member"),
        calls
    );

    let lamp = fx.stdout(&["refs", "Lamp"]);
    let types = vec![
        "src/glow.rs:6",
        "src/glow.rs:12",
        "src/glow.rs:14",
        "src/glow.rs:18",
        "src/lamp.rs:7",
        "src/lamp.rs:8",
        "src/lamp.rs:9",
        "src/lib.rs:24",
        "src/shelf/mod.rs:5",
    ];
    assert_eq!(text_rows(&lamp, "uses-type"), types, "{lamp}");
    assert_eq!(
        json_rows(&fx.json_out(&["refs", "Lamp", "--json"]), "uses-type"),
        types
    );

    let shine = fx.stdout(&["refs", "Shine"]);
    assert_eq!(
        text_rows(&shine, "implements"),
        vec!["src/glow.rs:6"],
        "{shine}"
    );
    assert_eq!(
        json_rows(&fx.json_out(&["refs", "Shine", "--json"]), "implements"),
        vec!["src/glow.rs:6"]
    );
}

#[test]
fn same_named_functions_and_shadowed_calls_keep_to_their_own_rows() {
    let fx = Fixture::with_package("same-name");
    let crate_light = fx.json_out(&["refs", "src/lib.rs#light", "--json"]);
    assert_eq!(
        json_rows(&crate_light, "uses-member"),
        vec![
            "src/lamp/wick.rs:6",
            "src/lib.rs:43",
            "src/main.rs:2",
            "tests/wiring.rs:5"
        ],
        "the closure named `light` in `dim` gives no row"
    );
    let wick_light = fx.json_out(&["refs", "src/lamp/wick.rs#light", "--json"]);
    assert_eq!(
        json_rows(&wick_light, "uses-member"),
        vec!["src/lamp.rs:16"]
    );
    let beam = fx.json_out(&["refs", "Shine::beam", "--json"]);
    assert_eq!(
        json_rows(&beam, "uses-member"),
        Vec::<String>::new(),
        "a method call is never typed by its receiver"
    );
}

#[test]
fn tests_lists_the_test_files_with_their_carriers_and_not_the_helper() {
    let fx = Fixture::with_package("tests");
    let text = fx.stdout(&["tests", "trim"]);
    assert!(
        text.contains("covered by 2 test file(s), 2 reference(s)"),
        "{text}"
    );
    assert!(text.contains("\nchecks/rim.rs\n  checks/rim.rs#an_empty_shelf_counts_nothing  lines: 12\n  checks/rim.rs#trimming_caps_the_wick  lines: 12\n"), "{text}");
    assert!(
        text.contains("\nsrc/lib.rs\n  src/lib.rs#tests::light_adds_the_buff  lines: 44\n"),
        "{text}"
    );

    let answer = fx.json_out(&["tests", "trim", "--json"]);
    let rows: Vec<(String, Vec<String>, Vec<u64>)> = answer["rows"]
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
                r["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l.as_u64().unwrap())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "checks/rim.rs".to_string(),
                vec![
                    "checks/rim.rs#an_empty_shelf_counts_nothing".to_string(),
                    "checks/rim.rs#trimming_caps_the_wick".to_string()
                ],
                vec![12]
            ),
            (
                "src/lib.rs".to_string(),
                vec!["src/lib.rs#tests::light_adds_the_buff".to_string()],
                vec![44]
            ),
        ]
    );

    let buff = fx.json_out(&["tests", "src/lib.rs#tools::buff", "--json"]);
    let files: Vec<&str> = buff["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert!(
        !files.contains(&"tests/common/mod.rs"),
        "a helper with no test is no test file: {files:?}"
    );
    assert!(!files.contains(&"checks/helpers.rs"), "{files:?}");
}

#[test]
fn impact_counts_the_test_files_it_reaches() {
    let fx = Fixture::with_package("impact");
    let text = fx.stdout(&["impact", "src/shelf/dust.rs"]);
    assert!(text.contains("tests=2"), "{text}");
    let rows: Vec<&str> = text
        .lines()
        .skip(3)
        .take_while(|line| !line.starts_with("next: "))
        .collect();
    assert_eq!(
        rows,
        vec![
            "src/shelf/mod.rs  1  1  sweep",
            "checks/helpers.rs  2  2  Shelf",
            "checks/rim.rs  2  1  count",
            "src/lib.rs  2  2  Shelf, count",
        ],
        "{text}"
    );
    assert_eq!(
        text.lines().last(),
        Some("next: devscout refs 'src/shelf/dust.rs#sweep'"),
        "{text}"
    );
}

#[test]
fn a_name_two_globs_provide_is_ambiguous_and_listed_under_both_candidates() {
    let fx = Fixture::empty("ambiguous");
    fx.write(
        "src/lib.rs",
        "mod a {\n    pub fn glow() {}\n}\nmod b {\n    pub fn glow() {}\n}\nuse a::*;\nuse b::*;\n\npub fn t() {\n    glow();\n}\n",
    );
    fx.init();
    for id in ["src/lib.rs#a::glow", "src/lib.rs#b::glow"] {
        let answer = fx.json_out(&["refs", id, "--json"]);
        let rows = answer["ambiguous"]["inbound"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{answer}");
        assert_eq!(
            (
                rows[0]["file"].as_str(),
                rows[0]["line"].as_u64(),
                rows[0]["origin"].as_str(),
                rows[0]["raw"].as_str(),
                rows[0]["candidateCount"].as_u64()
            ),
            (
                Some("src/lib.rs"),
                Some(11),
                Some("uses-member"),
                Some("glow"),
                Some(2)
            )
        );
        assert!(json_rows(&answer, "uses-member").is_empty());
    }
}

#[test]
fn an_unchanged_map_reuses_everything() {
    let fx = Fixture::with_package("reuse");
    let artifacts = [
        "graph/graph.json",
        "manifest.json",
        "graph/fragments-v44.json",
        "graph/fragments-index-v44.json",
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
fn a_one_file_edit_moves_only_the_edges_from_that_file() {
    let fx = Fixture::with_package("one-edit");
    let before = fx.edges();
    let path = fx.root.join("src/lamp.rs");
    let body = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("\n{body}")).unwrap();
    let map = fx.stdout(&["map", "."]);
    assert!(map.contains("graph rebuilt"), "{map}");
    let after = fx.edges();
    let from_lamp = |e: &String| e.contains("\"from_file\":\"src/lamp.rs\"");
    let other_before: Vec<&String> = before.iter().filter(|e| !from_lamp(e)).collect();
    let other_after: Vec<&String> = after.iter().filter(|e| !from_lamp(e)).collect();
    assert_eq!(other_before, other_after);
    let shifted: Vec<String> = before
        .iter()
        .filter(|e| from_lamp(e))
        .map(|e| {
            let v: serde_json::Value = serde_json::from_str(e).unwrap();
            let line = v["from_line"].as_u64().unwrap();
            e.replace(
                &format!("\"from_line\":{line},"),
                &format!("\"from_line\":{},", line + 1),
            )
        })
        .collect();
    let lamp_after: Vec<String> = after.iter().filter(|e| from_lamp(e)).cloned().collect();
    assert_eq!(shifted, lamp_after);
    assert_eq!(lamp_after.len(), 5);
}

#[test]
fn a_package_rename_rebuilds_and_drops_only_the_crate_name_edges() {
    let fx = Fixture::with_package("rename");
    let before = fx.edges();
    let manifest = fx.root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("name = \"rust-edges\"", "name = \"lamp-works\""),
    )
    .unwrap();
    let map = fx.stdout(&["map", "."]);
    assert!(map.contains("graph rebuilt"), "{map}");
    let after = fx.edges();
    let through_name = [
        ("src/main.rs", 2),
        ("src/main.rs", 3),
        ("tests/wiring.rs", 5),
        ("tests/wiring.rs", 6),
        ("tests/common/mod.rs", 2),
        ("checks/rim.rs", 7),
        ("checks/rim.rs", 12),
        ("checks/helpers.rs", 3),
        ("checks/helpers.rs", 4),
    ];
    let named = |e: &String| {
        let v: serde_json::Value = serde_json::from_str(e).unwrap();
        let site = (
            v["from_file"].as_str().unwrap(),
            v["from_line"].as_u64().unwrap() as usize,
        );
        through_name.contains(&site)
            && !e.contains("tests/common/mod.rs#expected")
            && !e.contains("checks/helpers.rs#empty")
    };
    let kept: Vec<&String> = before.iter().filter(|e| !named(e)).collect();
    assert_eq!(after.iter().collect::<Vec<_>>(), kept);
    assert!(before.len() > after.len());
    let crates = fs::read_to_string(fx.scout("graph/rust-crates.json")).unwrap();
    assert!(crates.contains("\"lib\":\"lamp_works\""), "{crates}");
}

#[test]
fn a_tree_without_a_manifest_writes_no_sidecar() {
    let fx = Fixture::empty("no-manifest");
    fx.write(
        "src/lib.rs",
        "mod shelf;\npub fn t() {\n    shelf::dust();\n}\n",
    );
    fx.write("src/shelf.rs", "pub fn dust() {}\n");
    fx.init();
    assert!(!fx.scout("graph/rust-crates.json").exists());
    let answer = fx.json_out(&["refs", "dust", "--json"]);
    assert_eq!(json_rows(&answer, "uses-member"), vec!["src/lib.rs:3"]);
}

#[test]
fn a_missing_sidecar_rebuilds_and_is_written_again() {
    let fx = Fixture::with_package("sidecar-gone");
    let sidecar = fx.scout("graph/rust-crates.json");
    let bytes = fs::read(&sidecar).unwrap();
    let graph = fs::read(fx.scout("graph/graph.json")).unwrap();
    fs::remove_file(&sidecar).unwrap();
    let map = fx.stdout(&["map", "."]);
    assert!(map.contains("graph rebuilt"), "{map}");
    assert_eq!(fs::read(&sidecar).unwrap(), bytes);
    assert_eq!(fs::read(fx.scout("graph/graph.json")).unwrap(), graph);
}
