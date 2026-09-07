use std::collections::HashMap;

use crate::graph;

use super::index::{def_files, symbol_refs, GraphIndex};
use super::refs_tables::{edge_loc, row_tier};
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
}

/// The outcome of a `tests` query: resolved, ambiguous, or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum TestsResult {
    /// Represents `Resolved`.
    Resolved(TestsModel),
    /// Represents `Ambiguous`.
    Ambiguous(Vec<String>),
    /// Represents `NotFound`.
    NotFound,
}

// The three inbound kinds of one adjacency, walked in `REF_KINDS` order.
fn collect_test_rows(index: &GraphIndex, kinds: [&[usize]; 3], heuristic: bool) -> Vec<TestRow> {
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
        Resolution::NotFound => return TestsResult::NotFound,
    };
    let refs = symbol_refs(index, &id);

    let precise = collect_test_rows(
        index,
        [
            &refs.inbound_inherits,
            &refs.inbound_uses_type,
            &refs.inbound_uses_member,
        ],
        false,
    );
    let heuristic = collect_test_rows(
        index,
        [
            &refs.heuristic_inbound_inherits,
            &refs.heuristic_inbound_uses_type,
            &refs.heuristic_inbound_uses_member,
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
    })
}
