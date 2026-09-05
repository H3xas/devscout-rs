//! Integration tests for TS/TSX bare-specifier resolution: nested `tsconfig.json`
//! `paths`/`baseUrl` (following relative `extends`), the repo-root fallback
//! chain for files with no nested config, and barrel re-exports
//! (`export * from`, `export { X } from`) followed to the file that actually
//! declares the name.
//!
//! Fixture: `fixtures/ts-resolution` (see its files for the exact shape).
//! `apps/web` carries its own `tsconfig.json` (`extends` the repo-root
//! `tsconfig.base.json`, overrides `paths` for `@/*`); `apps/api` carries none,
//! so its `@core/*` import must resolve through the root `tsconfig.base.json`
//! directly. `apps/web/src/pages/HomePage.tsx` imports `PrimaryButton` from
//! `@/components`, a two-hop barrel (`components/index.ts` ->
//! `components/buttons/index.ts` -> `components/buttons/PrimaryButton.tsx`)
//! and uses it as a JSX tag.
//!
//! Every test here goes through the COMPILED BINARY as a subprocess, with
//! HOME/SCOUT_REGISTRY/SCOUT_CONTENT_DB pointed at a fresh temp dir each --
//! same isolation rule and the same duplicated-per-file helpers as
//! tests/cli_root.rs and tests/semantic_audit.rs (each tests/*.rs file
//! compiles as an independent binary, so sharing these small helpers via a
//! common module would buy little).

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
        "scout-ts-resolution-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ts-resolution")
}

/// Recursive copy of the whole fixture tree, `tsconfig.json`/`tsconfig.base.json`
/// included, into a fresh repo dir.
fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

// Repo-relative file paths the fixture's edges are keyed on.
const HOME_PAGE: &str = "apps/web/src/pages/HomePage.tsx";
const COMPONENTS_BARREL: &str = "apps/web/src/components/index.ts";
const BUTTONS_BARREL: &str = "apps/web/src/components/buttons/index.ts";
const PRIMARY_BUTTON: &str = "apps/web/src/components/buttons/PrimaryButton.tsx";
const API_HANDLER: &str = "apps/api/src/handler.ts";
const FORMAT_LABEL: &str = "packages/core/src/formatLabel.ts";

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

    fn json(&self, args: &[&str]) -> Value {
        let stdout = self.expect_ok(args);
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("devscout {args:?} did not print JSON: {e}\n{stdout}"))
    }

    /// Reads and parses `.scout/graph/graph.json`. The fixture is never a git
    /// repo (no `git init` is run), so the graph lives directly under
    /// `<repo>/.scout/graph`, not a shared git-common dir.
    fn graph(&self) -> Value {
        let path = self.repo.join(".scout/graph/graph.json");
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
    }
}

fn edges(graph: &Value) -> &Vec<Value> {
    graph["edges"]
        .as_array()
        .expect("graph.json has an \"edges\" array")
}

fn edges_of_kind<'a>(all: &'a [Value], kind: &str) -> Vec<&'a Value> {
    all.iter().filter(|e| e["kind"] == kind).collect()
}

fn import_edges_from<'a>(all: &'a [Value], from_file: &str) -> Vec<&'a Value> {
    all.iter()
        .filter(|e| e["kind"] == "import" && e["from_file"] == from_file)
        .collect()
}

#[test]
fn a_nested_tsconfig_alias_resolves_through_chained_barrels_to_the_declaring_file() {
    let fx = Fixture::build("nested-alias");
    let graph = fx.graph();
    let all = edges(&graph);
    let from_home = import_edges_from(all, HOME_PAGE);

    let direct = from_home.iter().find(|e| {
        e["target"] == "@/components" && e["to_file"] == COMPONENTS_BARREL && e.get("via").is_none()
    });
    assert!(
        direct.is_some(),
        "no direct import edge from {HOME_PAGE} naming \"@/components\" -> {COMPONENTS_BARREL} \
         among: {:#?}",
        from_home
    );

    let via_barrel = from_home
        .iter()
        .find(|e| e["via"] == COMPONENTS_BARREL && e["to_file"] == PRIMARY_BUTTON);
    assert!(
        via_barrel.is_some(),
        "no barrel-followed import edge from {HOME_PAGE} with via={COMPONENTS_BARREL} -> \
         {PRIMARY_BUTTON} among: {:#?}",
        from_home
    );
}

