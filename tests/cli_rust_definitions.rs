//! Integration tests for Rust definitions: `find` and `read` over
//! `fixtures/rust-definitions/`, the purposes `map` records for Rust files,
//! incremental reuse, and a tree with no Cargo manifest.

// Every test goes through the compiled binary. Every spawned process gets
// HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB pointed at a scratch directory
// outside the mapped root, so no real settings, registry or content store is
// read or written. The fixture tree is copied, never mapped in place.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const CENSUS_MIXED: &str = "languages: 1 .cs (fully supported); 1 .ts (indexed and graphed, narrower edge coverage); 2 .rs (indexed and graphed, path-resolved edges); 1 .md (present, not indexed)";

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "scout-cli-rust-{prefix}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-definitions/src")
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

    fn with_rust_fixture(prefix: &str) -> Fixture {
        let fx = Fixture::empty(prefix);
        copy_tree(&fixture_src(), &fx.root.join("src"));
        let init = fx.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fx
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

    fn scout(&self, rel: &str) -> PathBuf {
        self.root.join(".scout").join(rel)
    }

    fn json(&self, rel: &str) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.scout(rel)).expect("read artifact"))
            .expect("artifact is json")
    }

    fn source_lines(&self, file: &str) -> Vec<String> {
        fs::read_to_string(self.root.join(file))
            .expect("read source")
            .split('\n')
            .map(str::to_string)
            .collect()
    }
}

// One recorded item: its unique name, kind, file, id, span start, name line,
// span end, and a `::` path that reads it when it has a module or owner.
struct Item {
    name: &'static str,
    kind: &'static str,
    file: &'static str,
    id: &'static str,
    line: usize,
    name_line: usize,
    end_line: usize,
    path: Option<&'static str>,
}

const ITEMS: &[Item] = &[
    Item {
        name: "cabinet",
        kind: "mod",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet",
        line: 3,
        name_line: 4,
        end_line: 64,
        path: None,
    },
    Item {
        name: "fittings",
        kind: "mod",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::fittings",
        line: 50,
        name_line: 50,
        end_line: 50,
        path: Some("cabinet::fittings"),
    },
    Item {
        name: "Widget",
        kind: "struct",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Widget",
        line: 7,
        name_line: 10,
        end_line: 12,
        path: None,
    },
    Item {
        name: "Hinge",
        kind: "struct",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::Hinge",
        line: 6,
        name_line: 7,
        end_line: 9,
        path: Some("cabinet::Hinge"),
    },
    Item {
        name: "Swing",
        kind: "enum",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::Swing",
        line: 11,
        name_line: 12,
        end_line: 15,
        path: Some("cabinet::Swing"),
    },
    Item {
        name: "Bits",
        kind: "union",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Bits",
        line: 20,
        name_line: 22,
        end_line: 25,
        path: None,
    },
    Item {
        name: "Render",
        kind: "trait",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Render",
        line: 28,
        name_line: 29,
        end_line: 40,
        path: None,
    },
    Item {
        name: "polish",
        kind: "fn",
        file: "src/shelf.rs",
        id: "src/shelf.rs#polish",
        line: 45,
        name_line: 46,
        end_line: 53,
        path: None,
    },
    Item {
        name: "assemble",
        kind: "fn",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::assemble",
        line: 32,
        name_line: 33,
        end_line: 35,
        path: Some("cabinet::assemble"),
    },
    Item {
        name: "oil",
        kind: "fn",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::Hinge::oil",
        line: 53,
        name_line: 54,
        end_line: 56,
        path: Some("Hinge::oil"),
    },
    Item {
        name: "outline",
        kind: "fn",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Render::outline",
        line: 36,
        name_line: 37,
        end_line: 39,
        path: Some("Render::outline"),
    },
    Item {
        name: "shelf_count",
        kind: "fn",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#shelf_count",
        line: 67,
        name_line: 68,
        end_line: 68,
        path: None,
    },
    Item {
        name: "match",
        kind: "fn",
        file: "src/shelf.rs",
        id: "src/shelf.rs#match",
        line: 120,
        name_line: 120,
        end_line: 122,
        path: None,
    },
    Item {
        name: "SHELF_DEPTH",
        kind: "const",
        file: "src/shelf.rs",
        id: "src/shelf.rs#SHELF_DEPTH",
        line: 55,
        name_line: 56,
        end_line: 56,
        path: None,
    },
    Item {
        name: "MAX_SPINE",
        kind: "const",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Widget::MAX_SPINE",
        line: 72,
        name_line: 73,
        end_line: 73,
        path: Some("Widget::MAX_SPINE"),
    },
    Item {
        name: "SHELF_LABEL",
        kind: "static",
        file: "src/shelf.rs",
        id: "src/shelf.rs#SHELF_LABEL",
        line: 58,
        name_line: 59,
        end_line: 59,
        path: None,
    },
    Item {
        name: "CABINET_MAKER",
        kind: "static",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::CABINET_MAKER",
        line: 40,
        name_line: 41,
        end_line: 41,
        path: Some("cabinet::CABINET_MAKER"),
    },
    Item {
        name: "Shade",
        kind: "type",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Shade",
        line: 42,
        name_line: 43,
        end_line: 43,
        path: None,
    },
    Item {
        name: "ShelfHandle",
        kind: "type",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#ShelfHandle",
        line: 71,
        name_line: 72,
        end_line: 72,
        path: None,
    },
    Item {
        name: "stack_of",
        kind: "macro_rules",
        file: "src/shelf.rs",
        id: "src/shelf.rs#stack_of",
        line: 61,
        name_line: 62,
        end_line: 67,
        path: None,
    },
    Item {
        name: "twice",
        kind: "macro_rules",
        file: "src/cabinet.rs",
        id: "src/cabinet.rs#cabinet::twice",
        line: 43,
        name_line: 44,
        end_line: 48,
        path: Some("cabinet::twice"),
    },
];

