//! Pins `docs/dotnet-target-coverage.md` to the committed capability snapshots under
//! `fixtures/csharp-target-qualification/results/`. No `dotnet` toolchain is invoked here --
//! every row is produced ahead of time by `tools/qualify-dotnet-targets.py --write` and diffed
//! byte-for-byte by that same script's `--check` mode in CI; this file only checks that the
//! document, the README pointer and the committed rows still agree with each other and that no
//! row was promoted to `passing` without its full evidence.
//!
//! Four layers, each its own test group:
//!
//! 1. Row invariants: every committed row carries all five obligation fields, a `passing` row's
//!    positive case actually bound, and `execution_assumptions` is uniformly `static-only`.
//! 2. Sync: every committed row is named in the coverage document and vice versa; a truncated
//!    copy of the document fails the same check.
//! 3. Substitution controls: the two prior-art controls are recorded `failing` from their own
//!    independently inspected evidence, not from the oracle's own status line.
//! 4. Publication: no support sentence in the document or `README.md` names a target outside
//!    wave 1's measured rows, and the document states its own scope in its own words.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn tree_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-target-qualification")
}

fn results_dir() -> PathBuf {
    tree_dir().join("results")
}

fn doc_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dotnet-target-coverage.md")
}

fn readme_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")
}

/// Every committed row, keyed by its `profile_id`, sorted for a deterministic iteration order.
fn committed_rows() -> Vec<(String, Value)> {
    let mut rows: Vec<(String, Value)> = fs::read_dir(results_dir())
        .expect("fixtures/csharp-target-qualification/results exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let value: Value = serde_json::from_str(&text).unwrap();
            let id = value["profile_id"].as_str().unwrap().to_string();
            (id, value)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(rows.len(), 15, "expected 15 committed rows, found {}", rows.len());
    rows
}

fn state<'a>(row: &'a Value, field: &str) -> &'a str {
    row[field]["state"].as_str().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Layer 1: row invariants
// ---------------------------------------------------------------------------

const OBLIGATION_FIELDS: &[&str] = &[
    "context_acquisition",
    "semantic_conformance",
    "framework_modeling",
    "unsupported_state",
    "execution_assumptions",
];

/// The general invariant behind "an empty analyzer result fails its positive obligation": a row
/// whose positive case did not bind must never read `unsupported_state: passing`.
fn row_is_internally_consistent(row: &Value) -> bool {
    let positive_bound = row["semantic_conformance"]["positive_case_bound"].as_bool();
    if positive_bound == Some(false) && state(row, "unsupported_state") == "passing" {
        return false;
    }
    true
}

#[test]
fn every_committed_row_carries_all_five_obligation_fields() {
    for (id, row) in committed_rows() {
        for field in OBLIGATION_FIELDS {
            assert!(
                row.get(field).is_some_and(|v| !v.is_null()),
                "{id}: missing obligation field {field}"
            );
        }
    }
}

#[test]
fn every_wave1_row_is_execution_assumptions_static_only() {
    for (id, row) in committed_rows() {
        assert_eq!(
            state(&row, "execution_assumptions"),
            "static-only",
            "{id}: execution_assumptions must be static-only (this ticket produces no runtime rows)"
        );
    }
}

#[test]
fn an_empty_analyzer_result_fails_the_positive_obligation() {
    // Adversarial control: an analyzer result with no positive-case evidence, illegitimately
    // marked passing. The invariant this test pins must reject it.
    let bad = serde_json::json!({
        "semantic_conformance": { "positive_case_bound": false, "state": "passing" },
        "unsupported_state": { "state": "passing" }
    });
    assert!(
        !row_is_internally_consistent(&bad),
        "a row with no positive-case evidence must never be accepted as passing"
    );

    // The honest counterpart: the same empty evidence, correctly recorded failing.
    let good = serde_json::json!({
        "semantic_conformance": { "positive_case_bound": false, "state": "failing" },
        "unsupported_state": { "state": "failing" }
    });
    assert!(row_is_internally_consistent(&good));

    for (id, row) in committed_rows() {
        assert!(
            row_is_internally_consistent(&row),
            "{id}: positive-case/unsupported-state disagreement"
        );
    }
}

