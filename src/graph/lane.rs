// `stats.lane`: whether a `map` run applied the enriched (compiler-facts)
// lane and, when it did not, why. Without it a stale or unreadable artifact
// degraded the run to the syntax lane with nothing on screen or in the graph
// to say so.
//
// The persisted fields are strings, not enums, so a graph written by a newer
// build carrying a state or reason this build does not know still reads. This
// build only ever writes the values its typed constructors produce.

use serde::{Deserialize, Serialize};

const APPLIED: &str = "applied";
const NOT_APPLIED: &str = "not-applied";

/// Why the enriched lane did not apply to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneReason {
    /// The checkout moved since capture: head, working tree or the capture's
    /// own snapshot.
    StaleArtifact,
    /// A build import file the capture recorded changed or disappeared.
    ImportsMoved,
    /// An entry sits at the artifact path but does not read as an admitted
    /// artifact.
    ArtifactUnreadable,
    /// `map --no-semantic` was given.
    NoSemantic,
}

impl LaneReason {
    /// The token written to `stats.lane.reason` and the lane line.
    pub fn as_str(self) -> &'static str {
        match self {
            LaneReason::StaleArtifact => "stale-artifact",
            LaneReason::ImportsMoved => "imports-moved",
            LaneReason::ArtifactUnreadable => "artifact-unreadable",
            LaneReason::NoSemantic => "no-semantic",
        }
    }
}

/// The run-level lane record, serialized as `{state, reason?, detail?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneStats {
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl LaneStats {
    /// The enriched lane applied to this run.
    pub fn applied() -> Self {
        LaneStats {
            state: APPLIED.to_string(),
            reason: None,
            detail: None,
        }
    }

    /// The enriched lane did not apply. `detail` carries the freshness token
    /// verbatim when there is one.
    pub fn not_applied(reason: LaneReason, detail: Option<String>) -> Self {
        LaneStats {
            state: NOT_APPLIED.to_string(),
            reason: Some(reason.as_str().to_string()),
            detail,
        }
    }

    /// The `state` token.
    pub fn state(&self) -> &str {
        &self.state
    }

    /// The `reason` token, when the record carries one.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// The `detail` token, when the record carries one.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Whether the enriched lane applied.
    pub fn is_applied(&self) -> bool {
        self.state == APPLIED
    }

    /// The line `map` prints after its summary line.
    pub fn line(&self) -> String {
        if self.is_applied() {
            return "lane: enriched".to_string();
        }
        match (&self.reason, &self.detail) {
            (Some(reason), Some(detail)) => {
                format!("lane: syntax (enriched lane not applied: {reason}: {detail})")
            }
            (Some(reason), None) => format!("lane: syntax (enriched lane not applied: {reason})"),
            (None, _) => format!("lane: {}", self.state),
        }
    }
}
