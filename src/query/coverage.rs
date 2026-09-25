use std::collections::HashMap;

use crate::graph;

use super::bus::{self, BusDirection, BusHopRow};
use super::index::{def_files, symbol_refs, GraphIndex};
use super::member::{self, MemberCandidate, MemberSeedResolution};
use super::refs_tables::{cap_rows, edge_loc, row_tier, Table, DEFAULT_CAP};
use super::symbol::{resolve_symbol, Resolution};

// ============================================================================
// build_tests_model -- test coverage.
// ============================================================================

/// Which vouch earned a row in a tests model: an attribute-carrying def
/// declared in the file (`Attribute`), or, absent that, the project model
/// placing the file's unit inside a project marked `test` (`Project`).
///
/// The two are checked in that order -- a file's own attributed def is always
/// the more specific vouch, so `Attribute` wins whenever both would apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestVia {
    /// A def in the file carries a test-runner attribute.
    Attribute,
    /// No such def; the file's unit in the project model is a test project.
    Project,
}

/// One row of a tests model: a test `file`, the test-carrying `test_defs` in
/// it (possibly empty for a `Project`-vouched row -- a harness file in a test
/// project need not declare any attributed method itself), the `lines` at
/// which it references the symbol, the `ref_count`, whether the reference was
/// guessed (`heuristic`), and which vouch (`via`) put the file in this model
/// at all.
#[derive(Debug, Clone, PartialEq)]
pub struct TestRow {
    /// The file value.
    pub file: String,
    /// The test defs value.
    pub test_defs: Vec<String>,
    /// The lines value.
    pub lines: Vec<usize>,
    /// The ref count value.
    pub ref_count: usize,
    /// The heuristic value.
    pub heuristic: bool,
    /// Which heuristic tier stands behind this FILE's references. A row folds
    /// every reference the file makes, so one extension edge among them names
    /// the whole row -- same rule as [`InboundRow::tier`], applied to a set.
    pub tier: Option<graph::HeuristicTier>,
    /// Which vouch (attribute or project) put this file in the model.
    pub via: TestVia,
}

/// The resolved `tests` result for one symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct TestsModel {
    /// The query value.
    pub query: String,
    /// The symbol value.
    pub symbol: String,
    /// The def files value.
    pub def_files: Vec<String>,
    /// Precise rows first, heuristic rows after -- the same discipline every
    /// other consumer keeps, so a guess never sits inside the list of facts.
    pub rows: Vec<TestRow>,
    /// The test file count value.
    pub test_file_count: usize,
    /// The ref count value.
    pub ref_count: usize,
    /// The heuristic file count value.
    pub heuristic_file_count: usize,
    /// The heuristic ref count value.
    pub heuristic_ref_count: usize,
    /// Test files that PUBLISH a message this symbol (a handler) receives,
    /// over a `bus-hop` -- a possible route, runtime routing unverified,
    /// never counted in `test_file_count`/`ref_count` or their heuristic
    /// twins: a possible route must never inflate precise coverage.
    /// Structurally separate from `rows` by construction -- a different
    /// field entirely, not a flag on an existing row -- so nothing has to
    /// remember to exclude it. `--json` omits the key entirely when empty.
    pub bus: Table<BusHopRow>,
}

/// The outcome of a `tests` query: resolved, ambiguous, or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum TestsResult {
    /// Represents `Resolved`.
    Resolved(TestsModel),
    /// Represents `Ambiguous`.
    Ambiguous(Vec<String>),
    /// The seed named a member declared by more than one type -- `tests`
    /// answers as the member's unique declaring type, so this can only ever
    /// arise from a member seed.
    MemberAmbiguous(Vec<MemberCandidate>),
    /// Represents `NotFound`.
    NotFound,
}