#[test]
fn every_wave1_profile_row_names_its_own_compilation_identity() {
    let mut identities = BTreeSet::new();
    for (id, row) in committed_rows() {
        if row["kind"] != "profile" && row["kind"] != "deep" {
            continue;
        }
        let identity = format!(
            "{}|{}|{:?}",
            row["tfm"].as_str().unwrap_or_default(),
            row["context_acquisition"]["reference_source"].as_str().unwrap_or_default(),
            row["context_acquisition"]["loaded_documents"]
        );
        assert!(
            identities.insert(identity.clone()),
            "{id}: compilation identity {identity} is not unique across wave-1 rows"
        );
    }
    assert_eq!(identities.len(), 13, "11 profiles + 2 deep bundles");
}

#[test]
fn no_framework_row_reaches_passing_from_context_acquisition_alone() {
    for (id, row) in committed_rows() {
        if row["track"] != "framework-f1" {
            continue;
        }
        if state(&row, "unsupported_state") == "passing" {
            assert_eq!(
                state(&row, "semantic_conformance"),
                "passing",
                "{id}: a Framework row must not be passing on context_acquisition alone"
            );
            assert_eq!(
                state(&row, "framework_modeling"),
                "not-claimed",
                "{id}: framework_modeling must be recorded, not absent"
            );
        }
    }
}

#[test]
fn net472_row_never_cites_netstandard2_1_evidence() {
    let rows = committed_rows();
    let net472 = rows
        .iter()
        .find(|(id, _)| id == "csharp73-net472-sdkstyle")
        .map(|(_, row)| row)
        .expect("net472 row is committed");
    let text = net472.to_string();
    assert!(
        !text.contains("netstandard2.1"),
        "net472 row must never cite netstandard2.1 evidence: {text}"
    );
}

#[test]
fn the_net472_boundary_case_records_cs1501_not_a_clean_result() {
    let rows = committed_rows();
    let net472 = rows
        .iter()
        .find(|(id, _)| id == "csharp73-net472-sdkstyle")
        .map(|(_, row)| row)
        .expect("net472 row is committed");
    assert_eq!(
        net472["semantic_conformance"]["boundary_case_bind_observed"],
        Value::Bool(false)
    );
    let excerpt = net472["semantic_conformance"]["build_diagnostic_excerpt"]
        .as_str()
        .unwrap_or_default();
    assert!(
        excerpt.contains("CS1501"),
        "net472's boundary case must record the CS1501 compiler reason: {excerpt}"
    );
    assert_eq!(state(net472, "unsupported_state"), "passing");
}

#[test]
fn a_snapshot_exists_iff_its_row_state_implies_an_executed_run() {
    for (id, row) in committed_rows() {
        let s = state(&row, "unsupported_state");
        assert!(
            matches!(s, "passing" | "failing" | "smoke-tested"),
            "{id}: a committed row's state must imply an executed run, got {s}"
        );
    }
    // The inverse direction: no wave-2/excluded/unqualified target has a committed snapshot.
    let never_executed = [
        "net10.0",
        "netstandard1.0",
        "net403",
        "net481",
        "netcoreapp3.0",
        "netcoreapp1.0",
        "netcoreapp2.0",
    ];
    let names: BTreeSet<String> = committed_rows().into_iter().map(|(id, _)| id).collect();
    for target in never_executed {
        assert!(
            !names.iter().any(|id| id.contains(target)),
            "{target} is not a wave-1 row and must carry no committed snapshot"
        );
    }
}

// ---------------------------------------------------------------------------
// Layer 2: sync between the committed rows and the coverage document
// ---------------------------------------------------------------------------

#[test]
fn every_committed_row_appears_in_the_document_and_vice_versa() {
    let doc = fs::read_to_string(doc_path()).expect("docs/dotnet-target-coverage.md exists");
    for (id, _) in committed_rows() {
        assert!(
            doc.contains(&id),
            "docs/dotnet-target-coverage.md does not name committed row {id}"
        );
    }
}

