//! The one refusal path `devscout export` has.
//!
//! Every way the verb can decline to produce a stream lands on one enum and one
//! renderer, so a consumer reads a single line shape and branches on a reason
//! word instead of matching prose. The reason words are the export's own
//! vocabulary, documented beside the verb in `docs/reference/fact-export.md`.
//!
//! Each variant is a distinct cause because each needs a different consumer
//! action: a missing index and an index a 0.9.0 build published are both "run
//! `map`", but only the second one tells the reader their cache is from another
//! generation, and only `index-changed` says the export raced a `map` and
//! retrying is enough.

/// Why `devscout export` declined to write a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportRefusal {
    /// No repository was found, or the repository carries neither a freshness
    /// record nor a graph.
    NoIndex(String),
    /// A graph exists but no published record certifies it: a pre-0.10.0 index,
    /// or a tree whose root has no Git administrative directory.
    NoIndexState(String),
    /// A record or digest table exists but this build cannot certify it.
    LegacyIndex(String),
    /// A private artifact the export reads is absent.
    ArtifactMissing(String),
    /// A private artifact is present but could not be read, or does not parse.
    ArtifactUnreadable(String),
    /// A private artifact is of a generation this build does not know.
    UnknownGeneration(String),
    /// The index moved while the export ran, or a `map` holds it right now.
    IndexChanged(String),
}

impl ExportRefusal {
    /// The reason word this refusal prints, and the one a consumer branches on.
    pub(crate) fn reason(&self) -> &'static str {
        match self {
            ExportRefusal::NoIndex(_) => "no-index",
            ExportRefusal::NoIndexState(_) => "no-index-state",
            ExportRefusal::LegacyIndex(_) => "legacy-index",
            ExportRefusal::ArtifactMissing(_) => "artifact-missing",
            ExportRefusal::ArtifactUnreadable(_) => "artifact-unreadable",
            ExportRefusal::UnknownGeneration(_) => "unknown-generation",
            ExportRefusal::IndexChanged(_) => "index-changed",
        }
    }

    /// What went wrong, named specifically enough to diagnose without the code.
    fn detail(&self) -> &str {
        match self {
            ExportRefusal::NoIndex(detail)
            | ExportRefusal::NoIndexState(detail)
            | ExportRefusal::LegacyIndex(detail)
            | ExportRefusal::ArtifactMissing(detail)
            | ExportRefusal::ArtifactUnreadable(detail)
            | ExportRefusal::UnknownGeneration(detail)
            | ExportRefusal::IndexChanged(detail) => detail,
        }
    }

    /// The single stderr line every refusal prints. `devscout map` is named in
    /// every one of them: it is the one recovery the export has.
    pub(crate) fn line(&self) -> String {
        format!(
            "devscout export: refused ({}): {}; run `devscout map`, then export again",
            self.reason(),
            self.detail()
        )
    }

    /// One instance of every variant, so a test can hold the whole vocabulary
    /// to the documented shape rather than to the paths that happen to be
    /// reachable from a fixture.
    #[cfg(test)]
    pub(crate) fn every_variant() -> Vec<ExportRefusal> {
        vec![
            ExportRefusal::NoIndex("detail".to_string()),
            ExportRefusal::NoIndexState("detail".to_string()),
            ExportRefusal::LegacyIndex("detail".to_string()),
            ExportRefusal::ArtifactMissing("detail".to_string()),
            ExportRefusal::ArtifactUnreadable("detail".to_string()),
            ExportRefusal::UnknownGeneration("detail".to_string()),
            ExportRefusal::IndexChanged("detail".to_string()),
        ]
    }
}
