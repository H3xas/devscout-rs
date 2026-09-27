//! Integration tests for the TypeScript edge kinds on the query verbs: `import`,
//! `call`, `jsx-use` and `dispatch` edges read by `refs`, `read`, `tests` and
//! `impact`, and the definitions an `import` edge binds.
//!
//! Fixture: `fixtures/ts-query-edges` (its README lists every reference form).
//!
//! Every test runs the compiled binary as a subprocess against a fresh copy of
//! the fixture, with HOME/SCOUT_REGISTRY/SCOUT_CONTENT_DB pointed at a fresh temp
//! dir -- the same isolation rule and per-file helpers as the other `cli_*`
//! tests, since each tests/*.rs file compiles as its own binary.

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
        "scout-ts-query-edges-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ts-query-edges")
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

const CART: &str = "src/model/cart.ts";
const MODEL_BARREL: &str = "src/model/index.ts";
const STORE: &str = "src/store/cartStore.ts";
const ACTIONS: &str = "src/actions/cartActions.ts";
const USE_CART: &str = "src/hooks/useCart.ts";
const BADGE: &str = "src/components/CartBadge.tsx";
const PAGE: &str = "src/pages/CartPage.tsx";
const APP: &str = "src/app/App.tsx";
const CHECKOUT: &str = "src/services/checkout.ts";
const AUDIT: &str = "src/services/audit.ts";
const POLYFILLS: &str = "src/setup/polyfills.ts";
const PUBLIC: &str = "src/public/index.ts";
const LEGACY: &str = "src/legacy/cartLegacy.js";
const REFILL: &str = "src/bulk/refill.ts";

struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    home: PathBuf,
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
        let fx = Fixture { base, repo, home };
        fx.expect_ok(&["init", "--no-hooks", "--no-map"]);
        fx.expect_ok(&["map", "."]);
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

    fn graph_path(&self) -> PathBuf {
        self.repo.join(".scout/graph/graph.json")
    }

    fn graph(&self) -> Value {
        let text = fs::read_to_string(self.graph_path()).expect("read graph.json");
        serde_json::from_str(&text).expect("graph.json is valid JSON")
    }
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

