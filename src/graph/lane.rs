// `stats.lane`: whether a `map` run applied the enriched (compiler-facts)
// lane, only partly (some compilations the facts came from were incomplete),
// or not at all and why. Without it a stale, unreadable or partial artifact
// degraded the run with nothing on screen or in the graph to say so.
//
// The persisted fields are strings, not enums, so a graph written by a newer
// build carrying a state or reason this build does not know still reads. This
// build only ever writes the values its typed constructors produce.

use serde::{Deserialize, Serialize};

const APPLIED: &str = "applied";
const PARTIAL: &str = "partial";
const NOT_APPLIED: &str = "not-applied";
const COMPILATIONS_INCOMPLETE: &str = "compilations-incomplete";

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
    /// At least one compilation is counted and none of them is complete, so
    /// no occurrence could confirm anything.
    NoCompleteCompilation,
    /// The artifact applied but confirmed and discovered nothing.
    NothingConfirmed,
}

impl LaneReason {
    /// The token written to `stats.lane.reason` and the lane line.
    pub fn as_str(self) -> &'static str {
        match self {
            LaneReason::StaleArtifact => "stale-artifact",
            LaneReason::ImportsMoved => "imports-moved",
            LaneReason::ArtifactUnreadable => "artifact-unreadable",
            LaneReason::NoSemantic => "no-semantic",
            LaneReason::NoCompleteCompilation => "no-complete-compilation",
            LaneReason::NothingConfirmed => "nothing-confirmed",
        }
    }
}

/// How many compilations the lane decision counted, and how many of them
/// were not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneCompilations {
    /// Every counted compilation.
    pub total: usize,
    /// The counted compilations that are not complete.
    pub partial: usize,
}

/// The run-level lane record, serialized as
/// `{state, reason?, detail?, compilations?}`.
///
/// `compilations` is written whenever the decision read the artifact's
/// compilations, and never for a reason decided before they were read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneStats {
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compilations: Option<LaneCompilations>,
}

impl LaneStats {
    /// The enriched lane applied to this run.
    pub fn applied() -> Self {
        LaneStats {
            state: APPLIED.to_string(),
            reason: None,
            detail: None,
            compilations: None,
        }
    }

    /// The enriched lane applied, but some counted compilation was not
    /// complete, so facts from it confirmed nothing.
    pub fn partial(detail: String) -> Self {
        LaneStats {
            state: PARTIAL.to_string(),
            reason: Some(COMPILATIONS_INCOMPLETE.to_string()),
            detail: Some(detail),
            compilations: None,
        }
    }

    /// The enriched lane did not apply. `detail` carries the freshness token
    /// verbatim when there is one.
    pub fn not_applied(reason: LaneReason, detail: Option<String>) -> Self {
        LaneStats {
            state: NOT_APPLIED.to_string(),
            reason: Some(reason.as_str().to_string()),
            detail,
            compilations: None,
        }
    }

    /// The same record carrying the compilation counts the decision read.
    pub fn with_compilations(self, compilations: LaneCompilations) -> Self {
        LaneStats {
            compilations: Some(compilations),
            ..self
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

    /// The compilation counts, when the decision read them.
    pub fn compilations(&self) -> Option<LaneCompilations> {
        self.compilations
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
        if self.state == PARTIAL {
            if let Some(detail) = &self.detail {
                return format!("lane: enriched, partial ({detail})");
            }
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