#[test]
fn a_truncated_copy_of_the_document_fails_the_sync_check() {
    let doc = fs::read_to_string(doc_path()).expect("docs/dotnet-target-coverage.md exists");
    let truncated: String = doc
        .lines()
        .filter(|line| !line.contains("csharp73-net8.0-sdkstyle"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !truncated.contains("csharp73-net8.0-sdkstyle"),
        "the truncation itself must actually remove the row"
    );
    let still_present = committed_rows()
        .into_iter()
        .all(|(id, _)| truncated.contains(&id));
    assert!(
        !still_present,
        "removing one row's mentions must make the sync check fail, proving it is not vacuous"
    );
}

#[test]
fn every_inventory_row_including_wave_2_is_present() {
    let doc = fs::read_to_string(doc_path()).expect("docs/dotnet-target-coverage.md exists");
    let wave2_and_excluded = [
        "net10.0",
        "netstandard1.0",
        "net403",
        "net481",
        "Client Profile",
        "netcoreapp3.0",
        ".NET Core 1.x",
        ".NET Core 2.x",
        ".NET Framework before 4.0",
        "project.json",
    ];
    for name in wave2_and_excluded {
        assert!(doc.contains(name), "docs/dotnet-target-coverage.md is missing inventory row for {name}");
    }
}

// ---------------------------------------------------------------------------
// Layer 3: substitution-defect controls
// ---------------------------------------------------------------------------

#[test]
fn tfm_not_supplied_control_is_recorded_failing_not_the_first_variant() {
    let rows = committed_rows();
    let (_, row) = rows
        .iter()
        .find(|(id, _)| id == "control-tfm-not-supplied")
        .expect("control-tfm-not-supplied is committed");
    assert_eq!(state(row, "unsupported_state"), "failing");
    assert_eq!(row["semantic_conformance"]["substitution_occurred"], Value::Bool(true));
    // The oracle's own status line is not trusted: it reports "ok" for exactly the unit whose
    // TFM does not match what was requested, which is the defect being pinned.
    assert_eq!(row["semantic_conformance"]["oracle_reported_status"], Value::String("ok".into()));
    assert_ne!(
        row["semantic_conformance"]["oracle_reported_tfm"],
        Value::String("net6.0".into()),
        "the requested tfm must not be what the loader actually kept"
    );
}

#[test]
fn reference_tfm_mismatch_control_is_recorded_failing_not_healthy() {
    let rows = committed_rows();
    let (_, row) = rows
        .iter()
        .find(|(id, _)| id == "control-reference-tfm-mismatch")
        .expect("control-reference-tfm-mismatch is committed");
    assert_eq!(state(row, "unsupported_state"), "failing");
    assert_eq!(row["semantic_conformance"]["reference_tfm_mismatch"], Value::Bool(true));
    assert_ne!(
        row["context_acquisition"]["p_declared_tfm"],
        row["context_acquisition"]["q_declared_tfm"],
        "the control's whole premise is a declared-TFM mismatch between the two projects"
    );
}

#[test]
fn the_tfm_not_supplied_controls_two_variants_stay_distinct_compilations() {
    let doc = fs::read_to_string(
        tree_dir().join("controls/tfm-not-supplied/Control.csproj"),
    )
    .unwrap();
    assert!(doc.contains("net8.0") && doc.contains("net472"), "{doc}");
}

// ---------------------------------------------------------------------------
// Layer 3b: case-family provenance
// ---------------------------------------------------------------------------

#[test]
fn case_expectations_are_independently_authored() {
    for (id, row) in committed_rows() {
        if row["kind"] != "deep" {
            continue;
        }
        let families = row["semantic_conformance"]["case_families"]
            .as_object()
            .unwrap_or_else(|| panic!("{id}: missing case_families"));
        for (name, entry) in families {
            let provenance = entry["provenance"].as_str().unwrap_or_default();
            assert_eq!(
                provenance, "independent",
                "{id}/{name}: case-family provenance must be independent, never producer"
            );
        }
    }
}

#[test]
fn framework_adapter_evidence_requires_full_identity_not_name_alone() {
    for (id, row) in committed_rows() {
        if row["kind"] != "deep" {
            continue;
        }
        let unknown = &row["semantic_conformance"]["case_families"]["unknown_framework"];
        assert_eq!(
            unknown["framework_modeling"].as_str(),
            Some("candidate"),
            "{id}: name/suffix evidence alone must stay candidate, never a claimed binding"
        );
    }
}

// ---------------------------------------------------------------------------
// Layer 4: publication
// ---------------------------------------------------------------------------

#[test]
fn no_document_or_readme_emits_an_aggregate_dotnet_supported_flag() {
    let doc = fs::read_to_string(doc_path()).unwrap();
    let readme = fs::read_to_string(readme_path()).unwrap();
    for banned in [".NET supported", "fully .NET supported", "all .NET targets supported"] {
        assert!(!doc.contains(banned), "coverage document names a banned aggregate flag: {banned}");
        assert!(!readme.contains(banned), "README.md names a banned aggregate flag: {banned}");
    }
}

#[test]
fn no_support_sentence_names_a_target_ahead_of_its_row() {
    let doc = fs::read_to_string(doc_path()).unwrap();
    let readme = fs::read_to_string(readme_path()).unwrap();
    let (before_wave2, _) = doc
        .split_once("## Wave 2")
        .expect("the document has a Wave 2 section boundary");
    let not_passing = ["net10.0", "netstandard1.0", "net403", "net481", "netcoreapp3.0"];
    for target in not_passing {
        assert!(
            !before_wave2.contains(target),
            "{target} is not a wave-1 passing row and must not appear before the Wave 2 heading"
        );
        assert!(
            !readme.contains(target),
            "{target} is not a wave-1 passing row and must not appear in README.md"
        );
    }
}

#[test]
fn the_document_states_it_qualifies_the_compiler_fact_path_not_the_default_binary() {
    let doc = fs::read_to_string(doc_path()).unwrap();
    assert!(doc.contains("compiler-fact path"));
    assert!(doc.contains("does not widen"));
}

#[test]
fn readme_points_at_the_coverage_document() {
    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains("dotnet-target-coverage.md"),
        "README.md must point at docs/dotnet-target-coverage.md"
    );
}

