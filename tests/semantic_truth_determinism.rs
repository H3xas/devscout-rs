//! Offline determinism: two in-process report-generation runs from the
//! same pinned inputs produce byte-identical output, and the syntax-only
//! and enriched lanes are persisted as separate artifacts, never summed.
//! No step in this file touches `dotnet` or the network.

use std::path::Path;

use devscout_rs::truth::manifest::parse_manifest;
use devscout_rs::truth::pin::compute_pin;
use devscout_rs::truth::report::{
    evaluate_case, lane_artifact_filename, report_to_json, write_lane_artifact, CaseOutcome, Lane,
    ObservedCase, ObservedFact, PinnedInputHeader, TruthReport,
};
use devscout_rs::truth::uncertainty::{ContextHealth, Uncertainty};

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth")
}

fn manifest_path() -> std::path::PathBuf {
    fixture_root().join("manifest.json")
}

fn reports_dir() -> std::path::PathBuf {
    fixture_root().join("reports")
}

/// Every fixture source file under `fixtures/csharp-truth/src/`, read from
/// disk and returned as `(repository-relative path, bytes)` pairs -- the
/// fixture pack `compute_pin` covers alongside the manifest, so a consumer
/// that edits a fixture source sees the committed pin change rather than
/// staying silently stale.
fn fixture_source_files() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(&fixture_root().join("src"), &fixture_root(), &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn pin() -> devscout_rs::truth::pin::Pin {
    let manifest_bytes = std::fs::read(manifest_path()).unwrap();
    let fixtures = fixture_source_files();
    let fixture_refs: Vec<(&str, &[u8])> = fixtures
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    compute_pin("semantic-truth-v1", &manifest_bytes, &fixture_refs)
}

/// The syntax-only lane's observations are synthesized from the manifest's
/// own `present` expectations (`build_syntax_only_report` below) -- no
/// producer output is read on this path. `producer_versions` is therefore
/// left empty rather than naming a producer that never ran, and
/// `corpus_revision` self-marks the lane the same way the enriched
/// stand-in already does, so a reader of the committed artifact alone
/// cannot mistake this for a graded run. See
/// `fixtures/csharp-truth/README.md`.
fn header() -> PinnedInputHeader {
    PinnedInputHeader {
        contract: "semantic-truth-v1".to_string(),
        corpus_revision:
            "fixtures/csharp-truth (synthetic syntax-only lane: observations are the manifest's own present-fact expectations; no producer executed)"
                .to_string(),
        producer_versions: vec![],
        resolved_context: "complete".to_string(),
        commands: vec!["cargo test --test semantic_truth_determinism".to_string()],
        pin: pin(),
    }
}

fn build_syntax_only_report() -> TruthReport {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let outcomes = manifest
        .cases
        .iter()
        .map(|case| {
            let observed = ObservedCase {
                context: ContextHealth::Complete,
                facts: case
                    .present
                    .iter()
                    .map(|p| ObservedFact {
                        identity: p.identity.clone(),
                        occurrence: p.occurrence.clone(),
                        state: Uncertainty::Confirmed,
                    })
                    .collect(),
                diagnostics: vec![],
            };
            CaseOutcome {
                case_id: case.id.clone(),
                verdict: evaluate_case(case, &observed),
            }
        })
        .collect();
    TruthReport {
        lane: Lane::SyntaxOnly,
        header: header(),
        outcomes,
    }
}

#[test]
fn two_runs_from_the_same_pinned_manifest_are_byte_identical() {
    let first = report_to_json(&build_syntax_only_report());
    let second = report_to_json(&build_syntax_only_report());
    assert_eq!(first, second);
}

#[test]
fn the_report_carries_its_pinned_input_header() {
    let json = report_to_json(&build_syntax_only_report());
    assert!(json.contains("\"pinnedInputs\""));
    assert!(json.contains("\"corpusRevision\":\"fixtures/csharp-truth (synthetic syntax-only lane"));
    assert!(json.contains("\"commands\""));
}

/// The committed pin covers the fixture pack, not only the manifest: a
/// producer whose file bytes changed but whose manifest entry did not must
/// still produce a different pin, or a consumer verifying the pin could
/// never detect an edited fixture source.
#[test]
fn editing_a_fixture_source_changes_the_pin() {
    let manifest_bytes = std::fs::read(manifest_path()).unwrap();
    let unedited = fixture_source_files();
    let unedited_refs: Vec<(&str, &[u8])> = unedited
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    let baseline_pin = compute_pin("semantic-truth-v1", &manifest_bytes, &unedited_refs);

    let mut edited = unedited.clone();
    let (_, first_bytes) = edited
        .first_mut()
        .expect("the fixture pack must contain at least one source file");
    first_bytes
        .extend_from_slice(b"\n// a single trailing byte the manifest does not know about\n");
    let edited_refs: Vec<(&str, &[u8])> = edited
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    let edited_pin = compute_pin("semantic-truth-v1", &manifest_bytes, &edited_refs);

    assert_ne!(
        baseline_pin, edited_pin,
        "an edited fixture source must change the pin even when manifest.json is untouched"
    );
    assert_eq!(
        baseline_pin,
        pin(),
        "the unedited fixture pack must reproduce the committed pin"
    );
}

/// The enriched lane has no real producer to grade yet; this synthetic,
/// explicitly marked stand-in proves the report shape and lane-separation
/// invariant hold structurally. It is not, and must not be read as, a real
/// enrichment-quality claim -- see the fixture pack's own README.
#[test]
fn the_syntax_only_and_synthetic_enriched_lanes_are_never_summed() {
    let syntax_only = build_syntax_only_report();
    let enriched_stand_in = TruthReport {
        lane: Lane::Enriched,
        header: PinnedInputHeader {
            corpus_revision: "synthetic-enriched-stand-in-pending-compiler-fact-enrichment"
                .to_string(),
            ..header()
        },
        outcomes: vec![CaseOutcome {
            case_id: "synthetic-enriched-stand-in".to_string(),
            verdict: devscout_rs::truth::report::CaseVerdict::Fail {
                reasons: vec!["no real enriched producer exists yet".to_string()],
            },
        }],
    };

    let syntax_json = report_to_json(&syntax_only);
    let enriched_json = report_to_json(&enriched_stand_in);
    assert_ne!(syntax_json, enriched_json);
    assert!(syntax_json.contains("\"lane\":\"syntax-only\""));
    assert!(enriched_json.contains("\"lane\":\"enriched\""));
    assert!(enriched_json.contains("synthetic-enriched-stand-in"));
}

fn build_enriched_stand_in_report() -> TruthReport {
    TruthReport {
        lane: Lane::Enriched,
        header: PinnedInputHeader {
            corpus_revision: "synthetic-enriched-stand-in-pending-compiler-fact-enrichment"
                .to_string(),
            ..header()
        },
        outcomes: vec![CaseOutcome {
            case_id: "synthetic-enriched-stand-in".to_string(),
            verdict: devscout_rs::truth::report::CaseVerdict::Fail {
                reasons: vec!["no real enriched producer exists yet".to_string()],
            },
        }],
    }
}

/// Both lanes are committed files under `fixtures/csharp-truth/reports/`,
/// not strings that only ever lived inside this test process -- a consumer
/// reads them the same way it reads `manifest.json` or
/// `capability-matrix.json`.
#[test]
fn the_syntax_only_lane_is_persisted_and_regenerates_byte_identical() {
    let committed =
        std::fs::read_to_string(reports_dir().join(lane_artifact_filename(Lane::SyntaxOnly)))
            .expect("fixtures/csharp-truth/reports/syntax-only.json must be readable");
    let regenerated = report_to_json(&build_syntax_only_report());
    assert_eq!(
        committed.trim_end(),
        regenerated,
        "regenerate fixtures/csharp-truth/reports/syntax-only.json and re-commit it"
    );
}

#[test]
fn the_enriched_stand_in_lane_is_persisted_and_regenerates_byte_identical() {
    let committed =
        std::fs::read_to_string(reports_dir().join(lane_artifact_filename(Lane::Enriched)))
            .expect("fixtures/csharp-truth/reports/enriched.json must be readable");
    let regenerated = report_to_json(&build_enriched_stand_in_report());
    assert_eq!(
        committed.trim_end(),
        regenerated,
        "regenerate fixtures/csharp-truth/reports/enriched.json and re-commit it"
    );
}

#[test]
fn a_report_can_be_written_to_and_read_back_from_its_lane_artifact_path() {
    let tmp = std::env::temp_dir().join(format!(
        "truth-lane-artifact-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let report = build_syntax_only_report();
    write_lane_artifact(&report, &tmp).unwrap();
    let path = tmp.join(lane_artifact_filename(Lane::SyntaxOnly));
    let persisted = std::fs::read_to_string(&path).unwrap();
    assert_eq!(persisted, report_to_json(&report));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Every case id the persisted manifest declares joins to an outcome row in
/// the persisted syntax-only report, by id -- the same join key a consumer
/// of the two committed files would use, exercised against real files on
/// disk rather than in-memory values only.
#[test]
fn every_manifest_case_id_joins_to_an_outcome_in_the_persisted_syntax_only_report() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let report_json =
        std::fs::read_to_string(reports_dir().join(lane_artifact_filename(Lane::SyntaxOnly)))
            .unwrap();
    for case in &manifest.cases {
        assert!(
            report_json.contains(&format!("\"caseId\":\"{}\"", case.id)),
            "case '{}' in the persisted manifest has no joined outcome in the persisted report",
            case.id
        );
    }
}
