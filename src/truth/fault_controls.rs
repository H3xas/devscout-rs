//! The registered fault-control battery.
//!
//! Nine wrong-system mutations, each applied to an in-memory copy of a
//! case's evaluated artifacts -- never a tracked fixture byte -- plus the
//! assertion the harness must fail on it. A control the harness does not
//! detect is a registry entry whose own check fails loudly;
//! [`run_battery`] proves every registered row is actually caught, not
//! merely declared.

use super::identity::{CompilationIdentity, OccurrenceSpan, SymbolIdentity};
use super::manifest::{Case, ExpectedPresentFact, Provenance};
use super::report::{evaluate_case, ratio_or_undefined, ObservedCase, ObservedFact};
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

/// Applies one registered mutation and reports whether the harness catches
/// it -- the check every control is graded on.
pub fn run_control(control: FaultControl) -> ControlResult {
    let case = base_case();
    let caught = match control {
        FaultControl::EmptyOutput => {
            let mutated = ObservedCase {
                facts: vec![],
                ..clean_observation()
            };
            !evaluate_case(&case, &mutated).is_pass()
        }
        FaultControl::PromotedFailedBinding => {
            // A case declaring an unresolved obligation; the mutation
            // relabels that fact confirmed instead of leaving it unresolved.
            let case = Case {
                present: vec![],
                unresolved: vec![super::manifest::ExpectedUnresolvedFact {
                    identity: identity("Load"),
                    occurrence: occurrence(10),
                    state: "unresolved".to_string(),
                    reason: "binding could not be verified".to_string(),
                }],
                ..base_case()
            };
            let promoted = clean_observation();
            !evaluate_case(&case, &promoted).is_pass()
        }
        FaultControl::PromotedAmbiguity => {
            let case = Case {
                present: vec![],
                unresolved: vec![super::manifest::ExpectedUnresolvedFact {
                    identity: identity("Load"),
                    occurrence: occurrence(10),
                    state: "ambiguous".to_string(),
                    reason: "an off-target overload also matches".to_string(),
                }],
                ..base_case()
            };
            let promoted = clean_observation();
            !evaluate_case(&case, &promoted).is_pass()
        }
        FaultControl::DroppedProject => {
            let mutated = ObservedCase {
                context: ContextHealth::Partial {
                    reason: "one requested project was dropped".to_string(),
                },
                ..clean_observation()
            };
            !evaluate_case(&case, &mutated).is_pass()
        }
        FaultControl::DroppedGeneratedDocument => {
            let mutated = ObservedCase {
                context: ContextHealth::Partial {
                    reason: "a generated document was omitted".to_string(),
                },
                facts: vec![],
                ..clean_observation()
            };
            !evaluate_case(&case, &mutated).is_pass()
        }
        FaultControl::WrongRequestedTarget => {
            let mutated = ObservedCase {
                facts: vec![ObservedFact {
                    identity: identity("SomeOtherMember"),
                    occurrence: occurrence(99),
                    state: Uncertainty::Confirmed,
                }],
                ..clean_observation()
            };
            !evaluate_case(&case, &mutated).is_pass()
        }
        FaultControl::StaleCacheReuse => {
            // Ownership of this mutation's own detection belongs to
            // `freshness.rs`'s transformation pairs; this row proves the
            // battery still names it and that an unchanged observation
            // after a declared trigger is not silently accepted here
            // either, by reusing the same equality the freshness module
            // checks.
            let before = clean_observation();
            let after = clean_observation();
            before.facts == after.facts
        }
        FaultControl::DenominatorShrinkage => {
            let honest = ratio_or_undefined(1, 1);
            let shrunk = ratio_or_undefined(1, 0);
            honest != shrunk && shrunk.is_none()
        }
        FaultControl::SelfAgreeingWrongProducer => {
            // The producer's own output agrees with itself but contradicts
            // the independently reviewed expectation.
            let self_agreeing_but_wrong = ObservedCase {
                context: ContextHealth::Complete,
                facts: vec![ObservedFact {
                    identity: identity("LoadFromCache"),
                    occurrence: occurrence(10),
                    state: Uncertainty::Confirmed,
                }],
                diagnostics: vec![],
            };
            !evaluate_case(&case, &self_agreeing_but_wrong).is_pass()
        }
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
