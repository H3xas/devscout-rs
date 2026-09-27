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
