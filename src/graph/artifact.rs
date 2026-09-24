use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::def::Def;
use super::edge::{Edge, EdgesByKind, HeuristicByTier};
use super::ordered::Percent1;
use super::paths::{atomic_write_json, graph_json_path};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `Stats`.
pub struct Stats {
    /// The def count value.
    pub def_count: usize,
    /// The file count value.
    pub file_count: usize,
    /// The edges by kind value.
    pub edges_by_kind: EdgesByKind,
    /// The ambiguous count value.
    pub ambiguous_count: usize,
    /// The ambiguous pct value.
    pub ambiguous_pct: Percent1,
    /// The unresolved external count value.
    pub unresolved_external_count: usize,
    /// Appended LAST, and always serialized (unlike the edge flag above): the
    /// key is written unconditionally, so a graph with no guesses still
    /// carries `"heuristic_edge_count":0`. `default` is for the READ side only
    /// -- a graph.json written before this counter existed has no such key,
    /// and it must read back as 0 rather than fail to parse.
    #[serde(default)]
    pub heuristic_edge_count: usize,
    /// Test-coverage stage -- appended LAST and always serialized, like the
    /// stage-4 counter above it. Counts merged DEF ROWS, not fragment entries,
    /// so a partial test class split across two files is one test def.
    #[serde(default)]
    pub test_def_count: usize,
    /// The TS resolver's own four counters, omitted entirely when the repo
    /// carries no TS fragment (the same omit-when-empty rule every other
    /// appended fact follows). Appended after `test_def_count`, and NOT last
    /// any more: `heuristic_by_tier` below is the newer fact and takes the
    /// tail, so a TS repo's stats block appends in the order the facts were
    /// added rather than interleaving the newest one before an older key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<crate::tsgraph::TsStats>,
    /// The tier split of `heuristic_edge_count`, appended LAST and always
    /// serialized, like the two counters above it. `default` is for the READ
    /// side only: a graph.json written before the tiers existed has no such
    /// key and must read back as two zeros rather than fail to parse.
    ///
    /// A C#-only repo writes no `ts` key at all, so for such a tree this key
    /// still follows `test_def_count` directly and the bytes are unchanged.
    #[serde(default)]
    pub heuristic_by_tier: HeuristicByTier,
    /// Whether the repository's own handler registrations contributed to the
    /// message vocabulary the bus pass ran with. Appended LAST and written
    /// ONLY alongside a bus-hop count, so a graph with no hop carries no such
    /// key and its bytes are unchanged.
    ///
    /// `Some(false)` is the answer worth reading: the pass fell back to the
    /// handler shapes this engine ships because the repository registers its
    /// handlers somewhere this pass cannot see them -- by scanning an
    /// assembly, most often. Hops are still emitted, but their coverage is
    /// whatever the fallback shapes happened to match, which is a gap to
    /// state rather than one to infer from a low count.
    #[serde(
        default,
        rename = "bus_vocabulary_derived",
        skip_serializing_if = "Option::is_none"
    )]
    pub bus_vocabulary_derived: Option<bool>,
}

/// One row of the full name index. Field order (`name`, `kind`,
/// `file`, `line`, `owner`) is significant; `owner` carries the
/// declaring type's def id for a member and is omitted for a type, an enum
/// member, and every markup or resource key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphName {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    /// The owner value.
    pub owner: String,
}

/// One `.csproj` project as graph.json persists it.
///
/// Field order (`id`, `name`, `refs`, `test`) is significant, and the last two are
/// omit-when-empty/omit-when-false: a leaf project that references nothing
/// and is not a test project serializes as just its `id` and `name`.
///
/// Deliberately NOT `project::Unit`: that type also carries `dir`, which is
/// always `id`'s parent directory and so is recomputed on read rather than
/// stored (`project::units_from_graph`). Nothing about which FILE belongs to
/// which unit is persisted either -- `ProjectModel` derives that from the
/// unit list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphUnit {
    /// Repo-relative path to the `.csproj` file, which is also this unit's
    /// identity -- what `refs` entries name.
    pub id: String,
    /// The project name (the csproj file name without its extension).
    pub name: String,
    /// The `id`s of this project's DIRECT `ProjectReference` targets, not
    /// transitively closed. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    /// Whether this is a test project. Omitted when false.
    #[serde(default, skip_serializing_if = "is_not_test")]
    pub test: bool,
}

fn is_not_test(b: &bool) -> bool {
    !*b
}

/// The version stamped into every graph.json this build writes, and the one
/// `rebuild_graph` demands before it reuses an artifact it did not just
/// produce.
///
/// Bumped to 2 when `uses-member` edges gained `tier` and `member`, and to 3
/// when the `implements`/`overrides` edge kinds joined the graph: a
/// schema-2 graph is READABLE (the two new `edges_by_kind` counters default to
/// 0 and no edge of either new kind can be present) but it is missing facts
/// the query layer now reports, so it gets rebuilt rather than trusted.
pub const GRAPH_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `Graph`.
pub struct Graph {
    /// The schema version value.
    pub schema_version: u32,
    /// The built at head value.
    pub built_at_head: Option<String>,
    /// The defs value.
    pub defs: Vec<Def>,
    /// The edges value.
    pub edges: Vec<Edge>,
    /// The stats value.
    pub stats: Stats,
    /// Appended LAST, after `stats`, and omitted when empty: the
    /// house rule for every added field, and what keeps a graph built over a
    /// set that declares no name byte-identical to what it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<GraphName>,
    /// The repo's `.csproj` projects, sorted by `id` -- appended LAST, after
    /// `names`, and omitted entirely when the repo declares none. A tree with
    /// no `.csproj` therefore serializes exactly as it did before the project
    /// model existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<GraphUnit>,
}

/// Reads and deserializes the repository graph, returning `None` on failure.
pub fn read_graph(root: &Path) -> Option<Graph> {
    let text = fs::read_to_string(graph_json_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn write_graph(root: &Path, graph: &Graph) -> io::Result<()> {
    atomic_write_json(&graph_json_path(root), graph)
}