#[test]
fn the_fixture_writes_every_ts_edge_kind_on_its_expected_lines() {
    let fx = Fixture::build("graph");
    let graph = fx.graph();
    let expected = [
        ("jsx-use", PAGE, 10, BADGE),
        ("jsx-use", APP, 4, PAGE),
        ("call", BADGE, 4, USE_CART),
        ("call", USE_CART, 5, STORE),
        ("call", PAGE, 8, CART),
        ("call", CHECKOUT, 5, ACTIONS),
        ("call", CHECKOUT, 6, STORE),
        ("call", LEGACY, 4, ACTIONS),
        ("dispatch", PAGE, 7, ACTIONS),
        ("dispatch", REFILL, 24, ACTIONS),
        ("import", PAGE, 1, BADGE),
        ("import", PAGE, 3, CART),
        ("import", PAGE, 4, CART),
        ("import", USE_CART, 2, MODEL_BARREL),
        ("import", USE_CART, 2, CART),
        ("import", PUBLIC, 1, STORE),
        ("import", PUBLIC, 2, MODEL_BARREL),
        ("import", CHECKOUT, 1, ACTIONS),
        ("import", AUDIT, 1, ACTIONS),
        ("import", POLYFILLS, 1, STORE),
        ("import", LEGACY, 1, ACTIONS),
    ];
    for (kind, from, line, to) in expected {
        assert!(
            has_edge(&graph, kind, from, line, to),
            "no {kind} edge {from}:{line} -> {to} in {:#?}",
            edge_sites(&graph)
        );
    }
    let by_kind = &graph["stats"]["edges_by_kind"];
    assert_eq!(
        (
            by_kind["import"].as_u64(),
            by_kind["call"].as_u64(),
            by_kind["jsx-use"].as_u64(),
            by_kind["dispatch"].as_u64()
        ),
        (Some(26), Some(32), Some(4), Some(13)),
        "edges_by_kind: {by_kind:#?}"
    );
    let ts = &graph["stats"]["ts"];
    assert_eq!(
        (
            ts["external_import_count"].as_u64(),
            ts["unresolved_ref_count"].as_u64()
        ),
        (Some(0), Some(0)),
        "stats.ts: {ts:#?}"
    );
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

fn id(file: &str, name: &str) -> String {
    format!("{file}#{name}")
}

#[test]
fn named_default_aliased_type_only_and_required_imports_bind_their_definitions() {
    let fx = Fixture::build("bound-named");
    let graph = fx.graph();
    let cases: Vec<(&str, u64, &str, Vec<String>)> = vec![
        (PAGE, 1, BADGE, vec![id(BADGE, "CartBadge")]),
        (PAGE, 2, ACTIONS, vec![id(ACTIONS, "loadCart")]),
        (PAGE, 3, CART, vec![id(CART, "emptyCart")]),
        (
            PAGE,
            4,
            CART,
            vec![id(CART, "CartId"), id(CART, "CartItem")],
        ),
        (USE_CART, 1, STORE, vec![id(STORE, "CartStore")]),
        (CHECKOUT, 2, STORE, vec![id(STORE, "CartStore")]),
        (LEGACY, 1, ACTIONS, vec![id(ACTIONS, "loadCart")]),
        (APP, 1, PAGE, vec![id(PAGE, "CartPage")]),
    ];
    for (from, line, to, expected) in cases {
        assert_eq!(
            bound_defs(&graph, from, line, to),
            expected,
            "bound_defs of {from}:{line} -> {to}"
        );
    }
}

#[test]
fn a_barrel_import_binds_its_definition_on_the_edge_to_the_declaring_file() {
    let fx = Fixture::build("bound-barrel");
    let graph = fx.graph();
    assert_eq!(
        bound_defs(&graph, USE_CART, 2, CART),
        vec![id(CART, "CartItem")]
    );
    assert!(bound_defs(&graph, USE_CART, 2, MODEL_BARREL).is_empty());
}

#[test]
fn a_named_re_export_binds_only_when_it_names_the_declaring_file() {
    let fx = Fixture::build("bound-reexport");
    let graph = fx.graph();
    assert_eq!(
        bound_defs(&graph, PUBLIC, 1, STORE),
        vec![id(STORE, "CartStore")]
    );
    assert!(bound_defs(&graph, PUBLIC, 2, MODEL_BARREL).is_empty());
    assert!(bound_defs(&graph, MODEL_BARREL, 1, CART).is_empty());
}

#[test]
fn namespace_and_side_effect_imports_bind_no_definition() {
    let fx = Fixture::build("bound-none");
    let graph = fx.graph();
    for (from, line, to) in [
        (CHECKOUT, 1, ACTIONS),
        (AUDIT, 1, ACTIONS),
        (POLYFILLS, 1, STORE),
    ] {
        assert!(
            bound_defs(&graph, from, line, to).is_empty(),
            "{from}:{line} -> {to} binds something"
        );
    }
    let text = fs::read_to_string(fx.graph_path()).expect("read graph.json");
    assert!(
        !text.contains("\"bound_defs\":[]"),
        "an empty list is written"
    );
}

#[test]
fn every_bound_definition_is_declared_in_its_edges_to_file_once() {
    let fx = Fixture::build("bound-property");
    let graph = fx.graph();
    let def_file: std::collections::HashMap<&str, &str> = graph["defs"]
        .as_array()
        .expect("defs array")
        .iter()
        .map(|d| (d["id"].as_str().unwrap(), d["file"].as_str().unwrap()))
        .collect();
    let mut bound = 0;
    for e in graph["edges"].as_array().expect("edges array") {
        let Some(defs) = e.get("bound_defs").and_then(Value::as_array) else {
            continue;
        };
        assert!(!defs.is_empty(), "an empty bound_defs is written: {e}");
        let mut seen = std::collections::HashSet::new();
        for d in defs {
            let d = d.as_str().expect("bound def is a string");
            assert_eq!(def_file.get(d), e["to_file"].as_str().as_ref(), "{e}");
            assert!(seen.insert(d), "{d} listed twice on {e}");
            bound += 1;
        }
    }
    assert!(bound > 0, "no edge binds a definition");
}

#[test]
fn a_previous_generation_cache_is_rebuilt_with_bound_definitions() {
    let fx = Fixture::build("bound-rebuild");
    let graph_dir = fx.repo.join(".scout/graph");
    let current = fs::read_dir(&graph_dir)
        .expect("read graph dir")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .find_map(|name| {
            name.strip_prefix("fragments-v")?
                .strip_suffix(".json")?
                .parse::<u32>()
                .ok()
        })
        .expect("a fragments-vN.json cache");
    let previous = current - 1;
    for stem in ["fragments", "fragments-index"] {
        fs::rename(
            graph_dir.join(format!("{stem}-v{current}.json")),
            graph_dir.join(format!("{stem}-v{previous}.json")),
        )
        .expect("plant the previous generation");
    }
    let mut stale = fx.graph();
    for e in stale["edges"].as_array_mut().expect("edges array") {
        e.as_object_mut().expect("edge object").remove("bound_defs");
    }
    fs::write(fx.graph_path(), serde_json::to_vec(&stale).unwrap()).expect("write stale graph");

    fx.expect_ok(&["map", "."]);

    let rebuilt = fx.graph();
    assert_eq!(
        bound_defs(&rebuilt, PAGE, 2, ACTIONS),
        vec![id(ACTIONS, "loadCart")]
    );
    assert!(graph_dir
        .join(format!("fragments-v{current}.json"))
        .exists());
    assert!(!graph_dir
        .join(format!("fragments-v{previous}.json"))
        .exists());
}