const UNRECORDED: &[&str] = &[
    "tucked_helper",
    "hidden_in_const",
    "made_by_macro",
    "TUCKED_COUNTER",
    "Crimson",
    "spine_width",
    "ShelfMap",
];

// The first line names the item, its kind and the answer's own inbound count;
// the count's VALUE is pinned by the count-line tests, so what this checks is
// that the count sits on that line and that the def line and exactly the span's
// source lines start on the next one -- a span one line too long or too short
// still fails here.
fn assert_read_opens_with(fx: &Fixture, item: &Item, text: &str) {
    let lines = fx.source_lines(item.file);
    let span = format!(
        "def: {}:{}-{}\n{}\ninbound:\n",
        item.file,
        item.line,
        item.end_line,
        lines[item.line - 1..item.end_line].join("\n")
    );
    let mut rest = text.split('\n');
    let first = rest.next().unwrap_or_default();
    assert!(
        first.starts_with(&format!("{}  ({})  ", item.id, item.kind)),
        "read should lead with the item and its kind, then the count: {text}"
    );
    assert!(
        first.contains(" inbound (showing "),
        "the first line states the full inbound count: {text}"
    );
    let tail = rest.collect::<Vec<_>>().join("\n");
    assert!(
        tail.starts_with(&span),
        "read should print the declaration span next: {text}"
    );
}

#[test]
fn find_prints_a_declaration_row_for_every_kind() {
    let fx = Fixture::with_rust_fixture("find");
    for item in ITEMS {
        let source = fx.source_lines(item.file)[item.name_line - 1]
            .trim()
            .to_string();
        assert!(source.contains(item.name), "{} on its own line", item.name);
        let row = format!("{}:{}  {}", item.file, item.name_line, source);
        let out = fx.stdout(&["find", item.name]);
        assert!(
            out.lines().any(|l| l == row),
            "find {} should print {row:?}, got:\n{out}",
            item.name
        );
    }
}

#[test]
fn unrecorded_constructs_have_no_declaration_row() {
    let fx = Fixture::with_rust_fixture("unrecorded");
    for name in UNRECORDED {
        let out = fx.stdout(&["find", name]);
        assert!(
            !out.lines()
                .any(|l| l.starts_with("src/") && l.contains(name)),
            "{name} must not have a declaration row:\n{out}"
        );
    }
    let graph = fx.json("graph/graph.json");
    for def in graph["defs"].as_array().unwrap() {
        let name = def["name"].as_str().unwrap();
        assert!(!UNRECORDED.contains(&name) && name != "_", "def {def}");
        assert_ne!(def["kind"], "impl", "an impl block is not an item: {def}");
    }
    for row in graph["names"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        assert!(!UNRECORDED.contains(&name) && name != "_", "name row {row}");
    }
}

