// Artifact build/load, fragments cache, incremental reuse.
// Writes are atomic (tmp+rename); fragments cache keying is load-bearing --
// a mismatch makes reuse break silently.
//
// This module owns every serde struct for graph.json + the fragments-cache
// pair (fragments-v13.json, fragments-index-v13.json), plus their path resolution,
// atomic I/O, and the cache-then-resolve-then-write orchestration
// (`rebuild_graph`). The pure resolution ladder that
// turns fragments into `defs`/`edges` lives in `resolve.rs` and returns the
// `Graph` value this module serializes -- see that module for the ladder
// itself.
//
// Schema notes (the on-disk format graph.json must keep):
//   - graph.json is compact JSON with NO indentation:
//     `{"key":value,...}`, no spaces. `serde_json::to_vec` (non-pretty)
//     matches this by default.
//   - Key order is insertion order, not sorted. Defs keep first-insertion
//     order (partial-class duplicates land in
//     `also_in`, not a second top-level entry) -- backed here by a
//     Vec<Def> + HashMap<id, index> pair (`build_def_index` in resolve.rs)
//     rather than a HashMap alone, which has no defined iteration order.
//   - `also_in` is omitted entirely (not `[]`) when a def has no additional
//     declaring sites -- `#[serde(skip_serializing_if = "Vec::is_empty")]`.
//   - `edges` are shape-heterogeneous by `kind`: inherits/uses-type/
//     uses-member share `{kind, from_file, from_line, to, to_file}`;
//     imports is `{kind, from_file, from_line, target}`; ambiguous is
//     `{kind, origin, from_file, from_line, raw, candidates,
//     candidate_count}`. Modeled as an internally-tagged enum
//     (`#[serde(tag = "kind")]`) -- serde always emits the tag field first,
//     matching every one of these shapes' field order.
//   - `stats.edges_by_kind` has a FIXED key order (inherits, uses-type,
//     imports, uses-member, ctor-di) -- not
//     alphabetical, not insertion order of first edge seen. A plain struct
//     with that declared field order reproduces it.
//   - `stats.ambiguous_pct` is `round(x*1000)/10`: a number that prints
//     WITHOUT a decimal point when whole (`20`, not `20.0`) and with
//     exactly one decimal digit otherwise (`33.3`). Rust's f64 Serialize
//     (ryu-backed) does not reproduce this -- see `Percent1` below, which
//     stores tenths as an integer and branches serialize_i64/serialize_f64
//     so whole values come out as bare integers, without needing serde_json's
//     `arbitrary_precision`/`raw_value` features (neither is enabled).
//   - fragments.json / fragments-index.json are objects keyed by
//     repo-relative path, in file-walk order (fragments-index.json is
//     rebuilt by iterating the same cache object fragments.json used).
//     A `HashMap` has no serialization order guarantee and `serde_json`'s
//     default `Map` (no `preserve_order` feature, which is off here) sorts
//     by key -- neither matches. `OrderedMap<V>` below hand-rolls
//     Serialize/Deserialize (using serde's own `SerializeMap` / `MapAccess`
//     traits, not a bespoke JSON writer) to preserve insertion order.
//   - `built_at_head` records `git -C <root> rev-parse HEAD` (`null` on any
//     failure incl. no commits yet). `manifest.rs` already owns that
//     (`manifest::git_head`) -- `resolve_graph` (resolve.rs) calls THAT, so
//     this module does not duplicate it.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::extract;
use crate::repo::{git_common_dir, scout_dir};

// ---------------------------------------------------------------------------
// Path resolution (private helpers for the graph directory and cache paths).
// ---------------------------------------------------------------------------

// The def/ref graph lives in the git COMMON dir (shared by every linked
// worktree, alongside the manifest), one level down; a non-git root falls
// back to `<root>/.scout/graph/`.
fn graph_dir(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join("scout").join("graph"),
        None => scout_dir(root).join("graph"),
    }
}

/// Path to the graph artifact (`graph.json`) for `root`.
pub fn graph_json_path(root: &Path) -> PathBuf {
    graph_dir(root).join("graph.json")
}

// The `-v13` in both cache filenames is the fragment SCHEMA version, bumped
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
// rather than skipping it. The rename IS the invalidation mechanism:
// pre-bump caches stop being found, every file reparses
// once, no reader carries version-compat logic. Writers delete every
// superseded generation (see `remove_superseded_caches`).
fn fragments_cache_path(root: &Path) -> PathBuf {
    graph_dir(root).join("fragments-v13.json")
}

fn fragments_index_path(root: &Path) -> PathBuf {
    graph_dir(root).join("fragments-index-v13.json")
}

// Every generation below the current one, not just the immediately previous:
// a repo mapped last under v1 and never since would otherwise keep its v1
// pair forever.
const SUPERSEDED_CACHE_FILES: &[&str] = &[
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
];

fn remove_superseded_caches(root: &Path) {
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

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("artifact");
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

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    atomic_write_bytes(path, &bytes)
}

// ---------------------------------------------------------------------------
// Insertion-ordered string-keyed map -- backs fragments.json's `{rel:
// {mtime, fragment}}` and fragments-index.json's `{rel: mtime}`. A HashMap
// has no defined iteration order and serde_json's default Map serializer
// (the `preserve_order` feature is not enabled) sorts by key; neither
// matches the required file-walk-order construction. Built on serde's own
// SerializeMap/MapAccess traits,
// not a hand-rolled JSON writer -- serde_json still owns the actual byte
// encoding.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct OrderedMap<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> OrderedMap<V> {
    pub fn new() -> Self {
        Self { entries: Vec::new(), index: HashMap::new() }
    }

    /// Insert, or overwrite in place when the key already exists --
    /// preserves that key's ORIGINAL position, like assigning to an existing
    /// key on a JSON object.
    pub fn insert(&mut self, key: String, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Serialize> Serialize for OrderedMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedMapVisitor<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for OrderedMapVisitor<V> {
            type Value = OrderedMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = OrderedMap::new();
                while let Some((k, v)) = map.next_entry::<String, V>()? {
                    out.insert(k, v);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_map(OrderedMapVisitor(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// `stats.ambiguous_pct` -- see module header. Stored as tenths (an i64) so
// the "is this whole?" branch is exact integer arithmetic, never a float
// epsilon comparison.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percent1(pub i64);

impl Percent1 {
    pub fn zero() -> Self {
        Percent1(0)
    }

    /// `ambiguous_pct` as `round((ambiguous / attempts) * 1000) / 10`, or 0
    /// when there are no attempts. Takes the pre-division inputs so the
    /// rounding happens exactly once.
    pub fn from_ratio(numerator: usize, denominator: usize) -> Self {
        if denominator == 0 {
            return Percent1::zero();
        }
        let raw = (numerator as f64 / denominator as f64) * 1000.0;
        Percent1(raw.round() as i64)
    }
}

impl Serialize for Percent1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0 % 10 == 0 {
            serializer.serialize_i64(self.0 / 10)
        } else {
            serializer.serialize_f64(self.0 as f64 / 10.0)
        }
    }
}

impl<'de> Deserialize<'de> for Percent1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(deserializer)?;
        Ok(Percent1((v * 10.0).round() as i64))
    }
}

// ---------------------------------------------------------------------------
// graph.json schema.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlsoIn {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Def {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub methods: Vec<String>,
    /// Test-coverage stage -- the one member fact that SURVIVES onto disk
    /// (`properties`/`fields`/`extensionMethods`/`bases` are resolution inputs
    /// only, stripped before the row is written): `devscout tests` reads it back
    /// out of graph.json. Positioned after `methods` and before `also_in`, and
    /// omitted when empty, so a def declaring no tests keeps its exact
    /// pre-stage bytes.
    #[serde(default, rename = "testMethods", skip_serializing_if = "Vec::is_empty")]
    pub test_methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_in: Vec<AlsoIn>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub file: String,
}

