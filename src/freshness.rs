//! Query-time index-freshness state for a programmatic consumer.
//!
//! `manifest::freshness_warning` already detects a stale index -- source
//! edited since the graph was last built, at the same HEAD -- but only as a
//! human-readable stderr line, and it collapses three different conditions
//! into the same `None`: no `index-state.json` sidecar, git unavailable, and
//! a genuinely fresh index. A `--json` consumer needs to tell those apart:
//! "verified fresh" is not the same claim as "cannot tell". This module
//! calls the exact same primitives `freshness_warning` does
//! (`manifest::read_index_state`, `manifest::git_head`,
//! `manifest::dirty_indexed_files_at`, `manifest::read_manifest`) so the two
//! answers can never disagree about the underlying facts, only about how
//! much of the distinction each one exposes. `manifest.rs` itself is frozen
//! at its current line count and gains no new logic here -- this is a
//! separate call site over its existing public functions, not a refactor of
//! it, so the frozen file's own stderr text and tests are untouched.

use std::collections::HashSet;
use std::path::Path;

use crate::manifest::{self, Value};

/// The query-time freshness state of an indexed root. Scoped to one root at
/// the moment it is computed; carries no cross-query identity of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshnessState {
    /// The index matches the working tree: the same HEAD as when it was
    /// built, and no indexed file has gone dirty since.
    Fresh,
    /// The index is behind the working tree.
    Stale {
        /// The HEAD the index was built at.
        indexed_head: String,
        /// The HEAD the working tree is at now.
        current_head: String,
        /// How many indexed files are dirty now but were not dirty when the
        /// index was built.
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
/// Collapsed together on `manifest::freshness_warning`'s stderr path (both
/// read as silence there); distinguished here because a machine reader must
/// be able to act differently on "never indexed with this feature" than on
/// "git is unavailable".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// No `index-state.json` sidecar was found or it did not parse -- an
    /// index built before this sidecar existed, or a corrupt one.
    NoIndexState,
    /// `git rev-parse HEAD` did not resolve -- git unavailable, or this root
    /// is not (or is no longer) a git repository.
    GitUnavailable,
}

impl UnknownReason {
    /// The stable word this reason serializes as under `--json`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UnknownReason::NoIndexState => "no-index-state",
            UnknownReason::GitUnavailable => "git-unavailable",
        }
    }
}

// The default scope `manifest::freshness_warning` falls back to when the
// manifest carries no non-empty `scoped_dirs` -- kept in step with that
// function's own fallback deliberately, not derived from it (it is not
// exposed as a constant there).
fn default_scope() -> Vec<String> {
    vec![".".to_string()]
}

fn string_array(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

/// Computes [`FreshnessState`] for `root`, from the same primitives
/// `manifest::freshness_warning` uses.
#[must_use]
pub fn index_freshness_state(root: &Path) -> FreshnessState {
    let Some(state) = manifest::read_index_state(root) else {
        return FreshnessState::Unknown {
            reason: UnknownReason::NoIndexState,
        };
    };
    let Some(stored_head) = state
        .get("head")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return FreshnessState::Unknown {
            reason: UnknownReason::NoIndexState,
        };
    };
    let Some(current_head) = manifest::git_head(root) else {
        return FreshnessState::Unknown {
            reason: UnknownReason::GitUnavailable,
        };
    };

    let manifest_doc = manifest::read_manifest(root).ok().flatten();
    let indexed_files: HashSet<String> = manifest_doc
        .as_ref()
        .and_then(|m| m.get("entries"))
        .and_then(Value::as_object)
        .map(|entries| entries.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let scope: Vec<String> = manifest_doc
        .as_ref()
        .and_then(|m| m.get("scoped_dirs"))
        .and_then(string_array)
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(default_scope);

    let current_dirty = manifest::dirty_indexed_files_at(root, &scope, &indexed_files);
    let baseline_dirty: HashSet<String> = state
        .get("dirty_indexed_files")
        .and_then(string_array)
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();
    let changed_files = current_dirty
        .iter()
        .filter(|p| !baseline_dirty.contains(*p))
        .count();

    if current_head == stored_head && changed_files == 0 {
        FreshnessState::Fresh
    } else {
        FreshnessState::Stale {
            indexed_head: stored_head,
            current_head,
            changed_files,
        }
    }
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
    }
}
