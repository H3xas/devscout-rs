//! The query side of freshness: one validation per invocation, bracketing the
//! artifacts the answer was read from.
//!
//! [`begin`] snapshots the record *before* the calling verb opens `graph.json`
//! or `manifest.json`; [`finish`] re-reads the record and re-stats both
//! artifacts *after* the answer is built. A `map` that lands in between changes
//! the generation or the identities, and the answer reports
//! `index-changed` rather than certifying a graph it did not read.
//!
//! Validation is bounded by three constants -- [`STATUS_ENTRY_CAP`],
//! [`CANDIDATE_PATH_CAP`] and [`CANDIDATE_BYTE_CAP`] -- rather than a wall-clock
//! deadline, so one tree gets the same answer on every machine. Past a cap the
//! state is `unknown` with `validation-budget`: an answer that cannot be
//! verified is never reported as verified.
//!
//! A candidate path is compared by its bytes, never by size or modification
//! time. The size read here is a budget estimate only; the digest decides.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::graph::graph_json_path;
use crate::layout;

use super::record::{
    self, ArtifactIdentity, DigestTable, Record, RecordState, RECORD_FORMAT, RECORD_VERSION,
};
use super::relevance;
use super::status::{git_status_paths, scope_pathspecs, StatusError, STATUS_ENTRY_CAP};
use super::{index_changed_line, FreshnessState, UnknownReason};
use super::{other_worktree_line, repo_id, stale_line, uncertifiable_line, validation_budget_line};

/// The most candidate paths one validation will hash.
pub const CANDIDATE_PATH_CAP: usize = 2_048;

/// The most candidate bytes one validation will read.
pub const CANDIDATE_BYTE_CAP: u64 = 16 * 1024 * 1024;

/// The freshness verdict plus the one stderr line that belongs to it.
///
/// The two travel together because they must come from the same validation:
/// `stderr` and `--json` describe one reading of one moment, never two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// What `--json` reports as `freshness`.
    pub state: FreshnessState,
    /// The stderr line for this verdict, `None` when stderr stays silent.
    pub warning: Option<String>,
}

/// A validation opened before the calling verb reads its artifacts.
pub struct Begin {
    root: PathBuf,
    // The worktree the query was asked from, by its Git administrative
    // directory. `None` when the invocation directory has no `.git` ancestor.
    asking: Option<String>,
    snapshot: RecordState,
}

/// Snapshots the record and resolves the asking worktree. Does no Git work.
#[must_use]
pub fn begin(root: &Path, invocation_dir: &Path) -> Begin {
    let asking = crate::repo::find_repo_root(invocation_dir).and_then(|r| record::owner_of(&r));
    Begin {
        root: root.to_path_buf(),
        asking,
        snapshot: record::read_record(root),
    }
}

fn unknown(reason: UnknownReason, warning: Option<String>) -> Validation {
    Validation {
        state: FreshnessState::Unknown { reason },
        warning,
    }
}

