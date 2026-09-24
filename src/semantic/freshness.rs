// Whole-artifact freshness: whether the checkout has moved since the
// admitted compiler-facts artifact was captured. The normal analysis path
// never spawns a compiler, so this cannot recompute a live compilation
// fingerprint -- it can only compare what the artifact itself recorded
// (`sourceSnapshot.headSha`/`.dirty`) against what the checkout can tell
// about itself without one: the current git head, and whether the tree is
// dirty right now. A mismatch on either leg invalidates every fact the
// artifact carries, even though the file consuming a given reference may be
// byte-identical to what it was when the artifact was captured -- the
// compilation context changed, which is exactly what freshness here means to
// catch. This is coarser than a per-dependency check (any tracked file
// changing anywhere invalidates the whole artifact, not only the affected
// compilation), which is the safe direction: it can only under-apply the
// layer, never emit a stale confirmed edge.
//
// The head/dirty pair alone is blind to context that changes without moving
// this repository's own git state at all: an MSBuild import
// (`Directory.Build.props`, `Directory.Packages.props`, a `.targets` file)
// living outside the mapped scope, or a package/reference asset resolved
// from outside the working tree (the global NuGet cache, an updated SDK's
// own defaults). Per-compilation `context.envelope.compilations[].imports[]`
// is exactly the producer's own record of which external files it consulted
// and their content hash at capture time -- `check_imports` re-hashes each
// one directly off disk, offline, and compares, so a changed import
// invalidates the artifact even when the consuming file and repository HEAD
// are both untouched. A changed PACKAGE reference and a changed COMPILER
// OPTION are typically the SAME trigger from this checkout's point of view
// (both usually live in a `.props`/`.targets` import), so one mechanism
// serves both named triggers, with the reason naming which import moved.

use std::path::Path;

use sha1::{Digest, Sha1};

use crate::manifest;

/// Whether the admitted artifact is still trustworthy at `map` time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// The checkout is exactly as it was when the artifact was captured.
    Fresh,
    /// Something has moved; every fact in the artifact falls through to the
    /// syntax ladder.
    Stale {
        /// The machine-readable trigger, folded into `Uncertainty::Stale`.
        /// Owned rather than `&'static str` because the import triggers
        /// (`check_imports`) name the specific import that moved, which is
        /// only known at evaluation time.
        reason: String,
    },
}

impl Freshness {
    /// Whether this is [`Freshness::Fresh`].
    pub fn is_fresh(&self) -> bool {
        matches!(self, Freshness::Fresh)
    }
}

/// One `context.envelope.compilations[].imports[]` entry: an external file
/// the producer consulted, and its content hash (lower-case hex SHA-1, the
/// producer's own encoding) at capture time. Collected across every
/// compilation the artifact embeds -- freshness here is whole-artifact, the
/// same coarseness the head/dirty checks above already apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef {
    /// The producer's own identity string, e.g.
    /// `"external-file:Directory.Build.props"`.
    pub identity: String,
    /// The producer's own SHA-1 hex digest of that file's content at
    /// capture time.
    pub hash: String,
}

/// `identity`'s own external-file marker -- see `ImportRef`'s own doc
/// comment. Anything else is a producer identity shape this checkout has no
/// path to re-hash (not a local file at all) and is skipped, never treated
/// as a mismatch.
const EXTERNAL_FILE_PREFIX: &str = "external-file:";

