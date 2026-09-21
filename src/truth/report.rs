//! Case evaluation and the `TruthReport` artifact.
//!
//! `evaluate_case` is the one comparator every case, fault control, and
//! freshness pair in this harness runs through: it fails an empty result
//! against a positive obligation, fails a case that satisfies only its
//! absent-facts list, and never reports a failed or partial context as
//! complete. The two lanes (`SyntaxOnly`/`Enriched`) are separate
//! top-level artifacts and are never summed or averaged into one figure
//! by anything in this file.

use std::io;
use std::path::Path;

use crate::query::json::J;

use super::identity::{OccurrenceSpan, SymbolIdentity};
use super::manifest::Case;
use super::pin::Pin;
use super::uncertainty::{ContextHealth, Uncertainty};

/// One fact an analyzer run actually produced for a case, in the same
/// shape a manifest's expected-present fact takes.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedFact {
    /// The observed symbol identity.
    pub identity: SymbolIdentity,
    /// The observed occurrence span.
    pub occurrence: OccurrenceSpan,
    /// The observed confidence state.
    pub state: Uncertainty,
}

/// One diagnostic an analyzer run actually emitted: its severity and code,
/// reduced to the pair a case's expected-diagnostic obligation checks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedDiagnostic {
    /// The diagnostic's severity.
    pub severity: String,
    /// The diagnostic's code.
    pub code: String,
}

/// What an analyzer run actually produced for one case: its facts,
/// diagnostics, and the context health it reports for the run.
///
/// Never upgraded -- there is no function anywhere in this crate that
/// turns a `Failed`/`Partial` health into `Complete`.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedCase {
    /// The context health this run reports.
    pub context: ContextHealth,
    /// The facts this run actually produced.
    pub facts: Vec<ObservedFact>,
    /// The diagnostics this run actually emitted.
    pub diagnostics: Vec<ObservedDiagnostic>,
}

/// The result of comparing an `ObservedCase` against its manifest case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseVerdict {
    /// Every obligation the case declares was satisfied.
    Pass,
    /// At least one obligation was missed; `reasons` names every one.
    Fail {
        /// Every obligation this case missed, one line per reason.
        reasons: Vec<String>,
    },
}

impl CaseVerdict {
    /// Whether this verdict is a pass.
    pub fn is_pass(&self) -> bool {
        matches!(self, CaseVerdict::Pass)
    }
}

fn fact_matches(
    expected_identity: &SymbolIdentity,
    expected_occurrence: &OccurrenceSpan,
    observed: &ObservedFact,
) -> bool {
    &observed.identity == expected_identity && &observed.occurrence == expected_occurrence
}

fn identity_asserted(identity: &SymbolIdentity, observed: &ObservedCase) -> bool {
    observed.facts.iter().any(|f| {
        &f.identity == identity
            && matches!(
                f.state,
                Uncertainty::Confirmed | Uncertainty::Candidate { .. }
            )
    })
}

