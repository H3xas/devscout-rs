//! Freshness and transformation controls: a declared before/after pair,
//! run in both directions.
//!
//! States in advance whether the facts should change (`Stale`) or must
//! not (`Preserved`) -- never inferred from what the pair happens to
//! produce. A dependency, reference, import, compiler option, or
//! generator-input change with the consuming file's bytes unchanged must
//! be detected as stale; a layout-only or consistent-rename
//! transformation must leave the mapped facts alone.

use super::report::ObservedCase;

/// What triggered the transformation between the before and after
/// observations of a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessTrigger {
    /// A referenced declaration changed.
    ReferencedDeclaration,
    /// A dependency's own compilation changed.
    DependencyCompilation,
    /// A reference or import changed.
    ReferenceOrImport,
    /// A compiler option changed.
    CompilerOption,
    /// A source-generator input changed.
    GeneratorInput,
    /// The file moved with no byte change.
    LayoutOnly,
    /// A symbol was renamed consistently everywhere.
    ConsistentRename,
}

/// The verdict a transformation pair declares before it is ever run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessVerdict {
    /// The mapped facts must stay unchanged.
    Preserved,
    /// The facts must be invalidated.
    Stale,
}

/// A before/after pair with its trigger and declared expectation, fixed at
/// authoring time.
#[derive(Debug, Clone)]
pub struct TransformationPair {
    /// The pair's stable id.
    pub id: &'static str,
    /// What changed between the before and after observation.
    pub trigger: FreshnessTrigger,
    /// The declared mapping from before facts to after facts.
    pub mapping: &'static str,
    /// The verdict declared for this pair before it is run.
    pub expected: FreshnessVerdict,
}

/// What running a pair actually found, relative to what it declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessOutcome {
    /// The declared expectation held.
    Detected,
    /// A trigger that should have invalidated the facts left them
    /// unchanged -- stale cache reuse.
    MissedStaleness,
    /// A layout-only or rename pair that should have preserved the mapped
    /// facts instead changed them.
    UnexpectedInvalidation,
}

impl FreshnessOutcome {
    /// Whether the pair's declared expectation held.
    pub fn is_detected(self) -> bool {
        matches!(self, FreshnessOutcome::Detected)
    }
}

/// Evaluates one direction of a pair: compares the before/after observed
/// facts against the pair's own declared expectation.
pub fn evaluate_pair(
    before: &ObservedCase,
    after: &ObservedCase,
    pair: &TransformationPair,
) -> FreshnessOutcome {
    let changed = before.facts != after.facts;
    match (pair.expected, changed) {
        (FreshnessVerdict::Stale, true) => FreshnessOutcome::Detected,
        (FreshnessVerdict::Stale, false) => FreshnessOutcome::MissedStaleness,
        (FreshnessVerdict::Preserved, false) => FreshnessOutcome::Detected,
        (FreshnessVerdict::Preserved, true) => FreshnessOutcome::UnexpectedInvalidation,
    }
}

/// Runs a pair in both directions (before-to-after and after-to-before);
/// a genuine before/after difference reads identically from either side,
/// so both directions must agree.
pub fn evaluate_pair_both_directions(
    before: &ObservedCase,
    after: &ObservedCase,
    pair: &TransformationPair,
) -> (FreshnessOutcome, FreshnessOutcome) {
    (
        evaluate_pair(before, after, pair),
        evaluate_pair(after, before, pair),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::truth::identity::{CompilationIdentity, OccurrenceSpan, SymbolIdentity};
    use crate::truth::report::ObservedFact;
    use crate::truth::uncertainty::{ContextHealth, Uncertainty};

    fn fact(member: &str) -> ObservedFact {
        ObservedFact {
            identity: SymbolIdentity {
                assembly: "Fixture".to_string(),
                declaring_type: "Fixture.Cases".to_string(),
                member: member.to_string(),
                generic_arity: 0,
                overload_signature: format!("{member}()"),
                compilation: CompilationIdentity {
                    project: "Cases".to_string(),
                    tfm: "net8.0".to_string(),
                    configuration: "Release".to_string(),
                },
            },
            occurrence: OccurrenceSpan {
                file: "src/Cases.cs".to_string(),
                start_line: 10,
                start_col: 1,
                end_line: 10,
                end_col: 12,
            },
            state: Uncertainty::Confirmed,
        }
    }

    fn observation(facts: Vec<ObservedFact>) -> ObservedCase {
        ObservedCase {
            context: ContextHealth::Complete,
            facts,
            diagnostics: vec![],
        }
    }

    #[test]
    fn a_dependency_change_with_unchanged_facts_is_missed_staleness() {
        let pair = TransformationPair {
            id: "dependency-compilation-changed",
            trigger: FreshnessTrigger::DependencyCompilation,
            mapping: "identity",
            expected: FreshnessVerdict::Stale,
        };
        let before = observation(vec![fact("Load")]);
        let after = observation(vec![fact("Load")]);
        assert_eq!(
            evaluate_pair(&before, &after, &pair),
            FreshnessOutcome::MissedStaleness
        );
    }

    #[test]
    fn a_dependency_change_with_updated_facts_is_detected() {
        let pair = TransformationPair {
            id: "dependency-compilation-changed",
            trigger: FreshnessTrigger::DependencyCompilation,
            mapping: "identity",
            expected: FreshnessVerdict::Stale,
        };
        let before = observation(vec![fact("Load")]);
        let after = observation(vec![fact("LoadAsync")]);
        assert!(evaluate_pair(&before, &after, &pair).is_detected());
    }

    #[test]
    fn a_layout_only_move_that_changes_facts_is_an_unexpected_invalidation() {
        let pair = TransformationPair {
            id: "directory-move",
            trigger: FreshnessTrigger::LayoutOnly,
            mapping: "same file, new directory",
            expected: FreshnessVerdict::Preserved,
        };
        let before = observation(vec![fact("Load")]);
        let after = observation(vec![fact("Load"), fact("Extra")]);
        assert_eq!(
            evaluate_pair(&before, &after, &pair),
            FreshnessOutcome::UnexpectedInvalidation
        );
    }

    #[test]
    fn a_layout_only_move_with_preserved_facts_is_detected_in_both_directions() {
        let pair = TransformationPair {
            id: "directory-move",
            trigger: FreshnessTrigger::LayoutOnly,
            mapping: "same file, new directory",
            expected: FreshnessVerdict::Preserved,
        };
        let before = observation(vec![fact("Load")]);
        let after = observation(vec![fact("Load")]);
        let (forward, backward) = evaluate_pair_both_directions(&before, &after, &pair);
        assert!(forward.is_detected());
        assert!(backward.is_detected());
    }

    #[test]
    fn a_consistent_rename_pair_is_declared_before_it_is_run() {
        let pair = TransformationPair {
            id: "rename-order-to-purchaseorder",
            trigger: FreshnessTrigger::ConsistentRename,
            mapping: "Order -> PurchaseOrder, every occurrence",
            expected: FreshnessVerdict::Preserved,
        };
        assert_eq!(pair.expected, FreshnessVerdict::Preserved);
    }
}
