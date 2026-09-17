//! Offline determinism: two in-process report-generation runs from the
//! same pinned inputs produce byte-identical output, and the syntax-only
//! and enriched lanes are persisted as separate artifacts, never summed.
//! No step in this file touches `dotnet` or the network.

use std::path::Path;

use devscout_rs::truth::manifest::parse_manifest;
use devscout_rs::truth::report::{
    evaluate_case, report_to_json, CaseOutcome, Lane, ObservedCase, ObservedFact,
    PinnedInputHeader, TruthReport,
};
use devscout_rs::truth::uncertainty::{ContextHealth, Uncertainty};

fn manifest_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/manifest.json")
}

fn header() -> PinnedInputHeader {
    PinnedInputHeader {
        contract: "semantic-truth-v1".to_string(),
        corpus_revision: "fixtures/csharp-truth".to_string(),
        producer_versions: vec![("scout-semantic".to_string(), "0.6.0".to_string())],
        resolved_context: "complete".to_string(),
        commands: vec!["cargo test --test semantic_truth_determinism".to_string()],
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
    assert!(json.contains("\"corpusRevision\":\"fixtures/csharp-truth\""));
    assert!(json.contains("\"commands\""));
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
