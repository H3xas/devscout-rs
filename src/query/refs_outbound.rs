// The `--out` half of `refs`: the outbound tables of one resolved symbol,
// ranked and capped together. Split out of `refs.rs` to keep that file under
// this crate's per-file line budget.

use std::collections::HashMap;
use std::path::Path;

use crate::graph;

use super::dispatch::{
    outbound_foreign, push_ranked, RankedOutbound, K_IMPLEMENTS, K_IMPORTS, K_INHERITS,
    K_OVERRIDES, K_USES_MEMBER, K_USES_TYPE,
};
use super::occurrence::tag_outbound_rows;
use super::refs::{hit_source, LineCache};
use super::refs_tables::{
    cap_rows, edge_loc, loc_cmp, row_tier, ImportRow, OutboundRow, OutboundTables, Table,
};
use super::symbol_refs::SymbolRefs;

// The six outbound kinds share ONE cap and ONE ranking under `--out`,
// mirroring `build_refs_model`'s own inbound block in `refs.rs`: resolved before
// heuristic, then the def's own project before every other (imports always
// foreign, per `outbound_foreign` above), then file, then line. Each shown hit
// carries the same trimmed source line an inbound hit does, read at the SAME
// `from_line` an inbound row reads -- for an outbound edge that is a line in
// the def's own file, the site actually making the reference, not the caller's.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered ranking pass over all six outbound kinds, sharing the same cap and tie-break rule across them"
)]
pub(super) fn build_outbound_tables(
    refs: &SymbolRefs,
    def_project: &str,
    edges: &[graph::Edge],
    root: &Path,
    outbound_cap: usize,
) -> OutboundTables {
    const EMPTY: &[usize] = &[];
    let mut ranked: Vec<RankedOutbound> = Vec::new();
    push_ranked(
        &mut ranked,
        K_INHERITS,
        &refs.outbound_inherits,
        &refs.heuristic_outbound_inherits,
    );
    push_ranked(
        &mut ranked,
        K_USES_TYPE,
        &refs.outbound_uses_type,
        &refs.heuristic_outbound_uses_type,
    );
    push_ranked(
        &mut ranked,
        K_USES_MEMBER,
        &refs.outbound_uses_member,
        &refs.heuristic_outbound_uses_member,
    );
    push_ranked(&mut ranked, K_IMPLEMENTS, &refs.outbound_implements, EMPTY);
    push_ranked(&mut ranked, K_OVERRIDES, &refs.outbound_overrides, EMPTY);
    push_ranked(&mut ranked, K_IMPORTS, &refs.outbound_imports, EMPTY);

    let mut totals = [0usize; 6];
    for r in &ranked {
        totals[r.kind] += 1;
    }
    ranked.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| {
                outbound_foreign(def_project, edges, a).cmp(&outbound_foreign(
                    def_project,
                    edges,
                    b,
                ))
            })
            .then_with(|| loc_cmp(&edges[a.edge], &edges[b.edge]))
    });
    let (shown, _) = cap_rows(ranked, outbound_cap);

    let mut source_cache: LineCache = HashMap::new();
    let mut rows: [Vec<OutboundRow>; 5] = Default::default();
    let mut imports = Vec::new();
    for r in shown {
        let (file, line) = edge_loc(&edges[r.edge]);
        let source = hit_source(root, file, line, &mut source_cache);
        if r.kind == K_IMPORTS {
            let graph::Edge::Imports { target, .. } = &edges[r.edge] else {
                unreachable!("outbound kind K_IMPORTS only ever holds imports edge indices");
            };
            imports.push(ImportRow {
                file: file.to_string(),
                line,
                target: target.clone(),
                source,
            });
            continue;
        }
        let (to_file, to) = match &edges[r.edge] {
            graph::Edge::Inherits { to_file, to, .. }
            | graph::Edge::UsesType { to_file, to, .. }
            | graph::Edge::UsesMember { to_file, to, .. }
            | graph::Edge::Implements { to_file, to, .. }
            | graph::Edge::Overrides { to_file, to, .. } => (to_file.clone(), to.clone()),
            _ => unreachable!("outbound ranked kinds other than K_IMPORTS only ever hold inherits/uses-type/uses-member/implements/overrides edge indices"),
        };
        let row = OutboundRow {
            file: file.to_string(),
            line,
            to_file,
            to,
            heuristic: r.heuristic,
            tier: row_tier(
                r.heuristic,
                edges[r.edge].tier() == Some(graph::HeuristicTier::Ext),
            ),
            source,
            occurrence_index: None,
        };
        rows[r.kind].push(row);
    }
    let [mut inherits, mut uses_type, mut uses_member, mut implements, mut overrides] = rows;
    // Same per-kind, post-cap tagging rule `build_refs_model_inner`'s inbound
    // side applies; `imports` is excluded -- an imports edge is never a
    // guess and never shares a line with another imports edge the way a
    // `uses-member` call site can, so `ImportRow` carries no such field.
    tag_outbound_rows(&mut inherits);
    tag_outbound_rows(&mut uses_type);
    tag_outbound_rows(&mut uses_member);
    tag_outbound_rows(&mut implements);
    tag_outbound_rows(&mut overrides);

    let table = |kind: usize, rows: Vec<OutboundRow>| Table {
        total: totals[kind],
        dropped: totals[kind] - rows.len(),
        rows,
    };
    OutboundTables {
        inherits: table(K_INHERITS, inherits),
        uses_type: table(K_USES_TYPE, uses_type),
        uses_member: table(K_USES_MEMBER, uses_member),
        implements: table(K_IMPLEMENTS, implements),
        overrides: table(K_OVERRIDES, overrides),
        imports: Table {
            total: totals[K_IMPORTS],
            dropped: totals[K_IMPORTS] - imports.len(),
            rows: imports,
        },
    }
}
