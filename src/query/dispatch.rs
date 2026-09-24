// Query-layer plumbing for `implements`/`overrides`: the index-build-time
// adjacency push (`record_dispatch_edge`) and the bare-member fallback's own
// exact-match half (`member_dispatch_edges` and its row-building helpers).
// Split out of `index.rs`/`refs.rs` to keep both under this crate's
// per-file line budget; see this crate's architecture guide for the
// invariants these edges keep. `inbound_walk_kinds` and `note_dispatch_files`
// are shared with the `bus-hop` kind too -- the seam that lets `impact` cross
// a bus hop and `index.rs`'s own `BusHop` arm push its adjacency with no
// duplicated bookkeeping.

use crate::graph;

use super::index::{def_files, note_file, GraphIndex, InboundEntry, OutboundEntry, SymbolRefs};
use super::refs::project_of;
use super::refs_tables::{loc_cmp, InboundRow, Table};
use super::seq::SeqSet;
use std::collections::{HashMap, HashSet};

// The interface def id(s) a class def's OWN file(s) declare an `inherits` OR
// `implements` edge to, restricted to a def of kind `"interface"` -- a plain
// base class is not part of the interface hop. `implements` joins `inherits`
// here -- a type-level (registration-driven) or member-level `implements`
// edge names an interface the SAME way an ordinary base-list `inherits` edge
// does, and this is the one place both widen the interface hop `impact_walk`
// walks. Reuses `outbound_by_file`'s file-level union rather than
// attributing an edge to one specific def in a multi-type file (an accepted
// imprecision). Insertion order is `def_files(def_id)` order, then each
// file's own inherits-then-implements array order.
pub(super) fn implemented_interfaces(index: &GraphIndex, def_id: &str) -> Vec<String> {
    let mut seen: SeqSet<String> = SeqSet::new();
    for file in def_files(index, def_id) {
        let Some(o) = index.outbound_by_file.get(&file) else {
            continue;
        };
        for &ei in o.inherits.iter().chain(o.implements.iter()) {
            let to = match &index.graph.edges[ei] {
                graph::Edge::Inherits { to, .. } | graph::Edge::Implements { to, .. } => to,
                _ => continue,
            };
            if seen.contains(to) {
                continue;
            }
            if index.def(to).map(|d| d.kind.as_str()) != Some("interface") {
                continue;
            }
            seen.insert(to.clone());
        }
    }
    seen.into_vec()
}

/// The six inbound edge-kind lists `impact`'s reverse walk crosses, in the
/// order it crosses them -- spelled here so the dispatch pair and the
/// bus-hop kind all join that walk in exactly one place.
pub(super) fn inbound_walk_kinds(inb: &InboundEntry) -> [&Vec<usize>; 6] {
    [
        &inb.inherits,
        &inb.uses_type,
        &inb.uses_member,
        &inb.implements,
        &inb.overrides,
        &inb.bus_hop,
    ]
}

/// The `note_file`/`hub_referrers_by_file` bookkeeping shared by every
/// dispatch-shaped edge kind (`implements`/`overrides`/`bus-hop`) before its
/// own adjacency push: both ends flagged against the manifest, then the
/// cross-file hub count -- pulled out once here so `index.rs`'s own
/// per-kind arms stay one call each rather than three copies of the same
/// eight lines.
pub(super) fn note_dispatch_files(
    flagged_files: &mut HashSet<String>,
    manifest_paths: Option<&HashSet<String>>,
    hub_referrers_by_file: &mut HashMap<String, HashSet<String>>,
    from_file: &str,
    to_file: &str,
) {
    note_file(flagged_files, manifest_paths, from_file);
    note_file(flagged_files, manifest_paths, to_file);
    if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
        hub_referrers_by_file
            .entry(to_file.to_string())
            .or_default()
            .insert(from_file.to_string());
    }
}

/// Pushes edge `i` into both `outbound_by_file[from_file]` and
/// `inbound[to]`'s `implements` or `overrides` field, whichever kind `e`
/// is. The caller has already confirmed `e` is one of the two kinds and
/// that `--no-dispatch` admits it.
pub(super) fn record_dispatch_edge(
    e: &graph::Edge,
    i: usize,
    from_file: &str,
    to: &str,
    outbound_by_file: &mut HashMap<String, OutboundEntry>,
    inbound: &mut HashMap<String, InboundEntry>,
) {
    let out_entry = outbound_by_file.entry(from_file.to_string()).or_default();
    let in_entry = inbound.entry(to.to_string()).or_default();
    match e {
        graph::Edge::Implements { .. } => {
            out_entry.implements.push(i);
            in_entry.implements.push(i);
        }
        graph::Edge::Overrides { .. } => {
            out_entry.overrides.push(i);
            in_entry.overrides.push(i);
        }
        _ => unreachable!("record_dispatch_edge only ever receives Implements/Overrides edges"),
    }
}

