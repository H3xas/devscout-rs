//! The registered fault-control battery.
//!
//! Nine wrong-system mutations, each applied to an in-memory copy of a
//! case's evaluated artifacts -- never a tracked fixture byte -- plus the
//! assertion the harness must fail on it. A control the harness does not
//! detect is a registry entry whose own check fails loudly;
//! [`run_battery`] proves every registered row is actually caught, not
//! merely declared.

use super::freshness::{
    evaluate_pair, FreshnessOutcome, FreshnessTrigger, FreshnessVerdict, TransformationPair,
};
use super::identity::{CompilationIdentity, OccurrenceSpan, SymbolIdentity};
use super::manifest::{Case, ExpectedPresentFact, Provenance};
use super::report::{
    denominator_shrunk, evaluate_case, ratio_or_undefined, ObservedCase, ObservedFact,
};
use super::uncertainty::{ContextHealth, Uncertainty};

/// One registered wrong-system mutation. `ALL` is the exhaustive,
/// registered list a manifest's fault battery must cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultControl {
    /// The analyzer produced nothing at all.
    EmptyOutput,
    /// A failed/unresolved binding is relabeled confirmed.
    PromotedFailedBinding,
    /// An ambiguous or off-target match is relabeled confirmed.
    PromotedAmbiguity,
    /// A requested project is silently dropped from the run.
    DroppedProject,
    /// A generated document is silently omitted.
    DroppedGeneratedDocument,
    /// A different profile's records are substituted for the one
    /// requested.
    WrongRequestedTarget,
    /// A cached record is reused after a freshness trigger fired.
    StaleCacheReuse,
    /// The reported denominator shrinks without a matching record removal.
    DenominatorShrinkage,
    /// The producer's output agrees with itself but contradicts the
    /// reviewed expectation.
    SelfAgreeingWrongProducer,
}

impl FaultControl {
    /// Every registered control, in declaration order.
    pub const ALL: [FaultControl; 9] = [
        FaultControl::EmptyOutput,
        FaultControl::PromotedFailedBinding,
        FaultControl::PromotedAmbiguity,
        FaultControl::DroppedProject,
        FaultControl::DroppedGeneratedDocument,
        FaultControl::WrongRequestedTarget,
        FaultControl::StaleCacheReuse,
        FaultControl::DenominatorShrinkage,
        FaultControl::SelfAgreeingWrongProducer,
    ];

    /// The wire label for this control.
    pub fn label(self) -> &'static str {
        match self {
            FaultControl::EmptyOutput => "empty-output",
            FaultControl::PromotedFailedBinding => "promoted-failed-binding",
            FaultControl::PromotedAmbiguity => "promoted-ambiguity",
            FaultControl::DroppedProject => "dropped-project",
            FaultControl::DroppedGeneratedDocument => "dropped-generated-document",
            FaultControl::WrongRequestedTarget => "wrong-requested-target",
            FaultControl::StaleCacheReuse => "stale-cache-reuse",
            FaultControl::DenominatorShrinkage => "denominator-shrinkage",
            FaultControl::SelfAgreeingWrongProducer => "self-agreeing-wrong-producer",
        }
    }
}

/// Whether a registered control's mutation was actually caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlResult {
    /// Which control this result is for.
    pub control: FaultControl,
    /// Whether the harness caught the mutation.
    pub caught: bool,
}

fn identity(member: &str) -> SymbolIdentity {
    SymbolIdentity {
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
    }
}

fn occurrence(line: u32) -> OccurrenceSpan {
    OccurrenceSpan {
        file: "src/Cases.cs".to_string(),
        start_line: line,
        start_col: 1,
        end_line: line,
        end_col: 20,
    }
}

fn base_case() -> Case {
    Case {
        id: "fault-control-base".to_string(),
        scenario_family: "shared-language-semantics".to_string(),
        language: "csharp".to_string(),
        profiles: vec!["csharp-net8.0-sdk".to_string()],
        prerequisites: vec![],
        fact_contract: "semantic-truth-v1".to_string(),
        source: vec!["src/Cases.cs".to_string()],
        context: "complete".to_string(),
        present: vec![ExpectedPresentFact {
            identity: identity("Load"),
            occurrence: occurrence(10),
            state: "confirmed".to_string(),
        }],
        absent: vec![],
        unresolved: vec![],
        diagnostics: vec![],
        provenance: Provenance::Reviewed,
    }
}

fn clean_observation() -> ObservedCase {
    ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![ObservedFact {
            identity: identity("Load"),
            occurrence: occurrence(10),
            state: Uncertainty::Confirmed,
        }],
        diagnostics: vec![],
    }
}

fn caught_empty_output(case: &Case) -> bool {
    let mutated = ObservedCase {
        facts: vec![],
        ..clean_observation()
    };
    !evaluate_case(case, &mutated).is_pass()
}

fn caught_promoted_unresolved(expected_state: &str, reason: &str) -> bool {
    let case = Case {
        present: vec![],
        unresolved: vec![super::manifest::ExpectedUnresolvedFact {
            identity: identity("Load"),
            occurrence: occurrence(10),
            state: expected_state.to_string(),
            reason: reason.to_string(),
        }],
        ..base_case()
    };
    let promoted = clean_observation();
    !evaluate_case(&case, &promoted).is_pass()
}