#[test]
fn read_quotes_the_whole_span_by_name_id_and_path() {
    let fx = Fixture::with_rust_fixture("read");
    for item in ITEMS {
        let mut spellings = vec![item.name, item.id];
        spellings.extend(item.path);
        for query in spellings {
            let out = fx.run(&["read", query]);
            assert!(out.status.success(), "read {query}: {out:?}");
            let text = String::from_utf8(out.stdout).unwrap();
            assert_read_opens_with(&fx, item, &text);
        }
    }
}

#[test]
fn shared_paths_are_ambiguous_and_the_suffixed_id_reads_the_second() {
    let fx = Fixture::with_rust_fixture("ambiguous");
    let out = fx.run(&["read", "Gadget::fmt"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text,
        "ambiguous symbol \"Gadget::fmt\" — 2 candidates:\nsrc/shelf.rs#Gadget::fmt  src/shelf.rs:99  fn\nsrc/shelf.rs#Gadget::fmt-1  src/shelf.rs:105  fn\n"
    );

    let second = Item {
        name: "fmt",
        kind: "fn",
        file: "src/shelf.rs",
        id: "src/shelf.rs#Gadget::fmt-1",
        line: 105,
        name_line: 105,
        end_line: 107,
        path: None,
    };
    let read = fx.run(&["read", second.id]);
    assert!(read.status.success(), "{read:?}");
    let text = String::from_utf8(read.stdout).unwrap();
    assert_read_opens_with(&fx, &second, &text);
    assert!(
        text.contains("write!(f, \"Gadget\")"),
        "the Debug body: {text}"
    );

    let twins = fx.run(&["read", "home_shelf-1"]);
    assert_eq!(
        twins.status.code(),
        Some(3),
        "a suffix alone is not a name: {twins:?}"
    );
    let twin = fx.run(&["read", "src/shelf.rs#home_shelf-1"]);
    assert!(twin.status.success(), "{twin:?}");
    let twin = String::from_utf8(twin.stdout).unwrap();
    assert!(
        twin.starts_with("src/shelf.rs#home_shelf-1  (fn)  "),
        "the suffixed id names the second item: {twin}"
    );
    assert!(
        twin.contains("\ndef: src/shelf.rs:115-118\n#[cfg(not(unix))]\n"),
        "the second item's own span: {twin}"
    );
}

#[test]
fn a_function_inside_a_body_is_not_found() {
    let fx = Fixture::with_rust_fixture("not-found");
    for query in ["tucked_helper", "polish::tucked_helper", "hidden_in_const"] {
        let out = fx.run(&["read", query]);
        assert_eq!(out.status.code(), Some(3), "read {query}: {out:?}");
    }
}

#[test]
fn read_json_carries_the_span() {
    let fx = Fixture::with_rust_fixture("json");
    for item in ITEMS {
        let out = fx.run(&["read", "--json", item.id]);
        assert!(out.status.success(), "{out:?}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let lines = fx.source_lines(item.file);
        assert_eq!(v["id"], item.id);
        assert_eq!(v["kind"], item.kind);
        assert_eq!(v["span"]["file"], item.file);
        assert_eq!(v["span"]["startLine"], item.line);
        assert_eq!(v["span"]["endLine"], item.end_line);
        assert_eq!(
            v["span"]["source"],
            lines[item.line - 1..item.end_line].join("\n")
        );
    }
}

#[test]
fn purposes_follow_the_rust_rule() {
    let fx = Fixture::with_rust_fixture("purposes");
    let manifest = fx.json("manifest.json");
    let entry = |rel: &str| {
        let e = &manifest["entries"][rel];
        (
            e["purpose"].as_str().unwrap().to_string(),
            e["source"].as_str().unwrap().to_string(),
        )
    };
    assert_eq!(
        entry("src/shelf.rs"),
        (
            "Shelves, the widgets that sit on them, and how they are polished. — struct Widget | struct Gadget | enum Tone | union Bits | trait Render | type Shade | fn polish | fn home_shelf | fn home_shelf | ...".to_string(),
            "ast".to_string()
        )
    );
    assert_eq!(
        entry("src/cabinet.rs"),
        (
            "Cabinets and the fittings inside them. — mod cabinet".to_string(),
            "ast".to_string()
        )
    );
    assert_eq!(
        entry("src/tones.rs"),
        (
            "fn default_tone | const TONE_COUNT".to_string(),
            "ast".to_string()
        )
    );
    assert_eq!(
        entry("src/prelude.rs"),
        (
            "pub use crate::cabinet::cabinet::Hinge;".to_string(),
            "ast-none".to_string()
        )
    );
}

fn rows_of<'a>(graph: &'a serde_json::Value, key: &str, file: &str) -> Vec<&'a serde_json::Value> {
    graph[key]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["file"] == file)
        .collect()
}

