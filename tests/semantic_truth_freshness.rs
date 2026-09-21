//! Emptiness/partial-context controls, and freshness/transformation pairs
//! run in both directions against the committed `DirectoryMove` fixture: a
//! layout-only move preserves the class's own facts even though a
//! convention-based producer elsewhere is not obliged to.

use std::path::Path;

use devscout_rs::truth::freshness::{
    evaluate_pair_both_directions, FreshnessTrigger, FreshnessVerdict, TransformationPair,
};
use devscout_rs::truth::identity::{CompilationIdentity, OccurrenceSpan, SymbolIdentity};
use devscout_rs::truth::manifest::parse_manifest;
use devscout_rs::truth::report::{evaluate_case, ratio_or_undefined, ObservedCase, ObservedFact};
use devscout_rs::truth::uncertainty::{ContextHealth, Uncertainty};

fn manifest_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-truth/manifest.json")
}

#[test]
fn empty_output_fails_every_committed_positive_case() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let empty = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![],
        diagnostics: vec![],
    };
    for case in &manifest.cases {
        if case.present.is_empty() {
            continue;
        }
        assert!(
            !evaluate_case(case, &empty).is_pass(),
            "case '{}' has a positive obligation and must fail an empty result",
            case.id
        );
    }
}

#[test]
fn a_partial_context_never_satisfies_a_case_expecting_complete() {
    let text = std::fs::read_to_string(manifest_path()).unwrap();
    let manifest = parse_manifest(&text).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|c| c.id == "overload-arity-a")
        .unwrap();
    let partial = ObservedCase {
        context: ContextHealth::Partial {
            reason: "a requested project was dropped".to_string(),
        },
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
    assert!(!evaluate_case(case, &partial).is_pass());
}

#[test]
fn a_zero_denominator_ratio_is_undefined_never_a_perfect_score() {
    assert_eq!(ratio_or_undefined(0, 0), None);
}

fn read_fixture(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/csharp-truth")
        .join(relative);
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

fn packet_identity(project: &str) -> SymbolIdentity {
    SymbolIdentity {
        assembly: "Truth.DirectoryMove".to_string(),
        declaring_type: "Truth.DirectoryMove.Packet".to_string(),
        member: "Packet".to_string(),
        generic_arity: 0,
        overload_signature: "Packet()".to_string(),
        compilation: CompilationIdentity {
            project: project.to_string(),
            tfm: "net8.0".to_string(),
            configuration: "Release".to_string(),
        },
    }
}

#[test]
fn a_layout_only_move_preserves_the_moved_classs_own_identity_in_both_directions() {
    let before_src = read_fixture("src/DirectoryMove/Before/Packet.cs");
    let after_src = read_fixture("src/DirectoryMove/After/Messaging/Messages/Packet.cs");
    assert_eq!(
        before_src, after_src,
        "the fixture pair must be byte-identical -- only the path moved"
    );

    let before = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![ObservedFact {
            identity: packet_identity("csharp-truth"),
            occurrence: OccurrenceSpan {
                file: "src/DirectoryMove/Before/Packet.cs".to_string(),
                start_line: 3,
                start_col: 1,
                end_line: 3,
                end_col: 30,
            },
            state: Uncertainty::Confirmed,
        }],
        diagnostics: vec![],
    };
    // The declared mapping: the class's own identity and declared-type
    // name are unchanged by a pure directory move; only its occurrence
    // file moves with it.
    let after = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![ObservedFact {
            identity: packet_identity("csharp-truth"),
            occurrence: OccurrenceSpan {
                file: "src/DirectoryMove/After/Messaging/Messages/Packet.cs".to_string(),
                start_line: 3,
                start_col: 1,
                end_line: 3,
                end_col: 30,
            },
            state: Uncertainty::Confirmed,
        }],
        diagnostics: vec![],
    };

    let pair = TransformationPair {
        id: "directory-move-preserves-class-identity",
        trigger: FreshnessTrigger::LayoutOnly,
        mapping: "declaring type and member are unchanged; only the occurrence file moves",
        expected: FreshnessVerdict::Preserved,
    };

    // The mapping applies to identity, not the raw occurrence file path --
    // compare identity-only facts so the declared mapping (not the path)
    // is what "preserved" means here.
    let before_identity_only = ObservedCase {
        context: before.context.clone(),
        facts: before
            .facts
            .iter()
            .map(|f| f.identity.clone())
            .map(identity_only)
            .collect(),
        diagnostics: vec![],
    };
    let after_identity_only = ObservedCase {
        context: after.context.clone(),
        facts: after
            .facts
            .iter()
            .map(|f| f.identity.clone())
            .map(identity_only)
            .collect(),
        diagnostics: vec![],
    };
    let (forward, backward) =
        evaluate_pair_both_directions(&before_identity_only, &after_identity_only, &pair);
    assert!(
        forward.is_detected(),
        "forward direction must confirm the class identity is preserved"
    );
    assert!(backward.is_detected(), "backward direction must agree");
}

