//! Integration tests for the TypeScript edge kinds on the query verbs: `import`,
//! `call`, `jsx-use` and `dispatch` edges read by `refs`, `read`, `tests` and
//! `impact`, and the definitions an `import` edge binds.
//!
//! Fixture: `fixtures/ts-query-edges` (its README lists every reference form).
//!
//! Every test runs the compiled binary as a subprocess against a fresh copy of
//! the fixture, with `HOME`, `SCOUT_REGISTRY` and `SCOUT_CONTENT_DB` pointed at a fresh temp
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
        (Some(26), Some(33), Some(4), Some(13)),
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

const BADGE_SPEC: &str = "src/components/CartBadge.spec.tsx";
const USE_CART_SPEC: &str = "src/hooks/useCart.spec.ts";
const ACTIONS_TEST_TS: &str = "src/actions/cartActions.test.ts";
const ACTIONS_TEST_JS: &str = "src/actions/cartActions.test.js";

impl Fixture {
    fn json(&self, args: &[&str]) -> Value {
        self.json_raw(args).0
    }

    fn json_raw(&self, args: &[&str]) -> (Value, String) {
        let stdout = self.expect_ok(args);
        let value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("devscout {args:?} did not print JSON: {e}\n{stdout}"));
        (value, stdout)
    }
}

/// The keys of the object `section` names in the raw JSON answer, in the
/// order the answer writes them (a parsed `Value` sorts its keys). Every
/// key of that object must be one of `expected`, so the scan below cannot
/// wander past its end.
fn assert_keys(raw: &str, parsed: &Value, section: &str, expected: &[&str]) {
    let mut have: Vec<String> = parsed[section]
        .as_object()
        .unwrap_or_else(|| panic!("no {section} object in {raw}"))
        .keys()
        .cloned()
        .collect();
    have.sort();
    let mut want: Vec<String> = expected.iter().map(|k| (*k).to_string()).collect();
    want.sort();
    assert_eq!(have, want, "{section} keys in {raw}");
    let mut at = raw
        .find(&format!("\"{section}\":{{"))
        .unwrap_or_else(|| panic!("no {section} object in {raw}"));
    for key in expected {
        let found = raw[at..]
            .find(&format!("\"{key}\":{{"))
            .unwrap_or_else(|| panic!("{key} out of order in {section}: {raw}"));
        at += found + 1;
    }
}

