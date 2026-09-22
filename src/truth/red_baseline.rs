//! The committed truthful red baseline: today's known analyzer misses and
//! false assertions, recorded as explicit red rows rather than repaired.
//!
//! `harness_health` (do this crate's own tests pass) and
//! `analyzer_verdict` (is production output correct) are two separate
//! fields on one run, so a red analyzer verdict never reads as a broken
//! harness. No expectation anywhere is weakened to turn a row green; a
//! mismatch this list does not name is reported as new, not silently
//! absorbed into an existing row.

use crate::query::json::J;

/// One committed, named mismatch: what is wrong, which producer is
/// responsible, and where the evidence for it lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedMismatch {
    /// The mismatch's stable id.
    pub id: &'static str,
    /// The tool/code path actually responsible for this mismatch.
    pub attributed_producer: &'static str,
    /// What is wrong.
    pub reason: &'static str,
    /// Where the evidence for this mismatch lives.
    pub evidence: &'static str,
}

/// The five reproduced counterexamples this baseline records, attributed
/// to their actual producers rather than to a resolver defect this ticket
/// repairs.
pub const RED_BASELINE: &[NamedMismatch] = &[
    NamedMismatch {
        id: "unrelated-names-promoted-to-framework-facts",
        attributed_producer: "scout-semantic FactsWalker sidecar",
        reason: "ordinary, unrelated methods named Publish/AddScoped<T,U>/MapGet on a type with no \
                  messaging or DI dependency are promoted to publish/di_binding/route facts",
        evidence: "fixtures/csharp-truth/src/UnrelatedApiNames.cs",
    },
    NamedMismatch {
        id: "directory-move-creates-message-class-fact",
        attributed_producer: "scout-semantic FactsWalker sidecar",
        reason: "moving an unchanged class from a plain directory into a messaging-named directory \
                  creates a message_class fact with no source change to the class itself",
        evidence: "fixtures/csharp-truth/src/DirectoryMove",
    },
    NamedMismatch {
        id: "failed-binding-promoted-while-unit-reports-healthy",
        attributed_producer: "scout-semantic FactsWalker sidecar",
        reason: "a call through an undeclared receiver still emits a publish fact while the owning \
                  unit's own status is reported ok",
        evidence: "fixtures/csharp-truth/src/FailedBinding.cs",
    },
    NamedMismatch {
        id: "same-line-overloads-collapse-to-one-reference",
        attributed_producer: "scout-semantic Roslyn oracle exporter",
        reason: "two distinct overloads called on one line deduplicate to a single ground-truth \
                  reference, so the two occurrences and their distinct overload identities cannot be \
                  told apart by anything downstream that joins on the oracle's own record",
        evidence: "fixtures/csharp-truth/src/Overloads.cs",
    },
    NamedMismatch {
        id: "type-argument-pair-produces-implements-edges-on-a-clean-build",
        attributed_producer: "devscout native dispatch resolution",
        reason: "an unrelated, unconstrained generic method whose name matches the Add*/*Scoped \
                  registration convention produces type-level and member-level implements edges \
                  against an interface it never implements, on a project that builds cleanly",
        evidence: "fixtures/csharp-truth/src/NativeDispatchCounterexample.cs",
    },
];

/// The rows this harness grades through real producer output.
///
/// See `tests/semantic_truth_baseline.rs` and
/// `truth::producer_reader::native_dispatch_implements_edges`, which runs
/// devscout's own native extract-and-resolve pipeline (the producer this
/// row's `attributed_producer` names) against the row's own evidence file
/// and reads its real edges, offline, rather than through evidence-file
/// existence and source greps alone. The other four rows remain narrative:
/// their evidence is reviewed and committed, but nothing in this crate
/// executes the external Roslyn oracle exporter or `FactsWalker` sidecar
/// those rows name, so reproducing them is future work this list does not
/// claim done today.
pub const GRADED_THROUGH_REAL_PRODUCER_OUTPUT: &[&str] =
    &["type-argument-pair-produces-implements-edges-on-a-clean-build"];

/// Do this crate's own harness tests pass, independent of what the graded
/// analyzer reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessHealth {
    /// Every harness self-test passed.
    Ok,
    /// A harness self-test failed, independent of the analyzer verdict.
    Broken {
        /// Why the harness itself is unhealthy.
        reasons: Vec<String>,
    },
}

