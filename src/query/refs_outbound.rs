// The `--out` half of `refs`: the outbound tables of one resolved symbol,
// ranked and capped together. Split out of `refs.rs` to keep that file under
// this crate's per-file line budget.

use std::collections::HashMap;
use std::path::Path;

use crate::graph;

use super::dispatch::{
    outbound_foreign, push_ranked, RankedOutbound, K_IMPLEMENTS, K_IMPORTS, K_INHERITS,
    K_OVERRIDES, K_TS_CALL, K_TS_DISPATCH, K_TS_IMPORT, K_TS_JSX_USE, K_USES_MEMBER, K_USES_TYPE,
};
use super::occurrence::tag_outbound_rows;
use super::refs::{hit_source, LineCache};
use super::refs_tables::{
    cap_rows, edge_loc, loc_cmp, row_tier, ImportRow, OutboundRow, OutboundTables, Table, TsTables,
};
use super::symbol_refs::SymbolRefs;

// The outbound kinds share ONE cap and ONE ranking under `--out`, mirroring
// `build_refs_model`'s own inbound block in `refs.rs`: resolved before
// heuristic, then the def's own project before every other (imports always
// foreign, per `outbound_foreign`), then file, then line. Each shown hit
// carries the same trimmed source line an inbound hit does, read at the SAME
// `from_line` an inbound row reads -- for an outbound edge that is a line in
// the def's own file, the site actually making the reference, not the caller's.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered ranking pass over every outbound kind, sharing the same cap and tie-break rule across them"
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
    push_ranked(&mut ranked, K_TS_CALL, &refs.outbound_ts.call, EMPTY);
    push_ranked(&mut ranked, K_TS_JSX_USE, &refs.outbound_ts.jsx_use, EMPTY);
    push_ranked(
        &mut ranked,
        K_TS_DISPATCH,
        &refs.outbound_ts.dispatch,
        EMPTY,
    );
    push_ts_imports(&mut ranked, &refs.outbound_ts.import, edges);

    let mut totals = [0usize; 10];
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
    let mut rows: [Vec<OutboundRow>; 10] = Default::default();
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
            | graph::Edge::Overrides { to_file, to, .. }
            | graph::Edge::Call { to_file, to, .. }
            | graph::Edge::JsxUse { to_file, to, .. }
            | graph::Edge::Dispatch { to_file, to, .. } => (to_file.clone(), to.clone()),
            graph::Edge::Import {
                to_file,
                bound_defs,
                ..
            } => (to_file.clone(), bound_defs[r.part].clone()),
            _ => unreachable!("outbound ranked kinds other than K_IMPORTS only ever hold edges that name a target definition"),
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
    // Same per-kind, post-cap tagging rule `build_refs_model_inner`'s inbound
    // side applies; `imports` is excluded -- an imports edge is never a
    // guess and never shares a line with another imports edge the way a
    // `uses-member` call site can, so `ImportRow` carries no such field.
    for kind_rows in &mut rows {
        tag_outbound_rows(kind_rows);
    }
    let [inherits, uses_type, uses_member, implements, overrides, _, call, jsx_use, dispatch, import] =
        rows;

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
        ts: TsTables {
            call: table(K_TS_CALL, call),
            jsx_use: table(K_TS_JSX_USE, jsx_use),
            dispatch: table(K_TS_DISPATCH, dispatch),
            import: table(K_TS_IMPORT, import),
        },
    }
}

// One ranked row per bound definition of each TS `import` edge, in
// `bound_defs` order: the table is symbol-exact, so a statement binding two
// names answers as two rows naming two targets.
fn push_ts_imports(ranked: &mut Vec<RankedOutbound>, imports: &[usize], edges: &[graph::Edge]) {
    for &e in imports {
        let graph::Edge::Import { bound_defs, .. } = &edges[e] else {
            continue;
        };
        for part in 0..bound_defs.len() {
            ranked.push(RankedOutbound {
                kind: K_TS_IMPORT,
                edge: e,
                heuristic: false,
                part,
            });
        }
    }
}
