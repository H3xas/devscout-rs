//! The crate's one Git ignore reader.
//!
//! Build output landing in a directory the fixed [`SKIP_DIRS`](super::SKIP_DIRS)
//! list does not name (`build/`, `lib/`, `out/`) duplicates a source
//! definition, and a bare-name query then cannot say which one it means. The
//! rule that keeps them out is Git's own: one
//! `git ls-files --others --ignored --exclude-standard --directory -z` per walk
//! over the scope's pathspecs answers with the complete set, so nested
//! `.gitignore` files, negations, `core.excludesFile` and the tracked-file
//! exception need no parser of ours and cannot drift from what Git does.
//!
//! A root with no `.git` entry keeps the fixed-name walk and spawns nothing.
//! In a Git root the answer is fail-closed: a spawn failure, a non-zero exit,
//! or a record this reader cannot parse (an interior empty record, non-UTF-8
//! bytes, an absolute path, one carrying an empty, `.` or `..` component, or a
//! final record left unterminated by truncated output) is one error and never a
//! partial set, so no caller mistakes "could not check" for "nothing is
//! ignored".
//!
//! `--no-optional-locks` keeps the query from taking `index.lock` out from
//! under a user running Git in the same tree, the same reason the status reader
//! sets it.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// The command this module's error message names, and the one it runs.
const QUERY: &str = "git ls-files --others --ignored --exclude-standard --directory";

/// The untracked paths Git reports as ignored for one walk's scope, split into
/// the directory records (`build/`) and the file records (`lib/slug.js`).
///
/// Empty for a root with no `.git` entry: the walk then applies its fixed
/// [`SKIP_DIRS`](super::SKIP_DIRS) list alone, exactly as it did before this
/// reader existed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ignored {
    dirs: HashSet<String>,
    files: HashSet<String>,
}

/// Why the ignore query produced no usable set. One catch-all: every cause
/// leaves through the same refusal so no caller can narrow an index on a
/// half-read answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreQueryError(String);

impl fmt::Display for IgnoreQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ignore query failed ({QUERY}): {}", self.0)
    }
}

impl std::error::Error for IgnoreQueryError {}

/// Every caller leaves through its existing `io::Result` path.
impl From<IgnoreQueryError> for io::Error {
    fn from(e: IgnoreQueryError) -> io::Error {
        io::Error::other(e.to_string())
    }
}

impl Ignored {
    /// Git's ignored set under `root` for `scope`, `"."` meaning the whole
    /// root -- same scope convention as [`list_source_files`](super::list_source_files).
    ///
    /// The query covers the scope's own pathspecs plus the ancestor
    /// `.csproj`/`Directory.Build.props` pathspecs model discovery reads, so
    /// every walk consumer inside one `map` can be handed the same set and no
    /// consumer can admit a path another one skipped.
    ///
    /// # Errors
    ///
    /// [`IgnoreQueryError`] when the root has a `.git` entry and Git cannot be
    /// run, exits non-zero, or answers with a record this reader cannot parse.
    pub fn for_scope(root: &Path, scope: &[String]) -> Result<Ignored, IgnoreQueryError> {
        if !root.join(".git").exists() {
            return Ok(Ignored::default());
        }
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(root).args([
            "--no-optional-locks",
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
            "--",
        ]);
        for spec in query_pathspecs(scope) {
            cmd.arg(spec);
        }
        let output = cmd
            .stdin(Stdio::null())
            .output()
            .map_err(|e| IgnoreQueryError(format!("could not run git: {e}")))?;
        if !output.status.success() {
            return Err(IgnoreQueryError(format!(
                "git exited with {}",
                output.status
            )));
        }
        Self::parse(&output.stdout)
    }

    /// Splits a NUL-separated `git ls-files` answer into the two sets.
    ///
    /// Git terminates every record, so an answer whose last byte is not that
    /// terminator is truncated output, and reading its tail as a record would
    /// be reading a path Git never reported. The empty piece a trailing NUL
    /// leaves behind is the only one dropped; an empty record anywhere else is
    /// one of the malformed answers.
    fn parse(bytes: &[u8]) -> Result<Ignored, IgnoreQueryError> {
        let records: Vec<&[u8]> = match bytes.split_last() {
            None => Vec::new(),
            Some((0, rest)) => rest.split(|b| *b == 0).collect(),
            Some(_) => return Err(unreadable()),
        };
        let mut ignored = Ignored::default();
        for record in records {
            let raw = std::str::from_utf8(record).map_err(|_| unreadable())?;
            let (rel, is_dir) = match raw.strip_suffix('/') {
                Some(dir) => (dir, true),
                None => (raw, false),
            };
            if !is_repo_relative(rel) {
                return Err(unreadable());
            }
            let target = if is_dir {
                &mut ignored.dirs
            } else {
                &mut ignored.files
            };
            target.insert(rel.to_string());
        }
        Ok(ignored)
    }