#[test]
fn unchanged_remap_reuses_everything_and_one_edit_moves_only_its_rows() {
    let fx = Fixture::with_rust_fixture("reuse");
    let artifacts = [
        "graph/graph.json",
        "manifest.json",
        "graph/fragments-v35.json",
        "graph/fragments-index-v35.json",
    ];
    let before: Vec<Vec<u8>> = artifacts
        .iter()
        .map(|a| fs::read(fx.scout(a)).expect("artifact present"))
        .collect();

    let again = fx.stdout(&["map", "."]);
    assert!(again.contains("graph unchanged"), "{again}");
    for (artifact, bytes) in artifacts.iter().zip(&before) {
        assert_eq!(&fs::read(fx.scout(artifact)).unwrap(), bytes, "{artifact}");
    }

    let old_graph = fx.json("graph/graph.json");
    let tones = fs::read_to_string(fx.root.join("src/tones.rs")).unwrap();
    let edited = tones.replacen(
        "}\n\n/// How many tones there are.",
        "}\n\n// Tone counts.\n\n/// How many tones there are.",
        1,
    );
    assert_ne!(edited, tones);
    fs::write(fx.root.join("src/tones.rs"), edited).unwrap();

    let rebuilt = fx.stdout(&["map", "."]);
    assert!(rebuilt.contains("graph rebuilt"), "{rebuilt}");
    let new_graph = fx.json("graph/graph.json");
    assert_eq!(new_graph["stats"], old_graph["stats"]);
    for file in ["src/shelf.rs", "src/cabinet.rs"] {
        assert_eq!(
            rows_of(&new_graph, "defs", file),
            rows_of(&old_graph, "defs", file)
        );
        assert_eq!(
            rows_of(&new_graph, "names", file),
            rows_of(&old_graph, "names", file)
        );
    }
    let moved = rows_of(&new_graph, "defs", "src/tones.rs");
    assert_eq!(moved.len(), 2);
    assert_eq!(
        (moved[1]["line"].as_u64(), moved[1]["endLine"].as_u64()),
        (Some(10), Some(11))
    );
    assert_ne!(
        rows_of(&new_graph, "defs", "src/tones.rs"),
        rows_of(&old_graph, "defs", "src/tones.rs")
    );
}

const MIXED_CS: &str = "namespace Shop\n{\n    public class Counter\n    {\n    }\n}\n";
const MIXED_TS: &str = "export function tally(): number {\n  return 1;\n}\n";
const MIXED_RS_A: &str = "/// Counts shelves.\npub fn shelf_total() -> u32 {\n    0\n}\n";
const MIXED_RS_B: &str = "pub struct Ledger;\n\nimpl Ledger {\n    pub fn close(&self) {}\n}\n";

fn mixed_root(prefix: &str, with_rust: bool) -> Fixture {
    let fx = Fixture::empty(prefix);
    fx.write("src/Counter.cs", MIXED_CS);
    fx.write("web/tally.ts", MIXED_TS);
    fx.write("notes.md", "# Notes\n");
    if with_rust {
        fx.write("crates/count/src/lib.rs", MIXED_RS_A);
        fx.write("crates/count/src/ledger.rs", MIXED_RS_B);
        fx.write("target/debug/stray.rs", "pub fn stray() {}\n");
    }
    fx
}

