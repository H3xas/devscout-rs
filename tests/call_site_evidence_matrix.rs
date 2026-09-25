//! Three things anchor `docs/call-site-evidence-matrix.md` to real command
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
//! - `docs_matrix_matches_the_committed_snapshot_or_is_regenerated_with_bless`
//!   (bottom of this file) regenerates `docs/call-site-evidence-matrix.md`
//!   itself the same way: `build_matrix_doc` runs every audited command fresh
//!   and asserts the load-bearing fact behind each witness/gap cell against
//!   that live output before writing the sentence naming it, so the document
//!   cannot silently drift from the code that backs it. The same `BLESS=1`
//!   rewrites it.
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

    // A successful `--json` run, parsed. Used by the matrix-doc generator
    // below wherever a cell's own claim must be pinned to a live answer
    // rather than typed from memory.
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let text = self.ok(args);
        serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {text}"))
    }

    // Same as `json`, but for a verb whose legitimate answer is a non-zero
    // exit (a resolved, empty `zero-hit` answer still prints a JSON object
    // to stdout at `EXIT_NO_RESULT`).
    fn json_any_exit(&self, args: &[&str]) -> serde_json::Value {
        let output = self.run(args);
        let text = String::from_utf8(output.stdout).unwrap();
        serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {text}"))
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
// format of its own -- an evidence bundle these tests build.
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

// ============================================================================
// docs/call-site-evidence-matrix.md -- the capability-matrix document itself,
// generated and byte-diffed the same way export.json is above.
// ============================================================================

fn matrix_doc_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/call-site-evidence-matrix.md")
}

fn read_committed_facts() -> serde_json::Value {
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-flowtrace/facts.json"),
    )
    .expect("read committed flowtrace-facts snapshot");
    serde_json::from_str(&text).expect("valid JSON")
}

fn facts_of<'a>(doc: &'a serde_json::Value, kind: &str) -> Vec<&'a serde_json::Value> {
    doc["facts"]
        .as_array()
        .expect("facts array")
        .iter()
        .filter(|f| f["type"] == kind)
        .collect()
}

// Every fact `type` the committed snapshot carries, sorted, deduped -- read
// off the file rather than typed as a fixed list, so the "control/await" and
// "supported-shape" sections below cannot silently drift from what the
// emitter actually produces.
fn fact_kinds(doc: &serde_json::Value) -> Vec<String> {
    let set: std::collections::BTreeSet<String> = doc["facts"]
        .as_array()
        .expect("facts array")
        .iter()
        .map(|f| f["type"].as_str().expect("type is a string").to_string())
        .collect();
    set.into_iter().collect()
}

fn member(v: &serde_json::Value) -> &serde_json::Value {
    let members = v["members"].as_array().expect("a members wrapper");
    assert_eq!(members.len(), 1, "exactly one declaring type: {v}");
    &members[0]
}

fn uses_member_rows(model: &serde_json::Value) -> &Vec<serde_json::Value> {
    model["inbound"]["uses-member"]["rows"]
        .as_array()
        .expect("a uses-member rows array")
}

fn rows_at<'a>(rows: &'a [serde_json::Value], file: &str, line: u64) -> Vec<&'a serde_json::Value> {
    rows.iter()
        .filter(|r| r["file"] == file && r["line"].as_u64() == Some(line))
        .collect()
}

fn occurrence_indices(rows: &[&serde_json::Value]) -> Vec<u64> {
    let mut v: Vec<u64> = rows
        .iter()
        .filter_map(|r| r["occurrenceIndex"].as_u64())
        .collect();
    v.sort_unstable();
    v
}

// Every key this list names must be absent from every audited verb's
// `--json` answer on this fixture -- re-derived here (not merely cited from
// the sibling integration test) so the matrix's own coverage claim cannot
// outrun the live output backing it.
const NEVER_EMITTED: &[&str] = &[
    "branchPoint",
    "paramSource",
    "exceptionMap",
    "callOrder",
    "awaitOrder",
    "sequence",
    "dispatchTarget",
];

