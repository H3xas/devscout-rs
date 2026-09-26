// The map-time lane decision: whether an admitted compiler-facts artifact
// may feed resolution, and the `stats.lane` record the run owes. Decided once,
// before resolution, so a layer that cannot apply is dropped rather than
// handed to the resolver: a stale layer changes no edge, but it used to leave
// a zeroed `stats.semantic` behind that `audit` read as the enriched lane.
//
// A record is due exactly when `--no-semantic` is given or an entry sits at
// the compiler-facts path. With neither, nothing is recorded, which keeps a
// build that never saw an artifact byte-identical to one from before the lane
// record existed. The same condition forces `map` to rebuild, so the record
// on disk always describes the current run.

use std::path::Path;

use crate::graph::{LaneReason, LaneStats, Stats};

use super::freshness::Freshness;
use super::layer::SemanticLayer;

/// One run's lane decision: the layer resolution may use, if any, and the
/// record this run owes, if any.
pub struct LaneDecision {
    layer: Option<SemanticLayer>,
    record: Option<LaneStats>,
}

impl LaneDecision {
    /// The layer to hand to resolution: present only when the lane applies.
    pub fn layer(&self) -> Option<&SemanticLayer> {
        self.layer.as_ref()
    }

    /// The record this run owes, before resolution.
    pub fn record(&self) -> Option<&LaneStats> {
        self.record.as_ref()
    }

    /// Whether the graph must be rebuilt so its record is this run's.
    pub fn forces_rebuild(&self) -> bool {
        self.record.is_some()
    }
}

/// Decides the lane for a `map` run at `root`. `--no-semantic` never reads
/// the artifact; every load failure, whatever its cause, is
/// `artifact-unreadable`.
pub fn decide(root: &Path, no_semantic: bool) -> LaneDecision {
    if no_semantic {
        return LaneDecision {
            layer: None,
            record: Some(LaneStats::not_applied(LaneReason::NoSemantic, None)),
        };
    }
    if !compiler_facts_artifact_present(root) {
        return LaneDecision {
            layer: None,
            record: None,
        };
    }
    let Some(layer) = SemanticLayer::load(root) else {
        return LaneDecision {
            layer: None,
            record: Some(LaneStats::not_applied(LaneReason::ArtifactUnreadable, None)),
        };
    };
    let record = record_for(layer.freshness());
    LaneDecision {
        layer: record.is_applied().then_some(layer),
        record: Some(record),
    }
}

fn record_for(freshness: &Freshness) -> LaneStats {
    let Some(token) = freshness.stale_reason() else {
        return LaneStats::applied();
    };
    let reason = if freshness.is_import_trigger() {
        LaneReason::ImportsMoved
    } else {
        LaneReason::StaleArtifact
    };
    LaneStats::not_applied(reason, Some(token.to_string()))
}

/// Writes the decision's record into the resolved graph's stats, after
/// resolution and before the graph is written.
pub fn record(decision: Option<&LaneDecision>, stats: &mut Stats) {
    stats.lane = decision.and_then(|d| d.record.clone());
}

/// Whether anything sits at the compiler-facts path, checked by presence
/// alone, never by parsing: an unparseable entry still owes a record.
pub(crate) fn compiler_facts_artifact_present(root: &Path) -> bool {
    crate::graph::compiler_facts_json_path(root).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scout-semantic-lane-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn plant(dir: &Path, bytes: &[u8]) {
        let path = crate::graph::compiler_facts_json_path(dir);
        std::fs::create_dir_all(path.parent().expect("artifact parent")).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }

    fn not_applied(reason: LaneReason, detail: Option<&str>) -> LaneStats {
        LaneStats::not_applied(reason, detail.map(str::to_string))
    }

    #[test]
    fn no_semantic_is_recorded_even_over_a_garbage_artifact() {
        let dir = scratch_dir("no-semantic");
        plant(&dir, b"not json");
        let decision = decide(&dir, true);
        assert!(decision.layer().is_none());
        assert_eq!(
            decision.record(),
            Some(&not_applied(LaneReason::NoSemantic, None))
        );
        assert!(decision.forces_rebuild());
    }

    #[test]
    fn no_artifact_owes_no_record_and_forces_nothing() {
        let dir = scratch_dir("absent");
        let decision = decide(&dir, false);
        assert!(decision.layer().is_none());
        assert!(decision.record().is_none());
        assert!(!decision.forces_rebuild());
    }

    #[test]
    fn an_entry_that_does_not_load_is_unreadable() {
        let garbage = scratch_dir("garbage");
        plant(&garbage, b"not json");
        let directory = scratch_dir("directory");
        std::fs::create_dir_all(crate::graph::compiler_facts_json_path(&directory)).unwrap();
        for dir in [&garbage, &directory] {
            let decision = decide(dir, false);
            assert!(decision.layer().is_none());
            assert_eq!(
                decision.record(),
                Some(&not_applied(LaneReason::ArtifactUnreadable, None))
            );
            assert!(decision.forces_rebuild());
        }
    }

    #[test]
    fn every_freshness_token_maps_to_its_reason_with_the_token_as_detail() {
        let cases = [
            (
                "source-snapshot-dirty-at-admission",
                LaneReason::StaleArtifact,
            ),
            ("source-snapshot-missing", LaneReason::StaleArtifact),
            ("working-tree-dirty", LaneReason::StaleArtifact),
            ("source-head-mismatch", LaneReason::StaleArtifact),
            ("source-head-unavailable", LaneReason::StaleArtifact),
            (
                "import-file-changed:external-file:../a.props",
                LaneReason::ImportsMoved,
            ),
            (
                "import-file-missing:external-file:../a.props",
                LaneReason::ImportsMoved,
            ),
        ];
        for (token, reason) in cases {
            let stale = Freshness::Stale {
                reason: token.to_string(),
            };
            assert_eq!(
                record_for(&stale),
                not_applied(reason, Some(token)),
                "{token}"
            );
        }
        assert_eq!(record_for(&Freshness::Fresh), LaneStats::applied());
    }

    #[test]
    fn record_copies_the_decision_into_stats_and_clears_it_without_one() {
        let mut stats: Stats = serde_json::from_str(concat!(
            r#"{"def_count":0,"file_count":0,"edges_by_kind":{"inherits":0,"uses-type":0,"#,
            r#""imports":0,"uses-member":0,"ctor-di":0,"implements":0,"overrides":0},"#,
            r#""ambiguous_count":0,"ambiguous_pct":0,"unresolved_external_count":0}"#
        ))
        .unwrap();
        let dir = scratch_dir("record");
        record(Some(&decide(&dir, true)), &mut stats);
        assert_eq!(stats.lane, Some(not_applied(LaneReason::NoSemantic, None)));
        record(None, &mut stats);
        assert_eq!(stats.lane, None);
    }

    #[test]
    fn presence_is_checked_on_disk_only_never_parsed() {
        let dir = scratch_dir("presence");
        assert!(!compiler_facts_artifact_present(&dir));
        plant(&dir, b"not json");
        assert!(compiler_facts_artifact_present(&dir));
    }
}
