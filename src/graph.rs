// Artifact build/load, fragments cache, incremental reuse.
// Writes are atomic (tmp+rename); fragments cache keying is load-bearing --
// a mismatch makes reuse break silently.
//
// This module owns every serde struct for graph.json + the fragments-cache
// pair (fragments-v18.json, fragments-index-v18.json), plus their path resolution,
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
//   - a `uses-member` edge appends three more keys AFTER that shared prefix,
//     in this exact order: `heuristic` (omitted when precise), `tier`, then
//     `member`. `tier` names WHICH guess tier emitted the row and is present
//     exactly when `heuristic` is; `member` names the member the reference
//     reads or calls and is written on EVERY uses-member edge, the precise
//     ones included -- it is the one fact the row never carried. Both are
//     omit-when-`None`, so the append-last rule every other added key follows
//     holds here too; the other two flagged kinds (inherits, uses-type) stay
//     exactly as they were.
//   - `schema_version` is `GRAPH_SCHEMA_VERSION`, which the `tier`/`member`
//     append above moved to 2. `rebuild_graph`'s unchanged fast path reads
//     the first bytes of an existing graph.json and refuses to reuse one
//     written at an older version, so a stale artifact is rebuilt on the next
//     `map` even when not one fragment moved.
//   - `units` is the LAST key of the whole object, appended after `names`
//     and omitted entirely (not `[]`) when the repo declares no `.csproj` --
//     so a tree without one serializes byte-for-byte as it did before the
//     project model existed. Each row's own key order is fixed too: `id`,
//     `name`, then `refs` (omitted when the project references nothing) and
//     `test` (omitted when false). A unit's DIRECTORY is not persisted: it
//     is `id`'s parent, recomputed on read (`project::units_from_graph`),
//     and neither is per-file/per-def membership -- that is derived from the
//     unit list by `ProjectModel` rather than stored.
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

mod artifact;
mod cache;
mod def;
mod edge;
mod fragment;
mod fragment_types;
mod ordered;
mod paths;
mod rebuild;

pub use artifact::{read_graph, Graph, GraphName, GraphUnit, Stats, GRAPH_SCHEMA_VERSION};
pub use cache::{index_is_stale, read_fragments_index, FragmentCacheEntry};
pub use def::{AlsoIn, Def};
pub use edge::{Candidate, Edge, EdgesByKind, HeuristicByTier, HeuristicTier};
pub use fragment::{fragment_from_extraction, markup_fragment};
pub use fragment_types::{
    AnyFragment, FragDef, FragExtensionMethod, FragFact, FragLambdaSlot, FragName, FragRef,
    FragUsing, Fragment,
};
pub use ordered::{OrderedMap, Percent1};
pub use paths::{graph_json_path, project_units_path};
pub use rebuild::{rebuild_graph, GraphFile, RebuildOutcome};

#[cfg(test)]
mod tests;
