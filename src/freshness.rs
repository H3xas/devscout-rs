//! Query-time index-freshness state for a programmatic consumer.
//!
//! An index is `fresh` only when the worktree asking published it and the bytes
//! it indexed -- dirty files included, compared by content -- still match the
//! working tree. Anything else is `stale`, or `unknown` with a reason. The
//! distinction matters because "verified fresh" is a much stronger claim than
//! "cannot tell", and the `--json` contract exists so a machine reader can act
//! on the difference.
//!
//! The state, the record it is read from and the validation that produces it
//! live apart on purpose:
//!
//! - [`record`] owns the on-disk format: the published record, the in-progress
//!   marker, the content-digest table and the artifact identities;
//! - [`publish`] owns the `map` side -- the lock, the marker, the final write;
//! - [`validate`] owns the query side -- one validation per invocation,
//!   bracketing the artifacts the answer came from, inside three caps;
//! - [`status`] owns the one `git status` reader both sides call;
//! - [`relevance`] owns which paths a change to can invalidate an index.
//!
//! The stderr lines live here, next to the states they describe, so a reader
//! can compare the human wording with the machine word in one place.

pub mod publish;
pub mod record;
pub mod relevance;
pub mod status;
pub mod validate;

use std::path::Path;

pub use record::{DigestTable, Record, RecordState};
pub use status::StatusError;

/// The query-time freshness state of an indexed root. Scoped to one root at
/// the moment it is computed; carries no cross-query identity of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshnessState {
    /// The index matches the working tree: this worktree published it, at the
    /// same HEAD, and no indexed byte differs.
    Fresh,
    /// The index is behind the working tree.
    Stale {
        /// The HEAD the index was built at.
        indexed_head: String,
        /// The HEAD the working tree is at now.
        current_head: String,
        /// How many indexed or configuration files differ from the bytes the
        /// index recorded.
        changed_files: usize,
    },
    /// Freshness could not be established either way.
    Unknown {
        /// Why this could not resolve to `Fresh` or `Stale`.
        reason: UnknownReason,
    },
}

/// Why [`FreshnessState::Unknown`] could not resolve further.
///
/// These are not collapsed: a machine reader must be able to act differently on
/// "this index belongs to another worktree" than on "git is unavailable" than on
/// "there were more changes than the check reads".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// No freshness record was found, or the one there cannot certify anything
    /// -- an index built before this record existed, a 0.9.0 one, or a corrupt
    /// one.
    NoIndexState,
    /// Git could not answer: `rev-parse HEAD` did not resolve, or the status
    /// read failed. Also this root not being a git repository at all.
    GitUnavailable,
    /// The index was published by a different worktree of this repository.
    OtherWorktree,
    /// The change set exceeded one of the validation caps, so freshness could
    /// not be verified inside the budget.
    ValidationBudget,
    /// The index was replaced or re-published while this query ran, or a `map`
    /// is running or was interrupted.
    IndexChanged,
}

impl UnknownReason {
    /// The stable word this reason serializes as under `--json`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UnknownReason::NoIndexState => "no-index-state",
            UnknownReason::GitUnavailable => "git-unavailable",
            UnknownReason::OtherWorktree => "other-worktree",
            UnknownReason::ValidationBudget => "validation-budget",
            UnknownReason::IndexChanged => "index-changed",
        }
    }
}

/// How a root is named in a stderr line: its own directory name, the same word
/// the 0.9.0 stale line used.
#[must_use]
pub fn repo_id(root: &Path) -> String {
    root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo")
        .to_string()
}

fn short(head: &str) -> &str {
    &head[..head.len().min(7)]
}

/// The stale line, byte for byte the format 0.9.0 printed.
#[must_use]
pub fn stale_line(repo: &str, indexed_head: &str, current_head: &str, changed: usize) -> String {
    format!(
        "devscout: index for {repo} is stale (indexed at {}, HEAD {}; {changed} changed files) — rebuild with devscout map",
        short(indexed_head),
        short(current_head),
    )
}

/// The line for an index another worktree published.
#[must_use]
pub fn other_worktree_line(repo: &str) -> String {
    format!(
        "devscout: index for {repo} was built in another worktree — run devscout map here to use it"
    )
}

/// The line for an index that changed under a running query, or one a `map` is
/// mid-way through writing.
#[must_use]
pub fn index_changed_line(repo: &str) -> String {
    format!(
        "devscout: index for {repo} changed while this query ran, or a map is running or was interrupted — re-run the query, or rebuild with devscout map"
    )
}

/// The line for a change set past the validation caps.
#[must_use]
pub fn validation_budget_line(repo: &str) -> String {
    format!(
        "devscout: freshness of the index for {repo} could not be verified (more changes than the check reads) — rebuild with devscout map"
    )
}

/// The line for a record this build cannot certify -- a 0.9.0 index, or a
/// corrupt one.
#[must_use]
pub fn uncertifiable_line(repo: &str) -> String {
    format!(
        "devscout: index for {repo} has no freshness record this version can verify — rebuild with devscout map"
    )
}

/// Computes [`FreshnessState`] for `root`, as the library convenience over a
/// full validation asked from `root` itself.
///
/// The caller keeps the state only; the stderr line [`validate::Begin::finish`]
/// also returns belongs to whoever prints it, and this entry point prints
/// nothing.
#[must_use]
pub fn index_freshness_state(root: &Path) -> FreshnessState {
    validate::begin(root, root).finish().state
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn no_index_state_sidecar_is_unknown_with_that_reason() {
        let dir = std::env::temp_dir().join(format!(
            "devscout-freshness-unit-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            index_freshness_state(&dir),
            FreshnessState::Unknown {
                reason: UnknownReason::NoIndexState
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_reason_words_are_stable() {
        assert_eq!(UnknownReason::NoIndexState.as_str(), "no-index-state");
        assert_eq!(UnknownReason::GitUnavailable.as_str(), "git-unavailable");
        assert_eq!(UnknownReason::OtherWorktree.as_str(), "other-worktree");
        assert_eq!(
            UnknownReason::ValidationBudget.as_str(),
            "validation-budget"
        );
        assert_eq!(UnknownReason::IndexChanged.as_str(), "index-changed");
    }

    #[test]
    fn each_reason_has_its_own_line_with_a_stable_key_phrase() {
        assert!(other_worktree_line("repo").contains("another worktree"));
        assert!(index_changed_line("repo").contains("changed while"));
        assert!(validation_budget_line("repo").contains("could not be verified"));
        assert!(uncertifiable_line("repo").contains("rebuild with devscout map"));
    }

    #[test]
    fn the_stale_line_keeps_the_legacy_shape() {
        assert_eq!(
            stale_line("repo", "abcdef0123", "1234567890", 2),
            "devscout: index for repo is stale (indexed at abcdef0, HEAD 1234567; 2 changed files) — rebuild with devscout map"
        );
    }
}