    /// Whether nothing is ignored, either because this is not a Git root or
    /// because the answer was empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.files.is_empty()
    }

    /// Whether `rel`, a `/`-joined repo-relative path, is in the set: either a
    /// file record for exactly that path, or a directory record for it or for
    /// any of its ancestors.
    ///
    /// Component boundaries hold, so a `build/` record never covers
    /// `buildx/`.
    #[must_use]
    pub fn covers(&self, rel: &str) -> bool {
        if self.files.contains(rel) || self.dirs.contains(rel) {
            return true;
        }
        let mut cur = rel;
        while let Some(i) = cur.rfind('/') {
            cur = &cur[..i];
            if self.dirs.contains(cur) {
                return true;
            }
        }
        false
    }
}

// The scope's own pathspecs, plus the ancestor pathspecs the `.csproj` and
// `Directory.Build.props` scan reads through.
fn query_pathspecs(scope: &[String]) -> Vec<String> {
    let mut specs = crate::freshness::status::scope_pathspecs(scope);
    specs.extend(crate::freshness::relevance::config_pathspecs(scope));
    specs
}

// A record Git could have written: a relative path whose every component is a
// real name. An absolute path, or an empty, `.` or `..` component, is not one.
fn is_repo_relative(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && rel
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn unreadable() -> IgnoreQueryError {
    IgnoreQueryError("unreadable output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn parse(records: &[&str]) -> Result<Ignored, IgnoreQueryError> {
        let mut bytes = Vec::new();
        for r in records {
            bytes.extend_from_slice(r.as_bytes());
            bytes.push(0);
        }
        Ignored::parse(&bytes)
    }

    fn scope(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_trailing_nul_leaves_no_empty_record_and_no_output_is_the_empty_set() {
        let set = parse(&["build/", "lib/slug.js"]).unwrap();
        assert_eq!(set.dirs.len(), 1);
        assert_eq!(set.files.len(), 1);
        assert!(Ignored::parse(b"").unwrap().is_empty());
    }

    #[test]
    fn an_empty_interior_record_is_unreadable() {
        let mut bytes = b"build/\0".to_vec();
        bytes.extend_from_slice(b"\0out/\0");
        assert_eq!(Ignored::parse(&bytes), Err(unreadable()));
    }

    #[test]
    fn an_answer_whose_last_record_is_unterminated_is_unreadable() {
        assert_eq!(Ignored::parse(b"build/"), Err(unreadable()));
        assert_eq!(Ignored::parse(b"build/\0out/"), Err(unreadable()));
        assert_eq!(Ignored::parse(b"\0"), Err(unreadable()));
    }

    #[test]
    fn non_utf8_and_escaping_records_are_unreadable() {
        assert_eq!(Ignored::parse(b"\xff\xfe\0"), Err(unreadable()));
        for bad in ["/etc/passwd", "..", "a/../b", "a//b", "./a"] {
            assert_eq!(
                Ignored::parse(format!("{bad}\0").as_bytes()),
                Err(unreadable()),
                "{bad} must not parse"
            );
        }
    }

    #[test]
    fn covers_respects_component_boundaries_and_directory_ancestors() {
        let set = parse(&["build/", "lib/slug.js", "gen/deep/"]).unwrap();
        assert!(set.covers("build"));
        assert!(set.covers("build/slug.mjs"));
        assert!(set.covers("lib/slug.js"));
        assert!(set.covers("gen/deep/anything/x.ts"));
        assert!(
            !set.covers("buildx/a.ts"),
            "a name prefix is not a component"
        );
        assert!(!set.covers("lib/keep.ts"), "a file record covers one path");
        assert!(!set.covers("gen/other/x.ts"));
    }

    #[test]
    fn a_root_without_a_git_entry_is_the_empty_set_and_runs_no_git() {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir: PathBuf =
            std::env::temp_dir().join(format!("scout-walk-ignored-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let set = Ignored::for_scope(&dir, &scope(&["."])).unwrap();
        assert!(set.is_empty());
        assert!(!set.covers("build/slug.mjs"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_nonempty_set_removes_exactly_its_paths_and_keeps_the_walk_order() {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "scout-walk-ignored-order-{}-{n}",
            std::process::id()
        ));
        for (rel, body) in [
            ("src/a.ts", "a"),
            ("src/z.ts", "z"),
            ("src/build/keep.ts", "k"),
            ("build/slug.mjs", "b"),
            ("build/sub/deep.ts", "d"),
        ] {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }

        let set = parse(&["build/", "lib/slug.js"]).unwrap();
        let files = crate::walk::list_source_files(&root, &[".".to_string()], &set).unwrap();
        assert_eq!(
            files,
            vec![
                "src/a.ts".to_string(),
                "src/build/keep.ts".to_string(),
                "src/z.ts".to_string(),
            ],
            "an ignored directory record drops its whole subtree, a name that \
             merely shares its prefix stays"
        );

        let nested =
            crate::walk::list_source_files(&root, &["build/sub".to_string()], &set).unwrap();
        assert!(
            nested.is_empty(),
            "a scope below an ignored ancestor is skipped whole: {nested:?}"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_query_covers_the_scope_and_its_ancestor_configuration_paths() {
        assert_eq!(query_pathspecs(&scope(&["."])), vec![".".to_string()]);
        let specs = query_pathspecs(&scope(&["a/b"]));
        assert!(specs.contains(&":(literal)a/b".to_string()));
        assert!(specs.contains(&":(literal)a/Directory.Build.props".to_string()));
        assert!(specs.contains(&":(glob)a/*.csproj".to_string()));
    }
}
