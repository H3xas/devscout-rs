use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fragment_types::AnyFragment;
use super::ordered::OrderedMap;
use super::paths::{atomic_write_json, fragments_cache_path, fragments_index_path};
use super::rebuild::GraphFile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `FragmentCacheEntry`.
pub struct FragmentCacheEntry {
    /// The mtime value.
    pub mtime: i64,
    /// The fragment value.
    pub fragment: AnyFragment,
}

// The cache generation's file shape: the entries plus the worktree that wrote
// them. The owner rides inside the payload rather than in the file name so a
// reader can tell "someone else's cache" from "no cache" in one parse, and so
// nothing needs to be renamed when a worktree takes the cache over.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnerStamped<V> {
    owner: Option<String>,
    entries: OrderedMap<V>,
}

fn owning_worktree(root: &Path) -> Option<String> {
    crate::freshness::record::owner_of(root)
}

pub(crate) fn read_fragments_cache(root: &Path) -> OrderedMap<FragmentCacheEntry> {
    let Ok(text) = fs::read_to_string(fragments_cache_path(root)) else {
        return OrderedMap::default();
    };
    let Ok(stamped) = serde_json::from_str::<OwnerStamped<FragmentCacheEntry>>(&text) else {
        return OrderedMap::default();
    };
    if stamped.owner != owning_worktree(root) {
        return OrderedMap::default();
    }
    stamped.entries
}

pub(crate) fn write_fragments_cache(
    root: &Path,
    cache: &OrderedMap<FragmentCacheEntry>,
) -> io::Result<()> {
    let stamped = OwnerStamped {
        owner: owning_worktree(root),
        entries: cache.clone(),
    };
    atomic_write_json(&fragments_cache_path(root), &stamped)
}

/// Read the fragments index (rel -> reuse key). Used for the per-file reuse
/// decision, independent of the graph rebuild.
pub fn read_fragments_index(root: &Path) -> OrderedMap<i64> {
    let Ok(text) = fs::read_to_string(fragments_index_path(root)) else {
        return OrderedMap::default();
    };
    let Ok(stamped) = serde_json::from_str::<OwnerStamped<i64>>(&text) else {
        return OrderedMap::default();
    };
    if stamped.owner != owning_worktree(root) {
        return OrderedMap::default();
    }
    stamped.entries
}

pub(crate) fn write_fragments_index(
    root: &Path,
    cache: &OrderedMap<FragmentCacheEntry>,
) -> io::Result<()> {
    let mut index = OrderedMap::new();
    for (rel, entry) in cache.iter() {
        index.insert(rel.clone(), entry.mtime);
    }
    let stamped = OwnerStamped {
        owner: owning_worktree(root),
        entries: index,
    };
    atomic_write_json(&fragments_index_path(root), &stamped)
}

/// Whether the fragments index is stale for the given graph files (any key
/// mismatch, or a differing file count).
pub fn index_is_stale(index: &OrderedMap<i64>, graph_files: &[GraphFile]) -> bool {
    graph_files
        .iter()
        .any(|f| index.get(&f.rel) != Some(&f.mtime))
        || index.len() != graph_files.len()
}
