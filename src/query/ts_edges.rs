// The TypeScript-family edge kinds in the query index: `call`, `jsx-use`,
// `dispatch` and `import`, each kept in its own list so `refs`/`read` render
// them as their own tables and `impact` walks them like every other precise
// kind. Admission is by edge kind, never by the source file's language, so a
// JavaScript caller answers too.

use std::collections::HashMap;

use crate::graph;

use super::index::{InboundEntry, OutboundEntry};

/// One def's (inbound) or one file's (outbound) TS edges, by kind: indices
/// into `graph.edges`, in graph order.
#[derive(Debug, Clone, Default)]
pub struct TsKinds {
    /// `call` edges.
    pub call: Vec<usize>,
    /// `jsx-use` edges.
    pub jsx_use: Vec<usize>,
    /// `dispatch` edges.
    pub dispatch: Vec<usize>,
    /// `import` edges that bind at least one definition.
    pub import: Vec<usize>,
}

impl TsKinds {
    /// The four lists in their fixed order: `call`, `jsx-use`, `dispatch`,
    /// `import`.
    pub fn lists(&self) -> [&Vec<usize>; 4] {
        [&self.call, &self.jsx_use, &self.dispatch, &self.import]
    }

    pub(super) fn extend_from(&mut self, other: &TsKinds) {
        self.call.extend(other.call.iter().copied());
        self.jsx_use.extend(other.jsx_use.iter().copied());
        self.dispatch.extend(other.dispatch.iter().copied());
        self.import.extend(other.import.iter().copied());
    }
}

// The parts of an admitted TS edge the adjacency needs: where it starts,
// the file it names, the definitions it is keyed under inbound, and which of
// the four lists it joins.
struct Admitted<'e> {
    from_file: &'e String,
    to_file: &'e String,
    targets: Vec<&'e String>,
    list: fn(&mut TsKinds) -> &mut Vec<usize>,
}

fn admitted(e: &graph::Edge) -> Option<Admitted<'_>> {
    let (from_file, to_file, targets, list): (_, _, _, fn(&mut TsKinds) -> &mut Vec<usize>) =
        match e {
            graph::Edge::Call {
                from_file,
                to,
                to_file,
                ..
            } => (from_file, to_file, vec![to], |k| &mut k.call),
            graph::Edge::JsxUse {
                from_file,
                to,
                to_file,
                ..
            } => (from_file, to_file, vec![to], |k| &mut k.jsx_use),
            graph::Edge::Dispatch {
                from_file,
                to,
                to_file,
                ..
            } => (from_file, to_file, vec![to], |k| &mut k.dispatch),
            graph::Edge::Import {
                from_file,
                to_file,
                bound_defs,
                ..
            } if !bound_defs.is_empty() => (from_file, to_file, bound_defs.iter().collect(), |k| {
                &mut k.import
            }),
            _ => return None,
        };
    Some(Admitted {
        from_file,
        to_file,
        targets,
        list,
    })
}

/// The def ids an admitted TS edge names, or `None` for a kind the adjacency
/// does not admit (a non-TS kind, and an `import` edge binding no definition).
/// `record_ts_edge` keys its inbound entries by exactly these targets, so a
/// caller that wants them without pushing an edge into a list reads them here
/// rather than reaching into `Admitted` for the one field it needs.
pub(super) fn admitted_targets(e: &graph::Edge) -> Option<Vec<&str>> {
    admitted(e).map(|a| a.targets.iter().map(|t| t.as_str()).collect())
}

/// Admit TS edge `e` (index `i`) into the adjacency: inbound under the def it
/// names -- every bound definition, for an `import` edge -- and outbound under
/// its source file. Returns the `(from_file, to_file)` pair the caller notes
/// for the manifest gap and the hub in-degree, or `None` for an edge that is
/// not admitted: any non-TS kind, and an `import` edge that binds no
/// definition (a namespace, side-effect or `export *` statement), since the
/// inbound adjacency is keyed by definition.
pub(super) fn record_ts_edge<'e>(
    e: &'e graph::Edge,
    i: usize,
    inbound: &mut HashMap<String, InboundEntry>,
    outbound_by_file: &mut HashMap<String, OutboundEntry>,
) -> Option<(&'e str, &'e str)> {
    let edge = admitted(e)?;
    for to in edge.targets {
        (edge.list)(&mut inbound.entry(to.clone()).or_default().ts).push(i);
    }
    (edge.list)(
        &mut outbound_by_file
            .entry(edge.from_file.clone())
            .or_default()
            .ts,
    )
    .push(i);
    Some((edge.from_file.as_str(), edge.to_file.as_str()))
}
