//! The committed capability matrix regenerates byte-identical from the
//! profile registry, and no entry it produces can hold `passing` without
//! an executed-obligation witness.

use std::path::Path;

use devscout_rs::truth::capability::{
    build_matrix, matrix_to_json, CapabilityState, ExecutedObligation,
};

fn committed_matrix_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/capability-matrix.json")
}

#[test]
fn the_committed_matrix_regenerates_byte_identical() {
    let committed = std::fs::read_to_string(committed_matrix_path())
        .expect("capability-matrix.json must be readable");
    let regenerated = matrix_to_json(&build_matrix());
    assert_eq!(
        committed.trim_end(),
        regenerated,
        "regenerate capability-matrix.json and re-commit it"
    );
}

#[test]
fn no_matrix_entry_holds_passing_with_nothing_executed() {
    let matrix = build_matrix();
    assert!(!matrix.entries.is_empty());
    for entry in &matrix.entries {
        assert!(
            !matches!(entry.state, CapabilityState::Passing(_)),
            "{} / {} claims passing from a registry that executed nothing",
            entry.profile_id,
            entry.axis.label()
        );
    }
}

#[test]
fn a_zero_execution_count_cannot_manufacture_a_passing_witness() {
    assert!(ExecutedObligation::record(0).is_none());
}

#[test]
fn the_four_demonstrated_targets_claim_at_most_smoke_tested_in_the_committed_matrix() {
    let matrix = build_matrix();
    for id in [
        "csharp-net8.0-sdk",
        "csharp-netcoreapp3.1-sdk",
        "csharp-net472-framework",
        "csharp-netstandard2.1-library",
    ] {
        let entries: Vec<_> = matrix
            .entries
            .iter()
            .filter(|e| e.profile_id == id)
            .collect();
        assert_eq!(
            entries.len(),
            5,
            "profile '{id}' must be tracked on all five axes"
        );
        for entry in entries {
            assert_eq!(entry.state, CapabilityState::SmokeTested);
        }
    }
}
