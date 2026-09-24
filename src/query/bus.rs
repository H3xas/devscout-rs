// `bus-hop` query-layer plumbing: the index-build-time adjacency push
// (`record_bus_edge`, the one-kind mirror of `dispatch.rs`'s own
// `record_dispatch_edge`), and the provenance row `refs`/`read` render --
// publisher, message, handler, evidence, and which side of the edge the
// queried symbol sits on. `impact`'s own reach comes for free once
// `bus_hop` joins `inbound_walk_kinds` (`dispatch.rs`) and `why.rs` names
// `Why::BusHop`; this module owns nothing impact-specific.

use std::collections::{HashMap, HashSet};

use crate::graph;

use super::index::{def_files, GraphIndex, InboundEntry, OutboundEntry};
use super::refs_tables::{cap_rows, loc_cmp, Table};

/// Which side of a `bus-hop` edge the queried symbol sits on.
///
/// `In` when the symbol is the edge's own handler (`to`), `Out` when one of
/// the symbol's own declaring files is the edge's publish site
/// (`from_file`). Carried on the row itself, not left to an inbound/outbound
/// section to imply -- unlike a plain code reference, a bus-hop row names
/// both ends of the edge regardless of which side the query started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusDirection {
    /// A message reaches the queried symbol.
    In,
    /// The queried symbol's own file publishes a message a handler
    /// elsewhere receives.
    Out,
}

impl BusDirection {
    /// The exact word this direction renders as, in text and `--json` alike.
    pub fn as_str(self) -> &'static str {
        match self {
            BusDirection::In => "in",
            BusDirection::Out => "out",
        }
    }
}

/// One `bus-hop` provenance row.
///
/// The publish site (`file`/`line`), the resolved `message`, the handler
/// (`to`/`to_file`), the evidence word `resolve/bus.rs` established the hop
/// through -- a pass-through, never respelled here, since that module's own
/// word list is the one place any of them is spelled -- and which side of
/// the edge the queried symbol sits on.
#[derive(Debug, Clone, PartialEq)]
pub struct BusHopRow {
    /// The publish site's file.
    pub file: String,
    /// The publish site's line.
    pub line: usize,
    /// The resolved message def id.
    pub message: String,
    /// The handler/consumer def id.
    pub to: String,
    /// The handler's declaring file.
    pub to_file: String,
    /// The evidence word the hop resolved through.
    pub evidence: String,
    /// Which side of the edge the queried symbol sits on.
    pub direction: BusDirection,
    /// How many distinct handlers this row's message reaches across the
    /// whole graph. A message with one handler is a route; a message with
    /// eighty is a shared contract whose every publisher appears to reach
    /// all of them, and a reader cannot tell those apart from the rows
    /// alone. Counted from the edges at query time rather than stored:
    /// the edges already carry it, so persisting it would only be a second
    /// copy that can disagree.
    pub message_handlers: usize,
}

// Distinct handlers per message, over every `bus-hop` edge in the graph.
fn handlers_by_message(edges: &[graph::Edge]) -> HashMap<&str, HashSet<&str>> {
    let mut out: HashMap<&str, HashSet<&str>> = HashMap::new();
    for e in edges {
        if let graph::Edge::BusHop { message, to, .. } = e {
            out.entry(message.as_str()).or_default().insert(to.as_str());
        }
    }
    out
}

fn bus_hop_row(
    e: &graph::Edge,
    direction: BusDirection,
    handlers: &HashMap<&str, HashSet<&str>>,
) -> BusHopRow {
    let graph::Edge::BusHop {
        from_file,
        from_line,
        message,
        to,
        to_file,
        evidence,
    } = e
    else {
        unreachable!("bus_hop_row is only ever called with a BusHop edge index");
    };
    BusHopRow {
        file: from_file.clone(),
        line: *from_line,
        message: message.clone(),
        to: to.clone(),
        to_file: to_file.clone(),
        evidence: evidence.clone(),
        direction,
        message_handlers: handlers.get(message.as_str()).map_or(0, HashSet::len),
    }
}

/// Pushes edge `i` into both `outbound_by_file[from_file]` and
/// `inbound[to]`'s `bus_hop` field -- the one-kind mirror of
/// `dispatch::record_dispatch_edge`.
pub(super) fn record_bus_edge(
    i: usize,
    from_file: &str,
    to: &str,
    outbound_by_file: &mut HashMap<String, OutboundEntry>,
    inbound: &mut HashMap<String, InboundEntry>,
) {
    outbound_by_file
        .entry(from_file.to_string())
        .or_default()
        .bus_hop
        .push(i);
    inbound.entry(to.to_string()).or_default().bus_hop.push(i);
}

/// An empty `bus-hop` table for a member seed, which has none of its own --
/// bus-hop provenance is a TYPE-level fact (a consumer base, a publish
/// site's own message), never a member's.
pub(super) fn empty_bus_table() -> Table<BusHopRow> {
    Table {
        total: 0,
        dropped: 0,
        rows: Vec::new(),
    }
}

/// Every `bus-hop` row for one resolved symbol: inbound (the symbol is the
/// handler) then outbound (one of the symbol's own files is a publish
/// site), each sorted by publish-site location, capped by `cap`. Never
/// ranked against the ordinary inbound/outbound budgets -- a bus-hop row is
/// a structural fact about a cross-cutting wire-up, not a usage call site.
pub(super) fn symbol_bus_rows(index: &GraphIndex, def_id: &str, cap: usize) -> Table<BusHopRow> {
    let edges = &index.graph.edges;
    let mut inbound: Vec<usize> = index
        .inbound
        .get(def_id)
        .map(|e| e.bus_hop.clone())
        .unwrap_or_default();
    inbound.sort_by(|&a, &b| loc_cmp(&edges[a], &edges[b]));
    let mut outbound: Vec<usize> = Vec::new();
    for file in def_files(index, def_id) {
        if let Some(o) = index.outbound_by_file.get(&file) {
            outbound.extend(o.bus_hop.iter().copied());
        }
    }
    outbound.sort_by(|&a, &b| loc_cmp(&edges[a], &edges[b]));
    let handlers = handlers_by_message(edges);
    let combined: Vec<BusHopRow> = inbound
        .iter()
        .map(|&e| bus_hop_row(&edges[e], BusDirection::In, &handlers))
        .chain(
            outbound
                .iter()
                .map(|&e| bus_hop_row(&edges[e], BusDirection::Out, &handlers)),
        )
        .collect();
    let total = combined.len();
    let (rows, dropped) = cap_rows(combined, cap);
    Table {
        total,
        dropped,
        rows,
    }
}
