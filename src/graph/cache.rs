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

pub(crate) fn read_fragments_cache(root: &Path) -> OrderedMap<FragmentCacheEntry> {
    match fs::read_to_string(fragments_cache_path(root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => OrderedMap::default(),
    }
}

pub(crate) fn write_fragments_cache(
    root: &Path,
    cache: &OrderedMap<FragmentCacheEntry>,
) -> io::Result<()> {
    atomic_write_json(&fragments_cache_path(root), cache)
}

/// Read the fragments index (rel -> mtime). Used for the per-file reuse
/// decision, independent of the graph rebuild.
pub fn read_fragments_index(root: &Path) -> OrderedMap<i64> {
    match fs::read_to_string(fragments_index_path(root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => OrderedMap::default(),
    }
}

pub(crate) fn write_fragments_index(
    root: &Path,
    cache: &OrderedMap<FragmentCacheEntry>,
) -> io::Result<()> {
    let mut index = OrderedMap::new();
    for (rel, entry) in cache.iter() {
        index.insert(rel.clone(), entry.mtime);
    }
    atomic_write_json(&fragments_index_path(root), &index)
}

/// Whether the fragments index is stale for the given graph files (any mtime
/// mismatch, or a differing file count).
pub fn index_is_stale(index: &OrderedMap<i64>, graph_files: &[GraphFile]) -> bool {
    graph_files
        .iter()
        .any(|f| index.get(&f.rel) != Some(&f.mtime))
        || index.len() != graph_files.len()
}
