//! The crate's one `git status` reader.
//!
//! Two callers want the same facts at different costs: `map` wants the complete
//! set of paths Git reports as changed inside its scope, and a query wants the
//! same set bounded by [`STATUS_ENTRY_CAP`] so a pathological tree cannot stall
//! an answer. One parser serves both, always `-z` (NUL-separated), always
//! fail-closed: a spawn failure, a non-zero exit, a malformed record or a
//! non-UTF-8 path is [`StatusError::GitUnavailable`], never an empty set, so no
//! caller mistakes "could not check" for "confirmed clean".
//!
//! `--no-renames` deliberately: a rename then shows as a deletion plus an
//! addition, which is exactly the pair of paths a content comparison needs, and
//! it removes the `-z` rename record's second path field from the grammar.
//! `--no-optional-locks` keeps a query from taking `index.lock` out from under a
//! user running Git in the same tree.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// The most `git status` records a bounded read accepts.
///
/// A tree past it reports [`StatusError::BudgetExceeded`] rather than a
/// truncated set, because a truncated set cannot tell "nothing else changed"
/// from "the rest is unknown".
pub const STATUS_ENTRY_CAP: usize = 5_000;

/// Why a `git status` read produced no usable path set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusError {
    /// Git could not be spawned, exited non-zero, or produced output this
    /// reader cannot parse.
    GitUnavailable,
    /// The read produced more records than the requested cap allows.
    BudgetExceeded,
}

/// The pathspecs that cover a scope: each scope directory literally, `"."`
/// meaning the whole repo.
#[must_use]
pub fn scope_pathspecs(scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .map(|d| {
            if d == "." {
                ".".to_string()
            } else {
                format!(":(literal){d}")
            }
        })
        .collect()
}

/// One `git status --porcelain -z` run over `pathspecs`, as the set of
/// repo-relative paths it reports.
///
/// `entry_cap` bounds the number of records: `None` takes every record (the
/// `map` side, which must see the whole union), `Some(n)` reports
/// [`StatusError::BudgetExceeded`] past `n` (the query side).
pub fn git_status_paths(
    root: &Path,
    pathspecs: &[String],
    entry_cap: Option<usize>,
) -> Result<BTreeSet<String>, StatusError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args([
        "--no-optional-locks",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--no-renames",
        "--",
    ]);
    for p in pathspecs {
        cmd.arg(p);
    }
    let output = cmd.output().map_err(|_| StatusError::GitUnavailable)?;
    if !output.status.success() {
        return Err(StatusError::GitUnavailable);
    }

    let mut paths = BTreeSet::new();
    for (index, record) in output
        .stdout
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .enumerate()
    {
        if entry_cap.is_some_and(|cap| index >= cap) {
            return Err(StatusError::BudgetExceeded);
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(StatusError::GitUnavailable);
        }
        let path = std::str::from_utf8(&record[3..]).map_err(|_| StatusError::GitUnavailable)?;
        paths.insert(path.to_string());
    }
    Ok(paths)
}