/// Grades one case's observed run against its manifest expectation.
/// Reasons accumulate rather than short-circuit, so a single failing run
/// names every obligation it missed, not just the first.
pub fn evaluate_case(case: &Case, observed: &ObservedCase) -> CaseVerdict {
    let mut reasons = Vec::new();

    if !case.present.is_empty() && observed.facts.is_empty() {
        reasons.push(format!(
            "case '{}': empty analyzer result against a positive obligation",
            case.id
        ));
    }

    for expected in &case.present {
        let matched = observed.facts.iter().any(|f| {
            fact_matches(&expected.identity, &expected.occurrence, f)
                && f.state.label() == expected.state
        });
        if !matched {
            reasons.push(format!(
                "case '{}': expected present fact '{}' at {}:{} not observed",
                case.id,
                expected.identity.member,
                expected.occurrence.file,
                expected.occurrence.start_line
            ));
        }
    }

    for expected in &case.absent {
        if identity_asserted(&expected.identity, observed) {
            reasons.push(format!(
                "case '{}': expected-absent fact '{}' was asserted",
                case.id, expected.identity.member
            ));
        }
    }

    for expected in &case.unresolved {
        let observed_state = observed
            .facts
            .iter()
            .find(|f| fact_matches(&expected.identity, &expected.occurrence, f))
            .map(|f| f.state.label());
        match observed_state {
            Some(label) if label == expected.state => {}
            Some("confirmed") => reasons.push(format!(
                "case '{}': fact '{}' expected {} but was promoted to confirmed",
                case.id, expected.identity.member, expected.state
            )),
            Some(other) => reasons.push(format!(
                "case '{}': fact '{}' expected {} but observed {}",
                case.id, expected.identity.member, expected.state, other
            )),
            None => reasons.push(format!(
                "case '{}': expected unresolved fact '{}' not observed at all",
                case.id, expected.identity.member
            )),
        }
    }

    for expected in &case.diagnostics {
        let matched = observed
            .diagnostics
            .iter()
            .any(|d| d.severity == expected.severity && d.code == expected.code);
        if !matched {
            reasons.push(format!(
                "case '{}': expected diagnostic '{}' ({}) not observed",
                case.id, expected.code, expected.severity
            ));
        }
    }

    if observed.context.label() != case.context {
        reasons.push(format!(
            "case '{}': expected context '{}', observed '{}'",
            case.id,
            case.context,
            observed.context.label()
        ));
    }

    if reasons.is_empty() {
        CaseVerdict::Pass
    } else {
        CaseVerdict::Fail { reasons }
    }
}

/// Whether a report's own denominator has shrunk below the manifest's
/// declared count.
///
/// This is the "quality figure improves by omission" failure mode a
/// dropped case produces even though its own ratio can look fine, or
/// better, without it.
pub fn denominator_shrunk(observed_denominator: u32, declared_case_count: u32) -> bool {
    observed_denominator < declared_case_count
}

/// A ratio that reports undefined (`None`) rather than a perfect score
/// when its denominator is zero.
pub fn ratio_or_undefined(passed: u32, denominator: u32) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(f64::from(passed) / f64::from(denominator))
    }
}

/// Which of the two persisted result lanes a report belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Graded from syntax-only facts, no compiler enrichment.
    SyntaxOnly,
    /// Graded from compiler-enriched facts.
    Enriched,
}

impl Lane {
    /// The wire label for this lane.
    pub fn label(self) -> &'static str {
        match self {
            Lane::SyntaxOnly => "syntax-only",
            Lane::Enriched => "enriched",
        }
    }
}

/// The pinned-input header every report carries: enough to replay the run
/// from the record alone.
#[derive(Debug, Clone)]
pub struct PinnedInputHeader {
    /// The contract version this report was produced under.
    pub contract: String,
    /// The fixture corpus revision the run was graded against.
    pub corpus_revision: String,
    /// Every producer/version pair the run consulted.
    pub producer_versions: Vec<(String, String)>,
    /// The resolved context health the run recorded.
    pub resolved_context: String,
    /// The exact commands the run issued, in order.
    pub commands: Vec<String>,
    /// The pinned foundation-candidate digest this run was graded against --
    /// a consumer reads this straight off the report header rather than
    /// recomputing it from a side channel.
    pub pin: Pin,
}

/// One case's verdict, ready to be sorted into a report.
#[derive(Debug, Clone)]
pub struct CaseOutcome {
    /// The case this outcome is for.
    pub case_id: String,
    /// What the comparator decided.
    pub verdict: CaseVerdict,
}

/// The persisted, two-lane truth-and-capability report.
#[derive(Debug, Clone)]
pub struct TruthReport {
    /// Which lane this report belongs to.
    pub lane: Lane,
    /// The pinned-input header.
    pub header: PinnedInputHeader,
    /// Every graded case's outcome.
    pub outcomes: Vec<CaseOutcome>,
}

impl TruthReport {
    /// How many cases passed.
    pub fn passed(&self) -> u32 {
        self.outcomes.iter().filter(|o| o.verdict.is_pass()).count() as u32
    }

    /// How many cases this report graded.
    pub fn denominator(&self) -> u32 {
        self.outcomes.len() as u32
    }
}