/// The guess tag, appended LAST on every edge kind that can
/// carry it. `heuristic: true` is set only on an edge a heuristic tier
/// emitted and never writes `heuristic: false`, so this side pairs
/// `skip_serializing_if` with `default`: absent == precise, and a precise
/// edge's bytes are exactly what they were before the tag existed. Only
/// the resolver's heuristic tiers set it today (both
/// `uses-member`), but the flag lives on all three targeted kinds because
/// the query layer's heuristic adjacency is kind-keyed, so a future tier
/// tagging a `uses-type` edge needs no schema change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Edge {
    #[serde(rename = "inherits")]
    Inherits {
        from_file: String,
        from_line: usize,
        to: String,
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        heuristic: bool,
    },
    #[serde(rename = "uses-type")]
    UsesType {
        from_file: String,
        from_line: usize,
        to: String,
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        heuristic: bool,
    },
    #[serde(rename = "uses-member")]
    UsesMember {
        from_file: String,
        from_line: usize,
        to: String,
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        heuristic: bool,
    },
    #[serde(rename = "imports")]
    Imports { from_file: String, from_line: usize, target: String },
    #[serde(rename = "ambiguous")]
    Ambiguous {
        origin: String,
        from_file: String,
        from_line: usize,
        raw: String,
        candidates: Vec<Candidate>,
        candidate_count: usize,
    },
    /// Constructor-parameter DI resolution (see resolve.rs's
    /// `resolve_ctor_param`). Field order (`kind`, `from_file`, `from_line`,
    /// `iface`, `resolution`, `args`, `to`, `candidates`) is significant --
    /// it fixes the serialized field order.
    /// `iface` is the injected type's bare identifier; `args` its closed
    /// generic arguments, present only when it has any; `to` the resolved
    /// implementation's def id, present only for 'plain'/'closed'/
    /// 'open-generic'; `candidates` the capped, sorted tie list, present only
    /// for 'ambiguous'.
    #[serde(rename = "ctor-di")]
    CtorDi {
        from_file: String,
        from_line: usize,
        iface: String,
        resolution: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        candidates: Vec<Candidate>,
    },
    /// A TS/TSX module import. Distinct from `imports` (C#'s
    /// `using` directive, which names a NAMESPACE and resolves to no file):
    /// `target` is the specifier as written and `to_file` the file it
    /// resolved to. `via` is appended LAST and present only on a
    /// barrel-followed edge, naming the module the source literally imported
    /// so a reader can tell the routing table apart from the dependency.
    #[serde(rename = "import")]
    Import {
        from_file: String,
        from_line: usize,
        target: String,
        to_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<String>,
    },
    /// A call or `new` naming an imported or locally-exported
    /// declaration.
    #[serde(rename = "call")]
    Call { from_file: String, from_line: usize, to: String, to_file: String },
    /// A JSX tag naming a component declaration.
    #[serde(rename = "jsx-use")]
    JsxUse { from_file: String, from_line: usize, to: String, to_file: String },
    /// An action creator or thunk handed to a dispatching call
    /// (`dispatch(...)`, `ofType(...)`).
    #[serde(rename = "dispatch")]
    Dispatch { from_file: String, from_line: usize, to: String, to_file: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EdgesByKind {
    pub inherits: usize,
    #[serde(rename = "uses-type")]
    pub uses_type: usize,
    pub imports: usize,
    #[serde(rename = "uses-member")]
    pub uses_member: usize,
    /// Appended LAST, matching the `ctor-di` slot in the fixed key order.
    #[serde(rename = "ctor-di")]
    pub ctor_di: usize,
    /// The four TS edge counts, appended after `ctor-di` in this exact order
    /// and ONLY when the repo carries a TS fragment at all. A C#-only repo's
    /// stats block omits them entirely -- which is why these are `Option` and
    /// not a plain `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<usize>,
    #[serde(rename = "jsx-use", default, skip_serializing_if = "Option::is_none")]
    pub jsx_use: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub def_count: usize,
    pub file_count: usize,
    pub edges_by_kind: EdgesByKind,
    pub ambiguous_count: usize,
    pub ambiguous_pct: Percent1,
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
    /// The TS resolver's own four counters, appended LAST inside
    /// `stats` and omitted entirely when the repo carries no TS fragment (the
    /// same omit-when-empty rule every other appended fact follows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<crate::tsgraph::TsStats>,
}

/// One row of the full name index. Field order (`name`, `kind`,
/// `file`, `line`, `owner`) is significant; `owner` carries the
/// declaring type's def id for a member and is omitted for a type, an enum
/// member, and every markup or resource key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphName {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub schema_version: u32,
    pub built_at_head: Option<String>,
    pub defs: Vec<Def>,
    pub edges: Vec<Edge>,
    pub stats: Stats,
    /// Appended LAST, after `stats`, and omitted when empty: the
    /// house rule for every added field, and what keeps a graph built over a
    /// set that declares no name byte-identical to what it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<GraphName>,
}