#[test]
fn a_jsx_tag_bound_through_a_barrel_becomes_a_jsx_use_edge() {
    let fx = Fixture::build("jsx-use");
    let graph = fx.graph();
    let all = edges(&graph);
    let jsx_uses = edges_of_kind(all, "jsx-use");

    // Line 6 of fixtures/ts-resolution/apps/web/src/pages/HomePage.tsx is the
    // `<PrimaryButton label="Continue" />` tag.
    const TAG_LINE: u64 = 6;
    let tag_edge = jsx_uses.iter().find(|e| {
        e["from_file"] == HOME_PAGE && e["to_file"] == PRIMARY_BUTTON && e["from_line"] == TAG_LINE
    });
    assert!(
        tag_edge.is_some(),
        "no jsx-use edge from {HOME_PAGE}:{TAG_LINE} -> {PRIMARY_BUTTON} among: {:#?}",
        jsx_uses
    );

    // The query surface folds only the C#-shaped edge kinds (README,
    // Limitations), so `refs` is checked here for the definition it resolves
    // the name to, not for the JSX site the graph edge above already pins.
    let refs = fx.json(&["refs", "PrimaryButton", "--json"]);
    assert_eq!(
        refs["id"],
        format!("{PRIMARY_BUTTON}#PrimaryButton"),
        "refs PrimaryButton --json resolves to a different definition: {refs:#?}"
    );
}

#[test]
fn every_barrel_hop_is_an_import_edge_and_nothing_falls_out_external() {
    let fx = Fixture::build("barrel-hops");
    let graph = fx.graph();
    let all = edges(&graph);

    // Each barrel's own re-export is an import edge of its own, so a file
    // walk from the declaring file reaches the page hop by hop as well as
    // through the `via` edge.
    for (from, to) in [
        (COMPONENTS_BARREL, BUTTONS_BARREL),
        (BUTTONS_BARREL, PRIMARY_BUTTON),
    ] {
        let hop = import_edges_from(all, from)
            .into_iter()
            .find(|e| e["to_file"] == to && e["via"].is_null());
        assert!(
            hop.is_some(),
            "no import edge {from} -> {to} among: {:#?}",
            import_edges_from(all, from)
        );
    }

    // Every specifier in the fixture names a file the repo carries, and every
    // reference binds: an alias or a barrel that fell out would show up here
    // as an external import or an unresolved reference.
    let ts = &graph["stats"]["ts"];
    assert_eq!(
        (
            ts["external_import_count"].as_u64(),
            ts["unresolved_ref_count"].as_u64()
        ),
        (Some(0), Some(0)),
        "stats.ts reports something external or unresolved: {ts:#?}"
    );
}

#[test]
fn a_root_alias_resolves_for_a_file_with_no_nested_tsconfig() {
    let fx = Fixture::build("root-alias");
    let graph = fx.graph();
    let all = edges(&graph);
    let from_handler = import_edges_from(all, API_HANDLER);

    let import_edge = from_handler
        .iter()
        .find(|e| e["target"] == "@core/formatLabel" && e["to_file"] == FORMAT_LABEL);
    assert!(
        import_edge.is_some(),
        "no import edge from {API_HANDLER} naming \"@core/formatLabel\" -> {FORMAT_LABEL} among: \
         {:#?}",
        from_handler
    );

    let calls = edges_of_kind(all, "call");
    let call_edge = calls
        .iter()
        .find(|e| e["from_file"] == API_HANDLER && e["to_file"] == FORMAT_LABEL);
    assert!(
        call_edge.is_some(),
        "no call edge from {API_HANDLER} -> {FORMAT_LABEL} among: {:#?}",
        calls
    );
}