/// Deterministic JSON for a report: outcomes sorted by case id, so two
/// in-process runs from the same pinned inputs are byte-identical.
pub fn report_to_json(report: &TruthReport) -> String {
    let mut outcomes = report.outcomes.clone();
    outcomes.sort_by(|a, b| a.case_id.cmp(&b.case_id));

    let ratio = ratio_or_undefined(report.passed(), report.denominator());

    J::Obj(vec![
        ("contract", J::Str(report.header.contract.clone())),
        ("lane", J::Str(report.lane.label().to_string())),
        (
            "pinnedInputs",
            J::Obj(vec![
                (
                    "corpusRevision",
                    J::Str(report.header.corpus_revision.clone()),
                ),
                (
                    "producerVersions",
                    J::Arr(
                        report
                            .header
                            .producer_versions
                            .iter()
                            .map(|(name, version)| {
                                J::Obj(vec![
                                    ("producer", J::Str(name.clone())),
                                    ("version", J::Str(version.clone())),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "resolvedContext",
                    J::Str(report.header.resolved_context.clone()),
                ),
                (
                    "commands",
                    J::Arr(
                        report
                            .header
                            .commands
                            .iter()
                            .map(|c| J::Str(c.clone()))
                            .collect(),
                    ),
                ),
                (
                    "pin",
                    J::Obj(vec![
                        ("contract", J::Str(report.header.pin.contract.clone())),
                        ("digestHex", J::Str(report.header.pin.digest_hex.clone())),
                    ]),
                ),
            ]),
        ),
        (
            "outcomes",
            J::Arr(
                outcomes
                    .iter()
                    .map(|o| {
                        let reasons = match &o.verdict {
                            CaseVerdict::Pass => Vec::new(),
                            CaseVerdict::Fail { reasons } => reasons.clone(),
                        };
                        J::Obj(vec![
                            ("caseId", J::Str(o.case_id.clone())),
                            ("pass", J::Bool(o.verdict.is_pass())),
                            ("reasons", J::Arr(reasons.into_iter().map(J::Str).collect())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("passed", J::UInt(u64::from(report.passed()))),
        ("denominator", J::UInt(u64::from(report.denominator()))),
        (
            "ratio",
            match ratio {
                Some(r) => J::RawNum(format!("{r:.6}")),
                None => J::Str("undefined".to_string()),
            },
        ),
    ])
    .to_json_string()
}

/// The committed file name for one lane's persisted artifact, inside
/// `fixtures/csharp-truth/reports/`.
pub fn lane_artifact_filename(lane: Lane) -> &'static str {
    match lane {
        Lane::SyntaxOnly => "syntax-only.json",
        Lane::Enriched => "enriched.json",
    }
}

/// Writes a report's JSON to its lane's persisted-artifact path under
/// `dir`, so the run is a committed file a consumer can read rather than a
/// string that only ever lived inside a test process.
pub fn write_lane_artifact(report: &TruthReport, dir: &Path) -> io::Result<()> {
    let path = dir.join(lane_artifact_filename(report.lane));
    std::fs::write(path, report_to_json(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::truth::identity::CompilationIdentity;
    use crate::truth::manifest::{ExpectedPresentFact, Provenance};

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

    fn positive_case() -> Case {
        Case {
            id: "positive-a".to_string(),
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

    #[test]
    fn empty_output_fails_a_positive_obligation() {
        let observed = ObservedCase {
            context: ContextHealth::Complete,
            facts: vec![],
            diagnostics: vec![],
        };
        let verdict = evaluate_case(&positive_case(), &observed);
        assert!(!verdict.is_pass());
    }

    #[test]
    fn satisfying_only_the_absent_list_cannot_pass_a_case() {
        let case = Case {
            absent: vec![super::super::manifest::ExpectedAbsentFact {
                identity: identity("NeverCalled"),
            }],
            ..positive_case()
        };
        // No facts at all: the absent obligation is trivially satisfied, but
        // the positive obligation is not -- this must still fail.
        let observed = ObservedCase {
            context: ContextHealth::Complete,
            facts: vec![],
            diagnostics: vec![],
        };
        assert!(!evaluate_case(&case, &observed).is_pass());
    }

    #[test]
    fn a_matching_observation_passes() {
        let observed = ObservedCase {
            context: ContextHealth::Complete,
            facts: vec![ObservedFact {
                identity: identity("Load"),
                occurrence: occurrence(10),
                state: Uncertainty::Confirmed,
            }],
            diagnostics: vec![],
        };
        assert!(evaluate_case(&positive_case(), &observed).is_pass());
    }

    #[test]
    fn a_partial_context_can_never_be_reported_complete() {
        let observed = ObservedCase {
            context: ContextHealth::Partial {
                reason: "one project missing".to_string(),
            },
            facts: vec![ObservedFact {
                identity: identity("Load"),
                occurrence: occurrence(10),
                state: Uncertainty::Confirmed,
            }],
            diagnostics: vec![],
        };
        let verdict = evaluate_case(&positive_case(), &observed);
        assert!(
            !verdict.is_pass(),
            "a partial context must not satisfy a case expecting complete"
        );
    }

    #[test]
    fn zero_denominator_ratio_is_undefined_not_perfect() {
        assert_eq!(ratio_or_undefined(0, 0), None);
        assert_eq!(ratio_or_undefined(3, 4), Some(0.75));
    }

    fn header() -> PinnedInputHeader {
        PinnedInputHeader {
            contract: "semantic-truth-v1".to_string(),
            corpus_revision: "fixtures/csharp-truth@test".to_string(),
            producer_versions: vec![("scout-semantic".to_string(), "0.6.0".to_string())],
            resolved_context: "complete".to_string(),
            commands: vec!["devscout map".to_string()],
            pin: crate::truth::pin::compute_pin("semantic-truth-v1", b"test", &[]),
        }
    }

    #[test]
    fn the_report_header_carries_the_pin() {
        let json = report_to_json(&TruthReport {
            lane: Lane::SyntaxOnly,
            header: header(),
            outcomes: vec![],
        });
        assert!(json.contains("\"pin\":{"));
        assert!(json.contains(&format!("\"digestHex\":\"{}\"", header().pin.digest_hex)));
    }

    #[test]
    fn a_failing_outcomes_reasons_are_in_the_record() {
        let report = TruthReport {
            lane: Lane::SyntaxOnly,
            header: header(),
            outcomes: vec![CaseOutcome {
                case_id: "a-case".to_string(),
                verdict: CaseVerdict::Fail {
                    reasons: vec!["expected present fact 'Load' not observed".to_string()],
                },
            }],
        };
        let json = report_to_json(&report);
        assert!(json.contains("expected present fact 'Load' not observed"));
    }

    #[test]
    fn two_runs_from_the_same_inputs_are_byte_identical() {
        let build = || TruthReport {
            lane: Lane::SyntaxOnly,
            header: header(),
            outcomes: vec![
                CaseOutcome {
                    case_id: "b-case".to_string(),
                    verdict: CaseVerdict::Pass,
                },
                CaseOutcome {
                    case_id: "a-case".to_string(),
                    verdict: CaseVerdict::Fail {
                        reasons: vec!["missing".to_string()],
                    },
                },
            ],
        };
        let first = report_to_json(&build());
        let second = report_to_json(&build());
        assert_eq!(first, second);
        // Sorted by case id regardless of insertion order.
        assert!(first.find("\"a-case\"").unwrap() < first.find("\"b-case\"").unwrap());
    }

    #[test]
    fn the_two_lanes_are_never_summed() {
        let syntax = TruthReport {
            lane: Lane::SyntaxOnly,
            header: header(),
            outcomes: vec![CaseOutcome {
                case_id: "a".to_string(),
                verdict: CaseVerdict::Pass,
            }],
        };
        let enriched = TruthReport {
            lane: Lane::Enriched,
            header: header(),
            outcomes: vec![CaseOutcome {
                case_id: "a".to_string(),
                verdict: CaseVerdict::Fail { reasons: vec![] },
            }],
        };
        let syntax_json = report_to_json(&syntax);
        let enriched_json = report_to_json(&enriched);
        assert_ne!(syntax_json, enriched_json);
        assert!(syntax_json.contains("\"lane\":\"syntax-only\""));
        assert!(enriched_json.contains("\"lane\":\"enriched\""));
    }
}