/// `(file, line, why)` of every row of one table.
fn rows(table: &Value) -> Vec<(String, u64, String)> {
    table["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
                r["why"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn row(file: &str, line: u64, why: &str) -> (String, u64, String) {
    (file.to_string(), line, why.to_string())
}

fn counts(table: &Value) -> (u64, u64, usize) {
    (
        table["total"].as_u64().unwrap(),
        table["dropped"].as_u64().unwrap(),
        table["rows"].as_array().unwrap().len(),
    )
}

#[test]
fn refs_on_a_hook_lists_its_call_and_import_rows() {
    let fx = Fixture::build("refs-hook");
    let (refs, refs_raw) = fx.json_raw(&["refs", "useCart", "--json"]);
    let inbound = &refs["inbound"];
    assert_keys(
        &refs_raw,
        &refs,
        "inbound",
        &["inherits", "uses-type", "uses-member", "call", "import"],
    );
    assert_eq!(
        rows(&inbound["call"]),
        vec![row(BADGE, 4, "call"), row(USE_CART_SPEC, 4, "call")]
    );
    assert_eq!(
        rows(&inbound["import"]),
        vec![row(BADGE, 1, "import"), row(USE_CART_SPEC, 1, "import")]
    );
    assert_eq!(
        inbound["call"]["rows"][0]["source"],
        "const items = useCart();"
    );
}

#[test]
fn refs_on_a_component_lists_its_jsx_use_rows() {
    let fx = Fixture::build("refs-component");
    let (refs, refs_raw) = fx.json_raw(&["refs", "CartBadge", "--json"]);
    let inbound = &refs["inbound"];
    assert_keys(
        &refs_raw,
        &refs,
        "inbound",
        &["inherits", "uses-type", "uses-member", "jsx-use", "import"],
    );
    assert_eq!(
        rows(&inbound["jsx-use"]),
        vec![row(BADGE_SPEC, 4, "jsx-use"), row(PAGE, 10, "jsx-use")]
    );
    assert_eq!(
        rows(&inbound["import"]),
        vec![row(BADGE_SPEC, 1, "import"), row(PAGE, 1, "import")]
    );
}

#[test]
fn refs_shares_one_cap_across_the_ts_kinds_and_all_lifts_it() {
    let fx = Fixture::build("refs-cap");
    let (capped, capped_raw) = fx.json_raw(&["refs", "loadCart", "--json"]);
    let inbound = &capped["inbound"];
    assert_keys(
        &capped_raw,
        &capped,
        "inbound",
        &[
            "inherits",
            "uses-type",
            "uses-member",
            "call",
            "dispatch",
            "import",
        ],
    );
    assert_eq!(counts(&inbound["call"]), (23, 1, 22));
    assert_eq!(counts(&inbound["dispatch"]), (13, 8, 5));
    assert_eq!(counts(&inbound["import"]), (5, 2, 3));
    let text = fx.expect_ok(&["refs", "loadCart"]);
    assert!(text.contains("\n  +11 more"), "{text}");

    let all = fx.json(&["refs", "loadCart", "--all", "--json"]);
    let inbound = &all["inbound"];
    assert_eq!(counts(&inbound["call"]), (23, 0, 23));
    assert_eq!(counts(&inbound["dispatch"]), (13, 0, 13));
    assert_eq!(
        rows(&inbound["import"]),
        vec![
            row(ACTIONS_TEST_JS, 1, "import"),
            row(ACTIONS_TEST_TS, 1, "import"),
            row(REFILL, 1, "import"),
            row(LEGACY, 1, "import"),
            row(PAGE, 2, "import"),
        ]
    );
    let calls = rows(&inbound["call"]);
    assert!(calls.contains(&row(LEGACY, 4, "call")), "{calls:?}");
    assert!(
        calls.contains(&row(ACTIONS_TEST_JS, 4, "call")),
        "{calls:?}"
    );
    assert_eq!(
        rows(&inbound["dispatch"]).last(),
        Some(&row(PAGE, 7, "dispatch"))
    );
}

#[test]
fn refs_text_and_compact_render_the_ts_tables() {
    let fx = Fixture::build("refs-text");
    let text = fx.expect_ok(&["refs", "CartBadge"]);
    let expected = "inbound:\n  inherits (0):\n  uses-type (0):\n  uses-member (0):\n  jsx-use (2):\n    \
         src/components/CartBadge.spec.tsx:4  jsx-use  const view = <CartBadge />;\n    \
         src/pages/CartPage.tsx:10  jsx-use  return <CartBadge key={id} data-size={empty.length} />;\n  \
         import (2):\n    \
         src/components/CartBadge.spec.tsx:1  import  import { CartBadge } from './CartBadge';\n    \
         src/pages/CartPage.tsx:1  import  import { CartBadge } from '../components/CartBadge';";
    assert!(text.contains(expected), "{text}");

    let compact = fx.expect_ok(&["refs", "CartBadge", "--compact"]);
    assert!(
        compact.contains(
            "in:jsx-use (2):\n  src/components/CartBadge.spec.tsx:4\n  src/pages/CartPage.tsx:10\n\
             in:import (2):\n  src/components/CartBadge.spec.tsx:1\n  src/pages/CartPage.tsx:1"
        ),
        "{compact}"
    );
    assert!(
        compact.contains("summary: edges=4 shown=4 dropped=0 ambiguous=0"),
        "{compact}"
    );
}

#[test]
fn refs_out_lists_the_ts_kinds_outbound_one_import_row_per_bound_definition() {
    let fx = Fixture::build("refs-out");
    let (refs, refs_raw) = fx.json_raw(&["refs", "CartPage", "--out", "--json"]);
    let outbound = &refs["outbound"];
    assert_keys(
        &refs_raw,
        &refs,
        "outbound",
        &[
            "inherits",
            "uses-type",
            "uses-member",
            "imports",
            "call",
            "jsx-use",
            "dispatch",
            "import",
        ],
    );
    let targets = |kind: &str| -> Vec<(u64, String)> {
        outbound[kind]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["line"].as_u64().unwrap(),
                    r["to"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    assert_eq!(
        targets("import"),
        vec![
            (1, id(BADGE, "CartBadge")),
            (2, id(ACTIONS, "loadCart")),
            (3, id(CART, "emptyCart")),
            (4, id(CART, "CartId")),
            (4, id(CART, "CartItem")),
        ]
    );
    assert_eq!(targets("call"), vec![(8, id(CART, "emptyCart"))]);
    assert_eq!(targets("dispatch"), vec![(7, id(ACTIONS, "loadCart"))]);
    assert_eq!(targets("jsx-use"), vec![(10, id(BADGE, "CartBadge"))]);
    assert_eq!(outbound["import"]["rows"][3]["toFile"], CART);
    assert_eq!(outbound["import"]["rows"][3]["why"], "import");

    let text = fx.expect_ok(&["refs", "CartPage", "--out"]);
    assert!(
        text.contains(
            "  import (5):\n    src/pages/CartPage.tsx:1  import  -> src/components/CartBadge.tsx#CartBadge"
        ),
        "{text}"
    );
}

#[test]
fn a_symbol_with_no_ts_rows_keeps_only_the_three_always_present_tables() {
    let fx = Fixture::build("refs-empty");
    let (refs, refs_raw) = fx.json_raw(&["refs", "auditLabel", "--json"]);
    assert_keys(
        &refs_raw,
        &refs,
        "inbound",
        &["inherits", "uses-type", "uses-member"],
    );
    let text = fx.expect_ok(&["refs", "auditLabel"]);
    for header in ["call (", "jsx-use (", "dispatch (", "import ("] {
        assert!(!text.contains(header), "{header} in {text}");
    }
}

#[test]
fn read_leaves_a_reference_inside_the_declaration_out_of_its_ts_tables() {
    let fx = Fixture::build("read-self");
    let refs = fx.json(&["refs", "walkTree", "--json"]);
    assert_eq!(
        rows(&refs["inbound"]["call"]),
        vec![row("src/tree/walk.ts", 2, "call")]
    );
    let (read, read_raw) = fx.json_raw(&["read", "walkTree", "--json"]);
    assert_keys(
        &read_raw,
        &read,
        "inbound",
        &["inherits", "uses-type", "uses-member"],
    );

    let read = fx.json(&["read", "useCart", "--json"]);
    assert_eq!(
        rows(&read["inbound"]["call"]),
        vec![row(BADGE, 4, "call"), row(USE_CART_SPEC, 4, "call")]
    );
}

#[test]
fn read_text_and_compact_render_the_ts_tables() {
    let fx = Fixture::build("read-text");
    let text = fx.expect_ok(&["read", "useCart"]);
    let expected =
        "inbound:\n  inherits (0):\n  uses-type (0):\n  uses-member (0):\n  call (2):\n    \
         src/components/CartBadge.tsx:4  call  const items = useCart();\n    \
         src/hooks/useCart.spec.ts:4  call  expect(useCart()).toEqual([]);\n  \
         import (2):\n    \
         src/components/CartBadge.tsx:1  import  import { useCart } from '../hooks/useCart';\n    \
         src/hooks/useCart.spec.ts:1  import  import { useCart } from './useCart';";
    assert!(text.contains(expected), "{text}");

    let text = fx.expect_ok(&["read", "CartBadge"]);
    assert!(
        text.contains(
            "  jsx-use (2):\n    \
             src/components/CartBadge.spec.tsx:4  jsx-use  const view = <CartBadge />;\n    \
             src/pages/CartPage.tsx:10  jsx-use  return <CartBadge key={id} data-size={empty.length} />;"
        ),
        "{text}"
    );
    let text = fx.expect_ok(&["read", "loadCart"]);
    assert!(
        text.contains(
            "  dispatch (13, 8 dropped):\n    src/bulk/refill.ts:24  dispatch  dispatch(loadCart());"
        ),
        "{text}"
    );
    assert!(text.trim_end().ends_with("  +11 more"), "{text}");

    let compact = fx.expect_ok(&["read", "useCart", "--compact"]);
    assert!(
        compact.contains(
            "in:call (2):\n  src/components/CartBadge.tsx:4\n  src/hooks/useCart.spec.ts:4\n\
             in:import (2):\n  src/components/CartBadge.tsx:1\n  src/hooks/useCart.spec.ts:1\n\
             summary: edges=4 shown=4 dropped=0 ambiguous=0"
        ),
        "{compact}"
    );
    let compact = fx.expect_ok(&["read", "CartBadge", "--compact"]);
    assert!(
        compact.contains(
            "in:jsx-use (2):\n  src/components/CartBadge.spec.tsx:4\n  src/pages/CartPage.tsx:10"
        ),
        "{compact}"
    );
    let compact = fx.expect_ok(&["read", "loadCart", "--compact"]);
    assert!(
        compact.contains("in:dispatch (13, 8 dropped):\n  src/bulk/refill.ts:24,25,26,27,28"),
        "{compact}"
    );
    assert!(
        compact.contains("summary: edges=41 shown=30 dropped=11 ambiguous=0"),
        "{compact}"
    );
}

#[test]
fn a_graph_written_without_bound_defs_answers_with_no_import_table() {
    let fx = Fixture::build("refs-stripped");
    let mut graph = fx.graph();
    for e in graph["edges"].as_array_mut().expect("edges array") {
        e.as_object_mut().expect("edge object").remove("bound_defs");
    }
    fs::write(fx.graph_path(), serde_json::to_vec(&graph).unwrap()).expect("write graph");
    let (refs, refs_raw) = fx.json_raw(&["refs", "useCart", "--json"]);
    assert_keys(
        &refs_raw,
        &refs,
        "inbound",
        &["inherits", "uses-type", "uses-member", "call"],
    );
    assert_eq!(counts(&refs["inbound"]["call"]), (2, 0, 2));
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

fn path_row(file: &str) -> (String, Vec<u64>, String, String) {
    (
        file.to_string(),
        vec![1, 4],
        "path".to_string(),
        "test-path".to_string(),
    )
}

#[test]
fn tests_counts_each_typescript_test_file_suffix_by_its_name() {
    let fx = Fixture::build("tests-suffixes");
    for (seed, file) in [
        ("useCart", USE_CART_SPEC),
        ("CartBadge", BADGE_SPEC),
        ("loadCart", ACTIONS_TEST_TS),
        ("CartPage", "src/pages/CartPage.test.tsx"),
    ] {
        let tests = fx.json(&["tests", seed, "--json"]);
        assert_eq!(test_rows(&tests), vec![path_row(file)], "tests {seed}");
        assert_eq!(tests["testFileCount"], 1, "tests {seed}");
        assert_eq!(tests["refCount"], 2, "tests {seed}");
    }
}

#[test]
fn tests_counts_a_file_below_a_nested_tests_directory_and_nothing_else_named_close_to_it() {
    let fx = Fixture::build("tests-segment");
    let tests = fx.json(&["tests", "CartStore", "--json"]);
    assert_eq!(
        test_rows(&tests),
        vec![path_row("src/store/__tests__/deep/total.ts")]
    );
    let refs = fx.json(&["refs", "CartStore", "--all", "--json"]);
    let referrers: Vec<String> = rows(&refs["inbound"]["call"])
        .into_iter()
        .map(|(file, _, _)| file)
        .collect();
    for not_a_test in ["tests/cartStore.ts", "src/store/cartStore__tests__.ts"] {
        assert!(
            referrers.contains(&not_a_test.to_string()),
            "{not_a_test} must still answer refs: {referrers:?}"
        );
    }
}

#[test]
fn a_javascript_test_file_is_not_counted_by_name() {
    let fx = Fixture::build("tests-js");
    let tests = fx.json(&["tests", "loadCart", "--json"]);
    let files: Vec<String> = test_rows(&tests).into_iter().map(|r| r.0).collect();
    assert_eq!(files, vec![ACTIONS_TEST_TS.to_string()]);
    let refs = fx.json(&["refs", "loadCart", "--all", "--json"]);
    assert!(rows(&refs["inbound"]["call"]).contains(&row(ACTIONS_TEST_JS, 4, "call")));
}

#[test]
fn tests_text_marks_a_row_vouched_by_its_name() {
    let fx = Fixture::build("tests-text");
    let text = fx.expect_ok(&["tests", "useCart"]);
    assert_eq!(
        text.trim_end(),
        "tests for src/hooks/useCart.ts#useCart\n\
         covered by 1 test file(s), 2 reference(s)\n\
         \n\
         src/hooks/useCart.spec.ts (test path)\n  \
         lines: 1, 4"
    );
}

#[test]
fn tests_text_marks_every_name_vouched_row() {
    let fx = Fixture::build("tests-text-all");
    for (seed, file) in [
        ("useCart", USE_CART_SPEC),
        ("CartBadge", BADGE_SPEC),
        ("loadCart", ACTIONS_TEST_TS),
        ("CartPage", "src/pages/CartPage.test.tsx"),
        ("CartStore", "src/store/__tests__/deep/total.ts"),
    ] {
        let text = fx.expect_ok(&["tests", seed]);
        assert!(
            text.contains(&format!("\n\n{file} (test path)\n  lines: 1, 4")),
            "tests {seed}: {text}"
        );
        assert_eq!(
            text.matches(" (test path)").count(),
            1,
            "tests {seed}: {text}"
        );
    }
    let text = fx.expect_ok(&["tests", "loadCart"]);
    assert!(!text.contains(ACTIONS_TEST_JS), "{text}");
}

/// `(file, hop, why)` of every `impact` row, sorted by file.
fn impact_rows(impact: &Value) -> Vec<(String, u64, String)> {
    let mut out: Vec<(String, u64, String)> = impact["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["hop"].as_u64().unwrap(),
                r["why"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

fn impact_json(fx: &Fixture, args: &[&str]) -> Value {
    let out = fx.run(args);
    let stdout = String::from_utf8(out.stdout).expect("stdout is utf-8");
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("devscout {args:?} did not print JSON: {e}\n{stdout}"))
}

#[test]
fn impact_at_one_hop_reaches_every_ts_referrer_but_no_namespace_or_side_effect_importer() {
    let fx = Fixture::build("impact-hop1");
    let impact = impact_json(&fx, &["impact", ACTIONS, "--hops", "1", "--json"]);
    assert_eq!(
        impact_rows(&impact),
        vec![
            (ACTIONS_TEST_JS.to_string(), 1, "import".to_string()),
            (ACTIONS_TEST_TS.to_string(), 1, "import".to_string()),
            (REFILL.to_string(), 1, "import".to_string()),
            (LEGACY.to_string(), 1, "import".to_string()),
            (PAGE.to_string(), 1, "import".to_string()),
            (CHECKOUT.to_string(), 1, "call".to_string()),
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert_eq!(
        impact["testsAffected"], 1,
        "only the .ts test file is a test"
    );
    let checkout = impact["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == CHECKOUT)
        .unwrap();
    assert_eq!(checkout["fromLines"]["direct"], 5);

    let store = impact_json(&fx, &["impact", STORE, "--hops", "1", "--json"]);
    let files: Vec<String> = impact_rows(&store).into_iter().map(|r| r.0).collect();
    assert!(files.contains(&PUBLIC.to_string()), "{files:?}");
    assert!(!files.contains(&POLYFILLS.to_string()), "{files:?}");
    assert!(!files.contains(&AUDIT.to_string()), "{files:?}");

    let badge = impact_json(&fx, &["impact", BADGE, "--hops", "1", "--json"]);
    let files: Vec<String> = impact_rows(&badge).into_iter().map(|r| r.0).collect();
    assert_eq!(files, vec![BADGE_SPEC.to_string(), PAGE.to_string()]);
}

#[test]
fn impact_at_two_hops_expands_through_a_ts_referrer() {
    let fx = Fixture::build("impact-hop2");
    let one = impact_json(&fx, &["impact", ACTIONS, "--hops", "1", "--json"]);
    assert!(!impact_rows(&one).iter().any(|r| r.0 == APP));
    let two = impact_json(&fx, &["impact", ACTIONS, "--hops", "2", "--json"]);
    let app = impact_rows(&two).into_iter().find(|r| r.0 == APP);
    assert_eq!(app, Some((APP.to_string(), 2, "import".to_string())));
}

#[test]
fn no_dispatch_keeps_the_ts_dispatch_rows() {
    let fx = Fixture::build("impact-no-dispatch");
    let full = fx.run(&["impact", ACTIONS, "--json"]).stdout;
    let narrowed = fx
        .run(&["impact", ACTIONS, "--no-dispatch", "--json"])
        .stdout;
    assert_eq!(
        String::from_utf8_lossy(&full),
        String::from_utf8_lossy(&narrowed)
    );
    let refs = fx.json(&["refs", "loadCart", "--no-dispatch", "--json"]);
    assert_eq!(refs["inbound"]["dispatch"]["total"], 13);
}

#[test]
fn ts_referrers_count_toward_the_hub_brake() {
    let fx = Fixture::build("impact-hub");
    let braked = impact_json(
        &fx,
        &[
            "impact",
            ACTIONS,
            "--hops",
            "2",
            "--hub-max-indegree",
            "2",
            "--json",
        ],
    );
    assert!(!impact_rows(&braked).iter().any(|r| r.0 == APP));
    let entry = braked["braked"]
        .as_array()
        .expect("a braked list")
        .iter()
        .find(|b| b["file"] == PAGE)
        .cloned();
    assert_eq!(entry.map(|b| b["indegree"].clone()), Some(Value::from(2)));
}

#[test]
fn the_answer_contract_documents_the_ts_words_tables_and_bound_defs() {
    let contract =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/answer-contract.md"))
            .expect("read the answer contract");
    for word in ["call", "jsx-use", "dispatch", "import", "test-path"] {
        assert!(
            contract.contains(&format!("| `{word}` |")),
            "the why table must document {word}"
        );
    }
    assert!(contract.contains("`bound_defs`"));
    assert!(contract.contains("via: \"path\""));
    assert!(!contract.contains("never reach `why`"));
}