pub fn read_graph(root: &Path) -> Option<Graph> {
    let text = fs::read_to_string(graph_json_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_graph(root: &Path, graph: &Graph) -> io::Result<()> {
    atomic_write_json(&graph_json_path(root), graph)
}

// ---------------------------------------------------------------------------
// Fragments cache schema -- the RAW (unresolved) per-file extraction output,
// keyed by repo-relative path. Mirrors extract.rs's DefRecord/UsingRecord/
// RefRecord field-for-field; the serde impls live here rather than on the
// extractor's own types.
// ---------------------------------------------------------------------------

/// Field order is significant: id, name, namespace, kind, line, methods, then
/// the member-fact additions in that exact order -- properties, fields,
/// methodReturns -- and after those extensionMethods, appended LAST. Each is
/// omitted entirely when empty, so a type declaring none of them serializes
/// exactly as it did before those additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragDef {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub line: usize,
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    /// Member name -> declared return type NAME, in FIRST-DECLARATION SOURCE
    /// ORDER -- deliberately `OrderedMap`, never a `BTreeMap`: this is an
    /// ordered pair list, and sorting the keys here would break the
    /// serialized bytes.
    #[serde(default, rename = "methodReturns", skip_serializing_if = "OrderedMap::is_empty")]
    pub method_returns: OrderedMap<String>,
    /// The extension methods this type declares, in source
    /// order, deduped by (name, thisType, arityMin, arityMax). Appended after
    /// `method_returns`, and omitted entirely when empty, so a type declaring
    /// none keeps its exact prior bytes.
    #[serde(default, rename = "extensionMethods", skip_serializing_if = "Vec::is_empty")]
    pub extension_methods: Vec<FragExtensionMethod>,
    /// DIRECT base-type identifiers, source
    /// order, deduped. Appended LAST, after `extension_methods`, omitted when
    /// empty. A resolution input only: `resolve_graph` strips it (along with
    /// properties/fields/extensionMethods) before graph.json's def rows, so
    /// the on-disk def bytes are unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    /// The declaring type's own type-parameter names, empty for
    /// every non-generic declaration. Appended after `bases`, omitted when
    /// empty. A resolution input, like `bases`: the ctor-DI resolver's
    /// "is this def itself an open-generic implementation" signal.
    #[serde(default, rename = "typeParams", skip_serializing_if = "Vec::is_empty")]
    pub type_params: Vec<String>,
    /// Per base name that carried a type-argument list, that
    /// list's generic-arg descriptors relative to `type_params` (`OrderedMap`
    /// for the same reason `method_returns` is one: the serialized key order
    /// is significant). Appended after `type_params`, omitted when
    /// empty.
    #[serde(default, rename = "baseGenericArgs", skip_serializing_if = "OrderedMap::is_empty")]
    pub base_generic_args: OrderedMap<Vec<String>>,
    /// The methods a test framework would DISCOVER as
    /// tests, source order, deduped. Appended LAST, after `baseGenericArgs`,
    /// omitted when empty. Unlike its neighbours this one is NOT stripped by
    /// `resolve_graph`: it is what `devscout tests` answers from.
    #[serde(default, rename = "testMethods", skip_serializing_if = "Vec::is_empty")]
    pub test_methods: Vec<String>,
    /// Property name -> declared type fact, in source order and
    /// under the same dedup as `properties`. An `OrderedMap` for the same
    /// reason `method_returns` is one: the serialized key order is significant.
    /// Appended LAST, after `test_methods`, omitted when empty. A
    /// resolution input only, like `properties`/`fields`.
    #[serde(default, rename = "propertyTypes", skip_serializing_if = "OrderedMap::is_empty")]
    pub property_types: OrderedMap<FragFact>,
}

/// One declared type fact: the type NAME, plus its top-level
/// type-argument descriptors when the declaration carried any. Field order
/// (`type`, `args`) is significant, and `args` is omitted when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragFact {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

/// One `extensionMethods` entry. Serialized field order (`name`, `thisType`,
/// `arityMin`, `arityMax`, `thisArgs`) is significant, and serde emits struct
/// fields in declaration order. The two arity halves are NOT optional: they joined the
/// schema with the v6 cache rename, so no fragment this reader can meet is
/// missing them. `arity_max` is signed because -1 is the unbounded-`params`
/// sentinel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragExtensionMethod {
    pub name: String,
    #[serde(rename = "thisType")]
    pub this_type: String,
    #[serde(rename = "arityMin")]
    pub arity_min: usize,
    #[serde(rename = "arityMax")]
    pub arity_max: i64,
    /// Present only when the this-parameter type is generic (the key is omitted
    /// key otherwise), so a non-generic entry keeps four fields exactly.
    #[serde(default, rename = "thisArgs", skip_serializing_if = "Option::is_none")]
    pub this_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FragUsing {
    Alias { alias: String, target: String, global: bool },
    Plain { text: String, global: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragRef {
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    pub line: usize,
    /// Always serialized, including when `null` (imports refs) -- NOT
    /// `skip_serializing_if`, unlike `qualified`/`member` which are omitted
    /// entirely when absent. See extract.rs's RefRecord doc comment.
    pub namespace: Option<String>,
    /// Type-certainty flag (see extract.rs's RefRecord). Serialized last and
    /// only when `true`; an absent key reads back as false, which also makes
    /// an older fragment JSON parse safely.
    #[serde(default, skip_serializing_if = "is_false")]
    pub generic: bool,
    /// Receiver fact (see extract.rs's RefRecord). Appended AFTER `generic`,
    /// and set only when a fact actually fired -- so it serializes last and
    /// only when present, and an absent key reads back as "no fact", the safe
    /// default.
    #[serde(default, rename = "receiverType", skip_serializing_if = "Option::is_none")]
    pub receiver_type: Option<String>,
    /// The callee arg count (see extract.rs's RefRecord). Appended AFTER
    /// `receiverType`, set only when the member access was the callee of an
    /// invocation -- so it serializes last and only when present, and an
    /// absent key reads back as "not a call", which is what keeps a property
    /// read out of the extension tier.
    #[serde(default, rename = "argCount", skip_serializing_if = "Option::is_none")]
    pub arg_count: Option<usize>,
    /// Receiver generic-arg descriptors (see extract.rs's RefRecord). Appended
    /// LAST, after `argCount`, set only when the receiver's DECLARED type was
    /// generic -- an absent key reads back as "not generic", which is what
    /// makes a generic-vs-non-generic pairing fail to unify.
    #[serde(default, rename = "receiverArgs", skip_serializing_if = "Option::is_none")]
    pub receiver_args: Option<Vec<String>>,
    /// Enclosing-type stack (see extract.rs's RefRecord). Appended LAST, after
    /// `receiverArgs`, and set only when non-empty. A Vec rather than an Option
    /// because empty and absent mean the same thing here -- a ref at namespace
    /// level and an older cached fragment both read back as "no enclosing
    /// type", which is what keeps them off the step.
    #[serde(default, rename = "outerTypes", skip_serializing_if = "Vec::is_empty")]
    pub outer_types: Vec<String>,
    /// Generic-arg descriptors for a 'ctor-param' ref (see extract.rs's
    /// RefRecord). Appended LAST of all, after `outerTypes`, and set only when
    /// the parameter's type was generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// (See extract.rs's RefRecord.) The type whose PROPERTY the
    /// qualifier's last segment is, for a two-segment chain whose head the
    /// enclosing scope could type. Appended after `args`, and never present
    /// alongside `receiverType`.
    #[serde(default, rename = "receiverPropertyOwner", skip_serializing_if = "Option::is_none")]
    pub receiver_property_owner: Option<String>,
    /// (See extract.rs's RefRecord.) The type whose METHOD a
    /// `var x = Q.M(...)` initializer called, and that method's name. Appended
    /// LAST of all, always as a pair, and never alongside `receiverType`: an
    /// absent pair reads back as "no call fact", which is what leaves the local
    /// taken-but-unknown.
    #[serde(default, rename = "receiverCallOwner", skip_serializing_if = "Option::is_none")]
    pub receiver_call_owner: Option<String>,
    #[serde(default, rename = "receiverCallMember", skip_serializing_if = "Option::is_none")]
    pub receiver_call_member: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One declared member, with the line its own NAME token sits on.
/// Field order (`name`, `kind`, `line`, `owner`) is significant, and `owner`
/// is omitted when empty -- which is how
/// a markup or resource key, owned by no C# type, serializes with three fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragName {
    pub name: String,
    pub kind: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
    pub defs: Vec<FragDef>,
    pub usings: Vec<FragUsing>,
    pub refs: Vec<FragRef>,
    /// Appended LAST, after `refs`. Always serialized, like its
    /// three siblings: a fragment's top-level arrays are a fixed shape, and
    /// only the fields INSIDE a record follow the omit-when-empty rule.
    #[serde(default)]
    pub names: Vec<FragName>,
}

/// The two shapes a cached fragment can have. Each rel is keyed to whichever
/// shape the file's grammar produced; the `ts: 1` tag is what tells them
/// apart, on disk and at the
/// resolver's door. Untagged, and TS FIRST: a C#/markup fragment carries no
/// `ts` key (so the TS arm always fails on it) and a TS fragment carries no
/// `usings` (so the C# arm always fails on it) -- the discrimination is total
/// in both directions, never a first-match-wins guess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnyFragment {
    Ts(extract::TsFragment),
    Cs(Fragment),
}

impl From<Fragment> for AnyFragment {
    fn from(f: Fragment) -> Self {
        AnyFragment::Cs(f)
    }
}

impl From<extract::TsFragment> for AnyFragment {
    fn from(f: extract::TsFragment) -> Self {
        AnyFragment::Ts(f)
    }
}

/// Build a graph fragment from this crate's extractor output.
/// `Extraction.purpose` has no fragment
/// counterpart -- the fragment is graph-only.
pub fn fragment_from_extraction(e: &extract::Extraction) -> Fragment {
    Fragment {
        defs: e
            .defs
            .iter()
            .map(|d| FragDef {
                id: d.id.clone(),
                name: d.name.clone(),
                namespace: d.namespace.clone(),
                kind: d.kind.clone(),
                line: d.line,
                methods: d.methods.clone(),
                properties: d.properties.clone(),
                fields: d.fields.clone(),
                method_returns: {
                    let mut m = OrderedMap::new();
                    for (name, returns) in &d.method_returns {
                        m.insert(name.clone(), returns.clone());
                    }
                    m
                },
                extension_methods: d
                    .extension_methods
                    .iter()
                    .map(|e| FragExtensionMethod {
                        name: e.name.clone(),
                        this_type: e.this_type.clone(),
                        arity_min: e.arity_min,
                        arity_max: e.arity_max,
                        this_args: e.this_args.clone(),
                    })
                    .collect(),
                bases: d.bases.clone(),
                type_params: d.type_params.clone(),
                base_generic_args: {
                    let mut m = OrderedMap::new();
                    for (name, args) in &d.base_generic_args {
                        m.insert(name.clone(), args.clone());
                    }
                    m
                },
                test_methods: d.test_methods.clone(),
                property_types: {
                    let mut m = OrderedMap::new();
                    for (name, fact) in &d.property_types {
                        m.insert(name.clone(), FragFact { type_name: fact.type_name.clone(), args: fact.args.clone() });
                    }
                    m
                },
            })
            .collect(),
        usings: e
            .usings
            .iter()
            .map(|u| match u {
                extract::UsingRecord::Alias { alias, target, global } => {
                    FragUsing::Alias { alias: alias.clone(), target: target.clone(), global: *global }
                }
                extract::UsingRecord::Plain { text, global } => FragUsing::Plain { text: text.clone(), global: *global },
            })
            .collect(),
        refs: e
            .refs
            .iter()
            .map(|r| FragRef {
                kind: r.kind.clone(),
                name: r.name.clone(),
                qualified: r.qualified.clone(),
                member: r.member.clone(),
                line: r.line,
                namespace: r.namespace.clone(),
                generic: r.generic,
                receiver_type: r.receiver_type.clone(),
                arg_count: r.arg_count,
                receiver_args: r.receiver_args.clone(),
                outer_types: r.outer_types.clone(),
                args: r.args.clone(),
                receiver_property_owner: r.receiver_property_owner.clone(),
                receiver_call_owner: r.receiver_call_owner.clone(),
                receiver_call_member: r.receiver_call_member.clone(),
            })
            .collect(),
        names: e
            .names
            .iter()
            .map(|n| FragName { name: n.name.clone(), kind: n.kind.clone(), line: n.line, owner: n.owner.clone() })
            .collect(),
    }
}

/// Markup and resource files never reach the extractor: a
/// line scan reads them instead (see markup.rs). They still get a fragment even
/// when they declare nothing, because the fragments index is what tells `map`
/// whether the graph's input set changed, and a file missing from it reads as a
/// permanent mismatch. `usings` is always empty: XAML has no using-directive
/// equivalent, and the `xmlns:` prefix declarations that stand in for one are
/// already resolved into fully-qualified ref names by the scan.
pub fn markup_fragment(root: &Path, rel: &str) -> Option<Fragment> {
    // Lossy read: an invalid byte becomes U+FFFD rather than dropping the
    // file.
    let text = String::from_utf8_lossy(&fs::read(root.join(rel)).ok()?).into_owned();
    let facts = crate::markup::markup_facts(rel, &text);
    Some(Fragment {
        defs: facts
            .defs
            .into_iter()
            .map(|d| FragDef {
                id: d.id,
                name: d.name,
                namespace: d.namespace,
                kind: d.kind,
                line: d.line,
                methods: Vec::new(),
                properties: Vec::new(),
                fields: Vec::new(),
                method_returns: OrderedMap::new(),
                extension_methods: Vec::new(),
                bases: Vec::new(),
                type_params: Vec::new(),
                base_generic_args: OrderedMap::new(),
                test_methods: Vec::new(),
                property_types: OrderedMap::new(),
            })
            .collect(),
        usings: Vec::new(),
        refs: facts
            .refs
            .into_iter()
            .map(|r| FragRef {
                kind: r.kind,
                name: r.name,
                qualified: r.qualified,
                member: r.member,
                line: r.line,
                // A markup ref site has no enclosing namespace of its own, and
                // the empty string is what a C# ref at file scope carries too --
                // never `None`, which is the 'imports' spelling.
                namespace: Some(String::new()),
                generic: false,
                receiver_type: None,
                arg_count: None,
                receiver_args: None,
                outer_types: Vec::new(),
                args: None,
                receiver_property_owner: None,
                receiver_call_owner: None,
                receiver_call_member: None,
            })
            .collect(),
        names: facts
            .names
            .into_iter()
            .map(|n| FragName { name: n.name, kind: n.kind, line: n.line, owner: n.owner })
            .collect(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentCacheEntry {
    pub mtime: i64,
    pub fragment: AnyFragment,
}

fn read_fragments_cache(root: &Path) -> OrderedMap<FragmentCacheEntry> {
    match fs::read_to_string(fragments_cache_path(root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => OrderedMap::default(),
    }
}

fn write_fragments_cache(root: &Path, cache: &OrderedMap<FragmentCacheEntry>) -> io::Result<()> {
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

fn write_fragments_index(root: &Path, cache: &OrderedMap<FragmentCacheEntry>) -> io::Result<()> {
    let mut index = OrderedMap::new();
    for (rel, entry) in cache.iter() {
        index.insert(rel.clone(), entry.mtime);
    }
    atomic_write_json(&fragments_index_path(root), &index)
}

/// Whether the fragments index is stale for the given graph files (any mtime
/// mismatch, or a differing file count).
pub fn index_is_stale(index: &OrderedMap<i64>, graph_files: &[GraphFile]) -> bool {
    graph_files.iter().any(|f| index.get(&f.rel) != Some(&f.mtime)) || index.len() != graph_files.len()
}

// ---------------------------------------------------------------------------
// `devscout map`'s cache-then-resolve-then-write cycle.
// ---------------------------------------------------------------------------

/// One file that contributes a graph fragment: C# (defs, refs, member names),
/// markup (`x:Class`/`x:Name`, `.resw` keys -- names only) or TS/JS
/// (imports, exported declarations, call/JSX/dispatch references).
#[derive(Debug, Clone)]
pub struct GraphFile {
    pub rel: String,
    pub mtime: i64,
}

#[derive(Debug)]
pub enum RebuildOutcome {
    NotRebuilt,
    Rebuilt(Graph),
}

/// `fresh_fragments`: fragments this run's extractor produced for files that
/// needed reparsing (the same set that got a fresh purpose signature in the
/// real `devscout map` flow -- `mapcmd::map_repo` assembles it). `changed`: from the
/// caller's own `indexIsStale`-equivalent check against `csFiles`, passed in
/// rather than recomputed so the unchanged path never opens any graph file.
pub fn rebuild_graph(
    root: &Path,
    graph_files: &[GraphFile],
    fresh_fragments: &HashMap<String, AnyFragment>,
    changed: bool,
) -> io::Result<RebuildOutcome> {
    if !changed && graph_json_path(root).exists() {
        return Ok(RebuildOutcome::NotRebuilt);
    }

    let cache = read_fragments_cache(root);
    // Split at the door: the two shapes share no field beyond
    // `defs`, and letting one resolver see the other's names would resolve a
    // C# type reference onto a same-named TypeScript const. Built as two vecs
    // here rather than one mixed vec split later so no fragment is cloned a
    // third time on the rebuild path.
    let mut merged_cs: Vec<(String, Fragment)> = Vec::new();
    let mut merged_ts: Vec<(String, extract::TsFragment)> = Vec::new();
    let mut new_cache: OrderedMap<FragmentCacheEntry> = OrderedMap::new();
    for f in graph_files {
        let fragment = match cache.get(&f.rel) {
            Some(entry) if entry.mtime == f.mtime => Some(entry.fragment.clone()),
            _ => fresh_fragments.get(&f.rel).cloned(),
        };
        let Some(fragment) = fragment else { continue };
        match &fragment {
            AnyFragment::Cs(c) => merged_cs.push((f.rel.clone(), c.clone())),
            AnyFragment::Ts(t) => merged_ts.push((f.rel.clone(), t.clone())),
        }
        new_cache.insert(f.rel.clone(), FragmentCacheEntry { mtime: f.mtime, fragment });
    }

    let graph = crate::resolve::resolve_graph_with_ts(root, &merged_cs, &merged_ts);
    write_graph(root, &graph)?;
    write_fragments_cache(root, &new_cache)?;
    write_fragments_index(root, &new_cache)?;
    remove_superseded_caches(root);
    Ok(RebuildOutcome::Rebuilt(graph))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("scout-graph-test-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- OrderedMap: order + round-trip -------------------------------

    #[test]
    fn ordered_map_preserves_insertion_order_through_json_round_trip() {
        let mut m: OrderedMap<i64> = OrderedMap::new();
        m.insert("z.cs".to_string(), 3);
        m.insert("a.cs".to_string(), 1);
        m.insert("m.cs".to_string(), 2);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"z.cs":3,"a.cs":1,"m.cs":2}"#);

        let reparsed: OrderedMap<i64> = serde_json::from_str(&json).unwrap();
        let keys: Vec<&String> = reparsed.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["z.cs", "a.cs", "m.cs"]);
    }

    #[test]
    fn ordered_map_insert_overwrite_keeps_original_position() {
        let mut m: OrderedMap<i64> = OrderedMap::new();
        m.insert("a".to_string(), 1);
        m.insert("b".to_string(), 2);
        m.insert("a".to_string(), 99);
        let keys: Vec<&String> = m.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(m.get("a"), Some(&99));
    }

    // --- Percent1: number-shaped formatting -------------------------

    #[test]
    fn percent1_whole_value_serializes_without_decimal_point() {
        let p = Percent1::from_ratio(2, 10); // 20.0 -> "20"
        assert_eq!(serde_json::to_string(&p).unwrap(), "20");
    }

    #[test]
    fn percent1_fractional_value_serializes_with_one_decimal_digit() {
        let p = Percent1::from_ratio(1, 3); // 33.333... -> round to 33.3
        assert_eq!(serde_json::to_string(&p).unwrap(), "33.3");
    }

    #[test]
    fn percent1_zero_denominator_is_zero() {
        assert_eq!(Percent1::from_ratio(0, 0), Percent1::zero());
        assert_eq!(serde_json::to_string(&Percent1::zero()).unwrap(), "0");
    }

    // --- FragDef's new keys, appended last, omitted when empty ----------------------

    fn frag_def(
        methods: &[&str],
        properties: &[&str],
        fields: &[&str],
        method_returns: &[(&str, &str)],
        extension_methods: &[(&str, &str, usize, i64)],
    ) -> FragDef {
        frag_def_with_bases(methods, properties, fields, method_returns, extension_methods, &[])
    }

    fn frag_def_with_bases(
        methods: &[&str],
        properties: &[&str],
        fields: &[&str],
        method_returns: &[(&str, &str)],
        extension_methods: &[(&str, &str, usize, i64)],
        bases: &[&str],
    ) -> FragDef {
        let mut mr = OrderedMap::new();
        for (k, v) in method_returns {
            mr.insert((*k).to_string(), (*v).to_string());
        }
        FragDef {
            id: "App.Facts.Widget".into(),
            name: "Widget".into(),
            namespace: "App.Facts".into(),
            kind: "class".into(),
            line: 3,
            methods: methods.iter().map(|s| s.to_string()).collect(),
            properties: properties.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
            method_returns: mr,
            extension_methods: extension_methods
                .iter()
                .map(|(n, t, lo, hi)| FragExtensionMethod {
                    name: (*n).to_string(),
                    this_type: (*t).to_string(),
                    arity_min: *lo,
                    arity_max: *hi,
                    this_args: None,
                })
                .collect(),
            bases: bases.iter().map(|s| s.to_string()).collect(),
            type_params: Vec::new(),
            base_generic_args: OrderedMap::new(),
            property_types: OrderedMap::new(),
            test_methods: Vec::new(),
        }
    }

    #[test]
    fn frag_def_appends_properties_fields_and_method_returns_last_in_that_order() {
        let json = serde_json::to_string(&frag_def(&["GetAsync"], &["Prefix"], &["_log"], &[("GetAsync", "Task")], &[])).unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["GetAsync"],"properties":["Prefix"],"fields":["_log"],"methodReturns":{"GetAsync":"Task"}}"#
        );
    }

    #[test]
    fn frag_def_omits_all_three_new_keys_when_the_type_declares_none_of_them() {
        let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"]}"#,
            "pre-stage-2 bytes preserved exactly"
        );
    }

    // --- extensionMethods lands AFTER methodReturns, entry keys
    // in (name, thisType, arityMin, arityMax, thisArgs) order, omitted when
    // empty; `bases` lands after extensionMethods -------------------------

    #[test]
    fn frag_def_appends_extension_methods_after_method_returns_with_the_arity_range_last() {
        let json = serde_json::to_string(&frag_def(
            &["Go"],
            &["Prefix"],
            &["_log"],
            &[("Go", "Task")],
            &[("Render", "Widget", 0, 0), ("Render", "Widget", 2, 3), ("Trim", "string", 0, -1)],
        ))
        .unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"properties":["Prefix"],"fields":["_log"],"methodReturns":{"Go":"Task"},"extensionMethods":[{"name":"Render","thisType":"Widget","arityMin":0,"arityMax":0},{"name":"Render","thisType":"Widget","arityMin":2,"arityMax":3},{"name":"Trim","thisType":"string","arityMin":0,"arityMax":-1}]}"#
        );
    }

    #[test]
    fn frag_def_this_args_lands_after_arity_max_and_bases_after_extension_methods() {
        let mut d = frag_def_with_bases(&["Go"], &[], &[], &[], &[("Each", "List", 0, 0)], &["BaseWidget", "IWidget"]);
        d.extension_methods[0].this_args = Some(vec!["Widget".into()]);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"extensionMethods":[{"name":"Each","thisType":"List","arityMin":0,"arityMax":0,"thisArgs":["Widget"]}],"bases":["BaseWidget","IWidget"]}"#
        );
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.bases, vec!["BaseWidget".to_string(), "IWidget".to_string()]);
        assert_eq!(reparsed.extension_methods[0].this_args, Some(vec!["Widget".to_string()]));
    }

    #[test]
    fn frag_def_omits_bases_when_the_type_lists_none() {
        let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
        assert!(!json.contains("bases"), "an empty base list must be omitted entirely: {json}");
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert!(reparsed.bases.is_empty(), "an absent key reads back as \"lists none\"");
    }

    // --- test-coverage stage: testMethods lands AFTER bases, omitted when
    // the type declares no tests ------------------------------------------

    #[test]
    fn frag_def_appends_test_methods_last_after_bases() {
        let mut d = frag_def_with_bases(&["Go"], &[], &[], &[], &[], &["BaseWidget"]);
        d.test_methods = vec!["TotalsAnEmptyOrder".into(), "TotalsALineItem".into()];
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"bases":["BaseWidget"],"testMethods":["TotalsAnEmptyOrder","TotalsALineItem"]}"#
        );
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.test_methods, vec!["TotalsAnEmptyOrder".to_string(), "TotalsALineItem".to_string()]);
    }

    #[test]
    fn frag_def_omits_test_methods_when_the_type_declares_no_tests() {
        let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
        assert!(!json.contains("testMethods"), "pre-stage bytes preserved exactly: {json}");
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert!(reparsed.test_methods.is_empty(), "an absent key reads back as \"declares no tests\"");
    }

    #[test]
    fn frag_def_omits_extension_methods_when_the_type_declares_none() {
        let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[("Go", "Task")], &[])).unwrap();
        assert!(!json.contains("extensionMethods"), "pre-stage-3 bytes preserved exactly: {json}");
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert!(reparsed.extension_methods.is_empty(), "an absent key reads back as \"declares none\"");
    }

    #[test]
    fn frag_def_method_returns_serializes_in_first_declaration_order_not_sorted() {
        // A BTreeMap here would emit Alpha before Zebra -- the required order
        // is insertion (first-declaration) order.
        let json = serde_json::to_string(&frag_def(&[], &[], &[], &[("Zebra", "Z"), ("Alpha", "A")], &[])).unwrap();
        assert!(json.ends_with(r#""methodReturns":{"Zebra":"Z","Alpha":"A"}}"#), "insertion order, not sorted: {json}");
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        let keys: Vec<&String> = reparsed.method_returns.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["Zebra", "Alpha"], "and the order survives a round trip");
    }

    // --- propertyTypes lands AFTER testMethods, omitted when the
    // type declares no typed property ---------------------------------------

    #[test]
    fn frag_def_appends_property_types_last_after_test_methods() {
        let mut d = frag_def_with_bases(&["Go"], &["Dial", "Slots"], &[], &[], &[], &["BaseWidget"]);
        d.test_methods = vec!["TotalsALineItem".into()];
        d.property_types.insert("Dial".into(), FragFact { type_name: "Gauge".into(), args: None });
        d.property_types.insert("Slots".into(), FragFact { type_name: "Toolbox".into(), args: Some(vec!["Gadget".into()]) });
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"properties":["Dial","Slots"],"bases":["BaseWidget"],"testMethods":["TotalsALineItem"],"propertyTypes":{"Dial":{"type":"Gauge"},"Slots":{"type":"Toolbox","args":["Gadget"]}}}"#
        );
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        let keys: Vec<&String> = reparsed.property_types.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["Dial", "Slots"], "source order survives a round trip, like methodReturns");
    }

    #[test]
    fn frag_def_omits_property_types_when_no_property_carries_a_fact() {
        let json = serde_json::to_string(&frag_def(&["Go"], &["Label"], &[], &[], &[])).unwrap();
        assert!(!json.contains("propertyTypes"), "pre-propertyTypes bytes preserved exactly: {json}");
        let reparsed: FragDef = serde_json::from_str(&json).unwrap();
        assert!(reparsed.property_types.is_empty(), "an absent key reads back as \"no property vouches for a type\"");
    }

    // --- FragRef's receiverType then argCount, appended
    // after generic in that order ------------------------------------------

    fn frag_ref(generic: bool, receiver_type: Option<&str>, arg_count: Option<usize>) -> FragRef {
        FragRef {
            kind: "uses-member".into(),
            name: "repo".into(),
            qualified: None,
            member: Some("Save".into()),
            line: 7,
            namespace: Some("App.Shape".into()),
            generic,
            receiver_type: receiver_type.map(String::from),
            arg_count,
            receiver_args: None,
            outer_types: Vec::new(),
            args: None,
            receiver_property_owner: None,
            receiver_call_owner: None,
            receiver_call_member: None,
        }
    }

    #[test]
    fn frag_ref_receiver_args_is_appended_last_after_arg_count() {
        let r = FragRef { receiver_args: Some(vec!["FutureState".into(), "*".into()]), ..frag_ref(false, Some("Binder"), Some(1)) };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","receiverType":"Binder","argCount":1,"receiverArgs":["FutureState","*"]}"#
        );
        let reparsed: FragRef = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.receiver_args, Some(vec!["FutureState".to_string(), "*".to_string()]));
    }

    #[test]
    fn frag_ref_receiver_type_then_arg_count_are_appended_last_after_generic() {
        let json = serde_json::to_string(&frag_ref(true, Some("IRepo"), Some(2))).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","generic":true,"receiverType":"IRepo","argCount":2}"#
        );
    }

    #[test]
    fn frag_ref_arg_count_zero_still_serializes_it() {
        // The guard is presence (argCount may be 0), never truthiness -- a
        // zero-argument call is a real call and its 0 is what matches an
        // arity-0 extension.
        let json = serde_json::to_string(&frag_ref(false, Some("IRepo"), Some(0))).unwrap();
        assert!(json.ends_with(r#""receiverType":"IRepo","argCount":0}"#), "argCount 0 must survive: {json}");
    }

    #[test]
    fn frag_ref_without_a_fact_keeps_its_pre_stage_2_bytes() {
        let json = serde_json::to_string(&frag_ref(false, None, None)).unwrap();
        assert_eq!(json, r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape"}"#);
        let reparsed: FragRef = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.receiver_type, None, "an absent key reads back as no fact");
        assert_eq!(reparsed.arg_count, None, "and an absent argCount reads back as \"not a call\"");
        assert_eq!(reparsed.receiver_args, None, "and an absent receiverArgs reads back as \"not generic\"");
    }

    // --- Def: also_in omission ------------------------------------------

    #[test]
    fn def_without_also_in_omits_the_key() {
        let d = Def {
            id: "Ns.Type".into(),
            name: "Type".into(),
            namespace: "Ns".into(),
            kind: "class".into(),
            file: "Ns/Type.cs".into(),
            line: 3,
            methods: vec![],
            test_methods: vec![],
            also_in: vec![],
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("also_in"), "empty also_in must be omitted: {json}");
    }

    #[test]
    fn def_with_also_in_includes_it_after_methods() {
        let d = Def {
            id: "Ns.Type".into(),
            name: "Type".into(),
            namespace: "Ns".into(),
            kind: "class".into(),
            file: "Ns/Type.cs".into(),
            line: 3,
            methods: vec!["M".into()],
            test_methods: vec![],
            also_in: vec![AlsoIn { file: "Ns/Type.Extra.cs".into(), line: 5 }],
        };
        let json = serde_json::to_string(&d).unwrap();
        let methods_pos = json.find("\"methods\"").unwrap();
        let also_in_pos = json.find("\"also_in\"").unwrap();
        assert!(methods_pos < also_in_pos, "also_in must come after methods: {json}");
    }

    // --- test-coverage stage: the def ROW keeps testMethods, between
    // methods and also_in ---------------------------------------------------

    #[test]
    fn def_row_places_test_methods_between_methods_and_also_in() {
        let d = Def {
            id: "Ns.TypeTests".into(),
            name: "TypeTests".into(),
            namespace: "Ns".into(),
            kind: "class".into(),
            file: "Ns/TypeTests.cs".into(),
            line: 3,
            methods: vec!["M".into()],
            test_methods: vec!["Fact1".into()],
            also_in: vec![AlsoIn { file: "Ns/TypeTests.Extra.cs".into(), line: 5 }],
        };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"id":"Ns.TypeTests","name":"TypeTests","namespace":"Ns","kind":"class","file":"Ns/TypeTests.cs","line":3,"methods":["M"],"testMethods":["Fact1"],"also_in":[{"file":"Ns/TypeTests.Extra.cs","line":5}]}"#
        );
    }

    #[test]
    fn def_row_omits_test_methods_when_the_def_declares_no_tests() {
        let d = Def {
            id: "Ns.Type".into(),
            name: "Type".into(),
            namespace: "Ns".into(),
            kind: "class".into(),
            file: "Ns/Type.cs".into(),
            line: 3,
            methods: vec!["M".into()],
            test_methods: vec![],
            also_in: vec![],
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("testMethods"), "empty testMethods must be omitted: {json}");
    }

    // --- test-coverage stage: stats gains test_def_count, LAST -------------

    #[test]
    fn stats_appends_test_def_count_last_and_always_writes_it() {
        let stats = Stats {
            def_count: 2,
            file_count: 1,
            edges_by_kind: EdgesByKind::default(),
            ambiguous_count: 0,
            ambiguous_pct: Percent1::zero(),
            unresolved_external_count: 0,
            heuristic_edge_count: 0,
            test_def_count: 0,
            ts: None,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.ends_with(r#""heuristic_edge_count":0,"test_def_count":0}"#), "test_def_count is LAST: {json}");
    }

    // --- Edge: shape-per-kind, tag first ---------------------------------

    #[test]
    fn imports_edge_has_target_not_to() {
        let e = Edge::Imports { from_file: "F.cs".into(), from_line: 1, target: "System".into() };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"imports","from_file":"F.cs","from_line":1,"target":"System"}"#);
    }

    #[test]
    fn uses_type_edge_shape() {
        let e = Edge::UsesType { from_file: "F.cs".into(), from_line: 1, to: "Ns.T".into(), to_file: "Ns/T.cs".into(), heuristic: false };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"uses-type","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs"}"#);
    }

    // The serialized shape in one assertion: `heuristic` is the LAST key, it
    // appears only when set, and a precise edge of the same kind is
    // byte-for-byte what it was before the tag existed.
    #[test]
    fn heuristic_flag_is_appended_last_and_omitted_when_false() {
        let precise =
            Edge::UsesMember { from_file: "F.cs".into(), from_line: 1, to: "Ns.T".into(), to_file: "Ns/T.cs".into(), heuristic: false };
        assert_eq!(
            serde_json::to_string(&precise).unwrap(),
            r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs"}"#
        );
        let guess =
            Edge::UsesMember { from_file: "F.cs".into(), from_line: 1, to: "Ns.T".into(), to_file: "Ns/T.cs".into(), heuristic: true };
        assert_eq!(
            serde_json::to_string(&guess).unwrap(),
            r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs","heuristic":true}"#
        );
        // And it reads back: an edge written by either runtime round-trips
        // with the flag intact, absent meaning precise.
        assert_eq!(serde_json::from_str::<Edge>(&serde_json::to_string(&guess).unwrap()).unwrap(), guess);
        assert_eq!(serde_json::from_str::<Edge>(&serde_json::to_string(&precise).unwrap()).unwrap(), precise);
    }

    #[test]
    fn ambiguous_edge_shape() {
        let e = Edge::Ambiguous {
            origin: "uses-type".into(),
            from_file: "F.cs".into(),
            from_line: 4,
            raw: "Money".into(),
            candidates: vec![Candidate { id: "A.Money".into(), file: "A/Money.cs".into() }],
            candidate_count: 2,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"ambiguous","origin":"uses-type","from_file":"F.cs","from_line":4,"raw":"Money","candidates":[{"id":"A.Money","file":"A/Money.cs"}],"candidate_count":2}"#
        );
    }

    // --- Stats: fixed edges_by_kind order ---------------------------------

    #[test]
    fn edges_by_kind_field_order_is_fixed_not_alphabetical() {
        let s = EdgesByKind { inherits: 1, uses_type: 2, imports: 3, uses_member: 4, ctor_di: 5, ..Default::default() };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"inherits":1,"uses-type":2,"imports":3,"uses-member":4,"ctor-di":5}"#);
    }

    // --- The cache holds two fragment shapes -------------------

    // The untagged discrimination is what keeps one cache file able to hold
    // both shapes: read the wrong arm and a whole repo's fragments come back
    // silently empty. Both directions are pinned here, on the exact
    // serialized bytes.
    #[test]
    fn any_fragment_round_trips_both_shapes_and_never_reads_one_as_the_other() {
        let cs = AnyFragment::Cs(Fragment {
            defs: Vec::new(),
            usings: Vec::new(),
            refs: Vec::new(),
            names: Vec::new(),
        });
        let cs_json = serde_json::to_string(&cs).unwrap();
        assert_eq!(cs_json, r#"{"defs":[],"usings":[],"refs":[],"names":[]}"#);
        assert!(matches!(serde_json::from_str::<AnyFragment>(&cs_json).unwrap(), AnyFragment::Cs(_)));

        let ts = AnyFragment::Ts(extract::TsFragment {
            ts: 1,
            defs: vec![extract::TsFragmentDef { name: "x".into(), kind: "const".into(), line: 1 }],
            imports: Vec::new(),
            reexports: Vec::new(),
            refs: Vec::new(),
            default: None,
        });
        let ts_json = serde_json::to_string(&ts).unwrap();
        assert_eq!(ts_json, r#"{"ts":1,"defs":[{"name":"x","kind":"const","line":1}],"imports":[],"reexports":[],"refs":[]}"#);
        assert!(matches!(serde_json::from_str::<AnyFragment>(&ts_json).unwrap(), AnyFragment::Ts(_)));
        assert_eq!(serde_json::from_str::<AnyFragment>(&ts_json).unwrap(), ts, "and round-trips to the same value");
    }

    // The four TS counts land AFTER `ctor-di`, in this order, and
    // only when the repo carries a TS fragment at all: a C#-only repo's stats
    // block keeps the exact bytes the assertion above pins.
    #[test]
    fn edges_by_kind_appends_the_four_ts_counts_after_ctor_di_when_present() {
        let s = EdgesByKind {
            inherits: 1,
            uses_type: 2,
            imports: 3,
            uses_member: 4,
            ctor_di: 5,
            import: Some(6),
            call: Some(7),
            jsx_use: Some(8),
            dispatch: Some(9),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"inherits":1,"uses-type":2,"imports":3,"uses-member":4,"ctor-di":5,"import":6,"call":7,"jsx-use":8,"dispatch":9}"#
        );
    }

    // --- atomic writes: crash-sim -----------------------------------

    #[test]
    fn atomic_write_leaves_target_intact_when_only_a_stray_tmp_exists() {
        let dir = temp_dir("atomic-crash");
        let target = dir.join("graph.json");
        fs::write(&target, b"{\"old\":true}").unwrap();
        // Simulate a crashed prior write: a tmp file was created but the
        // process died before the rename -- the target must be completely
        // unaffected by the stray file's mere presence.
        fs::write(dir.join(".graph.json.tmp.99999.7"), b"GARBAGE-PARTIAL").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"old\":true}");
    }

    #[test]
    fn atomic_write_replaces_target_and_leaves_no_tmp_file_behind() {
        let dir = temp_dir("atomic-clean");
        let target = dir.join("graph.json");
        fs::write(&target, b"{\"old\":true}").unwrap();
        atomic_write_bytes(&target, b"{\"new\":true}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"new\":true}");
        let leftover: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftover.is_empty(), "no tmp file should remain: {leftover:?}");
    }

    #[test]
    fn atomic_write_creates_parent_directories() {
        let dir = temp_dir("atomic-mkdir");
        let target = dir.join("nested").join("deeper").join("graph.json");
        atomic_write_bytes(&target, b"{}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
    }

    // --- rebuild_graph: unchanged path never touches the graph -----------

    #[test]
    fn rebuild_graph_skips_when_unchanged_and_graph_already_exists() {
        let dir = temp_dir("rebuild-unchanged");
        fs::create_dir_all(graph_dir(&dir)).unwrap();
        fs::write(graph_json_path(&dir), b"{\"schema_version\":1,\"built_at_head\":null,\"defs\":[],\"edges\":[],\"stats\":{\"def_count\":0,\"file_count\":0,\"edges_by_kind\":{\"inherits\":0,\"uses-type\":0,\"imports\":0,\"uses-member\":0},\"ambiguous_count\":0,\"ambiguous_pct\":0,\"unresolved_external_count\":0}}").unwrap();
        let outcome = rebuild_graph(&dir, &[], &HashMap::new(), false).unwrap();
        assert!(matches!(outcome, RebuildOutcome::NotRebuilt));
    }

    #[test]
    fn rebuild_graph_reuses_a_cached_fragment_at_matching_mtime() {
        let dir = temp_dir("rebuild-cache-hit");
        let fragment = Fragment {
            defs: vec![FragDef { id: "A".into(), name: "A".into(), namespace: "".into(), kind: "class".into(), line: 1, methods: vec![], properties: vec![], fields: vec![], method_returns: OrderedMap::new(), extension_methods: vec![], bases: vec![], type_params: vec![], base_generic_args: OrderedMap::new(), test_methods: vec![], property_types: OrderedMap::new() }],
            usings: vec![],
            refs: vec![],
            names: vec![],
        };
        // First build: nothing cached, comes from fresh_fragments.
        let mut fresh = HashMap::new();
        fresh.insert("A.cs".to_string(), AnyFragment::Cs(fragment.clone()));
        let graph_files = vec![GraphFile { rel: "A.cs".to_string(), mtime: 111 }];
        let first = rebuild_graph(&dir, &graph_files, &fresh, true).unwrap();
        assert!(matches!(first, RebuildOutcome::Rebuilt(_)));

        // Second build: same mtime, EMPTY fresh_fragments -- must reuse the
        // cache, not silently drop the file from the graph.
        let empty: HashMap<String, AnyFragment> = HashMap::new();
        let second = rebuild_graph(&dir, &graph_files, &empty, true).unwrap();
        match second {
            RebuildOutcome::Rebuilt(g) => assert_eq!(g.defs.len(), 1, "cached fragment must still be used"),
            RebuildOutcome::NotRebuilt => panic!("changed=true must always rebuild"),
        }
    }

    // --- The v13 cache generation --------------------------

    #[test]
    fn rebuild_graph_writes_the_v13_caches_and_deletes_every_superseded_generation() {
        let dir = temp_dir("rebuild-v13");
        fs::create_dir_all(graph_dir(&dir)).unwrap();
        // The v9 pair joined this list when fragments gained def
        // `typeParams`/`baseGenericArgs` and the `ctor-param` ref kind, so a v9
        // fragment read back today carries none and every ctor-DI fact would be
        // missing from the graph. The v10 pair joined when a markup
        // fragment gained the `x:Class` def and its element/binding refs, so a
        // v10 markup fragment read back carries names only and every XAML
        // declaration and instantiation would be missing from the graph. The v11
        // pair joined when a `.ts/.tsx/.js/.jsx` file gained a reference
        // fragment where it previously had none at all, so a v11 cache read back
        // would leave every TS/JS rel looking like a file the worker never saw.
        // This side records no TS fragment yet, but the cache generation is
        // shared -- two writers putting two generations into one git dir would
        // have each delete the other's cache on every map. The v12 pair joined
        // when defs gained propertyTypes and refs
        // gained receiverPropertyOwner and the receiverCallOwner/
        // receiverCallMember pair, so a v12 fragment read back carries none and
        // every property hop and every var-from-invocation receiver would
        // silently stay unresolved.
        assert_eq!(
            SUPERSEDED_CACHE_FILES.len(),
            24,
            "v1..v12 pairs -- the v12 pair joined the list at the propertyTypes/receiver fact additions"
        );
        for stale in SUPERSEDED_CACHE_FILES {
            fs::write(graph_dir(&dir).join(stale), b"{}").unwrap();
        }

        let fragment = Fragment {
            defs: vec![FragDef {
                id: "App.A".into(),
                name: "A".into(),
                namespace: "App".into(),
                kind: "class".into(),
                line: 1,
                methods: vec![],
                properties: vec![],
                fields: vec![],
                method_returns: OrderedMap::new(),
                extension_methods: vec![],
                bases: vec![],
                type_params: vec![],
                base_generic_args: OrderedMap::new(),
                test_methods: vec![],
                property_types: OrderedMap::new(),
            }],
            usings: vec![],
            refs: vec![],
            names: vec![],
        };
        let mut fresh = HashMap::new();
        fresh.insert("src/A.cs".to_string(), AnyFragment::Cs(fragment));
        let graph_files = vec![GraphFile { rel: "src/A.cs".to_string(), mtime: 222 }];
        rebuild_graph(&dir, &graph_files, &fresh, true).unwrap();

        assert!(graph_dir(&dir).join("fragments-v13.json").exists(), "the v13 payload cache is what gets written");
        assert!(graph_dir(&dir).join("fragments-index-v13.json").exists(), "and its mtime-only index alongside it");
        for stale in SUPERSEDED_CACHE_FILES {
            assert!(
                !graph_dir(&dir).join(stale).exists(),
                "{stale} must be deleted -- rename IS the invalidation"
            );
        }
    }

    // --- v8: FragRef's outerTypes, appended last -----------------------------

    #[test]
    fn frag_ref_outer_types_is_appended_last_after_receiver_args() {
        let r = FragRef {
            receiver_args: Some(vec!["FutureState".into()]),
            outer_types: vec!["Outer".into(), "Inner".into()],
            ..frag_ref(false, Some("Binder"), Some(1))
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","receiverType":"Binder","argCount":1,"receiverArgs":["FutureState"],"outerTypes":["Outer","Inner"]}"#
        );
        let reparsed: FragRef = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.outer_types, vec!["Outer".to_string(), "Inner".to_string()]);
    }

    // --- The two chain facts, appended after outerTypes ----

    #[test]
    fn frag_ref_property_owner_and_call_pair_are_appended_last_and_omitted_when_absent() {
        let hop = FragRef {
            qualified: Some("head.Dial".into()),
            name: "Dial".into(),
            receiver_property_owner: Some("Widget".into()),
            ..frag_ref(false, None, Some(0))
        };
        assert_eq!(
            serde_json::to_string(&hop).unwrap(),
            r#"{"kind":"uses-member","name":"Dial","qualified":"head.Dial","member":"Save","line":7,"namespace":"App.Shape","argCount":0,"receiverPropertyOwner":"Widget"}"#
        );

        let from_call = FragRef {
            receiver_call_owner: Some("Factory".into()),
            receiver_call_member: Some("Make".into()),
            ..frag_ref(false, None, Some(0))
        };
        assert_eq!(
            serde_json::to_string(&from_call).unwrap(),
            r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","argCount":0,"receiverCallOwner":"Factory","receiverCallMember":"Make"}"#
        );

        // An older cached fragment carries neither key, and absent must
        // read back as "no chain fact" -- which is what leaves such a ref
        // exactly where it was before this generation.
        let plain = serde_json::to_string(&frag_ref(false, Some("Binder"), Some(1))).unwrap();
        assert!(!plain.contains("receiverPropertyOwner") && !plain.contains("receiverCall"), "{plain}");
        let reparsed: FragRef = serde_json::from_str(&plain).unwrap();
        assert_eq!(reparsed.receiver_property_owner, None);
        assert_eq!(reparsed.receiver_call_owner, None);
        assert_eq!(reparsed.receiver_call_member, None);
    }

    #[test]
    fn frag_ref_an_empty_or_absent_outer_types_serializes_to_nothing_and_reads_back_empty() {
        let json = serde_json::to_string(&frag_ref(false, None, None)).unwrap();
        assert!(!json.contains("outerTypes"), "a namespace-level ref keeps its exact pre-v8 bytes: {json}");
        // A pre-v8 cached fragment has no key at all, and absent must mean the
        // same thing as empty -- which is what keeps it off the nested step.
        let pre_v8: FragRef =
            serde_json::from_str(r#"{"kind":"uses-type","name":"Widget","line":3,"namespace":"App.Core"}"#).unwrap();
        assert!(pre_v8.outer_types.is_empty());
    }

}