/// A plain bool over [`git_status_paths`], for callers that only need "is
/// anything dirty in scope".
///
/// Fail-open to `false` on unavailable Git, the same default the compiler-fact
/// freshness path persisted before this moved: a root Git cannot answer for is
/// not "dirty", it is unknown. A tree past the cap is dirty by any reading, so
/// the bounded arm answers `true` rather than pretending it is clean.
#[must_use]
pub fn is_working_tree_dirty(root: &Path, scope: &[String]) -> bool {
    match git_status_paths(root, &scope_pathspecs(scope), Some(STATUS_ENTRY_CAP)) {
        Ok(paths) => !paths.is_empty(),
        Err(StatusError::BudgetExceeded) => true,
        Err(StatusError::GitUnavailable) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let wanted = std::env::temp_dir().join(format!(
            "scout-freshness-status-{label}-{}-{n}",
            std::process::id()
        ));
        fs::canonicalize(claim(wanted)).unwrap()
    }

    // Claims a new directory at `wanted`, or at the first free `-<n>` variant: a
    // repeated process id and counter must not reopen an earlier run's directory
    // and with it the files a test expects to have created itself.
    fn claim(wanted: PathBuf) -> PathBuf {
        let name = wanted
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for attempt in 0..10_000u32 {
            let candidate = if attempt == 0 {
                wanted.clone()
            } else {
                wanted.with_file_name(format!("{name}-{attempt}"))
            };
            match fs::create_dir(&candidate) {
                Ok(()) => return candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create scratch dir {}: {e}", candidate.display()),
            }
        }
        panic!("no free scratch directory name for {}", wanted.display());
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .expect("git must be installed");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    // Commits the working tree the way the integration fixtures do -- never
    // `git commit`, which would need an identity the test machine may not set.
    fn commit_all(dir: &Path) {
        git(dir, &["add", "-A"]);
        let tree = capture(dir, &["write-tree"]);
        let sha = capture(dir, &["commit-tree", &tree, "-m", "init"]);
        git(dir, &["update-ref", "refs/heads/master", &sha]);
        git(dir, &["symbolic-ref", "HEAD", "refs/heads/master"]);
    }

    fn capture(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .expect("git must be installed");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn repo(label: &str) -> PathBuf {
        let dir = scratch(label);
        git(&dir, &["init", "-q", "."]);
        dir
    }

    #[test]
    fn scope_pathspecs_marks_each_dir_literal() {
        assert_eq!(
            scope_pathspecs(&["src".to_string(), "app".to_string()]),
            vec![":(literal)src".to_string(), ":(literal)app".to_string()]
        );
        assert_eq!(scope_pathspecs(&[".".to_string()]), vec![".".to_string()]);
    }

    #[test]
    fn a_clean_repo_reads_as_an_empty_set_not_a_failure() {
        let dir = repo("clean");
        fs::write(dir.join("a.cs"), "class A {}\n").unwrap();
        commit_all(&dir);
        let set = git_status_paths(&dir, &[".".to_string()], None).unwrap();
        assert!(set.is_empty(), "{set:?}");
    }

    #[test]
    fn untracked_files_and_paths_with_spaces_quotes_and_non_ascii_parse() {
        let dir = repo("names");
        for name in ["has space.cs", "has\"quote.cs", "café.cs", "plain.cs"] {
            fs::write(dir.join(name), "class A {}\n").unwrap();
        }
        let set = git_status_paths(&dir, &[".".to_string()], None).unwrap();
        for name in ["has space.cs", "has\"quote.cs", "café.cs", "plain.cs"] {
            assert!(set.contains(name), "{name} missing from {set:?}");
        }
    }

    #[test]
    fn pathspecs_bound_the_read_to_the_scope() {
        let dir = repo("scope");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("other")).unwrap();
        fs::write(dir.join("src/in.cs"), "class A {}\n").unwrap();
        fs::write(dir.join("other/out.cs"), "class B {}\n").unwrap();
        let set = git_status_paths(&dir, &scope_pathspecs(&["src".to_string()]), None).unwrap();
        assert!(set.contains("src/in.cs"));
        assert!(!set.contains("other/out.cs"), "{set:?}");
    }

    #[test]
    fn a_cap_reports_budget_rather_than_a_truncated_set() {
        let dir = repo("cap");
        for i in 0..3 {
            fs::write(dir.join(format!("f{i}.cs")), "class A {}\n").unwrap();
        }
        let at_cap = git_status_paths(&dir, &[".".to_string()], Some(3)).unwrap();
        assert_eq!(at_cap.len(), 3);
        assert_eq!(
            git_status_paths(&dir, &[".".to_string()], Some(2)),
            Err(StatusError::BudgetExceeded)
        );
    }

    #[test]
    fn a_non_repo_is_git_unavailable_and_false() {
        let dir = scratch("norepo");
        fs::write(dir.join("a.cs"), "class A {}\n").unwrap();
        assert_eq!(
            git_status_paths(&dir, &[".".to_string()], None),
            Err(StatusError::GitUnavailable)
        );
        assert!(!is_working_tree_dirty(&dir, &[".".to_string()]));
    }

    #[test]
    fn a_dirty_tree_reports_true() {
        let dir = repo("dirty");
        fs::write(dir.join("a.cs"), "class A {}\n").unwrap();
        commit_all(&dir);
        fs::write(dir.join("a.cs"), "class A { int X; }\n").unwrap();
        assert!(is_working_tree_dirty(&dir, &[".".to_string()]));
    }

    #[test]
    fn a_scratch_directory_left_by_an_earlier_run_is_never_reopened() {
        let base = scratch("reuse");
        let stale = base.join("stale");
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join("a.cs"), "class A { }\n").unwrap();

        let claimed = claim(stale.clone());

        assert_ne!(
            claimed, stale,
            "a directory an earlier run holds is not reused"
        );
        assert!(
            !claimed.join("a.cs").exists(),
            "a claimed scratch directory holds no earlier run's file"
        );
    }
}
