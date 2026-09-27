// The part of the lane decision that reads the admitted artifact's
// compilations. A fresh artifact used to read as the plain enriched lane even
// when the producer marked its compilations partial, although an occurrence
// from a partial compilation confirms nothing: the lane then applied little
// or nothing while `map` and `audit` said `enriched`.
//
// Two points, both pure: before resolution, an artifact with no complete
// compilation is dropped like a stale one, so resolution never sees it; after
// resolution, the counts of what the layer actually did decide between
// `nothing-confirmed`, `partial` and `applied`.

use crate::graph::{CompilationTally, LaneCompilations, LaneReason, LaneStats, SemanticStats};

fn counts(tally: &CompilationTally) -> LaneCompilations {
    LaneCompilations {
        total: tally.total(),
        partial: tally.partial(),
    }
}

/// The not-applied record owed before resolution, or `None` when the layer
/// may go on to resolution.
pub fn before_resolution(tally: &CompilationTally) -> Option<LaneStats> {
    tally.none_complete().then(|| {
        LaneStats::not_applied(LaneReason::NoCompleteCompilation, tally.detail())
            .with_compilations(counts(tally))
    })
}

/// The final record once resolution has run with the layer, and whether
/// `stats.semantic` stays. A run that confirmed and discovered nothing did not
/// apply the lane, so it keeps no counters that would read as if it had.
pub fn after_resolution(
    tally: &CompilationTally,
    semantic: Option<&SemanticStats>,
) -> (LaneStats, bool) {
    if let Some(record) = before_resolution(tally) {
        return (record, false);
    }
    let used = semantic.map_or(0, |s| s.confirmed + s.discovered);
    let (record, keep) = if used == 0 {
        (
            LaneStats::not_applied(LaneReason::NothingConfirmed, tally.detail()),
            false,
        )
    } else if let Some(detail) = tally.detail() {
        (LaneStats::partial(detail), true)
    } else {
        (LaneStats::applied(), true)
    };
    (record.with_compilations(counts(tally)), keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(states: &[(&str, &str)]) -> CompilationTally {
        CompilationTally::from_states(states.iter().copied())
    }

    fn used(confirmed: usize, discovered: usize) -> SemanticStats {
        SemanticStats {
            confirmed,
            disagreements: 0,
            discovered,
        }
    }

    const COMPLETE: (&str, &str) = ("complete", "complete");
    const PARTIAL: (&str, &str) = ("partial", "workspace-failure");

    #[test]
    fn no_complete_compilation_is_decided_before_resolution_with_its_detail_and_counts() {
        let record = before_resolution(&tally(&[PARTIAL, PARTIAL])).expect("not applied");
        assert_eq!(
            record,
            LaneStats::not_applied(
                LaneReason::NoCompleteCompilation,
                Some("2 of 2 compilations partial: workspace-failure 2".to_string())
            )
            .with_compilations(LaneCompilations {
                total: 2,
                partial: 2
            })
        );
        assert_eq!(
            record.line(),
            "lane: syntax (enriched lane not applied: no-complete-compilation: 2 of 2 compilations partial: workspace-failure 2)"
        );
    }

    #[test]
    fn one_complete_compilation_or_none_counted_lets_the_layer_reach_resolution() {
        assert_eq!(before_resolution(&tally(&[COMPLETE, PARTIAL])), None);
        assert_eq!(before_resolution(&tally(&[])), None);
        assert_eq!(
            before_resolution(&tally(&[("excluded", "not-requested")])),
            None
        );
    }

    #[test]
    fn nothing_confirmed_drops_the_counters_and_names_partial_compilations_only_when_there_are_some(
    ) {
        let (healthy, keep) = after_resolution(&tally(&[COMPLETE]), Some(&used(0, 0)));
        assert!(!keep);
        assert_eq!(
            healthy,
            LaneStats::not_applied(LaneReason::NothingConfirmed, None).with_compilations(
                LaneCompilations {
                    total: 1,
                    partial: 0
                }
            )
        );
        assert_eq!(
            healthy.line(),
            "lane: syntax (enriched lane not applied: nothing-confirmed)"
        );

        let (mixed, keep) = after_resolution(&tally(&[COMPLETE, PARTIAL]), None);
        assert!(!keep);
        assert_eq!(mixed.reason(), Some("nothing-confirmed"));
        assert_eq!(
            mixed.detail(),
            Some("1 of 2 compilations partial: workspace-failure 1")
        );
    }

    #[test]
    fn facts_used_over_a_partial_compilation_keep_the_counters_and_read_partial() {
        let (record, keep) = after_resolution(&tally(&[COMPLETE, PARTIAL]), Some(&used(0, 1)));
        assert!(keep);
        assert_eq!(
            record,
            LaneStats::partial("1 of 2 compilations partial: workspace-failure 1".to_string())
                .with_compilations(LaneCompilations {
                    total: 2,
                    partial: 1
                })
        );
        assert_eq!(record.state(), "partial");
        assert_eq!(record.reason(), Some("compilations-incomplete"));
        assert!(!record.is_applied());
        assert_eq!(
            record.line(),
            "lane: enriched, partial (1 of 2 compilations partial: workspace-failure 1)"
        );
    }

    #[test]
    fn facts_used_over_complete_compilations_apply_with_counts() {
        let (record, keep) = after_resolution(&tally(&[COMPLETE, COMPLETE]), Some(&used(1, 0)));
        assert!(keep);
        assert_eq!(
            record,
            LaneStats::applied().with_compilations(LaneCompilations {
                total: 2,
                partial: 0
            })
        );
        assert_eq!(record.line(), "lane: enriched");
    }

    #[test]
    fn with_no_counted_compilation_the_decision_rests_on_the_counters_alone() {
        let (applied, keep) = after_resolution(&tally(&[]), Some(&used(1, 0)));
        assert!(keep);
        assert!(applied.is_applied());
        assert_eq!(
            applied.compilations(),
            Some(LaneCompilations {
                total: 0,
                partial: 0
            })
        );
        let (nothing, keep) = after_resolution(&tally(&[]), Some(&used(0, 0)));
        assert!(!keep);
        assert_eq!(nothing.reason(), Some("nothing-confirmed"));
    }

    #[test]
    fn the_record_serializes_compilations_last_and_reads_back() {
        let (record, _) = after_resolution(&tally(&[COMPLETE, PARTIAL]), Some(&used(1, 0)));
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(
            text,
            r#"{"state":"partial","reason":"compilations-incomplete","detail":"1 of 2 compilations partial: workspace-failure 1","compilations":{"total":2,"partial":1}}"#
        );
        assert_eq!(serde_json::from_str::<LaneStats>(&text).unwrap(), record);
    }
}
