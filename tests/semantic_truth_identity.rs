//! Identity and occurrence fidelity against the committed fixture's own
//! overload/generic-arity/implementation cases: same-line calls and
//! distinct overloads stay distinct occurrences, a mutation that merges
//! them must fail, and the legacy `(file, startLine, member)` join is
//! shown, deliberately, to collapse what full identity keeps apart.

use std::path::Path;

use devscout_rs::truth::identity::{CompatibilityJoinKey, CompilationIdentity, SymbolIdentity};
use devscout_rs::truth::manifest::parse_manifest;
use devscout_rs::truth::report::{evaluate_case, ObservedCase, ObservedDiagnostic, ObservedFact};
use devscout_rs::truth::uncertainty::{ContextHealth, Uncertainty};

fn manifest_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/manifest.json")
}

/// Faithful across every obligation kind a case can declare, not just
/// present facts -- an expected diagnostic must round-trip too, or a case
/// that declares one can never pass here even when perfectly observed.
fn observed_from_present(case: &devscout_rs::truth::manifest::Case) -> ObservedCase {
    ObservedCase {
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
        diagnostics: case
            .diagnostics
            .iter()
            .map(|d| ObservedDiagnostic {
                severity: d.severity.clone(),
                code: d.code.clone(),
            })
            .collect(),
    }
}

#[test]
fn the_two_overloads_on_one_line_are_distinct_identities_and_occurrences() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "overload-arity-a")
        .unwrap();
    assert_eq!(case.present.len(), 2);
    let int_call = &case.present[0];
    let string_call = &case.present[1];

    assert_ne!(
        int_call.identity, string_call.identity,
        "distinct overloads must be distinct identities"
    );
    assert_ne!(
        int_call.occurrence, string_call.occurrence,
        "same-line calls must be distinct occurrences"
    );
    assert_eq!(
        int_call.occurrence.start_line, string_call.occurrence.start_line,
        "both are on the same line"
    );

    let key_a = CompatibilityJoinKey::from_identity(&int_call.identity, &int_call.occurrence);
    let key_b = CompatibilityJoinKey::from_identity(&string_call.identity, &string_call.occurrence);
    assert!(
        key_a.joins(&key_b),
        "the legacy join deliberately cannot tell them apart"
    );
}

#[test]
fn the_generic_arity_pair_is_distinct_by_arity_alone() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "generic-arity-a")
        .unwrap();
    let one_arg = &case.present[0];
    let two_arg = &case.present[1];
    assert_eq!(one_arg.identity.generic_arity, 1);
    assert_eq!(two_arg.identity.generic_arity, 2);
    assert_ne!(one_arg.identity, two_arg.identity);
}

#[test]
fn a_mutation_that_merges_the_two_overloads_into_one_fact_fails_the_case() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "overload-arity-a")
        .unwrap();
    let mut observed = observed_from_present(case);
    // Collapse: keep only the first of the two distinct occurrences,
    // simulating a producer that deduplicates same-line overloads onto one
    // reference.
    observed.facts.truncate(1);
    assert!(
        !evaluate_case(case, &observed).is_pass(),
        "a collapsed observation must fail the case"
    );
}

#[test]
fn a_faithful_observation_round_trips_through_evaluation_unchanged() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    for case in &manifest.cases {
        if case.present.is_empty() {
            continue;
        }
        let observed = observed_from_present(case);
        assert!(
            evaluate_case(case, &observed).is_pass(),
            "case '{}' must pass when every present fact is faithfully observed",
            case.id
        );
    }
}

#[test]
fn a_mutation_that_merges_the_two_generic_arities_into_one_fact_fails_the_case() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "generic-arity-a")
        .unwrap();
    let mut observed = observed_from_present(case);
    // Collapse: keep only the one-type-argument overload's fact, simulating
    // a producer that merges the two arities of the same generic member
    // into one reported fact.
    observed.facts.truncate(1);
    assert!(
        !evaluate_case(case, &observed).is_pass(),
        "a fact set missing one of two distinct generic arities must fail the case"
    );
}

#[test]
fn a_mutation_that_merges_two_compilation_contexts_into_one_fails_the_case() {
    // Two present facts for the same member, distinguished only by
    // compilation identity (different target framework) -- observing only
    // one of the two compilation contexts must fail the case that expects
    // both, the same way a producer that failed to keep per-TFM facts
    // separate would.
    let identity_for = |tfm: &str| SymbolIdentity {
        assembly: "Fixture".to_string(),
        declaring_type: "Fixture.Cases".to_string(),
        member: "Load".to_string(),
        generic_arity: 0,
        overload_signature: "Load()".to_string(),
        compilation: CompilationIdentity {
            project: "Cases".to_string(),
            tfm: tfm.to_string(),
            configuration: "Release".to_string(),
        },
    };
    let occurrence = devscout_rs::truth::identity::OccurrenceSpan {
        file: "src/Cases.cs".to_string(),
        start_line: 10,
        start_col: 1,
        end_line: 10,
        end_col: 12,
    };
    let case = devscout_rs::truth::manifest::Case {
        id: "compilation-context-merge".to_string(),
        scenario_family: "shared-language-semantics".to_string(),
        language: "csharp".to_string(),
        profiles: vec!["csharp-net8.0-sdk".to_string()],
        prerequisites: vec![],
        fact_contract: "semantic-truth-v1".to_string(),
        source: vec!["src/Cases.cs".to_string()],
        context: "complete".to_string(),
        present: vec![
            devscout_rs::truth::manifest::ExpectedPresentFact {
                identity: identity_for("net8.0"),
                occurrence: occurrence.clone(),
                state: "confirmed".to_string(),
            },
            devscout_rs::truth::manifest::ExpectedPresentFact {
                identity: identity_for("net472"),
                occurrence,
                state: "confirmed".to_string(),
            },
        ],
        absent: vec![],
        unresolved: vec![],
        diagnostics: vec![],
        provenance: devscout_rs::truth::manifest::Provenance::Reviewed,
    };
    let net8_only = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![ObservedFact {
            identity: identity_for("net8.0"),
            occurrence: case.present[0].occurrence.clone(),
            state: Uncertainty::Confirmed,
        }],
        diagnostics: vec![],
    };
    assert!(
        !evaluate_case(&case, &net8_only).is_pass(),
        "merging two distinct compilation contexts into one observed fact must fail the case"
    );
}

#[test]
fn implementation_identity_names_the_class_never_the_interface() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "implementation-identity-a")
        .unwrap();
    let present = &case.present[0];
    assert_eq!(
        present.identity.declaring_type,
        "Truth.ImplementationIdentity.Greeter"
    );
    let absent = &case.absent[0];
    assert_eq!(
        absent.identity.declaring_type,
        "Truth.ImplementationIdentity.IGreeter"
    );
}
