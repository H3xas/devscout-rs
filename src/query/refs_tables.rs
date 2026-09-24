use crate::graph;

// The three reference kinds and their fixed order. Not iterated generically
// here -- each kind gets its own struct field (`inherits`/`uses_type`/
// `uses_member`) throughout this module rather than a keyed collection. This
// constant exists only as an anchor for the kind list and its order.
#[allow(dead_code)]
const REF_KINDS: [&str; 3] = ["inherits", "uses-type", "uses-member"];

/// Default per-table row cap for the outbound and ambiguous tables. The three
/// inbound tables share [`INBOUND_CAP`] instead.
pub const DEFAULT_CAP: usize = 50;
/// Default cap shared across all three inbound kinds -- one cap total, not one
/// per table.
pub const INBOUND_CAP: usize = 30;
/// Default cap for the `--out` view: one cap shared across all four outbound
/// kinds (inherits/uses-type/uses-member/imports), the mirror of
/// [`INBOUND_CAP`]. `--all` lifts this cap.
pub const OUTBOUND_CAP: usize = 30;
/// Maximum length of a displayed source line, in UTF-16 code units.
/// Truncation counts code units (see `clip_source`) so the cut is stable
/// regardless of any astral characters on the line.
pub const SOURCE_MAX: usize = 120;
// ============================================================================
// build_refs_model.
// ============================================================================

pub(super) fn cap_rows<T>(mut rows: Vec<T>, cap: usize) -> (Vec<T>, usize) {
    if rows.len() <= cap {
        (rows, 0)
    } else {
        let dropped = rows.len() - cap;
        rows.truncate(cap);
        (rows, dropped)
    }
}

// The (from_file, from_line) an edge originates at, the key location sorts use.
// See the module header re: `str::cmp` ordering.
pub(super) fn edge_loc(e: &graph::Edge) -> (&str, usize) {
    match e {
        graph::Edge::Inherits { from_file, from_line, .. }
        | graph::Edge::UsesType { from_file, from_line, .. }
        | graph::Edge::UsesMember { from_file, from_line, .. }
        | graph::Edge::Implements { from_file, from_line, .. }
        | graph::Edge::Overrides { from_file, from_line, .. }
        | graph::Edge::BusHop { from_file, from_line, .. }
        | graph::Edge::Imports { from_file, from_line, .. }
        | graph::Edge::Ambiguous { from_file, from_line, .. }
        // Never actually reached: a 'ctor-di' edge is never pushed into any
        // structure this helper sorts (see the query-index builder's own
        // CtorDi arm). Included only for exhaustiveness, as are the four
        // TS/TSX kinds below, for the same reason.
        | graph::Edge::CtorDi { from_file, from_line, .. }
        | graph::Edge::Import { from_file, from_line, .. }
        | graph::Edge::Call { from_file, from_line, .. }
        | graph::Edge::JsxUse { from_file, from_line, .. }
        | graph::Edge::Dispatch { from_file, from_line, .. } => (from_file.as_str(), *from_line),
    }
}

pub(super) fn loc_cmp(a: &graph::Edge, b: &graph::Edge) -> std::cmp::Ordering {
    let (af, al) = edge_loc(a);
    let (bf, bl) = edge_loc(b);
    if af == bf {
        al.cmp(&bl)
    } else {
        af.cmp(bf)
    }
}

/// Which tier a ROW reports, folded from the tiers of the edges behind it.
///
/// One extension edge is enough to call the whole row an extension: that tier
/// ran C#'s own lookup rule and only failed to check the receiver, so it is the
/// stronger of the two and a name guess sitting beside it does not weaken it.
/// Everything else heuristic is a guess, an edge carrying NO tier included -- a
/// graph written before schema 2 cannot prove it was anything better, and
/// reporting an unproven edge as the stronger tier is the one direction this
/// surface must never round.
pub(super) fn row_tier(heuristic: bool, ext_seen: bool) -> Option<graph::HeuristicTier> {
    if !heuristic {
        return None;
    }
    Some(if ext_seen {
        graph::HeuristicTier::Ext
    } else {
        graph::HeuristicTier::Guess
    })
}

/// One inbound-table row: `file` and `line` of the referencing site, then
/// `heuristic` (whether the edge was guessed), `tier` (which guess tier said
/// so) and `source` (the trimmed referencing line). An empty `source` is
/// omitted from `--json`. `occurrence_index` distinguishes two rows that would
/// otherwise be equal on every other field (see the `occurrence` module);
/// it is `None` on every row outside such a collision.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundRow {
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    /// The heuristic value.
    pub heuristic: bool,
    /// Which heuristic tier stands behind this row; `None` exactly when
    /// `heuristic` is false (see [`row_tier`]).
    pub tier: Option<graph::HeuristicTier>,
    /// The source value.
    pub source: String,
    /// A 0-based position among rows of this same table that are otherwise
    /// identical, assigned in the table's existing stable emission order;
    /// `None` for a row with no such collision. Scoped to one table of one
    /// answer against one graph snapshot -- never a cross-query or
    /// cross-snapshot identity. See the `occurrence` module.
    pub occurrence_index: Option<usize>,
}