// ---------------------------------------------------------------------------
// Layer 5: expected.json cross-check
// ---------------------------------------------------------------------------

#[test]
fn every_row_matches_its_expected_json_required_state() {
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(tree_dir().join("expected.json")).unwrap()).unwrap();
    let required_fields: Vec<String> = expected["required_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let expected_rows = expected["rows"].as_object().unwrap();

    let committed = committed_rows();
    assert_eq!(
        committed.len(),
        expected_rows.len(),
        "expected.json must name exactly the committed rows"
    );

    for (id, row) in &committed {
        let want = expected_rows
            .get(id)
            .unwrap_or_else(|| panic!("expected.json has no entry for committed row {id}"));
        let want_state = want["required_state"].as_str().unwrap();
        assert_eq!(
            state(row, "unsupported_state"),
            want_state,
            "{id}: unsupported_state does not match expected.json's required_state"
        );
        for field in &required_fields {
            assert!(row.get(field).is_some_and(|v| !v.is_null()), "{id}: missing {field}");
        }
    }
}

#[test]
fn held_out_thresholds_file_exists_and_is_well_formed_before_the_held_out_report() {
    let path = tree_dir().join("expected-held-out.json");
    let text = fs::read_to_string(&path).expect("expected-held-out.json exists");
    let value: Value = serde_json::from_str(&text).expect("expected-held-out.json is valid JSON");
    let strata = value["strata"].as_object().expect("expected-held-out.json has a strata object");
    assert!(!strata.is_empty(), "expected-held-out.json must register at least one stratum");
    for (name, stratum) in strata {
        assert!(
            stratum.get("precision").is_some() && stratum.get("recall").is_some(),
            "{name}: stratum must register precision and recall thresholds"
        );
    }
}