impl Begin {
    /// A reference to the root this validation was opened for.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The asking worktree's identity.
    #[must_use]
    pub fn asking(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    /// Runs the validation, once per invocation.
    #[must_use]
    pub fn finish(&self) -> Validation {
        let root = self.root.as_path();
        let id = repo_id(root);
        let published = match self.certifiable_record(&id) {
            Ok(record) => record,
            Err(validation) => return validation,
        };

        let Some(head) = crate::manifest::git_head(root) else {
            return unknown(UnknownReason::GitUnavailable, None);
        };

        let candidates = match self.candidates(root, &published, &id) {
            Ok(candidates) => candidates,
            Err(validation) => return validation,
        };

        // A clean tree opens no digest table at all: there is nothing to
        // compare, and the common case pays nothing for the check.
        let digests: Option<DigestTable> = if candidates.is_empty() {
            None
        } else {
            match record::read_digests(root) {
                Some(table) if table.generation == published.generation => Some(table),
                // A table from another run, or none at all, describes bytes that
                // are not the ones this record certifies.
                _ => return unknown(UnknownReason::IndexChanged, Some(index_changed_line(&id))),
            }
        };

        let changed_files =
            match changed_candidates(root, &published, &candidates, digests.as_ref()) {
                Ok(count) => count,
                Err(validation) => return validation,
            };

        if let Some(validation) = end_changed(root, &published, &id) {
            return validation;
        }

        let indexed_head = published.indexed_head.clone().unwrap_or_default();
        if published.indexed_head.as_deref() == Some(head.as_str()) && changed_files == 0 {
            Validation {
                state: FreshnessState::Fresh,
                warning: None,
            }
        } else {
            Validation {
                warning: Some(stale_line(&id, &indexed_head, &head, changed_files)),
                state: FreshnessState::Stale {
                    indexed_head,
                    current_head: head,
                    changed_files,
                },
            }
        }
    }

    // The record arms that decide before any Git work: nothing to certify, a
    // record that cannot certify, a run in flight, or an index another worktree
    // published.
    fn certifiable_record(&self, id: &str) -> Result<Record, Validation> {
        let root = self.root.as_path();
        let published: Record = match &self.snapshot {
            RecordState::Missing => return Err(unknown(UnknownReason::NoIndexState, None)),
            RecordState::Uncertifiable => {
                return Err(unknown(
                    UnknownReason::NoIndexState,
                    Some(uncertifiable_line(id)),
                ))
            }
            RecordState::InProgress(_) => {
                return Err(unknown(
                    UnknownReason::IndexChanged,
                    Some(index_changed_line(id)),
                ))
            }
            RecordState::Published(record) => record.as_ref().clone(),
        };

        // The asking worktree, this root's own worktree and the record's owner
        // must all be the same worktree: any other answer here would certify
        // bytes this worktree never indexed.
        let root_owner = record::owner_of(root);
        if root_owner.as_deref() != Some(published.owner.as_str())
            || self.asking.as_deref() != Some(published.owner.as_str())
        {
            return Err(unknown(
                UnknownReason::OtherWorktree,
                Some(other_worktree_line(id)),
            ));
        }
        Ok(published)
    }

    // The candidate set: everything the status read reports that could
    // invalidate the index, plus everything the run itself recorded as already
    // changed.
    fn candidates(
        &self,
        root: &Path,
        published: &Record,
        id: &str,
    ) -> Result<BTreeSet<String>, Validation> {
        let status_paths =
            match git_status_paths(root, &status_pathspecs(published), Some(STATUS_ENTRY_CAP)) {
                Ok(paths) => paths,
                Err(StatusError::GitUnavailable) => {
                    return Err(unknown(UnknownReason::GitUnavailable, None))
                }
                Err(StatusError::BudgetExceeded) => {
                    return Err(unknown(
                        UnknownReason::ValidationBudget,
                        Some(validation_budget_line(id)),
                    ))
                }
            };

        let mut candidates: BTreeSet<String> = status_paths
            .iter()
            .filter(|p| relevance::is_relevant(p, &published.scope))
            .cloned()
            .collect();
        candidates.extend(published.dirty_at_map.clone().unwrap_or_default());
        if candidates.len() > CANDIDATE_PATH_CAP {
            return Err(unknown(
                UnknownReason::ValidationBudget,
                Some(validation_budget_line(id)),
            ));
        }
        Ok(candidates)
    }
}

// How many candidates differ from the bytes the record certifies, inside the
// byte cap.
fn changed_candidates(
    root: &Path,
    published: &Record,
    candidates: &BTreeSet<String>,
    digests: Option<&DigestTable>,
) -> Result<usize, Validation> {
    let mut estimated_bytes: u64 = 0;
    for candidate in candidates {
        if let Ok(meta) = fs::symlink_metadata(root.join(candidate)) {
            if meta.is_file() {
                estimated_bytes = estimated_bytes.saturating_add(meta.len());
            }
        }
    }
    if estimated_bytes > CANDIDATE_BYTE_CAP {
        return Err(budget(published, root));
    }

    let reads: Vec<CandidateRead> = candidates
        .par_iter()
        .map(|candidate| read_candidate(root, candidate))
        .collect();

    let read_bytes: u64 = reads
        .iter()
        .filter_map(|read| read.bytes)
        .fold(0u64, u64::saturating_add);
    if read_bytes > CANDIDATE_BYTE_CAP {
        return Err(budget(published, root));
    }

    Ok(reads
        .iter()
        .filter(|read| {
            match recorded_digest(published, digests, &read.path) {
                Some(expected) => match (&read.digest, read.regular) {
                    (Some(digest), true) => digest != expected,
                    // Recorded, but no longer a regular file.
                    _ => true,
                },
                // Never recorded: a file that is there now was added.
                None => read.regular,
            }
        })
        .count())
}

fn budget(published: &Record, root: &Path) -> Validation {
    let _ = published;
    unknown(
        UnknownReason::ValidationBudget,
        Some(validation_budget_line(&repo_id(root))),
    )
}

// The bracket's closing half: a `map` that landed while this query ran changed
// the record or one of the two artifacts the answer came from.
fn end_changed(root: &Path, published: &Record, id: &str) -> Option<Validation> {
    match record::read_record(root) {
        RecordState::Published(end) if end.generation == published.generation => {}
        _ => {
            return Some(unknown(
                UnknownReason::IndexChanged,
                Some(index_changed_line(id)),
            ))
        }
    }
    if ArtifactIdentity::of(&graph_json_path(root)) != published.artifacts.graph
        || ArtifactIdentity::of(&manifest_artifact_path(root)) != published.artifacts.manifest
    {
        return Some(unknown(
            UnknownReason::IndexChanged,
            Some(index_changed_line(id)),
        ));
    }
    None
}

/// The manifest artifact's path, matching `manifest.json`'s own resolution
/// beside the record.
#[must_use]
pub fn manifest_artifact_path(root: &Path) -> PathBuf {
    record::state_dir(root).join(layout::MANIFEST_FILE)
}

// The pathspecs a query's status read covers: the published scope, every
// configuration input the ancestor scan can reach, and every configuration
// input this run actually recorded -- which catches a recorded input sitting
// outside the scope's own pathspecs.
fn status_pathspecs(published: &Record) -> Vec<String> {
    let mut specs = scope_pathspecs(&published.scope);
    specs.extend(relevance::config_pathspecs(&published.scope));
    for rel in published.config_inputs.keys() {
        specs.push(format!(":(literal){rel}"));
    }
    specs
}

fn recorded_digest<'a>(
    published: &'a Record,
    digests: Option<&'a DigestTable>,
    rel: &str,
) -> Option<&'a String> {
    if relevance::is_config_input(rel, &published.scope) {
        return published.config_inputs.get(rel);
    }
    digests.and_then(|t| t.files.get(rel))
}