// The inbound kinds of one adjacency, walked in `REF_KINDS` order (plus
// `implements`/`overrides`, which never carry a heuristic tier).
fn collect_test_rows(index: &GraphIndex, kinds: [&[usize]; 5], heuristic: bool) -> Vec<TestRow> {
    let mut rows: Vec<TestRow> = Vec::new();
    let mut by_file: HashMap<String, usize> = HashMap::new();
    for idxs in kinds {
        for &i in idxs {
            let (from_file, from_line) = edge_loc(&index.graph.edges[i]);
            let test_defs = index.test_defs_by_file.get(from_file);
            // The attribute vouch wins when it is there. Absent it, the file
            // still earns a row -- with an empty `test_defs` -- when
            // `is_test_file` vouches for it through the project model alone;
            // a file neither vouch reaches is not a test file and is skipped,
            // same as before this vouch existed.
            let via = if test_defs.is_some() {
                TestVia::Attribute
            } else if index.is_test_file(from_file) {
                TestVia::Project
            } else {
                continue;
            };
            let slot = match by_file.get(from_file) {
                Some(&slot) => slot,
                None => {
                    let slot = rows.len();
                    rows.push(TestRow {
                        file: from_file.to_string(),
                        test_defs: test_defs
                            .map(|defs| {
                                defs.iter()
                                    .map(|&d| index.graph.defs[d].id.clone())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        lines: Vec::new(),
                        ref_count: 0,
                        heuristic,
                        tier: row_tier(heuristic, false),
                        via,
                    });
                    by_file.insert(from_file.to_string(), slot);
                    slot
                }
            };
            // A row folds every reference ONE file makes, so its tier can only
            // ever be raised as the rest of that file's edges arrive: the first
            // extension edge names the row, and no later name guess takes that
            // back (see `row_tier`).
            if index.graph.edges[i].tier() == Some(graph::HeuristicTier::Ext) {
                rows[slot].tier = row_tier(heuristic, true);
            }
            rows[slot].lines.push(from_line);
            rows[slot].ref_count += 1;
        }
    }
    for row in &mut rows {
        row.lines.sort_unstable();
    }
    rows
}

/// Which TEST files reference this symbol, at which lines, via which
/// test-carrying defs. Nothing here is a new kind of edge: it is the same
/// inbound set `refs` reports, filtered to the files `test_defs_by_file`
/// vouches for, which is why a symbol nothing tests answers "none" rather than
/// falling back to a name convention.
///
/// Row order is FIRST-SEEN edge order, kind by kind (not sorted), so the file a
/// consumer reads first is the one the graph reached first; the edge array
/// walked is itself built in a pinned order.
pub fn build_tests_model(index: &GraphIndex, query: &str) -> TestsResult {
    let id = match resolve_symbol(index, query) {
        Resolution::Resolved(id) => id,
        Resolution::Ambiguous(ids) => return TestsResult::Ambiguous(ids),
        // The member path is a FALLBACK, reached only when nothing in the
        // graph declares `query` as a type -- same order `refs`/`impact`
        // apply. `tests` has no member-shaped answer of its own: a member
        // seed answers as its unique declaring type, exactly like `impact`.
        Resolution::NotFound => match member::resolve_member_seed(index, query) {
            MemberSeedResolution::Resolved(id) => id,
            MemberSeedResolution::Ambiguous(candidates) => {
                return TestsResult::MemberAmbiguous(candidates)
            }
            MemberSeedResolution::NotFound => return TestsResult::NotFound,
        },
    };
    let refs = symbol_refs(index, &id);

    let precise = collect_test_rows(
        index,
        [
            &refs.inbound_inherits,
            &refs.inbound_uses_type,
            &refs.inbound_uses_member,
            &refs.inbound_implements,
            &refs.inbound_overrides,
        ],
        false,
    );
    const EMPTY: &[usize] = &[];
    let heuristic = collect_test_rows(
        index,
        [
            &refs.heuristic_inbound_inherits,
            &refs.heuristic_inbound_uses_type,
            &refs.heuristic_inbound_uses_member,
            EMPTY,
            EMPTY,
        ],
        true,
    );

    let test_file_count = precise.len();
    let ref_count: usize = precise.iter().map(|r| r.ref_count).sum();
    let heuristic_file_count = heuristic.len();
    let heuristic_ref_count: usize = heuristic.iter().map(|r| r.ref_count).sum();

    let mut rows = precise;
    rows.extend(heuristic);

    TestsResult::Resolved(TestsModel {
        query: query.to_string(),
        symbol: id.clone(),
        def_files: def_files(index, &id),
        rows,
        test_file_count,
        ref_count,
        heuristic_file_count,
        heuristic_ref_count,
        bus: test_bus_rows(index, &id),
    })
}

// Every `bus-hop` row reaching this symbol (a handler) FROM a publish site a
// test file vouches for -- the same `In`-direction half `refs`' own `bus`
// table shows, filtered to the publish site's own file being a test file by
// the SAME rule `collect_test_rows` applies to an ordinary reference
// (`test_defs_by_file`, falling back to `is_test_file`). Reuses
// `query::bus::symbol_bus_rows` rather than a second bus-hop lookup, so a
// route the reverse walk already resolved is never re-derived here; the
// filter runs on the UNCAPPED set (`usize::MAX`) so a large `Out`-direction
// or non-test share of this symbol's own bus rows can never push a
// legitimate test-vouched row out before the filter ever sees it, and the
// cap this table itself reports is applied only after filtering.
fn test_bus_rows(index: &GraphIndex, id: &str) -> Table<BusHopRow> {
    let all = bus::symbol_bus_rows(index, id, usize::MAX);
    let filtered: Vec<BusHopRow> = all
        .rows
        .into_iter()
        .filter(|r| {
            r.direction == BusDirection::In
                && (index.test_defs_by_file.contains_key(&r.file) || index.is_test_file(&r.file))
        })
        .collect();
    let total = filtered.len();
    let (rows, dropped) = cap_rows(filtered, DEFAULT_CAP);
    Table {
        total,
        dropped,
        rows,
    }
}
