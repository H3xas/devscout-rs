use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fragment_types::{AnyFragment, FragHandlerRegistration, FragPublish, FragRegistration};
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
//
// The payload stamp is required, so a pair an earlier build of the same
// generation wrote in another shape fails to parse. The index is what reuse
// planning trusts, and an index that still parsed would keep fragments the
// rebuild then cannot read: their files would drop out of the graph, and the
// old cache would stay on disk unreplaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnerStamped<V> {
    payload: u32,
    owner: Option<String>,
    entries: OrderedMap<V>,
}

/// The fragment payload's form within the current generation; it changes when
/// the cached shape changes without a generation move.
pub(super) const PAYLOAD_FORM: u32 = 1;

impl<V> OwnerStamped<V> {
    fn of(root: &Path, entries: OrderedMap<V>) -> Self {
        OwnerStamped {
            payload: PAYLOAD_FORM,
            owner: owning_worktree(root),
            entries,
        }
    }
}

fn parse_stamped<V: serde::de::DeserializeOwned>(text: &str) -> Option<OwnerStamped<V>> {
    serde_json::from_str::<OwnerStamped<V>>(text)
        .ok()
        .filter(|stamped| stamped.payload == PAYLOAD_FORM)
}

fn owning_worktree(root: &Path) -> Option<String> {
    crate::freshness::record::owner_of(root)
}

pub(crate) fn read_fragments_cache(root: &Path) -> OrderedMap<FragmentCacheEntry> {
    let Ok(text) = fs::read_to_string(fragments_cache_path(root)) else {
        return OrderedMap::default();
    };
    let Some(stamped) = parse_stamped::<FragmentCacheEntry>(&text) else {
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
    atomic_write_json(
        &fragments_cache_path(root),
        &OwnerStamped::of(root, cache.clone()),
    )
}

/// Read the fragments index (rel -> reuse key). Used for the per-file reuse
/// decision, independent of the graph rebuild.
pub fn read_fragments_index(root: &Path) -> OrderedMap<i64> {
    let Ok(text) = fs::read_to_string(fragments_index_path(root)) else {
        return OrderedMap::default();
    };
    let Some(stamped) = parse_stamped::<i64>(&text) else {
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
    atomic_write_json(&fragments_index_path(root), &OwnerStamped::of(root, index))
}

/// Whether the fragments index is stale for the given graph files (any key
/// mismatch, or a differing file count).
pub fn index_is_stale(index: &OrderedMap<i64>, graph_files: &[GraphFile]) -> bool {
    graph_files
        .iter()
        .any(|f| index.get(&f.rel) != Some(&f.mtime))
        || index.len() != graph_files.len()
}

// ---------------------------------------------------------------------------
// The strict reader.
//
// `read_fragments_cache` above fails open: a missing, unparseable or
// foreign-owned cache reads as "no cache", which `map` recovers from by
// re-extracting every file. A consumer that publishes what it read cannot fail
// open -- an empty cache would silently export a repository with no messaging
// facts at all -- so it needs the same parse to answer which of those three it
// found.
//
// The owner it checks against is the CALLER's, never the reading worktree's:
// the exporting worktree is usually not the one that ran `map`, and comparing
// against itself would read every other worktree's cache as empty.
// ---------------------------------------------------------------------------

/// What a strict read of the fragment cache found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrictCacheError {
    /// The current generation's cache file is not there.
    Missing,
    /// The file is present but does not parse as this generation's shape.
    Unreadable,
    /// The file parses but was written by another worktree.
    ForeignOwner,
    /// The file's identity changed between the two reads bracketing the parse.
    Changed,
}

/// The cached messaging facts of one file: what the export publishes about a
/// repository's message bus and its dependency registrations.
///
/// Deserialized in place of a whole fragment so the read is bounded by these
/// arrays rather than by the entire cache, which is tens of megabytes per
/// repository. A TypeScript or Rust entry carries none of the three keys and
/// parses as an empty view; the other keys of a C# entry are skipped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FragMessaging {
    /// The file's two-type-argument DI service registrations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registrations: Vec<FragRegistration>,
    /// The file's message-bus publish-site facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishes: Vec<FragPublish>,
    /// The file's one-type-argument handler registrations.
    #[serde(
        default,
        rename = "handlerRegistrations",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub handler_registrations: Vec<FragHandlerRegistration>,
}

#[derive(Debug, Clone, Deserialize)]
struct LightEntry {
    #[serde(default)]
    fragment: FragMessaging,
}

/// Reads every repository file's messaging facts, in the cache's own file
/// order, or names why it could not.
pub(crate) fn read_fragments_messaging(
    root: &Path,
    owner: &str,
) -> Result<Vec<(String, FragMessaging)>, StrictCacheError> {
    let path = fragments_cache_path(root);
    let before = crate::freshness::record::ArtifactIdentity::of(&path);
    let text = fs::read_to_string(&path).map_err(|_| StrictCacheError::Missing)?;
    let stamped = parse_stamped::<LightEntry>(&text).ok_or(StrictCacheError::Unreadable)?;
    if stamped.owner.as_deref() != Some(owner) {
        return Err(StrictCacheError::ForeignOwner);
    }
    let after = crate::freshness::record::ArtifactIdentity::of(&path);
    if before.is_none() || before != after {
        return Err(StrictCacheError::Changed);
    }
    Ok(stamped
        .entries
        .iter()
        .map(|(rel, entry)| (rel.clone(), entry.fragment.clone()))
        .collect())
}

/// Whether any fragment cache of another generation is on disk beside the
/// current one's path -- what tells a missing current-generation cache apart
/// from an index this build does not know how to read.
pub(crate) fn superseded_cache_present(root: &Path) -> bool {
    let current = fragments_cache_path(root);
    let Some(dir) = current.parent().map(Path::to_path_buf) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return false;
        };
        entry.path() != current && name.starts_with("fragments-v") && name.ends_with(".json")
    })
}
