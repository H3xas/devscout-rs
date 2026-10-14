//! The filesystem half of `.csproj` discovery: the ancestor walk, the
//! non-recursive directory scan, and the lossy reader that records what it
//! read.
//!
//! The reader records a content digest for every configuration file it parses,
//! because that digest is the only honest record of what the model was built
//! from: a `.csproj` or `Directory.Build.props` is not a source extension, so no
//! walked file's key moves when one is edited.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::freshness::record::sha256_hex;
use crate::repo;

/// Relative path -> SHA-256 of the bytes discovery parsed, for every
/// configuration input the `.csproj` model read.
pub(super) type ConfigDigests = BTreeMap<String, String>;

// The ancestor directories of `dir`, starting at its parent and continuing
// up to and including `root`. Empty when `dir == root` (no ancestors left to
// check within the repo) or when `dir` is not under `root` at all.
pub(super) fn ancestors_up_to(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = dir.to_path_buf();
    while cur != root {
        let Some(parent) = cur.parent() else {
            break;
        };
        out.push(parent.to_path_buf());
        if parent == root {
            break;
        }
        cur = parent.to_path_buf();
    }
    out
}

// Non-recursive `.csproj`/`Directory.Build.props` scan of a single
// directory's direct entries. A missing or unreadable directory (e.g. an
// ancestor computed lexically from a scope element that does not exist on
// disk) is silently skipped, matching `walk`'s "nonexistent scope element"
// behavior rather than failing the whole discovery.
pub(super) fn scan_ancestor_dir(
    root: &Path,
    dir: &Path,
    csproj_files: &mut std::collections::HashSet<PathBuf>,
    props_by_dir: &mut HashMap<PathBuf, String>,
    digests: &mut ConfigDigests,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".csproj"))
        {
            csproj_files.insert(path);
        } else if is_directory_build_props(&path) {
            if let Some(text) = read_lossy(root, &path, digests) {
                props_by_dir.insert(dir.to_path_buf(), text);
            }
        }
    }
}

pub(super) fn is_directory_build_props(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("Directory.Build.props")
}

// Lossy UTF-8 decode, never failing on bad encoding -- only on a real I/O
// failure. The digest is of the bytes read here, which are the same bytes the
// caller parses.
pub(super) fn read_lossy(root: &Path, path: &Path, digests: &mut ConfigDigests) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    digests.insert(repo::rel_path(root, path), sha256_hex(&bytes));
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