// The interface-member half of the bare-member fallback: `implements`/
// `overrides` edges naming `name` exactly, in declaration order. Unlike the
// `uses-member` half `refs.rs` itself still builds, no `line_has_token`
// verification is needed -- the edge's own `member` field already names the
// satisfied member precisely, so a textual check would only add a
// false-negative risk on a line that happens not to spell the name out (the
// implementing type's own declaration line, not a call site).
pub(super) fn member_dispatch_edges(
    refs: &SymbolRefs,
    edges: &[graph::Edge],
    name: &str,
) -> Vec<usize> {
    let named = |e: usize| matches!(&edges[e], graph::Edge::Implements { member, .. } | graph::Edge::Overrides { member, .. } if member.as_deref() == Some(name));
    let mut kept: Vec<usize> = refs
        .inbound_implements
        .iter()
        .chain(refs.inbound_overrides.iter())
        .copied()
        .filter(|&e| named(e))
        .collect();
    kept.sort_by(|&a, &b| loc_cmp(&edges[a], &edges[b]));
    kept
}

// Never capped and never dropped -- structural facts, not usage call sites,
// so there is no budget to share with `uses-member`'s own cap.
pub(super) fn dispatch_table(rows: Vec<InboundRow>) -> Table<InboundRow> {
    Table {
        total: rows.len(),
        dropped: 0,
        rows,
    }
}

// Splits `member_dispatch_edges`' combined, already-sorted list back into its
// two kinds -- one pass over the edges, each row built the same way an
// ordinary inbound row is.
pub(super) fn split_dispatch_rows(
    edges: &[graph::Edge],
    dispatch: &[usize],
    mut row_of: impl FnMut(usize) -> InboundRow,
) -> (Vec<InboundRow>, Vec<InboundRow>) {
    let mut implements_rows = Vec::new();
    let mut overrides_rows = Vec::new();
    for &e in dispatch {
        let row = row_of(e);
        match &edges[e] {
            graph::Edge::Implements { .. } => implements_rows.push(row),
            graph::Edge::Overrides { .. } => overrides_rows.push(row),
            _ => unreachable!(
                "member_dispatch_edges only ever returns implements/overrides edge indices"
            ),
        }
    }
    (implements_rows, overrides_rows)
}

/// The kind indices `RankedOutbound` and the inbound ranking array carry,
/// fixed across both so `outbound_foreign`'s own kind test stays meaningful.
pub(super) const K_INHERITS: usize = 0;
/// The kind index for `uses-type`.
pub(super) const K_USES_TYPE: usize = 1;
/// The kind index for `uses-member`.
pub(super) const K_USES_MEMBER: usize = 2;
/// The kind index for `implements`.
pub(super) const K_IMPLEMENTS: usize = 3;
/// The kind index for `overrides`.
pub(super) const K_OVERRIDES: usize = 4;
/// The kind index for `imports`, last because it is the one kind that names a
/// namespace rather than a file.
pub(super) const K_IMPORTS: usize = 5;

/// One outbound edge awaiting the global cap: which kind's table it belongs
/// to (`K_INHERITS`..`K_IMPORTS`), which edge it is, and whether it was
/// guessed. `imports`, `implements` and `overrides` are never guesses -- the
/// builder never marks one heuristic, by construction.
pub(super) struct RankedOutbound {
    /// The kind index this edge's table is keyed by.
    pub kind: usize,
    /// The edge's index into the graph's edge list.
    pub edge: usize,
    /// Whether the edge declares itself a guess.
    pub heuristic: bool,
}

// The five ref kinds name a `to_file` -- ranked same-project/foreign against
// it, exactly as an inbound edge ranks its `from_file`. An imports edge names a
// namespace string, never a file, so nothing proves it shares the def's own
// project: it never earns the same-project rank and always sorts as foreign,
// the never-guess rule applied to ranking rather than to resolution.
pub(super) fn outbound_foreign(
    def_project: &str,
    edges: &[graph::Edge],
    r: &RankedOutbound,
) -> usize {
    if r.kind == K_IMPORTS {
        return 1;
    }
    let to_file = match &edges[r.edge] {
        graph::Edge::Inherits { to_file, .. }
        | graph::Edge::UsesType { to_file, .. }
        | graph::Edge::UsesMember { to_file, .. }
        | graph::Edge::Implements { to_file, .. }
        | graph::Edge::Overrides { to_file, .. } => to_file.as_str(),
        _ => unreachable!(
            "outbound ranked kinds other than imports only ever hold inherits/uses-type/uses-member/implements/overrides edge indices"
        ),
    };
    usize::from(project_of(to_file) != def_project)
}

/// Appends one kind's precise hits then its guessed ones, in that order --
/// the order the ranking pass's own `heuristic` tie-break then re-establishes
/// across kinds.
pub(super) fn push_ranked(
    ranked: &mut Vec<RankedOutbound>,
    kind: usize,
    precise: &[usize],
    heuristic: &[usize],
) {
    for &e in precise {
        ranked.push(RankedOutbound {
            kind,
            edge: e,
            heuristic: false,
        });
    }
    for &e in heuristic {
        ranked.push(RankedOutbound {
            kind,
            edge: e,
            heuristic: true,
        });
    }
}