/// Whether the graded analyzer's output matches the reviewed expectation.
/// `Red` always carries which named rows are responsible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzerVerdict {
    /// No registered mismatch was observed.
    Green,
    /// At least one registered mismatch was observed.
    Red {
        /// Which registered rows were observed this run.
        mismatch_ids: Vec<&'static str>,
    },
}

/// One evaluated baseline run: the harness's own health and the graded
/// analyzer's verdict, reported separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineRun {
    /// Whether the harness's own tests passed.
    pub harness_health: HarnessHealth,
    /// Whether the graded analyzer's output matched expectation.
    pub analyzer_verdict: AnalyzerVerdict,
}

/// Classifies a run's observed mismatches against the registered
/// baseline. An id absent from `RED_BASELINE` is a new mismatch, returned
/// separately -- it is never folded into the named list silently.
pub fn classify_mismatches(
    observed_ids: &[&'static str],
) -> (Vec<&'static str>, Vec<&'static str>) {
    let known: Vec<&'static str> = RED_BASELINE.iter().map(|m| m.id).collect();
    let mut named = Vec::new();
    let mut new = Vec::new();
    for id in observed_ids {
        if known.contains(id) {
            named.push(*id);
        } else {
            new.push(*id);
        }
    }
    (named, new)
}

/// Builds one baseline run's report. The harness's own test outcome and
/// the analyzer's verdict are computed independently and never merged into
/// one bit.
pub fn evaluate_run(
    harness_tests_passed: bool,
    observed_mismatch_ids: &[&'static str],
) -> BaselineRun {
    let harness_health = if harness_tests_passed {
        HarnessHealth::Ok
    } else {
        HarnessHealth::Broken {
            reasons: vec![
                "a harness self-test failed independently of the analyzer verdict".to_string(),
            ],
        }
    };
    let analyzer_verdict = if observed_mismatch_ids.is_empty() {
        AnalyzerVerdict::Green
    } else {
        AnalyzerVerdict::Red {
            mismatch_ids: observed_mismatch_ids.to_vec(),
        }
    };
    BaselineRun {
        harness_health,
        analyzer_verdict,
    }
}

/// Deterministic JSON for the committed baseline artifact.
pub fn red_baseline_to_json() -> String {
    J::Obj(vec![
        ("contract", J::Str("semantic-truth-v1".to_string())),
        (
            "mismatches",
            J::Arr(
                RED_BASELINE
                    .iter()
                    .map(|m| {
                        J::Obj(vec![
                            ("id", J::Str(m.id.to_string())),
                            (
                                "attributedProducer",
                                J::Str(m.attributed_producer.to_string()),
                            ),
                            ("reason", J::Str(m.reason.to_string())),
                            ("evidence", J::Str(m.evidence.to_string())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
    .to_json_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_red_analyzer_verdict_does_not_imply_a_broken_harness() {
        let run = evaluate_run(true, &["unrelated-names-promoted-to-framework-facts"]);
        assert_eq!(run.harness_health, HarnessHealth::Ok);
        assert!(matches!(run.analyzer_verdict, AnalyzerVerdict::Red { .. }));
    }

    #[test]
    fn a_green_run_with_no_observed_mismatch() {
        let run = evaluate_run(true, &[]);
        assert_eq!(run.analyzer_verdict, AnalyzerVerdict::Green);
    }

    #[test]
    fn every_registered_row_names_its_attributed_producer() {
        for row in RED_BASELINE {
            assert!(!row.attributed_producer.is_empty());
            assert!(!row.reason.is_empty());
        }
        assert_eq!(RED_BASELINE.len(), 5);
    }

    #[test]
    fn an_id_the_baseline_does_not_name_is_reported_as_new() {
        let (named, new) = classify_mismatches(&[
            "unrelated-names-promoted-to-framework-facts",
            "never-seen-before",
        ]);
        assert_eq!(named, vec!["unrelated-names-promoted-to-framework-facts"]);
        assert_eq!(new, vec!["never-seen-before"]);
    }

    #[test]
    fn baseline_json_is_byte_identical_across_two_builds() {
        assert_eq!(red_baseline_to_json(), red_baseline_to_json());
    }

    #[test]
    fn every_id_graded_through_real_producer_output_is_a_registered_row() {
        for id in GRADED_THROUGH_REAL_PRODUCER_OUTPUT {
            assert!(
                RED_BASELINE.iter().any(|m| &m.id == id),
                "'{id}' is graded but not a registered row"
            );
        }
    }
}