fn caught_dropped_project(case: &Case) -> bool {
    let mutated = ObservedCase {
        context: ContextHealth::Partial {
            reason: "one requested project was dropped".to_string(),
        },
        ..clean_observation()
    };
    !evaluate_case(case, &mutated).is_pass()
}

fn caught_dropped_generated_document(case: &Case) -> bool {
    let mutated = ObservedCase {
        context: ContextHealth::Partial {
            reason: "a generated document was omitted".to_string(),
        },
        facts: vec![],
        ..clean_observation()
    };
    !evaluate_case(case, &mutated).is_pass()
}

fn caught_wrong_requested_target(case: &Case) -> bool {
    let mutated = ObservedCase {
        facts: vec![ObservedFact {
            identity: identity("SomeOtherMember"),
            occurrence: occurrence(99),
            state: Uncertainty::Confirmed,
        }],
        ..clean_observation()
    };
    !evaluate_case(case, &mutated).is_pass()
}

/// The battery's own verdict is DERIVED from `freshness.rs`'s detection,
/// not a reflexive equality declared here: a producer that reuses cached
/// facts unchanged after a declared freshness trigger must be caught as
/// `MissedStaleness` by `evaluate_pair`. If that detection were ever
/// deleted, this function would stop catching the control too.
fn caught_stale_cache_reuse() -> bool {
    let pair = TransformationPair {
        id: "fault-control-stale-cache-reuse",
        trigger: FreshnessTrigger::DependencyCompilation,
        mapping: "identity",
        expected: FreshnessVerdict::Stale,
    };
    let reused_after_trigger = clean_observation();
    let still_cached = clean_observation();
    let outcome = evaluate_pair(&reused_after_trigger, &still_cached, &pair);
    matches!(outcome, FreshnessOutcome::MissedStaleness)
}

/// A report that silently drops one of two requested cases must be caught
/// even though its own ratio does not fall.
fn caught_denominator_shrinkage() -> bool {
    let declared_case_count = 2;
    let full_denominator = 2;
    let dropped_denominator = 1;
    let full_ratio = ratio_or_undefined(1, full_denominator);
    let dropped_ratio = ratio_or_undefined(1, dropped_denominator);
    dropped_ratio > full_ratio && denominator_shrunk(dropped_denominator, declared_case_count)
}

fn caught_self_agreeing_wrong_producer(case: &Case) -> bool {
    // The producer's own output agrees with itself but contradicts the
    // independently reviewed expectation.
    let self_agreeing_but_wrong = ObservedCase {
        context: ContextHealth::Complete,
        facts: vec![ObservedFact {
            identity: identity("LoadFromCache"),
            occurrence: occurrence(10),
            state: Uncertainty::Confirmed,
        }],
        diagnostics: vec![],
    };
    !evaluate_case(case, &self_agreeing_but_wrong).is_pass()
}

/// Applies one registered mutation and reports whether the harness catches
/// it -- the check every control is graded on.
pub fn run_control(control: FaultControl) -> ControlResult {
    let case = base_case();
    let caught = match control {
        FaultControl::EmptyOutput => caught_empty_output(&case),
        FaultControl::PromotedFailedBinding => {
            caught_promoted_unresolved("unresolved", "binding could not be verified")
        }
        FaultControl::PromotedAmbiguity => {
            caught_promoted_unresolved("ambiguous", "an off-target overload also matches")
        }
        FaultControl::DroppedProject => caught_dropped_project(&case),
        FaultControl::DroppedGeneratedDocument => caught_dropped_generated_document(&case),
        FaultControl::WrongRequestedTarget => caught_wrong_requested_target(&case),
        FaultControl::StaleCacheReuse => caught_stale_cache_reuse(),
        FaultControl::DenominatorShrinkage => caught_denominator_shrinkage(),
        FaultControl::SelfAgreeingWrongProducer => caught_self_agreeing_wrong_producer(&case),
    };
    ControlResult { control, caught }
}

/// Runs every registered control.
pub fn run_battery() -> Vec<ControlResult> {
    FaultControl::ALL.iter().map(|c| run_control(*c)).collect()
}

/// The meta-check: the registered list is exhaustive against `ALL`, and
/// every registered row is actually caught by the harness that exists
/// today.
///
/// An undetectable control fails this, loudly, rather than being silently
/// dropped.
pub fn battery_is_exhaustive_and_all_caught() -> Result<(), Vec<FaultControl>> {
    let results = run_battery();
    let missed: Vec<FaultControl> = results
        .iter()
        .filter(|r| !r.caught)
        .map(|r| r.control)
        .collect();
    if missed.is_empty() && results.len() == FaultControl::ALL.len() {
        Ok(())
    } else {
        Err(missed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_control_is_caught() {
        assert_eq!(battery_is_exhaustive_and_all_caught(), Ok(()));
    }

    #[test]
    fn each_row_reports_which_control_it_is() {
        for result in run_battery() {
            assert!(result.caught, "{} was not caught", result.control.label());
        }
    }

    #[test]
    fn the_registered_list_has_nine_rows() {
        assert_eq!(FaultControl::ALL.len(), 9);
    }
}