#[test]
fn rust_files_index_in_a_tree_with_no_cargo_manifest() {
    let fx = mixed_root("mixed", true);
    let init = fx.run(&["init", "--no-hooks"]);
    assert!(init.status.success(), "{init:?}");
    let text = String::from_utf8(init.stdout).unwrap();
    assert!(text.lines().any(|l| l == CENSUS_MIXED), "{text}");
    assert!(text.contains("map: mapped 5 files under ."), "{text}");

    let find = fx.stdout(&["find", "shelf_total"]);
    assert!(
        find.lines()
            .any(|l| l == "crates/count/src/lib.rs:2  pub fn shelf_total() -> u32 {"),
        "{find}"
    );
    let read = fx.stdout(&["read", "Ledger::close"]);
    assert!(
        read.starts_with("crates/count/src/ledger.rs#Ledger::close  (fn)  "),
        "{read}"
    );

    let manifest = fx.json("manifest.json");
    let entries = manifest["entries"].as_object().unwrap();
    assert!(
        entries.keys().all(|k| !k.starts_with("target/")),
        "{entries:?}"
    );
    assert!(entries.contains_key("crates/count/src/lib.rs"));
}

#[test]
fn rust_rows_are_a_suffix_and_move_only_their_own_stats() {
    let with = mixed_root("suffix-with", true);
    let without = mixed_root("suffix-without", false);
    for fx in [&with, &without] {
        let init = fx.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "{init:?}");
    }
    let mut g_with = with.json("graph/graph.json");
    let g_without = without.json("graph/graph.json");

    let is_rust = |r: &serde_json::Value| r["file"].as_str().unwrap().ends_with(".rs");
    let is_rust_edge =
        |e: &serde_json::Value| e["from_file"].as_str().unwrap_or("").ends_with(".rs");
    for key in ["defs", "names"] {
        let rows = g_with[key].as_array().unwrap();
        let first_rust = rows.iter().position(is_rust).expect("rust rows present");
        assert!(
            rows[first_rust..].iter().all(is_rust),
            "{key}: rust rows are a suffix"
        );
    }
    let edges = g_with["edges"].as_array().unwrap();
    let first_rust_edge = edges.iter().position(is_rust_edge).unwrap_or(edges.len());
    assert!(
        edges[first_rust_edge..].iter().all(is_rust_edge),
        "edges: rust edges are a suffix"
    );
    let rust_defs = g_with["defs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| is_rust(r))
        .count();
    assert_eq!(rust_defs, 3);

    for key in ["defs", "names"] {
        let kept: Vec<serde_json::Value> = g_with[key]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| !is_rust(r))
            .cloned()
            .collect();
        g_with[key] = serde_json::Value::Array(kept);
    }
    let rust_edges: Vec<serde_json::Value> = g_with["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| is_rust_edge(e))
        .cloned()
        .collect();
    let kept: Vec<serde_json::Value> = g_with["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| !is_rust_edge(e))
        .cloned()
        .collect();
    g_with["edges"] = serde_json::Value::Array(kept);
    assert_eq!(
        rust_edges.len(),
        1,
        "the `impl Ledger` self type: {rust_edges:?}"
    );
    for edge in &rust_edges {
        let kind = edge["kind"].as_str().unwrap();
        let count = &mut g_with["stats"]["edges_by_kind"][kind];
        *count = serde_json::Value::from(count.as_u64().unwrap() - 1);
    }
    let stats_with = g_with["stats"].clone();
    assert_eq!(
        stats_with["file_count"].as_u64().unwrap(),
        g_without["stats"]["file_count"].as_u64().unwrap() + 2
    );
    assert_eq!(
        stats_with["def_count"].as_u64().unwrap(),
        g_without["stats"]["def_count"].as_u64().unwrap() + 3
    );
    g_with["stats"]["file_count"] = g_without["stats"]["file_count"].clone();
    g_with["stats"]["def_count"] = g_without["stats"]["def_count"].clone();
    assert_eq!(g_with, g_without);
}

#[test]
fn every_id_path_is_safe_as_a_shell_argument() {
    let fx = Fixture::with_rust_fixture("shell-safe");
    let forbidden = [
        '<', '>', '[', ']', '(', ')', '{', '}', '\'', '"', '$', '*', '?', '!', ';', '&', '|',
    ];
    let graph = fx.json("graph/graph.json");
    let defs = graph["defs"].as_array().unwrap();
    assert_eq!(defs.len(), 46);
    for def in defs {
        let id = def["id"].as_str().unwrap();
        let path = &id[id.rfind('#').expect("a Rust id carries #") + 1..];
        assert!(
            !path
                .chars()
                .any(|c| c.is_whitespace() || forbidden.contains(&c)),
            "unsafe path in {id}"
        );
    }
}