/// Re-hashes every `ImportRef` directly off `root`'s current disk state and
/// compares it against the producer's own recorded hash. `None` when every
/// import still matches (or there is nothing to check); `Some(reason)` on
/// the first mismatch -- a changed import invalidates the whole artifact,
/// the same coarse, safe-direction rule the head/dirty checks apply. The
/// reason names the one import that moved (its own `identity` string, e.g.
/// `"import-file-changed:external-file:Directory.Build.props"`), so a reader
/// of `Uncertainty::Stale`'s reason can tell WHICH import triggered it
/// without re-deriving the whole artifact's import list.
fn check_imports(root: &Path, imports: &[ImportRef]) -> Option<String> {
    for import in imports {
        let Some(rel) = import.identity.strip_prefix(EXTERNAL_FILE_PREFIX) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(root.join(rel)) else {
            return Some(format!("import-file-missing:{}", import.identity));
        };
        let digest = Sha1::digest(&bytes);
        let current_hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if current_hash != import.hash {
            return Some(format!("import-file-changed:{}", import.identity));
        }
    }
    None
}

/// Evaluates freshness for one admitted artifact against `root`'s current
/// checkout state. `admitted_head_sha` and `admitted_dirty` come straight off
/// the artifact's own `sourceSnapshot` object (see `layer::parse_raw`);
/// `admitted_dirty` defaults to `true` when the artifact does not say,
/// because an unstated dirty flag cannot be trusted to mean clean. `imports`
/// is every compilation's own `ImportRef` list, flattened -- see this
/// module's own header comment.
pub fn evaluate(
    root: &Path,
    admitted_head_sha: Option<&str>,
    admitted_dirty: bool,
    imports: &[ImportRef],
) -> Freshness {
    if admitted_dirty {
        return Freshness::Stale {
            reason: "source-snapshot-dirty-at-admission".to_string(),
        };
    }
    let Some(admitted_head) = admitted_head_sha else {
        return Freshness::Stale {
            reason: "source-snapshot-missing".to_string(),
        };
    };
    if manifest::is_working_tree_dirty(root, &[".".to_string()]) {
        return Freshness::Stale {
            reason: "working-tree-dirty".to_string(),
        };
    }
    match manifest::git_head(root) {
        Some(current) if current == admitted_head => {}
        Some(_) => {
            return Freshness::Stale {
                reason: "source-head-mismatch".to_string(),
            }
        }
        None => {
            return Freshness::Stale {
                reason: "source-head-unavailable".to_string(),
            }
        }
    }
    if let Some(reason) = check_imports(root, imports) {
        return Freshness::Stale { reason };
    }
    Freshness::Fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch_repo(label: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-semantic-freshness-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .expect("git available for the test");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "test"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        dir
    }

    fn head_of(dir: &Path) -> String {
        manifest::git_head(dir).expect("head available")
    }

    #[test]
    fn dirty_at_admission_is_stale_regardless_of_the_current_tree() {
        let dir = scratch_repo("dirty-admission");
        let head = head_of(&dir);
        assert_eq!(
            evaluate(&dir, Some(&head), true, &[]),
            Freshness::Stale {
                reason: "source-snapshot-dirty-at-admission".to_string()
            }
        );
    }

    #[test]
    fn a_missing_head_sha_is_stale() {
        let dir = scratch_repo("missing-head");
        assert_eq!(
            evaluate(&dir, None, false, &[]),
            Freshness::Stale {
                reason: "source-snapshot-missing".to_string()
            }
        );
    }

    #[test]
    fn a_matching_clean_head_is_fresh() {
        let dir = scratch_repo("matching-head");
        let head = head_of(&dir);
        assert_eq!(evaluate(&dir, Some(&head), false, &[]), Freshness::Fresh);
    }

    #[test]
    fn a_moved_head_is_stale_even_with_the_consuming_file_untouched() {
        let dir = scratch_repo("moved-head");
        let admitted_head = head_of(&dir);
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "second"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert_eq!(
            evaluate(&dir, Some(&admitted_head), false, &[]),
            Freshness::Stale {
                reason: "source-head-mismatch".to_string()
            }
        );
    }

    #[test]
    fn an_uncommitted_edit_now_is_stale_even_at_the_same_admitted_head() {
        let dir = scratch_repo("dirty-now");
        let head = head_of(&dir);
        std::fs::write(dir.join("a.txt"), "changed").unwrap();
        assert_eq!(
            evaluate(&dir, Some(&head), false, &[]),
            Freshness::Stale {
                reason: "working-tree-dirty".to_string()
            }
        );
    }

    // Import triggers (the package/reference-asset and compiler-option
    // controls): a package/reference asset or compiler option most often
    // lives in an MSBuild import (`Directory.Build.props`,
    // `Directory.Packages.props`, a `.targets` file). When that import sits
    // OUTSIDE the mapped repository (a common upward-search layout, and the
    // case this checkout's own git dirty check is structurally blind to --
    // `git status` run from `dir` never sees a path outside it at all), the
    // consuming file and repository HEAD both stay untouched while the
    // import itself changes. Each test below writes the import one level
    // ABOVE the scratch repo, deliberately outside its git tree, so the
    // existing head/dirty checks cannot be the ones catching it -- only
    // `check_imports` can.

    fn sha1_hex(bytes: &[u8]) -> String {
        Sha1::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn an_import_matching_its_recorded_hash_is_fresh() {
        let dir = scratch_repo("import-fresh");
        let head = head_of(&dir);
        let import_path = dir.parent().unwrap().join(format!(
            "Directory.Build.props-{}",
            dir.file_name().unwrap().to_str().unwrap()
        ));
        std::fs::write(&import_path, b"<Project />").unwrap();
        let imports = [ImportRef {
            identity: format!(
                "external-file:../{}",
                import_path.file_name().unwrap().to_str().unwrap()
            ),
            hash: sha1_hex(b"<Project />"),
        }];
        assert_eq!(
            evaluate(&dir, Some(&head), false, &imports),
            Freshness::Fresh
        );
        std::fs::remove_file(&import_path).ok();
    }

    #[test]
    fn a_changed_import_is_stale_even_with_head_unchanged_and_the_tree_clean() {
        let dir = scratch_repo("import-changed");
        let head = head_of(&dir);
        let import_path = dir.parent().unwrap().join(format!(
            "Directory.Build.props-{}",
            dir.file_name().unwrap().to_str().unwrap()
        ));
        std::fs::write(
            &import_path,
            b"<Project><Nullable>enable</Nullable></Project>",
        )
        .unwrap();
        // Recorded hash is of the OLD content -- this checkout's own git
        // state (this scratch repo's head and working tree) never moved.
        let identity = format!(
            "external-file:../{}",
            import_path.file_name().unwrap().to_str().unwrap()
        );
        let imports = [ImportRef {
            identity: identity.clone(),
            hash: sha1_hex(b"<Project />"),
        }];
        assert_eq!(
            evaluate(&dir, Some(&head), false, &imports),
            Freshness::Stale {
                reason: format!("import-file-changed:{identity}"),
            },
            "the reason must name WHICH import moved, not just that one did"
        );
        std::fs::remove_file(&import_path).ok();
    }

    #[test]
    fn a_missing_import_is_stale() {
        let dir = scratch_repo("import-missing");
        let head = head_of(&dir);
        let identity = "external-file:../does-not-exist.props".to_string();
        let imports = [ImportRef {
            identity: identity.clone(),
            hash: sha1_hex(b"anything"),
        }];
        assert_eq!(
            evaluate(&dir, Some(&head), false, &imports),
            Freshness::Stale {
                reason: format!("import-file-missing:{identity}"),
            },
            "the reason must name WHICH import is missing, not just that one is"
        );
    }

    #[test]
    fn an_import_identity_with_no_external_file_marker_is_never_checked() {
        let dir = scratch_repo("import-unmarked");
        let head = head_of(&dir);
        let imports = [ImportRef {
            identity: "metadata:System.Private.CoreLib|mvid:00000000".to_string(),
            hash: "not-a-real-hash".to_string(),
        }];
        assert_eq!(
            evaluate(&dir, Some(&head), false, &imports),
            Freshness::Fresh
        );
    }
}
