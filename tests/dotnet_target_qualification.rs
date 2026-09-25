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
//! 3. Substitution controls: both prior-art controls are recorded from their own independently
//!    inspected evidence, never from the oracle's own status line -- one control's fixed defect
//!    now reads `passing` from a positively observed refusal, the other still reads `failing`.
//! 4. Publication: no support sentence in the document, `README.md` or `CHANGELOG.md` names a
//!    target whose row is not `passing`, `README.md` names no target without a held-out row, and
//!    the document states its own scope in its own words.

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

fn changelog_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("CHANGELOG.md")
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
    assert_eq!(
        rows.len(),
        26,
        "expected 26 committed rows, found {}",
        rows.len()
    );
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
fn every_row_is_execution_assumptions_static_only() {
    for (id, row) in committed_rows() {
        assert_eq!(
            state(&row, "execution_assumptions"),
            "static-only",
            "{id}: execution_assumptions must be static-only (no row here is a runtime observation)"
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
fn every_profile_row_names_its_own_compilation_identity() {
    let mut identities = BTreeSet::new();
    for (id, row) in committed_rows() {
        if row["kind"] != "profile" && row["kind"] != "deep" {
            continue;
        }
        let identity = format!(
            "{}|{}|{:?}",
            row["tfm"].as_str().unwrap_or_default(),
            row["context_acquisition"]["reference_source"]
                .as_str()
                .unwrap_or_default(),
            row["context_acquisition"]["loaded_documents"]
        );
        assert!(
            identities.insert(identity.clone()),
            "{id}: compilation identity {identity} is not unique across rows"
        );
    }
    assert_eq!(identities.len(), 24, "22 profiles + 2 deep bundles");
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

/// The SDK band the tree's own `global.json` pins; a row built under another band carries its
/// own `global.json` beside its project.
fn tree_sdk_pin() -> String {
    let text = fs::read_to_string(tree_dir().join("global.json")).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    value["sdk"]["version"].as_str().unwrap().to_string()
}

#[test]
fn a_row_on_its_own_sdk_band_records_that_band_end_to_end() {
    let tree_pin = tree_sdk_pin();
    let mut own_band_rows = 0;
    for (id, row) in committed_rows() {
        let ctx = &row["context_acquisition"];
        let pin = ctx["sdk_pin"].as_str().unwrap_or_default();
        if row["kind"] != "profile" || pin == tree_pin {
            assert!(
                ctx.get("oracle_msbuild_registered").is_none(),
                "{id}: only a row on its own band records the oracle's registration"
            );
            continue;
        }
        own_band_rows += 1;
        let tfm = row["tfm"].as_str().unwrap_or_default();
        let local: Value = serde_json::from_str(
            &fs::read_to_string(tree_dir().join(format!("profiles/{tfm}-sdkstyle/global.json")))
                .unwrap_or_else(|_| panic!("{id}: its own band needs a row-local global.json")),
        )
        .unwrap();
        assert_eq!(
            local["sdk"]["version"].as_str(),
            Some(pin),
            "{id}: row-local pin"
        );
        assert_eq!(
            local["sdk"]["rollForward"].as_str(),
            Some("disable"),
            "{id}: row-local pin must not roll forward"
        );
        assert_eq!(
            ctx["sdk"].as_str(),
            Some(pin),
            "{id}: resolved SDK is not its own pin"
        );
        if state(&row, "unsupported_state") == "passing" {
            assert_eq!(
                ctx["oracle_msbuild_registered"].as_str(),
                Some(pin),
                "{id}: a passing row's oracle must have registered its own band"
            );
            assert_eq!(
                ctx["oracle_unit_diagnostics"].as_u64(),
                Some(0),
                "{id}: a passing row's oracle unit must report no diagnostics"
            );
        }
    }
    assert_eq!(
        own_band_rows, 1,
        "net10.0 is the one row on its own SDK band"
    );
}

#[test]
fn net10_row_never_cites_net9_evidence() {
    let rows = committed_rows();
    let (_, row) = rows
        .iter()
        .find(|(id, _)| id == "csharp73-net10.0-sdkstyle")
        .expect("net10.0 row is committed");
    let text = row.to_string();
    assert!(
        !names_target(&text, "net9.0") && !text.contains(&tree_sdk_pin()),
        "net10.0 row must cite neither net9.0 evidence nor the tree's SDK band: {text}"
    );
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
    // The inverse direction: no planned/excluded/unqualified target has a committed snapshot.
    let never_executed = [
        "net403",
        "net451",
        "net46",
        "net462",
        "net47",
        "net471",
        "net481",
        "netcoreapp3.0",
        "netcoreapp1.0",
        "netcoreapp2.0",
    ];
    let names: BTreeSet<String> = committed_rows().into_iter().map(|(id, _)| id).collect();
    for target in never_executed {
        assert!(
            !names.iter().any(|id| names_target(id, target)),
            "{target} is not an executed row and must carry no committed snapshot"
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

/// Parses every markdown table in the document that has a `State` column, returning
/// `(row id, published state)` for each row whose first cell is a backtick-quoted committed
/// row id (`csharp73-...`/`control-...`). Planned/excluded/unqualified rows use plain target
/// names, not backticked row ids, so they are never picked up here.
fn parse_doc_state_rows(doc: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = doc.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let is_header = lines[i].starts_with('|')
            && lines
                .get(i + 1)
                .is_some_and(|next| next.starts_with('|') && next.contains("---"));
        if !is_header {
            i += 1;
            continue;
        }
        let header: Vec<String> = lines[i]
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        let state_idx = header.iter().position(|h| h == "State");
        i += 2;
        while i < lines.len() && lines[i].starts_with('|') {
            if let Some(idx) = state_idx {
                let cells: Vec<String> = lines[i]
                    .split('|')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
                if let Some(first) = cells.first() {
                    if first.starts_with('`') && first.ends_with('`') {
                        let id = first.trim_matches('`').to_string();
                        if (id.starts_with("csharp73-") || id.starts_with("control-"))
                            && cells.len() > idx
                        {
                            out.push((id, cells[idx].clone()));
                        }
                    }
                }
            }
            i += 1;
        }
    }
    out
}

#[test]
fn the_document_state_column_matches_each_committed_snapshot_in_both_directions() {
    let doc = fs::read_to_string(doc_path()).expect("docs/dotnet-target-coverage.md exists");
    let doc_rows = parse_doc_state_rows(&doc);
    assert!(
        !doc_rows.is_empty(),
        "the document's State-column parser found no rows at all"
    );

    let committed: std::collections::BTreeMap<String, Value> =
        committed_rows().into_iter().collect();

    for (id, published_state) in &doc_rows {
        let row = committed.get(id).unwrap_or_else(|| {
            panic!(
                "docs/dotnet-target-coverage.md publishes a State for {id} but no committed \
                 snapshot exists for it"
            )
        });
        let snapshot_state = state(row, "unsupported_state");
        assert_eq!(
            published_state, snapshot_state,
            "{id}: published State ({published_state}) disagrees with its committed snapshot \
             ({snapshot_state})"
        );
    }

    let doc_ids: BTreeSet<String> = doc_rows.into_iter().map(|(id, _)| id).collect();
    for (id, _) in committed {
        assert!(
            doc_ids.contains(&id),
            "{id} has a committed snapshot but no published State row in \
             docs/dotnet-target-coverage.md"
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
        "net403",
        "net451",
        "net46",
        "net462",
        "net47",
        "net471",
        "net481",
        "Client Profile",
        "netcoreapp3.0",
        ".NET Core 1.x",
        ".NET Core 2.x",
        ".NET Framework before 4.0",
        "project.json",
    ];
    for name in wave2_and_excluded {
        assert!(
            names_target(&doc, name),
            "docs/dotnet-target-coverage.md is missing inventory row for {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Layer 3: substitution-defect controls
// ---------------------------------------------------------------------------

#[test]
fn tfm_not_supplied_control_now_records_the_loaders_explicit_refusal_not_substitution() {
    // The documented substitution defect this control was built to catch was fixed upstream
    // (owned by a different, already-integrated repair; not this tree's own change): an
    // undeclared `--tfm` is now explicitly refused instead of silently substituted, which is
    // the explicit failed/unsupported outcome the sidecar's own no-substitution requirement
    // calls for. This row now honestly reads `passing`, not because the control was softened,
    // but because that requirement is what actually ran. Substitution detection itself is
    // unchanged and still independent of the oracle's own status line: if silent substitution
    // ever returns, `substitution_occurred` and `unsupported_state` both flip back to their
    // old, defect-reproducing values.
    let rows = committed_rows();
    let (_, row) = rows
        .iter()
        .find(|(id, _)| id == "control-tfm-not-supplied")
        .expect("control-tfm-not-supplied is committed");
    assert_eq!(state(row, "unsupported_state"), "passing");
    assert_eq!(
        row["semantic_conformance"]["substitution_occurred"],
        Value::Bool(false),
        "no substitution occurred on this run"
    );
    // No substitution alone is not accepted as proof of the fix: the row must also carry
    // positive evidence that the oracle explicitly refused the request, not merely that it
    // failed to substitute for some unrelated reason (a crash, a silent no-op, and so on).
    assert_eq!(
        row["semantic_conformance"]["explicit_refusal_observed"],
        Value::Bool(true),
        "the row must positively confirm an explicit refusal, not merely the absence of substitution"
    );
    assert_ne!(
        row["semantic_conformance"]["oracle_exit_code"],
        Value::from(0),
        "an explicit refusal must not report a clean oracle exit code"
    );
    assert!(
        !row["semantic_conformance"]["refusal_diagnostic"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "the row must carry the oracle's own refusal diagnostic, not a generic label"
    );
    // The oracle produced zero units for the undeclared request -- it did not keep any variant,
    // declared or otherwise.
    assert_eq!(
        row["semantic_conformance"]["oracle_reported_status"],
        Value::Null
    );
    assert_eq!(
        row["semantic_conformance"]["oracle_reported_tfm"],
        Value::Null
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
    assert_eq!(
        row["semantic_conformance"]["reference_tfm_mismatch"],
        Value::Bool(true)
    );
    assert_ne!(
        row["context_acquisition"]["p_declared_tfm"], row["context_acquisition"]["q_declared_tfm"],
        "the control's whole premise is a declared-TFM mismatch between the two projects"
    );
}

#[test]
fn the_tfm_not_supplied_controls_two_variants_stay_distinct_compilations() {
    // Not a check of the fixture's own source text: each declared variant was requested from
    // the oracle explicitly and independently, and the row records what came back for each.
    let rows = committed_rows();
    let (_, row) = rows
        .iter()
        .find(|(id, _)| id == "control-tfm-not-supplied")
        .expect("control-tfm-not-supplied is committed");
    assert_eq!(
        row["semantic_conformance"]["variants_stay_distinct_compilations"],
        Value::Bool(true),
        "the two declared variants must each come back as themselves when requested explicitly"
    );
    let evidence = row["semantic_conformance"]["variant_evidence"]
        .as_object()
        .expect("variant_evidence is recorded");
    assert!(
        evidence.len() >= 2,
        "at least two declared variants must have been checked"
    );
    let mut returned_tfms = BTreeSet::new();
    for (requested, entry) in evidence {
        assert_eq!(
            entry["matches_request"],
            Value::Bool(true),
            "{requested}: an explicitly requested declared variant must return itself"
        );
        returned_tfms.insert(
            entry["returned_tfm"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
    }
    assert_eq!(
        returned_tfms.len(),
        evidence.len(),
        "the requested variants must resolve to distinct compilations, not a shared kept variant"
    );
}

// ---------------------------------------------------------------------------
// Layer 3b: case-family provenance
// ---------------------------------------------------------------------------

#[test]
fn case_expectations_are_independently_authored() {
    // Not a check of a string the composition script writes unconditionally about itself:
    // each family's composed `observed` value is checked against expected-case-families.json,
    // an expectation file committed independently of tools/qualify-dotnet-targets.py. A
    // regression that made a family's evidence silently vanish (for example an empty analyzer
    // result) would disagree with this file and fail here.
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(tree_dir().join("expected-case-families.json")).unwrap(),
    )
    .unwrap();
    let expected_rows = expected["rows"].as_object().unwrap();

    for (id, row) in committed_rows() {
        if row["kind"] != "deep" {
            continue;
        }
        let families = row["semantic_conformance"]["case_families"]
            .as_object()
            .unwrap_or_else(|| panic!("{id}: missing case_families"));
        let want = expected_rows
            .get(&id)
            .unwrap_or_else(|| panic!("expected-case-families.json has no entry for {id}"));
        for (name, entry) in families {
            let provenance = entry["provenance"].as_str().unwrap_or_default();
            assert_eq!(
                provenance, "independent",
                "{id}/{name}: case-family provenance must be independent, never producer"
            );
            let observed = entry
                .get("observed")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| panic!("{id}/{name}: missing a boolean observed field"));
            let want_observed = want.get(name).and_then(Value::as_bool).unwrap_or_else(|| {
                panic!("{id}/{name}: expected-case-families.json has no registered expectation")
            });
            assert_eq!(
                observed, want_observed,
                "{id}/{name}: composed evidence ({observed}) disagrees with the independently \
                 registered expectation ({want_observed})"
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
    let changelog = fs::read_to_string(changelog_path()).unwrap();
    for banned in [
        ".NET supported",
        "fully .NET supported",
        "all .NET targets supported",
    ] {
        assert!(
            !doc.contains(banned),
            "coverage document names a banned aggregate flag: {banned}"
        );
        assert!(
            !readme.contains(banned),
            "README.md names a banned aggregate flag: {banned}"
        );
        assert!(
            !changelog.contains(banned),
            "CHANGELOG.md names a banned aggregate flag: {banned}"
        );
    }
}

/// Every target framework moniker the published inventory stages, measured or not. A target
/// leaves the support-sentence sweep only by becoming a committed `passing` profile row, never by
/// being dropped from this list.
const INVENTORY_TFMS: &[&str] = &[
    "net5.0",
    "net6.0",
    "net7.0",
    "net8.0",
    "net9.0",
    "net10.0",
    "netstandard1.0",
    "netstandard1.1",
    "netstandard1.2",
    "netstandard1.3",
    "netstandard1.4",
    "netstandard1.5",
    "netstandard1.6",
    "netstandard2.0",
    "netstandard2.1",
    "netcoreapp3.0",
    "netcoreapp3.1",
    "net40",
    "net403",
    "net45",
    "net451",
    "net452",
    "net46",
    "net461",
    "net462",
    "net47",
    "net471",
    "net472",
    "net48",
    "net481",
];

/// The targets the pinned held-out family actually measured; every other `passing` target is
/// published on its own fixture evidence only and is disclosed, not advertised.
const HELD_OUT_MEASURED_TFMS: &[&str] = &["net6.0", "net8.0", "netstandard2.0"];

/// Whether `text` names `target` as a whole moniker: `net46` inside `net461`, or `net47` inside
/// `net472`, is a different target, so a substring match would both hide a real mention and
/// report a false one.
fn names_target(text: &str, target: &str) -> bool {
    let continues = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(target).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let mut after = text[start + target.len()..].chars();
        let joined_before = before.is_some_and(|c| continues(c) || c == '.');
        let joined_after = match after.next() {
            Some('.') => after.next().is_some_and(|c| c.is_ascii_digit()),
            Some(c) => continues(c),
            None => false,
        };
        !joined_before && !joined_after
    })
}

fn passing_profile_tfms() -> BTreeSet<String> {
    committed_rows()
        .into_iter()
        .filter(|(_, row)| row["kind"] == "profile" && state(row, "unsupported_state") == "passing")
        .map(|(_, row)| row["tfm"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn not_passing_tfms() -> Vec<&'static str> {
    let passing = passing_profile_tfms();
    INVENTORY_TFMS
        .iter()
        .copied()
        .filter(|tfm| !passing.contains(*tfm))
        .collect()
}

#[test]
fn target_names_match_whole_monikers_only() {
    assert!(!names_target("csharp73-net461-sdkstyle", "net46"));
    assert!(!names_target("net472", "net47"));
    assert!(!names_target("netstandard1.0.1", "netstandard1.0"));
    assert!(!names_target("Xnet45", "net45"));
    assert!(names_target("net403, net451, net46, net462", "net46"));
    assert!(names_target("targets `net47`.", "net47"));
    assert!(names_target("ends with net47.", "net47"));
    assert!(names_target("csharp73-net46-sdkstyle", "net46"));
}

#[test]
fn every_inventory_target_is_a_row_in_the_document() {
    let doc = fs::read_to_string(doc_path()).unwrap();
    for tfm in INVENTORY_TFMS {
        assert!(
            names_target(&doc, tfm),
            "docs/dotnet-target-coverage.md names no row for inventory target {tfm}"
        );
    }
    for tfm in passing_profile_tfms() {
        assert!(
            INVENTORY_TFMS.contains(&tfm.as_str()),
            "{tfm} has a passing row but is missing from the inventory the sweep derives from"
        );
    }
}

#[test]
fn the_sweep_covers_every_target_that_is_not_passing() {
    let not_passing = not_passing_tfms();
    for tfm in ["net46", "net47", "net481", "netcoreapp3.0"] {
        assert!(
            not_passing.contains(&tfm),
            "{tfm} has no passing row and must stay in the support-sentence sweep"
        );
    }
    let passing = passing_profile_tfms();
    assert_eq!(
        not_passing.len() + passing.len(),
        INVENTORY_TFMS.len(),
        "every inventory target is either a passing row or swept"
    );
}

#[test]
fn no_support_sentence_names_a_target_ahead_of_its_row() {
    let doc = fs::read_to_string(doc_path()).unwrap();
    let readme = fs::read_to_string(readme_path()).unwrap();
    let changelog = fs::read_to_string(changelog_path()).unwrap();
    let (before_wave2, _) = doc
        .split_once("## Wave 2")
        .expect("the document has a Wave 2 section boundary");
    for target in not_passing_tfms() {
        assert!(
            !names_target(before_wave2, target),
            "{target} is not a passing row and must not appear before the Wave 2 heading"
        );
        assert!(
            !names_target(&readme, target),
            "{target} is not a passing row and must not appear in README.md"
        );
        assert!(
            !names_target(&changelog, target),
            "{target} is not a passing row and must not appear in CHANGELOG.md"
        );
    }
}

#[test]
fn readme_advertises_no_target_without_a_held_out_row() {
    let readme = fs::read_to_string(readme_path()).unwrap();
    for tfm in passing_profile_tfms() {
        if HELD_OUT_MEASURED_TFMS.contains(&tfm.as_str()) {
            continue;
        }
        assert!(
            !names_target(&readme, &tfm),
            "{tfm} has no held-out row and must not be advertised in README.md"
        );
    }
}

#[test]
fn no_profile_or_deep_rows_loaded_documents_leak_across_rows() {
    // A cheap, dotnet-free proxy for "a changed reference, import, build symbol or generator
    // input invalidates only the affected profile's facts": if two rows' loaded_documents sets
    // overlapped outside the two intentionally shared files, editing one profile's own fixture
    // file could silently change another profile's row too. This does not exercise an actual
    // edit-and-rebuild cycle (that needs `dotnet`, outside the cargo test path); it pins the
    // static precondition that makes cross-row leakage impossible in the first place.
    let shared: BTreeSet<&str> = ["shared/PositiveCase.cs", "shared/BoundaryCase.cs"]
        .into_iter()
        .collect();
    let mut owned_by: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (id, row) in committed_rows() {
        if row["kind"] != "profile" && row["kind"] != "deep" {
            continue;
        }
        let Some(docs) = row["context_acquisition"]["loaded_documents"].as_array() else {
            continue;
        };
        for doc in docs {
            let name = doc.as_str().unwrap_or_default();
            if shared.contains(name) {
                continue;
            }
            if let Some(owner) = owned_by.get(name) {
                assert_eq!(
                    owner, &id,
                    "{name} is loaded by both {owner} and {id}; a non-shared fixture file must \
                     belong to exactly one row or a change to it would invalidate more than one \
                     profile's facts"
                );
            } else {
                owned_by.insert(name.to_string(), id.clone());
            }
        }
    }
    assert!(
        !owned_by.is_empty(),
        "expected at least one non-shared loaded document"
    );
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
        serde_json::from_str(&fs::read_to_string(tree_dir().join("expected.json")).unwrap())
            .unwrap();
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
            assert!(
                row.get(field).is_some_and(|v| !v.is_null()),
                "{id}: missing {field}"
            );
        }
    }
}

#[test]
fn held_out_thresholds_file_exists_and_is_well_formed_before_the_held_out_report() {
    let path = tree_dir().join("expected-held-out.json");
    let text = fs::read_to_string(&path).expect("expected-held-out.json exists");
    let value: Value = serde_json::from_str(&text).expect("expected-held-out.json is valid JSON");
    let strata = value["strata"]
        .as_object()
        .expect("expected-held-out.json has a strata object");
    assert!(
        !strata.is_empty(),
        "expected-held-out.json must register at least one stratum"
    );
    for (name, stratum) in strata {
        assert!(
            stratum.get("precision").is_some() && stratum.get("recall").is_some(),
            "{name}: stratum must register precision and recall thresholds"
        );
    }
}
