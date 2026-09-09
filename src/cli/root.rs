// Repo-root and graph-artifact resolution shared by every verb: the cwd a
// command actually runs from, the `.scout`/`.git` ancestor climb, the `-C`
// global flag, and the mapped `graph.json` every graph-reading verb needs
// before it can build an answer.

use std::path::{Path, PathBuf};

use crate::graph;
use crate::query;

pub(crate) fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// The repo root for `cwd`: an initialized `.scout` ancestor wins; otherwise fall
// back to the nearest `.git` ancestor. `Err` carries the message callers wrap as
// `"error: {message}"`.
//
// `pub(crate)`: `audit.rs`'s `cmd_audit` resolves its repo root the same way
// every other command here does.
pub(crate) fn require_repo(cwd: &Path) -> Result<PathBuf, String> {
    require_repo_for_path(cwd, None)
}

// Root resolution with a fallback to the verb's own path argument. `arg_path` is
// consulted only after the caller's directory has come up empty, so a caller
// sitting in a repo never has its root decided by an argument pointing outside
// it.
pub(crate) fn require_repo_for_path(cwd: &Path, arg_path: Option<&str>) -> Result<PathBuf, String> {
    crate::repo::find_scout_root(cwd)
        .or_else(|| crate::repo::find_repo_root(cwd))
        .or_else(|| root_from_arg(cwd, arg_path))
        .ok_or_else(|| {
            "no .scout or .git ancestor; run 'devscout init' from the repo or directory root"
                .to_string()
        })
}

// Only a path-shaped argument naming something that is actually on disk is
// allowed to decide a root; a symbol, or a repo-relative path that means nothing
// from where the caller stands, resolves to a directory that has no bearing on
// the query and is refused.
fn root_from_arg(cwd: &Path, arg_path: Option<&str>) -> Option<PathBuf> {
    let arg_path = arg_path?;
    if !query::looks_like_file_path(arg_path) {
        return None;
    }
    let abs = crate::repo::resolve_from(cwd, Path::new(arg_path));
    if !abs.exists() {
        return None;
    }
    crate::repo::find_scout_root(&abs).or_else(|| crate::repo::find_repo_root(&abs))
}

// The manifest and the graph key files by their exact repo-relative path, so an
// absolute or subdirectory-relative argument is a guaranteed miss; rewriting one
// that names a real path inside the root can only turn that miss into an answer.
// Everything else -- a symbol, a path that is not on disk, a path outside the
// root -- is handed back untouched, so output matches the same query run from the
// root itself.
pub(crate) fn repo_relative_arg(cwd: &Path, root: &Path, arg: &str) -> String {
    if !query::looks_like_file_path(arg) {
        return arg.to_string();
    }
    let abs = crate::repo::resolve_from(cwd, Path::new(arg));
    if !abs.exists() {
        return arg.to_string();
    }
    match abs.strip_prefix(root) {
        Ok(rest) if !rest.as_os_str().is_empty() => crate::repo::rel_path(root, &abs),
        _ => arg.to_string(),
    }
}

// `-C <dir>`, git's own semantics: every root resolution and every relative path
// argument below reads as if the process had started in `<dir>`. Consumed before
// the subcommand, so it composes on repeat exactly as git's does. `args[0]` (the
// program name) is carried through untouched, keeping every caller's argument
// indices as they were.
pub(crate) fn apply_global_options(
    cwd: &Path,
    args: &[String],
) -> Result<(PathBuf, Vec<String>), String> {
    let mut cwd = cwd.to_path_buf();
    let mut idx = 1;
    while args.get(idx).map(String::as_str) == Some("-C") {
        let Some(dir) = args.get(idx + 1) else {
            return Err("no directory given for '-C' option".to_string());
        };
        let next = crate::repo::resolve_from(&cwd, Path::new(dir));
        if !next.is_dir() {
            return Err(format!("cannot change to '{dir}': no such directory"));
        }
        cwd = next;
        idx += 2;
    }
    if idx == 1 {
        return Ok((cwd, args.to_vec()));
    }
    let mut rest = vec![args[0].clone()];
    rest.extend_from_slice(&args[idx..]);
    Ok((cwd, rest))
}

// The mapped graph the four graph-reading verbs need, or the single refusal all
// four print when the repo was never mapped -- one `(code, message)` the caller
// returns unchanged, so those refusals cannot drift apart.
pub(crate) fn require_graph(root: &Path) -> Result<graph::Graph, (i32, String)> {
    graph::read_graph(root).ok_or_else(|| {
        (
            1,
            "no graph.json for this repo — run `devscout map` on a C# scope first".to_string(),
        )
    })
}
