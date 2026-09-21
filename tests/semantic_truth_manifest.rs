//! The committed case manifest parses whole, every case carries every
//! required field, and a case missing one is rejected by name rather than
//! accepted with a hole in it. A case whose expectation names the
//! producer it grades is refused at the same gate.

use std::path::Path;

use devscout_rs::truth::manifest::{parse_manifest, validate_case, ManifestError, Provenance};
use devscout_rs::truth::report::{evaluate_case, ObservedCase};
use devscout_rs::truth::uncertainty::ContextHealth;

fn manifest_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/manifest.json")
}

fn readme_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/README.md")
}

/// The fixture pack's own evidence clause for a case is its README row, so a
/// case the table has fallen behind on documents nothing. Every committed
/// case id must appear as a backtick-quoted README cell, or this test names
/// which one does not.
#[test]
fn every_committed_case_id_appears_in_the_fixture_readme() {
    let manifest_text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&manifest_text).unwrap();
    let readme = std::fs::read_to_string(readme_path()).unwrap();
    for case in &manifest.cases {
        assert!(
            readme.contains(&format!("`{}`", case.id)),
            "case '{}' is not documented in fixtures/csharp-truth/README.md",
            case.id
        );
    }
}

#[test]
fn the_committed_manifest_parses_whole() {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json must be readable");
    let manifest = parse_manifest(&text).expect("every committed case must validate");
    assert_eq!(manifest.contract, "semantic-truth-v1");
    assert!(
        !manifest.cases.is_empty(),
        "the manifest must declare at least one case"
    );
    for case in &manifest.cases {
        assert!(!case.id.is_empty());
        assert!(
            !case.profiles.is_empty(),
            "case '{}' names no profile",
            case.id
        );
    }
}

#[test]
fn every_committed_case_declares_reviewed_or_a_named_separate_compiler() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    for case in &manifest.cases {
        match &case.provenance {
            Provenance::Reviewed => {}
            Provenance::SeparatelySelectedCompiler { tool } => assert!(!tool.is_empty()),
        }
    }
}

#[test]
fn a_case_copy_missing_a_required_field_is_rejected_and_named() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let cases = doc.get_mut("cases").unwrap().as_array_mut().unwrap();
    let mut broken = cases[0].clone();
    let broken_id = broken["id"].as_str().unwrap().to_string();
    broken.as_object_mut().unwrap().remove("profiles");
    let err = validate_case(&broken).unwrap_err();
    assert_eq!(
        err,
        ManifestError::MissingField {
            case_id: broken_id,
            field: "profiles".to_string(),
        }
    );
}

#[test]
fn a_case_naming_the_graded_producer_as_its_own_expectation_source_is_refused() {
    let mut case = serde_json::json!({
        "id": "self-graded",
        "scenarioFamily": "shared-language-semantics",
        "language": "csharp",
        "profiles": ["csharp-net8.0-sdk"],
        "prerequisites": [],
        "factContract": "semantic-truth-v1",
        "source": ["src/Overloads.cs"],
        "expect": {
            "context": "complete",
            "present": [],
            "absent": [],
            "unresolved": [],
            "diagnostics": []
        },
        "provenance": { "kind": "reviewed" }
    });
    case["provenance"] = serde_json::json!({ "kind": "separately-selected-compiler", "tool": "scout-semantic@0.6.0" });
    let err = validate_case(&case).unwrap_err();
    assert!(matches!(
        err,
        ManifestError::ProducerGradesOwnExpectation { .. }
    ));
}

#[test]
fn the_committed_manifest_covers_all_four_scenario_families() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let families: std::collections::HashSet<&str> = manifest
        .cases
        .iter()
        .map(|c| c.scenario_family.as_str())
        .collect();
    for expected in [
        "shared-language-semantics",
        "compatibility-boundaries",
        "framework-semantics",
        "transformations",
    ] {
        assert!(
            families.contains(expected),
            "no committed case declares scenario family '{expected}'"
        );
    }
}

#[test]
fn the_committed_manifest_has_at_least_one_expected_diagnostic_case() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    assert!(
        manifest.cases.iter().any(|c| !c.diagnostics.is_empty()),
        "no committed case declares an expected diagnostic"
    );
}

#[test]
fn an_empty_analyzer_result_cannot_satisfy_the_committed_overload_case() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let overload_case = manifest
        .cases
        .iter()
        .find(|c| c.id == "overload-arity-a")
        .unwrap();
    assert!(
        !overload_case.present.is_empty(),
        "the fixture case must declare a positive obligation"
    );
    let empty = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![],
        diagnostics: vec![],
    };
    assert!(!evaluate_case(overload_case, &empty).is_pass());
}
