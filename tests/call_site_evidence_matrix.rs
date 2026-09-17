//! Two things anchor `docs/call-site-evidence-matrix.md` to real command
//! output rather than to memory:
//!
//! - `pinned_export_matches_the_committed_snapshot_or_is_regenerated_with_bless`
//!   regenerates `fixtures/call-site-evidence/export.json` -- a pinned,
//!   byte-diffed evidence bundle an independent consumer can be run against --
//!   from the audited surfaces against `fixtures/call-site-evidence/Witness.cs`,
//!   and asserts it is byte-identical to the committed file, the same
//!   generate-then-diff shape `semantic-audit` already applies to
//!   `fixtures/csharp-flowtrace/facts.json`. Set `BLESS=1` to rewrite the
//!   committed file instead of asserting against it -- the only sanctioned way
//!   to change it, so a silent drift always fails `cargo test` first.
//! - The remaining tests each back one specific factual claim the matrix
//!   makes about a surface `tests/call_site_evidence.rs` does not already
//!   cover (`find`, `tests`, persisted graph edges, the `flowtrace-facts`
//!   fact-kind set), so a behavior change that would invalidate the matrix's
//!   prose fails here too, not only silently in the document.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-call-site-evidence-matrix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/call-site-evidence");
        fs::copy(source.join("Witness.cs"), root.join("Witness.cs")).unwrap();
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.ok(&["init", "--no-hooks"]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn graph(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn export_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/call-site-evidence/export.json")
}

// Builds the pinned export: the raw `--json` answers of every audited query
// this fixture proves a witness or a gap for, each parsed once (to pin key
// ORDER via a `BTreeMap` at the top level only -- the per-answer bytes below
// that are untouched, still hand-built by `query::json`) and re-serialized
// with two-space indentation for a reviewable diff. Not a `devscout` output
// format of its own -- an evidence bundle this ticket's own tests build.
fn build_export(fx: &Fixture) -> String {
    let mut bundle: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    for query in ["Record", "RecordAsync", "Recurse"] {
        let text = fx.ok(&["refs", query, "--json"]);
        let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");
        bundle.insert(
            match query {
                "Record" => "refs_Record",
                "RecordAsync" => "refs_RecordAsync",
                "Recurse" => "refs_Recurse",
                _ => unreachable!(),
            },
            value,
        );
    }
    let read_text = fx.ok(&["read", "Ledger", "--json"]);
    bundle.insert(
        "read_Ledger",
        serde_json::from_str(read_text.trim()).expect("valid JSON"),
    );
    let mut out = serde_json::to_string_pretty(&bundle).expect("serialize export bundle");
    out.push('\n');
    out
}

#[test]
fn pinned_export_matches_the_committed_snapshot_or_is_regenerated_with_bless() {
    let fx = Fixture::new();
    let generated = build_export(&fx);

    if std::env::var("BLESS").as_deref() == Ok("1") {
        fs::write(export_path(), &generated).expect("write export.json");
        return;
    }

    let committed = fs::read_to_string(export_path()).unwrap_or_default();
    assert_eq!(
        generated, committed,
        "fixtures/call-site-evidence/export.json is stale -- regenerate with `BLESS=1 cargo test --test call_site_evidence_matrix`"
    );
}

#[test]
fn find_has_no_invocation_evidence_surface_for_a_bare_member_name() {
    let fx = Fixture::new();
    // `find` takes no `--json` flag at all -- confirmed by its own usage
    // text carrying none, unlike refs/read/impact/tests.
    let help = fx.run(&["find", "Record", "--json"]);
    let stdout = String::from_utf8(help.stdout).unwrap();
    // `--json` is swallowed as part of the query text on this verb (it has
    // no flag parser for it), so it answers a literal miss -- proving `find`
    // carries no call-site evidence for this seed either way.
    assert!(
        stdout.trim().is_empty() || !stdout.trim_start().starts_with('{'),
        "find never answers a JSON object: {stdout}"
    );
}

#[test]
fn tests_verb_answers_empty_for_a_fixture_with_no_test_attribute() {
    let fx = Fixture::new();
    let out = fx.ok(&["tests", "Ledger", "--json"]);
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("valid JSON");
    assert_eq!(v["rows"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(v["testFileCount"].as_u64(), Some(0), "{v}");
}

#[test]
fn the_persisted_graph_carries_two_byte_identical_uses_member_edges_at_the_collision_line() {
    let fx = Fixture::new();
    let graph = fx.graph();
    let edges = graph["edges"].as_array().expect("edges array");
    let at_40: Vec<&serde_json::Value> = edges
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == "Witness.cs"
                && e["from_line"].as_u64() == Some(40)
        })
        .collect();
    assert_eq!(
        at_40.len(),
        2,
        "the graph itself never lost the second call -- both edges already exist: {at_40:?}"
    );
    assert_eq!(
        at_40[0], at_40[1],
        "the two persisted edge objects are themselves byte-identical -- the closed gap sits at \
         the query/serialization layer, not the graph: {at_40:?}"
    );
}

#[test]
fn no_optional_fact_kind_this_fixture_emits_carries_an_await_or_branch_field() {
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-flowtrace/facts.json"),
    )
    .expect("read committed flowtrace-facts snapshot");
    for key in [
        "branchPoint",
        "paramSource",
        "exceptionMap",
        "awaitOrder",
        "callOrder",
    ] {
        assert!(
            !text.contains(&format!("\"{key}\"")),
            "the optional sidecar's own committed fixture must never carry {key:?}"
        );
    }
}
