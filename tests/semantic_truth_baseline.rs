//! The truthful red baseline and the pinned foundation candidate. Harness
//! health and analyzer verdict are reported separately; a mismatch the
//! baseline does not name is reported as new; and a disagreeing
//! consumer's output can never rewrite the pin.

use std::path::Path;

use devscout_rs::truth::pin::{compute_pin, verify_pin};
use devscout_rs::truth::producer_reader::native_dispatch_implements_edges;
use devscout_rs::truth::red_baseline::{
    classify_mismatches, evaluate_run, red_baseline_to_json, AnalyzerVerdict, HarnessHealth,
    GRADED_THROUGH_REAL_PRODUCER_OUTPUT, RED_BASELINE,
};

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth")
}

#[test]
fn the_committed_baseline_regenerates_byte_identical() {
    let committed = std::fs::read_to_string(fixture_root().join("red-baseline.json"))
        .expect("red-baseline.json must be readable");
    assert_eq!(
        committed.trim_end(),
        red_baseline_to_json(),
        "regenerate red-baseline.json and re-commit it"
    );
}

#[test]
fn every_named_mismatchs_evidence_file_exists_in_the_fixture_pack() {
    for row in RED_BASELINE {
        let evidence_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(row.evidence);
        assert!(
            evidence_path.exists(),
            "evidence for '{}' does not exist: {}",
            row.id,
            row.evidence
        );
    }
}

#[test]
fn a_red_run_still_reports_a_healthy_harness_separately() {
    let observed: Vec<&'static str> = RED_BASELINE.iter().map(|m| m.id).collect();
    let run = evaluate_run(true, &observed);
    assert_eq!(run.harness_health, HarnessHealth::Ok);
    match run.analyzer_verdict {
        AnalyzerVerdict::Red { mismatch_ids } => assert_eq!(mismatch_ids.len(), RED_BASELINE.len()),
        AnalyzerVerdict::Green => {
            panic!("a run reproducing every known counterexample must not read green")
        }
    }
}

#[test]
fn a_mismatch_the_baseline_does_not_name_is_reported_as_new_not_absorbed() {
    let (named, new) = classify_mismatches(&[
        "same-line-overloads-collapse-to-one-reference",
        "a-never-before-seen-mismatch",
    ]);
    assert_eq!(named, vec!["same-line-overloads-collapse-to-one-reference"]);
    assert_eq!(new, vec!["a-never-before-seen-mismatch"]);
}

#[test]
fn the_unrelated_api_names_fixture_reproduces_the_named_counterexample_source() {
    let src = std::fs::read_to_string(fixture_root().join("src/UnrelatedApiNames.cs")).unwrap();
    assert!(src.contains("public void Publish(Payload value)"));
    assert!(src.contains("public void AddScoped<TService, TImplementation>()"));
    assert!(src.contains("public void MapGet(string path, System.Action action)"));
}

#[test]
fn the_pin_is_stable_across_two_computations_and_order_independent() {
    let manifest_bytes = std::fs::read(fixture_root().join("manifest.json")).unwrap();
    let src_a = std::fs::read(fixture_root().join("src/Overloads.cs")).unwrap();
    let src_b = std::fs::read(fixture_root().join("src/GenericArity.cs")).unwrap();

    let pin_1 = compute_pin(
        "semantic-truth-v1",
        &manifest_bytes,
        &[
            ("src/Overloads.cs", src_a.as_slice()),
            ("src/GenericArity.cs", src_b.as_slice()),
        ],
    );
    let pin_2 = compute_pin(
        "semantic-truth-v1",
        &manifest_bytes,
        &[
            ("src/GenericArity.cs", src_b.as_slice()),
            ("src/Overloads.cs", src_a.as_slice()),
        ],
    );
    assert_eq!(pin_1, pin_2);
    assert_eq!(verify_pin(&pin_1, &pin_1.digest_hex), Ok(()));
}

/// The fifth row is not asserted from a path `exists()` check, it is
/// measured by running devscout's own native extract-and-resolve pipeline --
/// the producer this row's `attributedProducer` names -- against the row's
/// own evidence file and reading its real `implements` edges, offline, with
/// no compiler.
#[test]
fn the_native_dispatch_row_is_measured_through_real_resolver_output_not_asserted() {
    let row = RED_BASELINE
        .iter()
        .find(|m| m.id == "type-argument-pair-produces-implements-edges-on-a-clean-build")
        .expect("the fifth row must still be registered");
    assert!(GRADED_THROUGH_REAL_PRODUCER_OUTPUT.contains(&row.id));

    let source = std::fs::read_to_string(
        fixture_root().join(row.evidence.strip_prefix("fixtures/csharp-truth/").unwrap()),
    )
    .unwrap();
    let edges = native_dispatch_implements_edges(
        &fixture_root(),
        "src/NativeDispatchCounterexample.cs",
        &source,
    );
    assert!(
        !edges.is_empty(),
        "today's resolver must still produce implements edges for this fixture, or the row \
         is stale and must be re-reviewed rather than kept as a silent claim"
    );

    // Feed the real, measured observation through the same classification
    // path a run's observed ids take -- this is not a hand-built id, it is
    // derived from `edges` actually being non-empty.
    let observed_ids: Vec<&'static str> = if edges.is_empty() {
        vec![]
    } else {
        vec!["type-argument-pair-produces-implements-edges-on-a-clean-build"]
    };
    let (named, new) = classify_mismatches(&observed_ids);
    assert_eq!(
        named,
        vec!["type-argument-pair-produces-implements-edges-on-a-clean-build"]
    );
    assert!(new.is_empty());
}

#[test]
fn a_disagreeing_consumer_output_leaves_the_pin_unchanged_and_is_reported_as_a_disagreement() {
    let manifest_bytes = std::fs::read(fixture_root().join("manifest.json")).unwrap();
    let pin = compute_pin("semantic-truth-v1", &manifest_bytes, &[]);
    let pin_before = pin.clone();

    let result = verify_pin(
        &pin,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(result.is_err());
    // The pin itself is data the caller still owns; nothing in `verify_pin`
    // could have mutated it even if a mutating path existed here.
    assert_eq!(pin, pin_before);
}