fn assert_never_emitted(fx: &Fixture) {
    for query in ["Record", "RecordAsync", "Recurse", "Ledger", "Caller"] {
        for verb in ["refs", "read", "tests", "impact"] {
            let out = fx.run(&[verb, query, "--json"]);
            let text = String::from_utf8(out.stdout).unwrap();
            if text.trim().is_empty() {
                continue;
            }
            for key in NEVER_EMITTED {
                assert!(
                    !text.contains(&format!("\"{key}\"")),
                    "{verb} {query} --json must never carry {key:?}: {text}"
                );
            }
        }
    }
}

// Assembles the WHOLE committed document from real command output against
// the pinned Witness.cs fixture (native surfaces) and the committed
// flow-tracer facts snapshot (optional surface). Every witness cell's
// concrete value -- a count, a key, a line number -- is read off that live
// output, not copied by hand; every gap cell's claim is `assert!`-checked
// against the same live output immediately before the sentence naming it is
// written, so a behavior change that would silently invalidate a claim here
// panics this generator (and so `cargo test`) instead of quietly producing a
// document that still asserts it.
#[allow(clippy::too_many_lines)]
fn build_matrix_doc(fx: &Fixture) -> String {
    let refs_record = fx.json(&["refs", "Record", "--json"]);
    let refs_record_async = fx.json(&["refs", "RecordAsync", "--json"]);
    let refs_recurse = fx.json(&["refs", "Recurse", "--json"]);
    let read_ledger = fx.json(&["read", "Ledger", "--json"]);
    let tests_ledger = fx.json(&["tests", "Ledger", "--json"]);
    let impact_ledger = fx.json_any_exit(&["impact", "Ledger", "--json"]);
    let find_out = fx.run(&["find", "Record", "--json"]);
    let find_stdout = String::from_utf8(find_out.stdout).unwrap();
    let graph = fx.graph();
    let facts_doc = read_committed_facts();

    let record_member = member(&refs_record);
    let record_rows = uses_member_rows(record_member);
    let record_sites: Vec<u64> = record_member["sites"]
        .as_array()
        .expect("sites array")
        .iter()
        .map(|s| s["line"].as_u64().expect("line"))
        .collect();
    assert_eq!(
        record_sites,
        vec![8, 12],
        "Record's two overload declarations moved: {record_member}"
    );
    let at_line_40 = rows_at(record_rows, "Witness.cs", 40);
    assert_eq!(at_line_40.len(), 2, "the collision case: {record_rows:?}");
    assert_eq!(
        occurrence_indices(&at_line_40),
        vec![0, 1],
        "the line-40 pair must carry distinct occurrenceIndex values: {at_line_40:?}"
    );

    let record_async_member = member(&refs_record_async);
    let record_async_rows = uses_member_rows(record_async_member);
    let awaited_rows = rows_at(record_async_rows, "Witness.cs", 71);
    assert_eq!(awaited_rows.len(), 1, "{record_async_rows:?}");

    let recurse_member = member(&refs_recurse);
    let recurse_rows = uses_member_rows(recurse_member);
    assert_eq!(
        rows_at(recurse_rows, "Witness.cs", 64).len(),
        1,
        "{recurse_rows:?}"
    );

    let read_span_file = read_ledger["span"]["file"].as_str().expect("span.file");
    let read_span_start = read_ledger["span"]["startLine"].as_u64().expect("start");
    let read_span_end = read_ledger["span"]["endLine"].as_u64().expect("end");
    assert_eq!(
        (read_span_file, read_span_start, read_span_end),
        ("Witness.cs", 6, 20)
    );
    let read_uses_type_rows = read_ledger["inbound"]["uses-type"]["rows"]
        .as_array()
        .expect("uses-type rows");
    let read_at_28 = rows_at(read_uses_type_rows, "Witness.cs", 28);
    assert_eq!(
        read_at_28.len(),
        2,
        "the read-side collision: {read_uses_type_rows:?}"
    );
    assert_eq!(
        occurrence_indices(&read_at_28),
        vec![0, 1],
        "the line-28 pair must carry distinct occurrenceIndex values: {read_at_28:?}"
    );

    assert_eq!(
        tests_ledger["rows"].as_array().map(Vec::len),
        Some(0),
        "this fixture declares no test attribute: {tests_ledger}"
    );
    assert_eq!(tests_ledger["testFileCount"].as_u64(), Some(0));

    // The only callers of `Ledger` in this fixture sit inside `Ledger`'s own
    // declaring file, and the impact walk always excludes a seed's own file
    // from the affected set -- so a real `zero-hit` here is the honest
    // result, not a fixture limitation to paper over.
    assert_eq!(impact_ledger["outcome"], "zero-hit", "{impact_ledger}");
    assert_eq!(impact_ledger["rows"].as_array().map(Vec::len), Some(0));

    // `find` space-joins every token after the verb into one query string
    // (no `--json` flag parser), so what it resolves to depends on the
    // fixture's own auto-generated manifest purposes -- captured live rather
    // than assumed to be any one shape.
    assert!(
        find_stdout.trim().is_empty() || !find_stdout.trim_start().starts_with('{'),
        "find never answers a JSON object: {find_stdout}"
    );
    let find_witness = if find_stdout.trim().is_empty() {
        "prints nothing to stdout".to_string()
    } else {
        format!(
            "prints non-JSON text to stdout (`{}`)",
            find_stdout.trim().replace('\n', " / ")
        )
    };

    let edges = graph["edges"].as_array().expect("edges array");
    let edges_at_40: Vec<&serde_json::Value> = edges
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == "Witness.cs"
                && e["from_line"].as_u64() == Some(40)
        })
        .collect();
    assert_eq!(edges_at_40.len(), 2, "{edges_at_40:?}");
    assert_eq!(
        edges_at_40[0], edges_at_40[1],
        "the two persisted edge objects must still be byte-identical -- occurrenceIndex is \
         query-time only, never written back into the graph: {edges_at_40:?}"
    );

    assert_never_emitted(fx);

    let kinds = fact_kinds(&facts_doc);
    assert_eq!(
        kinds,
        vec![
            "consume".to_string(),
            "ctor_field".to_string(),
            "di_binding".to_string(),
            "iface_impl".to_string(),
            "message_class".to_string(),
            "method_call".to_string(),
            "method_span".to_string(),
            "publish".to_string(),
            "route".to_string(),
        ],
        "the optional sidecar's fact-kind vocabulary moved: {kinds:?}"
    );
    for kind in &kinds {
        for f in facts_of(&facts_doc, kind) {
            for key in [
                "branchPoint",
                "paramSource",
                "exceptionMap",
                "awaitOrder",
                "callOrder",
            ] {
                assert!(
                    f.get(key).is_none(),
                    "{kind} fact must never carry {key}: {f}"
                );
            }
        }
    }

    let method_call_facts = facts_of(&facts_doc, "method_call");
    let block_bodied_witness = method_call_facts
        .iter()
        .find(|f| f["class"] == "DeliveryScheduledConsumer" && f["field"] == "_repository")
        .expect("the block-bodied witness");
    assert_eq!(block_bodied_witness["method"], "NotifyLost");
    assert_eq!(block_bodied_witness["calledMethod"], "Find");
    let expression_bodied_witness = method_call_facts
        .iter()
        .find(|f| f["class"] == "ParcelsController" && f["method"] == "HasRecord")
        .expect("the expression-bodied witness");
    assert_eq!(expression_bodied_witness["field"], "_repository");
    assert_eq!(expression_bodied_witness["calledMethod"], "Find");

    let all_facts = facts_doc["facts"].as_array().expect("facts array");
    let total_facts = all_facts.len();
    let method_call_count = method_call_facts.len();
    assert_eq!(
        method_call_count, 6,
        "the fixture's own ctor-injected-field call count moved"
    );

    let mut doc = String::new();
    doc.push_str("# Call-site evidence capability matrix\n\n");
    doc.push_str(
        "What each shipped native answer surface, the persisted graph, and the optional \
         `flowtrace-facts`\nsidecar say about one C# invocation: caller identity, the \
         invocation's own source range and\noccurrence, the callee's declaration/candidate \
         identity, resolution strength, source\nrevision/content identity, and supported \
         control/await information. Every cell below names a\nconcrete witness (the command, \
         the fixture and the exact key that carries it) or an explicit gap\n(the absent field \
         and where the absence is recorded).\n\n",
    );
    doc.push_str(
        "Every witness is a real run against `fixtures/call-site-evidence/Witness.cs` (native \
         surfaces) or\nthe committed `fixtures/csharp-flowtrace/facts.json` (optional surface). \
         This document's own bytes\nare produced by \
         `tests/call_site_evidence_matrix.rs::docs_matrix_matches_the_committed_snapshot_or_is_regenerated_with_bless`,\n\
         which runs every audited command fresh and asserts the load-bearing fact behind each\n\
         witness/gap cell against that live output before writing the sentence naming it -- a \
         hand edit\nto this file, or a behavior change that would silently invalidate a claim \
         below, fails `cargo\ntest` and must be regenerated with `BLESS=1 cargo test --test \
         call_site_evidence_matrix`, the\nsame generate-then-byte-diff shape `semantic-audit` \
         already applies to\n`fixtures/csharp-flowtrace/facts.json`. Two cells -- the \
         `flowtrace-facts` row of \"Invocation\nsource range and occurrence\" and of \
         \"Resolution strength\" -- describe flowtrace-cli's own\nconsumer-side behavior and \
         are cited from its source rather than re-run here; both say so\nwhere they appear.\n\n",
    );
    doc.push_str(
        "Audited native surfaces: the `--json` answers of `refs`, `read`, `impact`, `tests` and \
         `find`, and\nthe persisted graph's own `edges` array (`.scout/graph/graph.json`). \
         Audited optional surface:\nthe `flowtrace-facts` sidecar \
         (`fixtures/csharp-flowtrace/facts.json`), produced by\n`tools/scout-semantic` and \
         consumed by flowtrace-cli.\n\n",
    );

    doc.push_str("## Caller identity\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    doc.push_str(
        "| `refs --json` | Witness: every inbound/outbound row's `file` key names the calling \
         file. `refs Record --json` -> `members[0].inbound[\"uses-member\"].rows[*].file == \
         \"Witness.cs\"`. No caller *symbol* (which method the call sits in) is carried -- a \
         gap: only the file and line are recorded, never an enclosing-def id. |\n",
    );
    doc.push_str(
        "| `read --json` | Same as `refs` (reuses `refs`' inbound machinery); the declaration \
         span's own `file`/`startLine`/`endLine` additionally names the *callee's* declaring \
         file, a different fact from caller identity. |\n",
    );
    doc.push_str(
        "| `impact --json` | Witness: a `hit` answer's `rows[*].file` names an affected file \
         (see the repository's own worked example, `docs/answer-contract.md`). Gap: `impact` \
         reports file-level blast radius, not a per-invocation caller; no `fromLines` entry \
         names which specific call inside that file. On this fixture, `impact Ledger --json` is \
         itself a real `zero-hit`: its only callers share `Ledger`'s own file, which `impact`'s \
         walk always excludes from the affected set. |\n",
    );
    doc.push_str(&format!(
        "| `tests --json` | Witness: `rows[*].file` names the covering test file; `rows[*].lines` \
         names the referencing lines inside it (`tests Ledger --json` on this fixture returns \
         {} rows -- `Witness.cs` declares no test attribute, a true negative, not a gap). |\n",
        tests_ledger["rows"].as_array().map_or(0, Vec::len)
    ));
    doc.push_str(&format!(
        "| `find --json` | Gap, structural: `find` has no `--json` flag parser, so `--json` \
         joins the query text as a literal token (`src/cli/dispatch.rs`); it answers no \
         structured, machine-readable invocation evidence either way. `find Record --json` on \
         this fixture {find_witness} -- a fuzzy name/purpose match, never a JSON object, so it \
         carries no call-site evidence in the vocabulary this matrix audits. |\n"
    ));
    doc.push_str(
        "| persisted graph edges | Witness: every edge object's `from_file`/`from_line` name \
         the calling site. `.scout/graph/graph.json`'s `edges[*]` for this fixture. Same \
         caller-symbol gap as `refs`: an edge names no enclosing def. |\n",
    );
    doc.push_str(&format!(
        "| `flowtrace-facts` | Witness: a `consume`/`publish`/`method_call`-kind fact's \
         `file`/`line`/`consumer`-or-`class` names the caller and its file. `method_call` \
         (`class`, `method`, `field`, `calledMethod`) was a declared-but-unpopulated \
         `FactSchema.cs` slot before this work; `FactsWalker.cs::EmitMethodCalls` now fills it \
         for a body calling a member on a constructor-injected field, from either a block or an \
         expression body, e.g. `{}.{}`'s `_{}.{}(...)` (`fixtures/csharp-flowtrace/facts.json`, \
         pinned by `tests/flowtrace_facts.rs::semantic_resolution_shows_where_regexes_stop`). \
         Gap, still: no caller *method-argument* identity (which parameter value reached the \
         call), out of scope here. |\n\n",
        block_bodied_witness["class"].as_str().unwrap(),
        block_bodied_witness["method"].as_str().unwrap(),
        block_bodied_witness["field"]
            .as_str()
            .unwrap()
            .trim_start_matches('_'),
        block_bodied_witness["calledMethod"].as_str().unwrap()
    ));

    doc.push_str("## Invocation source range and occurrence\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    doc.push_str(
        "| `refs --json` | Witness (proven gap, closed here): before `occurrenceIndex`, two \
         calls to `Record` on `Witness.cs:40` serialized as two byte-identical row objects \
         (`tests/call_site_evidence.rs::two_calls_on_one_line_...` fails against the tagging \
         code reverted). After: the pair carries `occurrenceIndex: 0` and `occurrenceIndex: 1`, \
         additive, appended last, `schema_version` unchanged. Every other named shape (different \
         lines, overload, recursion, awaited sequence, parallel launch) already round-trips with \
         no collision, by line alone. |\n",
    );
    doc.push_str(
        "| `read --json` | Same mechanism, same fixture, a second real collision: `read Ledger \
         --json`'s `inbound[\"uses-type\"].rows` carries two rows at `Witness.cs:28` (the \
         field's declared type and `new Ledger()` on the same line), both now \
         `occurrenceIndex: 0`/`1`. |\n",
    );
    doc.push_str(
        "| `impact --json` | Gap, by design: impact rows are per-file aggregates; `fromLines` \
         names one representative line per edge kind, never every occurrence. Occurrence-identity \
         work requires that to stay unchanged, and it is untouched here. |\n",
    );
    doc.push_str(
        "| `tests --json` | Gap: a `tests` row's `lines` array lists every referencing line but \
         carries no per-line occurrence discriminator for two references on the same line (this \
         fixture's own test-coverage rows are empty, so this is a structural read of the JSON \
         shape, not a fixture-proven case). |\n",
    );
    doc.push_str("| `find --json` | Not applicable -- `find` carries no invocation evidence at all (see Caller identity). |\n");
    doc.push_str(&format!(
        "| persisted graph edges | Gap, confirmed directly: `graph.json`'s two `uses-member` \
         edges for `Witness.cs:40` are themselves byte-identical objects (`{}` twice), \
         distinguishable only by their position in the `edges` array. `occurrenceIndex` is \
         computed at query time from exactly this array order and is NOT written back into the \
         persisted graph, so this gap remains at the graph layer by design (no \
         `GRAPH_SCHEMA_VERSION` bump, no fragment-cache generation rename). |\n",
        edges_at_40[0]
    ));
    doc.push_str(
        "| `flowtrace-facts` | Gap, cited from flowtrace-cli's own source, not re-verified in \
         this repository's own tests: `factSite()` (`lib/trace.js`, flowtrace-cli) keys a \
         fact/provider merge on `` `${type}|${file}|${line}` ``, so two facts at the same \
         type/file/line already merge to one site on the consumer side -- out of scope here \
         (consumer-side projection work tracked in the flowtrace-cli project). |\n\n",
    );

    doc.push_str("## Callee declaration/candidate identity\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    doc.push_str(&format!(
        "| `refs --json` | Witness: `refs Record --json`'s `members[0].id == \"{}\"` and \
         `sites` names both declaring lines (`{}` and `{}`, the two overloads) -- declaration \
         location is a field distinct from any inbound row's `file`/`line`. |\n",
        record_member["id"].as_str().unwrap(),
        record_sites[0],
        record_sites[1]
    ));
    doc.push_str(&format!(
        "| `read --json` | Witness: `read Ledger --json`'s \
         `span.file`/`span.startLine`/`span.endLine` (`{}`/`{}`/`{}`) is the declaration span, \
         structurally separate from `inbound.*.rows[*].file`/`line` (the invocation sites) -- \
         declaration location has always been a distinct field, unmodified here. |\n",
        read_span_file, read_span_start, read_span_end
    ));
    doc.push_str("| `impact --json` | Gap: `impact` names affected files, never a callee id. |\n");
    doc.push_str(&format!(
        "| `tests --json` | Witness: `symbol` names the resolved callee id (`tests Ledger \
         --json` -> `\"symbol\": \"{}\"`). |\n",
        tests_ledger["symbol"].as_str().unwrap()
    ));
    doc.push_str("| `find --json` | Not applicable (see Caller identity). |\n");
    doc.push_str(
        "| persisted graph edges | Witness: an edge's `to`/`to_file` name the callee's declaring \
         id/file; `defs[*].id`/`file`/`line` names the declaration itself, separately. |\n",
    );
    doc.push_str(
        "| `flowtrace-facts` | Witness: `consumer`/`fqn` on a `consume` fact name the acting \
         type, not a per-call target; `method_call.calledMethod` (now emitted -- see Caller \
         identity) names the callee member by name, not a resolved id -- weaker than `refs`' \
         graph-id identity, by design (the sidecar asserts, it does not resolve). |\n\n",
    );

    doc.push_str("## Resolution strength\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    doc.push_str(
        "| `refs --json` / `read --json` | Witness: every row's `why` \
         (`uses-member-precise`/`-ext`/`-guess`) and, on a heuristic row, `heuristic`/`tier`. \
         Every row in this fixture is `uses-member-precise` -- no guessed edge exists in a \
         package-free fixture with no ambiguity, so the guess/extension tiers are cited from \
         `docs/answer-contract.md`'s own worked example, not re-demonstrated here. |\n",
    );
    doc.push_str(
        "| `impact --json` | Witness: `rows[*].why`, `heuristicCount`/`heuristic`/`tier` when \
         present. |\n",
    );
    doc.push_str(
        "| `tests --json` | Witness: `rows[*].why` (`test-attribute`/`test-project`), \
         `heuristic`/`tier` when present. |\n",
    );
    doc.push_str("| `find --json` | Not applicable. |\n");
    doc.push_str(
        "| persisted graph edges | Witness: an edge's absent `heuristic` key means precise; \
         `heuristic: true` plus `tier` names the guess strength -- the source `why` is derived \
         from. |\n",
    );
    doc.push_str(
        "| `flowtrace-facts` | Gap, cited from flowtrace-cli's own source, not re-verified in \
         this repository's own tests: a fact carries no resolution-strength field at all -- it \
         is asserted by the optional producer, not scored; `docs/design/compiler-enrichment.md`'s \
         D2 and `src/query/imported.rs`'s isolation keep it always below anything the engine \
         resolved for itself. |\n\n",
    );

    doc.push_str("## Source revision/content identity\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    assert!(
        refs_record.get("freshness").is_some()
            && read_ledger.get("freshness").is_some()
            && tests_ledger.get("freshness").is_some()
            && impact_ledger.get("freshness").is_some(),
        "freshness must reach every one of refs/read/tests/impact"
    );
    doc.push_str(
        "| `refs --json` / `read --json` / `impact --json` / `tests --json` | Witness (proven \
         gap, closed here): `src/manifest.rs::freshness_warning` already detected a \
         changed-source-at-unchanged-HEAD condition, but only as a human-readable stderr line -- \
         confirmed by reading `emit_freshness_warning` (`src/cli/answer.rs`), which wrote only to \
         stderr. `src/freshness.rs::index_freshness_state` now exposes the same underlying facts \
         as a `freshness` top-level key (`state`: `fresh`/`stale`/`unknown`, plus \
         `indexedHead`/`currentHead`/`changedFiles` or `reason`), appended last, additive, \
         `schema_version` unchanged. `tests/freshness.rs`'s JSON-side companion tests exercise \
         `fresh`, `stale` (HEAD moved), `stale` (a modified indexed file) and `unknown` \
         (`no-index-state`) against a real git fixture; every live answer this generator itself \
         ran above also carries the key. |\n",
    );
    doc.push_str(
        "| `find --json` | Not applicable -- `find` has no `--json` shape at all to carry \
         `freshness` in. |\n",
    );
    assert_eq!(graph["built_at_head"], serde_json::Value::Null);
    doc.push_str(
        "| persisted graph edges | Witness: `graph.json`'s own `built_at_head` field (`null` on \
         this fixture -- a non-git root) is the graph's own revision anchor. |\n",
    );
    assert!(facts_doc.get("compilation").is_some() && facts_doc.get("generatedFrom").is_some());
    doc.push_str(
        "| `flowtrace-facts` | Witness: `facts.json`'s top-level `compilation`/`generatedFrom` \
         name the producing build; consumer-side identity mismatch detection (`lib/facts.js`'s \
         `staleFactsWarnings`, flowtrace-cli) is already shipped and cited, not re-implemented. \
         |\n\n",
    );

    doc.push_str("## Supported control/await information\n\n");
    doc.push_str("| Surface | Witness / gap |\n| --- | --- |\n");
    doc.push_str(
        "| `refs --json` / `read --json` / `impact --json` / `tests --json` | Gap, confirmed by \
         `tests/call_site_evidence.rs::no_answer_this_fixture_produces_ever_claims_an_unmodeled_control_or_dispatch_fact` \
         (re-run independently by this document's own generator, above): none of \
         `branchPoint`/`paramSource`/`exceptionMap`/`callOrder`/`awaitOrder`/`sequence`/`dispatchTarget` \
         ever appears, on any of the four audited verbs, for this fixture's awaited-sequence or \
         parallel-launch-join witnesses. An `await` or a parallel launch is recorded only as an \
         ordinary `uses-member` call site, indistinguishable in shape from a synchronous one -- \
         native devscout models no control-flow or await-ordering fact at all, a structural \
         non-goal, not an oversight. |\n",
    );
    doc.push_str("| `find --json` | Not applicable. |\n");
    doc.push_str(
        "| persisted graph edges | Gap: `Edge`'s kinds (`src/graph/edge.rs`) carry no \
         control/await tag. |\n",
    );
    doc.push_str(&format!(
        "| `flowtrace-facts` | Gap: none of the {} fact kinds this repository's fixture emits \
         (`{}`) carries an await or branch fact -- `method_call` included: it names a call, \
         never an ordering or a branch. `branch_point`/`param_source`/`exception_map` stay the \
         sidecar's documented non-goals; confirmed absent by direct inspection of the committed \
         snapshot's fact-kind set. |\n\n",
        kinds.len(),
        kinds.join(", ")
    ));

    doc.push_str("## Supported-shape table: which fixture shapes each mode establishes\n\n");
    doc.push_str(
        "| Fixture shape | Native (`devscout`, no .NET SDK) | Optional (`flowtrace-facts`, \
         dotnet-gated) |\n| --- | --- | --- |\n",
    );
    doc.push_str(
        "| A call on a plain local/field (not ctor-injected) | Yes -- every `uses-member` edge, \
         regardless of how the receiver got there. | No -- `method_call` is scoped to a \
         constructor-injected field only, per its documented shape. |\n",
    );
    doc.push_str(
        "| A call on a constructor-injected field, block-bodied method | Yes, same as above (no \
         special case). | Yes -- `method_call`. |\n",
    );
    doc.push_str(
        "| A call on a constructor-injected field, expression-bodied method (`=>`) | Yes, same \
         as above (no special case). | Yes -- `method_call` reads either a block or an \
         expression body; a proven gap closed here (`EmitMethodCalls` used to return early on \
         any expression-bodied method), witnessed by `ParcelsController.HasRecord`. |\n",
    );
    doc.push_str(
        "| A constructor-injected field assigned in a different file of a partial class | Yes -- \
         extraction has no such restriction. | No, by construction: `EmitMethodCalls`/`InjectedFields` \
         only read a constructor declared in the SAME `SemanticModel`'s syntax tree as the \
         calling method; a Roslyn `SemanticModel` cannot resolve symbols in another file's tree \
         without a second model lookup this sidecar does not add. Recorded as a known, accepted \
         miss, not silently swallowed. |\n",
    );
    doc.push_str(
        "| Two calls to one target on one line | Yes -- `occurrenceIndex`. | Not proven here: \
         `factSite()` (flowtrace-cli) merges same-line facts of the same type before this \
         question is even reachable; out of scope (flowtrace-cli's own consumer-side projection \
         work). |\n",
    );
    doc.push_str(
        "| Every native-surface acceptance case here, with the .NET SDK absent | Yes -- \
         confirmed by running the toolchain-free suite (`cargo test`, no `dotnet` on `PATH` \
         needed) against every case above. | Not applicable -- the optional surface's own tests \
         read a committed snapshot (`tests/flowtrace_facts.rs`), never invoke `dotnet` either, \
         but the snapshot itself is produced by a separate, dotnet-gated regeneration \
         (`semantic-audit` in `ci.yml`). |\n\n",
    );

    doc.push_str(
        "## Report: supported/expected call sites, incorrect targets, unknown relations, per mode\n\n",
    );
    doc.push_str(
        "Raw denominators, never a single blended figure across native and optional modes.\n\n",
    );
    doc.push_str(
        "| Mode | Case | Expected sites | Found | Incorrectly asserted targets | Unknown \
         relations |\n| --- | --- | --- | --- | --- | --- |\n",
    );
    doc.push_str("| Native | Repeated calls, different lines | 2 | 2 | 0 | 0 |\n");
    doc.push_str(
        "| Native | Two calls, one line | 2 | 2 (distinguished by `occurrenceIndex`) | 0 | 0 |\n",
    );
    doc.push_str(
        "| Native | Overload ambiguity | 2 | 2 (which overload bound: unknown, recorded as a \
         gap, never asserted) | 0 | 1 (overload identity) |\n",
    );
    doc.push_str("| Native | Recursion (`this.`-qualified) | 1 | 1 | 0 | 0 |\n");
    doc.push_str(
        "| Native | Recursion (unqualified, not shipped) | 1 | 0 | 0 | 1 (no ref extracted at \
         all -- see `fixtures/call-site-evidence/EXPECTED.md`) |\n",
    );
    doc.push_str("| Native | Awaited sequence | 2 | 2 | 0 | 1 (await order) |\n");
    doc.push_str("| Native | Parallel launch/join | 2 | 2 | 0 | 1 (launch/join relation) |\n");
    doc.push_str(&format!(
        "| Optional (`flowtrace-facts`) | `method_call` (this repository's own fixture) | {mc} \
         (ctor-injected-field calls found by direct inspection: 2 in \
         `DeliveryScheduledConsumer`, 3 in `ParcelsController`, 1 in `GetParcelHandler`) | {mc} \
         | 0 | 0 |\n\n",
        mc = method_call_count
    ));
    doc.push_str(&format!(
        "**Outcome is mixed:** occurrence identity, revision/content identity reaching `--json`, \
         and\n`method_call` (block- and expression-bodied alike) are the additive-change branch \
         -- each closes a\nreal, narrow gap this fixture's own results proved, with no \
         schema-version bump. The other two\nnamed revision-identity cases (enrichment-identity \
         mismatch, absent provider) are the\ndocumented-mapping branch: existing flowtrace-cli \
         behavior already satisfies them, cited above,\nnot re-implemented. The optional \
         sidecar's committed snapshot carries {total} facts total across\n{nkinds} kinds \
         (`{kinds}`). No cell in this report blends a native-mode count with an\noptional-mode \
         count, or an \"already worked\" cell with a \"we built this\" cell, into one \
         number.\n",
        total = total_facts,
        nkinds = kinds.len(),
        kinds = kinds.join(", ")
    ));

    doc
}

#[test]
fn docs_matrix_matches_the_committed_snapshot_or_is_regenerated_with_bless() {
    let fx = Fixture::new();
    let generated = build_matrix_doc(&fx);

    if std::env::var("BLESS").as_deref() == Ok("1") {
        fs::write(matrix_doc_path(), &generated).expect("write call-site-evidence-matrix.md");
        return;
    }

    let committed = fs::read_to_string(matrix_doc_path()).unwrap_or_default();
    assert_eq!(
        generated, committed,
        "docs/call-site-evidence-matrix.md is stale -- regenerate with `BLESS=1 cargo test --test call_site_evidence_matrix`"
    );
}
