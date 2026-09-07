use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::repo::{git_common_dir, scout_dir};

// ---------------------------------------------------------------------------
// Path resolution (private helpers for the graph directory and cache paths).
// ---------------------------------------------------------------------------

// The def/ref graph lives in the git COMMON dir (shared by every linked
// worktree, alongside the manifest), one level down; a non-git root falls
// back to `<root>/.scout/graph/`.
pub(crate) fn graph_dir(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join("scout").join("graph"),
        None => scout_dir(root).join("graph"),
    }
}

/// Path to the graph artifact (`graph.json`) for `root`.
pub fn graph_json_path(root: &Path) -> PathBuf {
    graph_dir(root).join("graph.json")
}

/// Path to the project-model staleness sidecar for `root`.
///
/// Holds exactly the bytes `graph.json`'s `units` array would carry -- the
/// serialized `Vec<GraphUnit>` and nothing else. `map` compares this file
/// against a freshly discovered model to notice a `.csproj` edit, which no
/// mtime in the fragments index can see: `.csproj` is not a `SOURCE_EXT`, so
/// editing one moves no graph file and `index_is_stale` stays false. Written
/// by `rebuild_graph` only when a model exists and DELETED when one does not,
/// so a repo that never had a `.csproj` never grows the file and one whose
/// last `.csproj` was removed still sees a difference on the next run.
pub fn project_units_path(root: &Path) -> PathBuf {
    graph_dir(root).join("project-units.json")
}

// The version in both cache filenames is the fragment SCHEMA version, bumped
// whenever the extractor starts recording something old cached fragments
// lack -- v10 added def `type_params`/`base_generic_args` and the new
// `ctor-param` ref kind (with its `args` field), v11 added markup graph
// facts (a `.xaml` file's fragment now carries the `x:Class` def and its
// element/`x:Bind` refs, where every cached markup fragment before it carried
// names only), v12 added the TS/JS reference fragment, v13 added def
// `propertyTypes` and ref `receiverPropertyOwner` plus the ref
// `receiverCallOwner`/`receiverCallMember` pair, plus the foreach
// element-type fact -- no new field (it settles into the existing
// `receiver_type`), but a cached fragment from before it can still disagree
// with a fresh one for the same unchanged file, so it rides the same bump
// rather than skipping it. v16 added the ref `receiverBase` flag, set only
// for a `base.` qualifier -- a cached v15 fragment carries none, so every
// `base.` receiver would silently resolve (or fail to resolve) as if it
// were a plain `this.` receiver. v17 added no field: the extractor now
// blanks the inactive arms of `#if`/`#else` groups before parsing, so a
// cached v16 fragment of an unchanged file can carry twin defs and refs
// from both arms that a fresh one no longer records -- the same
// "disagrees for an unchanged file" case that moved v13, so it rides a
// bump too. v18 added def `methodParams` and ref `receiverLambda`, the
// untyped-lambda-parameter callee slot -- a cached v17 fragment carries
// neither, so every per-overload parameter shape and every untyped-lambda
// callee-slot lookup they back would silently see no candidates. The
// rename IS the invalidation mechanism:
// pre-bump caches stop being found, every file reparses
// once, no reader carries version-compat logic. Writers delete every
// superseded generation (see `remove_superseded_caches`).
pub(crate) fn fragments_cache_path(root: &Path) -> PathBuf {
    graph_dir(root).join("fragments-v18.json")
}

pub(crate) fn fragments_index_path(root: &Path) -> PathBuf {
    graph_dir(root).join("fragments-index-v18.json")
}

// Every generation below the current one, not just the immediately previous:
// a repo mapped last under v1 and never since would otherwise keep its v1
// pair forever.
pub(crate) const SUPERSEDED_CACHE_FILES: &[&str] = &[
    "fragments.json",
    "fragments-index.json",
    "fragments-v2.json",
    "fragments-index-v2.json",
    "fragments-v3.json",
    "fragments-index-v3.json",
    "fragments-v4.json",
    "fragments-index-v4.json",
    "fragments-v5.json",
    "fragments-index-v5.json",
    "fragments-v6.json",
    "fragments-index-v6.json",
    "fragments-v7.json",
    "fragments-index-v7.json",
    "fragments-v8.json",
    "fragments-index-v8.json",
    "fragments-v9.json",
    "fragments-index-v9.json",
    "fragments-v10.json",
    "fragments-index-v10.json",
    "fragments-v11.json",
    "fragments-index-v11.json",
    "fragments-v12.json",
    "fragments-index-v12.json",
    "fragments-v13.json",
    "fragments-index-v13.json",
    "fragments-v14.json",
    "fragments-index-v14.json",
    "fragments-v15.json",
    "fragments-index-v15.json",
    "fragments-v16.json",
    "fragments-index-v16.json",
    "fragments-v17.json",
    "fragments-index-v17.json",
];

pub(crate) fn remove_superseded_caches(root: &Path) {
    for name in SUPERSEDED_CACHE_FILES {
        // Best-effort cleanup only -- a leftover superseded file is inert.
        let _ = fs::remove_file(graph_dir(root).join(name));
    }
}

// ---------------------------------------------------------------------------
// Atomic writes -- tmp file in the SAME directory as the target (so
// the final `rename` is same-filesystem and therefore atomic), unique name
// so concurrent writers (this process's own sequential graph/fragments/index
// writes, or a genuinely concurrent process) never collide. Writing to a
// tmp file and renaming is a deliberate upgrade over an in-place write --
// output BYTES are unaffected, only the write's crash-safety.
// ---------------------------------------------------------------------------

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("artifact");
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(".{file_name}.tmp.{}.{counter}", std::process::id()));
    let write_result = fs::write(&tmp_path, bytes);
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
        write_result?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub(crate) fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    atomic_write_bytes(path, &bytes)
}