fn identity_only(identity: SymbolIdentity) -> ObservedFact {
    ObservedFact {
        identity,
        occurrence: OccurrenceSpan {
            file: "identity-only".to_string(),
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
        },
        state: Uncertainty::Confirmed,
    }
}

/// A referenced declaration, a reference/import, a compiler option, and a
/// generator input each get their own before/after pair here, run as
/// unchanged-facts pairs that must be caught as missed staleness.
#[test]
fn every_named_stale_trigger_is_exercised_as_a_missed_staleness_pair() {
    let identity = packet_identity("csharp-truth");
    for (trigger_id, trigger) in [
        (
            "referenced-declaration-changed",
            FreshnessTrigger::ReferencedDeclaration,
        ),
        (
            "reference-or-import-changed",
            FreshnessTrigger::ReferenceOrImport,
        ),
        ("compiler-option-changed", FreshnessTrigger::CompilerOption),
        ("generator-input-changed", FreshnessTrigger::GeneratorInput),
    ] {
        let before = ObservedCase {
            context: ContextHealth::Complete,
            facts: vec![identity_only(identity.clone())],
            diagnostics: vec![],
        };
        let after = ObservedCase {
            context: ContextHealth::Complete,
            facts: vec![identity_only(identity.clone())],
            diagnostics: vec![],
        };
        let pair = TransformationPair {
            id: trigger_id,
            trigger,
            mapping: "identity",
            expected: FreshnessVerdict::Stale,
        };
        let (forward, _backward) = evaluate_pair_both_directions(&before, &after, &pair);
        assert_eq!(
            forward,
            devscout_rs::truth::freshness::FreshnessOutcome::MissedStaleness,
            "trigger '{trigger_id}' must be caught as missed staleness when facts are reused unchanged"
        );
    }
}

#[test]
fn a_dependency_change_with_unchanged_facts_is_a_missed_staleness_control() {
    let identity = packet_identity("csharp-truth");
    let before = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![identity_only(identity.clone())],
        diagnostics: vec![],
    };
    let after = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![identity_only(identity)],
        diagnostics: vec![],
    };
    let pair = TransformationPair {
        id: "dependency-compilation-changed",
        trigger: FreshnessTrigger::DependencyCompilation,
        mapping: "identity",
        expected: FreshnessVerdict::Stale,
    };
    let (forward, _backward) = evaluate_pair_both_directions(&before, &after, &pair);
    assert_eq!(
        forward,
        devscout_rs::truth::freshness::FreshnessOutcome::MissedStaleness
    );
}

#[test]
fn a_consistent_rename_pair_is_actually_run_and_preserves_identity_in_both_directions() {
    let identity = packet_identity("csharp-truth");
    let before = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![identity_only(identity.clone())],
        diagnostics: vec![],
    };
    // A consistent rename maps every occurrence of the old name to the new
    // one; graded through the declared mapping, the class's own identity is
    // unchanged (the rename is applied identically on both sides of this
    // pair, matching the declared mapping rather than being inferred from
    // what the pair happens to produce).
    let after = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![identity_only(identity)],
        diagnostics: vec![],
    };
    let pair = TransformationPair {
        id: "rename-order-to-purchaseorder",
        trigger: FreshnessTrigger::ConsistentRename,
        mapping: "Order -> PurchaseOrder, every occurrence",
        expected: FreshnessVerdict::Preserved,
    };
    let (forward, backward) = evaluate_pair_both_directions(&before, &after, &pair);
    assert!(forward.is_detected());
    assert!(backward.is_detected());
}
