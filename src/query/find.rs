use std::collections::HashMap;
use std::path::Path;

use crate::graph;
use crate::suggest::kind_rank;

use super::refs::trim_source;

// ============================================================================
// The full name index, and the source line a hit points at.
// ============================================================================

/// The literal line a declaration sits on, ASCII-trimmed by `trim_source`.
/// Returns `""` on an unreadable file or an out-of-range line; the caller then
/// prints `file:line` with nothing after it rather than a row ending in a
/// dangling separator.
pub fn source_line(root: &Path, file: &str, line: usize) -> String {
    let Ok(body) = std::fs::read_to_string(root.join(file)) else {
        return String::new();
    };
    let lines: Vec<&str> = body.split('\n').collect();
    if line < 1 || line > lines.len() {
        return String::new();
    }
    let mut raw = lines[line - 1];
    // A UTF-8 BOM survives the read and is not in the trim set, so line 1 of a
    // BOM'd file would otherwise print it.
    if line == 1 {
        raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    }
    trim_source(raw).to_string()
}

/// Name search: every whitespace-split token of `query` has to appear,
/// case-insensitively, as a substring of the declared NAME -- path and purpose
/// are the manifest search's haystack, not this one's. The caller owns the
/// graph (see the module header's two-phase note). Results come back in index
/// build order; ranking is the caller's job.
pub fn find_names<'g>(graph: &'g graph::Graph, query: &str) -> Vec<&'g graph::GraphName> {
    let tokens: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    graph
        .names
        .iter()
        .filter(|n| {
            let hay = n.name.to_lowercase();
            tokens.iter().all(|t| hay.contains(t.as_str()))
        })
        .collect()
}

/// Every FILE's own first declaration line, keyed by path: the minimum `line`
/// across every `graph.names` entry that file carries, over the WHOLE index.
/// Unlike [`find_names`], this is never filtered to one query's matches,
/// because a manifest-pool row needs a line to open regardless of the query. A
/// file the name index carries nothing for (no declared symbol at all -- a
/// config file, a doc, an asset) has no entry here; the caller falls back to
/// line 1.
pub fn first_decl_line_by_file(graph: &graph::Graph) -> HashMap<String, usize> {
    let mut lines: HashMap<String, usize> = HashMap::new();
    for n in &graph.names {
        lines
            .entry(n.file.clone())
            .and_modify(|line| *line = (*line).min(n.line))
            .or_insert(n.line);
    }
    lines
}

/// The per-file precise inbound-edge count `find`'s tie-break ranks by.
///
/// File path -> how many PRECISE reference edges land on the definitions that
/// file declares, keyed by the edge's target file. This direct edge count IS
/// the whole centrality measure behind `find`'s tie-break -- it deliberately
/// shares nothing with `impact_walk`'s weighting (no hops, no `PageRank`, no
/// distinct-referrer folding): one precise edge, one count.
///
/// Counted: `inherits`/`uses-type`/`uses-member` without the guess tag, plus
/// the three TS reference kinds `call`/`jsx-use`/`dispatch`, which on a TS repo
/// ARE the reference graph -- leaving them out would rank every TS file at
/// zero. NOT counted: heuristic edges (a guess never enters a table a fact is
/// read from), `imports`/`import` (they name a namespace or module, never a
/// definition), `ctor-di` (DI wiring, not a reference), `ambiguous` (candidates,
/// not resolutions), and a file's references to ITSELF (`from_file ==
/// to_file`) -- how much OTHER code pulls on a file is the measure, so a
/// self-citation is no more centrality here than it is for `hub_indegree`.
pub fn file_inbound_counts(graph: &graph::Graph) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in &graph.edges {
        let (from_file, to_file) = match e {
            graph::Edge::Inherits {
                from_file,
                to_file,
                heuristic,
                ..
            }
            | graph::Edge::UsesType {
                from_file,
                to_file,
                heuristic,
                ..
            }
            | graph::Edge::UsesMember {
                from_file,
                to_file,
                heuristic,
                ..
            } => {
                if *heuristic {
                    continue;
                }
                (from_file, to_file)
            }
            graph::Edge::Call {
                from_file, to_file, ..
            }
            | graph::Edge::JsxUse {
                from_file, to_file, ..
            }
            | graph::Edge::Dispatch {
                from_file, to_file, ..
            } => (from_file, to_file),
            // Listed rather than caught by a wildcard so a future edge kind
            // still fails the exhaustiveness check here. `implements`/
            // `overrides` sit out for the same reason `ctor-di` does: DI/
            // dispatch wiring, not an ordinary reference this centrality
            // measure counts.
            graph::Edge::Imports { .. }
            | graph::Edge::Import { .. }
            | graph::Edge::CtorDi { .. }
            | graph::Edge::Implements { .. }
            | graph::Edge::Overrides { .. }
            | graph::Edge::Ambiguous { .. } => continue,
        };
        if from_file == to_file {
            continue;
        }
        *counts.entry(to_file.clone()).or_insert(0) += 1;
    }
    counts
}

/// Collapses `suggest::kind_rank` into three tiers for `find`'s default
/// view: tier 1 is a code symbol (`kind_rank` 0 -- a type or top-level
/// declaration -- or 1 -- a member), tier 2 is a markup or binding name
/// (`kind_rank` 2), tier 3 is a resource key (`kind_rank` 3). `find` matches
/// tier 1 then 2 by default and demotes tier 3 to a one-line trailer
/// (`--resources` includes it).
pub fn name_tier(kind: &str) -> u8 {
    let rank = kind_rank(kind);
    if rank <= 1 {
        1
    } else {
        rank
    }
}