/// One outbound-table row: `file`/`line` of the referencing site, `to_file`/`to`
/// of the target, then `heuristic` and `source` (same omit-when-empty rule as
/// [`InboundRow`]). `source` is read at the site actually making the reference
/// -- for an outbound edge that is a line in the def's own file, not the
/// target's.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundRow {
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    /// The to file value.
    pub to_file: String,
    /// The to value.
    pub to: String,
    /// The heuristic value.
    pub heuristic: bool,
    /// Which heuristic tier stands behind this row, same rule as
    /// [`InboundRow::tier`].
    pub tier: Option<graph::HeuristicTier>,
    /// The source value.
    pub source: String,
    /// Same rule as [`InboundRow::occurrence_index`], scoped to this table.
    pub occurrence_index: Option<usize>,
}

/// One imports-table row: `file`/`line`, the imported `target` namespace, and
/// `source`. An imports edge is never a guess, so unlike [`OutboundRow`] this
/// carries no `heuristic` field.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportRow {
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    /// The target value.
    pub target: String,
    /// The source value.
    pub source: String,
}

/// One ambiguous-table row: the referencing `file`/`line`, the `origin` and
/// `raw` text of the reference, and how many candidates it matched.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousRow {
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    /// The origin value.
    pub origin: String,
    /// The raw value.
    pub raw: String,
    /// The candidate count value.
    pub candidate_count: usize,
}

/// A capped table: `total` rows before capping, how many were `dropped`, and
/// the surviving `rows`.
#[derive(Debug, Clone, PartialEq)]
pub struct Table<R> {
    /// The total value.
    pub total: usize,
    /// The dropped value.
    pub dropped: usize,
    /// The rows value.
    pub rows: Vec<R>,
}

pub(super) fn build_table<R>(
    mut idxs: Vec<usize>,
    edges: &[graph::Edge],
    cap: usize,
    map_row: fn(&graph::Edge) -> R,
) -> Table<R> {
    idxs.sort_by(|&a, &b| loc_cmp(&edges[a], &edges[b]));
    let total = idxs.len();
    let (shown, dropped) = cap_rows(idxs, cap);
    let rows = shown.into_iter().map(|i| map_row(&edges[i])).collect();
    Table {
        total,
        dropped,
        rows,
    }
}

// The per-table, per-kind outbound builder was removed here: the outbound
// tables now go through `build_outbound_tables`'s shared cap and rank instead
// (below `RankedOutbound`), the same way the inbound tables stopped going
// through a per-kind builder. `build_table` above still backs the two
// ambiguous tables.

pub(super) fn ambiguous_row(e: &graph::Edge) -> AmbiguousRow {
    match e {
        graph::Edge::Ambiguous {
            origin,
            from_file,
            from_line,
            raw,
            candidate_count,
            ..
        } => AmbiguousRow {
            file: from_file.clone(),
            line: *from_line,
            origin: origin.clone(),
            raw: raw.clone(),
            candidate_count: *candidate_count,
        },
        _ => unreachable!("ambiguous table only ever holds ambiguous edge indices"),
    }
}

/// The five inbound tables, one per kind.
///
/// `implements`/`overrides` are never heuristic, and a renderer shows
/// either only when its own total is non-zero -- unlike the first three,
/// which always show their header -- so a symbol untouched by dispatch
/// edges renders byte-identical to before this pair existed.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundTables {
    /// The inherits value.
    pub inherits: Table<InboundRow>,
    /// The uses type value.
    pub uses_type: Table<InboundRow>,
    /// The uses member value.
    pub uses_member: Table<InboundRow>,
    /// The implements value.
    pub implements: Table<InboundRow>,
    /// The overrides value.
    pub overrides: Table<InboundRow>,
}

/// The six outbound tables: three by kind, `implements`/`overrides`, plus
/// imports.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundTables {
    /// The inherits value.
    pub inherits: Table<OutboundRow>,
    /// The uses type value.
    pub uses_type: Table<OutboundRow>,
    /// The uses member value.
    pub uses_member: Table<OutboundRow>,
    /// The implements value.
    pub implements: Table<OutboundRow>,
    /// The overrides value.
    pub overrides: Table<OutboundRow>,
    /// The imports value.
    pub imports: Table<ImportRow>,
}

/// The inbound and outbound ambiguous-reference tables.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousTables {
    /// The inbound value.
    pub inbound: Table<AmbiguousRow>,
    /// The outbound value.
    pub outbound: Table<AmbiguousRow>,
}