struct CandidateRead {
    path: String,
    digest: Option<String>,
    bytes: Option<u64>,
    regular: bool,
}

fn read_candidate(root: &Path, rel: &str) -> CandidateRead {
    let path = root.join(rel);
    let regular = fs::symlink_metadata(&path).is_ok_and(|m| m.is_file());
    match fs::read(&path) {
        Ok(bytes) => CandidateRead {
            path: rel.to_string(),
            bytes: Some(bytes.len() as u64),
            digest: Some(record::sha256_hex(&bytes)),
            regular,
        },
        Err(_) => CandidateRead {
            path: rel.to_string(),
            digest: None,
            bytes: None,
            regular,
        },
    }
}

/// Whether a record parses as this build's published shape, used by tests and
/// the hook gate to state the format they expect.
#[must_use]
pub fn is_published_format(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            Some(
                v.get("format")?.as_str()? == RECORD_FORMAT
                    && v.get("version")?.as_u64()? == u64::from(RECORD_VERSION)
                    && v.get("state")?.as_str()? == "published",
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_are_the_documented_values() {
        assert_eq!(STATUS_ENTRY_CAP, 5_000);
        assert_eq!(CANDIDATE_PATH_CAP, 2_048);
        assert_eq!(CANDIDATE_BYTE_CAP, 16 * 1024 * 1024);
    }

    #[test]
    fn a_missing_record_opens_and_finishes_silently() {
        let dir = std::env::temp_dir().join(format!("scout-validate-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let begin = begin(&dir, &dir);
        let validation = begin.finish();
        assert_eq!(
            validation.state,
            FreshnessState::Unknown {
                reason: UnknownReason::NoIndexState
            }
        );
        assert!(validation.warning.is_none());
    }

    #[test]
    fn a_map_between_begin_and_finish_reports_index_changed() {
        let dir = std::env::temp_dir().join(format!(
            "scout-validate-bracket-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_AUTHOR_NAME", "devscout-test")
                .env("GIT_AUTHOR_EMAIL", "devscout-test@example.com")
                .env("GIT_COMMITTER_NAME", "devscout-test")
                .env("GIT_COMMITTER_EMAIL", "devscout-test@example.com")
                .current_dir(&dir)
                .output()
                .expect("git must be installed");
            assert!(out.status.success(), "git {args:?}: {out:?}");
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q", "."]);
        fs::write(dir.join("a.cs"), "class A {}\n").unwrap();

        let scope = vec![".".to_string()];
        let opts = crate::mapcmd::MapOptions::default();
        git(&["add", "-A"]);
        let tree = git(&["write-tree"]);
        let sha = git(&["commit-tree", &tree, "-m", "init"]);
        git(&["update-ref", "refs/heads/main", &sha]);
        // `git init` names its first branch from configuration, which differs between machines.
        git(&["symbolic-ref", "HEAD", "refs/heads/main"]);

        crate::mapcmd::map_repo(&dir, &scope, opts.clone()).unwrap();

        let begin = begin(&dir, &dir);
        // A second run publishes a new generation while the answer is built.
        crate::mapcmd::map_repo(&dir, &scope, opts).unwrap();

        assert_eq!(
            begin.finish().state,
            FreshnessState::Unknown {
                reason: UnknownReason::IndexChanged
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_published_format_probe_rejects_the_legacy_shape() {
        assert!(is_published_format(
            r#"{"format":"devscout-index-state","version":2,"state":"published"}"#
        ));
        assert!(!is_published_format(r#"{"head":"abc"}"#));
    }
}
