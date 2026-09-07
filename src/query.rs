// refs/impact query support: enum-member union, `impact_walk`, and
// personalized PageRank. A `tests` block below pins the behavioral contract
// with fixture cases exercising every code path.
//
// Design notes:
//
// - **Two-phase index build.** The index cannot be built in one call that
//   also reads the graph and then hands back a struct borrowing that graph
//   -- the result would be self-referential. So the caller reads the graph
//   first with `graph::read_graph(root)`, which returns `None` when there is
//   no graph, letting the caller check the `Option` before proceeding, then
//   calls `load_graph_index(&graph, root)` here, which owns the manifest
//   join and the index build.
//
// - **Manifest corrupt-JSON fail-open.** A corrupt manifest.json surfaces as
//   `Err(ManifestError::InvalidJson)` from `manifest::read_manifest`.
//   Panicking through a query path for an operator-error edge case (a
//   hand-corrupted manifest) is worse than failing open, so
//   `load_graph_index` treats `Err(_)` the same as "no manifest" -- the same
//   fail-open-on-parse-error convention `graph::read_graph` follows. A
//   manifest present but missing its `entries` key is handled the same way.
//
// - **Insertion-order-preserving scratch structures.** Several of
//   `impact_walk`'s scratch maps/sets are NOT just membership/lookup
//   structures -- their insertion order is directly observable in the final
//   result: `Hit::symbols` feeds `top_symbols`'s first-3 truncation, and
//   `visited` key order feeds the `nodes` array handed to
//   `personalized_page_rank`, whose accumulation loop is float-addition over
//   that exact order -- not associative, so a different iteration order can
//   produce a bit-different (though not wrong) result.
//   `std::collections::{HashMap,HashSet}` make no iteration-order guarantee,
//   so this module has two tiny private order-preserving helpers,
//   `SeqSet`/`SeqMap` (a `Vec` plus an index `HashMap`, first insertion wins
//   the slot). They are pure algorithm scratch state, not artifact structs;
//   `graph::Def`/`Edge`/`Graph` are reused directly from `graph.rs`.
//
// - **`str::cmp` ordering.** Location sorts use plain Unicode-codepoint
//   `str::cmp` rather than locale-aware collation, the same accepted
//   trade-off documented at `resolve.rs`'s `capped_candidates`: the two
//   orderings coincide for every path/id shape this crate actually produces,
//   and diverge only in a pathological mix of leading case or a
//   `+`/`.`-adjacent tie -- flagged, not solved, no new ICU dependency.
//
// - **Result models.** `RefsModel`/`ImpactModel` (and their row/table
//   sub-types) are the typed surface `render.rs` consumes. Field names are
//   `snake_case` (e.g. `model.inbound.uses_type`). Nothing here is
//   `Serialize` -- `--json` output byte-shape is `cli.rs`'s job, not this
//   module's.
//
// Split by verb, plus the shared substrate the verbs sit on: `seq` (the
// order-preserving scratch structures), `index` (`GraphIndex` and its
// loading, plus `def_files`/`def_sites`/`symbol_refs`), `symbol`
// (`resolve_symbol`'s ladder), and `rank` (personalized PageRank). The verbs
// themselves: `find`, `refs`/`refs_tables` (the row/table shaping `read`
// reuses), `read`, `coverage` (test coverage for a symbol -- named to avoid
// colliding with this module's own `tests`), and `impact` (plus `infra`, the
// hub-file name-pattern classification `impact` widens against). Every
// public item keeps the path it had before the split via the `pub use`s
// below.

mod coverage;
mod find;
mod impact;
mod index;
mod infra;
// `--json` rendering of every query model, moved here from `cli.rs` to keep
// that file under its size ratchet; `pub(crate)` so `cli.rs` can still call
// it as `query::json::...`.
pub(crate) mod json;
mod member;
mod outcome;
mod rank;
mod read;
mod refs;
mod refs_tables;
mod seq;
mod symbol;

pub use coverage::{build_tests_model, TestRow, TestVia, TestsModel, TestsResult};
pub use find::{file_inbound_counts, find_names, first_decl_line_by_file, name_tier, source_line};
pub use impact::{
    build_impact_model, impact_walk, looks_like_file_path, resolve_impact_seed, BrakedFile,
    BrakedIface, ImpactModel, ImpactResult, ImpactRow, ImpactWalkResult, KindLines, SeedKind,
    SeedResolution, VisitedEntry, DEFAULT_HOPS, DEFAULT_IFACE_MAX_FANIN,
};
pub use index::{
    def_files, def_sites, load_graph_index, load_graph_index_with, symbol_refs, DefSite,
    GraphIndex, HeuristicEntry, InboundEntry, IndexOptions, OutboundEntry, SymbolRefs,
};
pub use infra::{is_infra_file, DEFAULT_HUB_MAX_INDEGREE};
pub use member::{
    qualified_member_owners, qualified_seed, resolve_member_seed, MemberCandidate,
    MemberSeedResolution,
};
pub use outcome::Outcome;
pub use rank::{personalized_page_rank, DEFAULT_DAMPING, DEFAULT_ITERATIONS};
pub use read::{build_read_model, ReadModel, ReadResult, ReadSpan};
pub use refs::{build_refs_model, LineCache, MemberRefEntry, MemberRefs, RefsModel, RefsResult};
pub use refs_tables::{
    AmbiguousRow, AmbiguousTables, ImportRow, InboundRow, InboundTables, OutboundRow,
    OutboundTables, Table, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, SOURCE_MAX,
};
pub use seq::{SeqMap, SeqSet};
pub use symbol::{resolve_symbol, Resolution};

#[cfg(test)]
mod tests;
