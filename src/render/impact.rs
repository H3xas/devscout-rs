use crate::graph;
use crate::query;

use super::blocks::rle;
use super::markers::{
    compact_marker, heuristic_suffix, seed_kind_str, BUS_HOP_UNVERIFIED, INFRA_SUFFIX,
};

// The default-renderer suffix for a row every path to it crossed a
// `bus-hop`: the SAME disclosure sentence `refs`/`read`'s own bus-hop rows
// carry, plus the identity to re-check -- an impact row is a file-level
// aggregate with no message/handler fields of its own to show otherwise,
// unlike a `refs` row which already names them. Empty when the row is not
// bus-only, or when (defensively) no origin identity survived to be shown.
fn bus_origin_suffix(bus_only: bool, origin: Option<&query::BusOrigin>) -> String {
    if !bus_only {
        return String::new();
    }
    match origin {
        Some(o) => format!(
            " ({BUS_HOP_UNVERIFIED}: message={} handler={} handlerFile={})",
            o.message, o.to, o.to_file
        ),
        None => format!(" ({BUS_HOP_UNVERIFIED})"),
    }
}

/// Default (non-compact) `impact` rendering.
pub fn render_impact_text(query: &str, model: &query::ImpactModel) -> String {
    let mut out: Vec<String> = Vec::new();
    let joined = model.seed_files.join(", ");
    let seeds = if joined.is_empty() {
        "-".to_string()
    } else {
        joined
    };
    out.push(format!(
        "impact: {query}  ({}, seed files: {seeds})  hops<={}",
        seed_kind_str(model.kind),
        model.hops
    ));
    // The affected count stays the count of files reached by real edges, and
    // the guesses are declared beside it rather than folded into it. The
    // parenthetical appears ONLY when there is something to declare, so a
    // graph with no heuristic edges renders this line byte-for-byte unchanged.
    let heuristic_note = if model.heuristic_affected != 0 {
        format!(" (+{} heuristic)", model.heuristic_affected)
    } else {
        String::new()
    };
    // Test-coverage stage -- the term joins the summary only when the blast
    // radius actually reaches a test file, so a graph built before this stage
    // (and every query that reaches none) renders the line byte-for-byte as it
    // always did. Its ABSENCE is the gap signal, which is why it is never
    // printed as `tests=0`.
    let tests_note = if model.tests_affected != 0 {
        format!(" tests={}", model.tests_affected)
    } else {
        String::new()
    };
    out.push(format!(
        "affected files: {}{heuristic_note}  shown: {}  dropped: {}{tests_note}",
        model.total_affected,
        model.rows.len(),
        model.dropped
    ));
    out.push("file  hops  via  top-symbols".to_string());
    for r in &model.rows {
        let mut syms = r.top_symbols.join(", ");
        if r.top_symbols_more != 0 {
            syms.push_str(&format!(" +{}", r.top_symbols_more));
        }
        // A heuristic-only row's viaCount is zero by definition -- printing it
        // would claim the file was reached by nothing. It reports the guesses
        // that DID reach it; the row's own suffix is what says they were
        // guesses.
        let via = if r.heuristic {
            r.heuristic_count.to_string()
        } else if r.ambiguous_count != 0 {
            format!("{}(+{}amb)", r.via_count, r.ambiguous_count)
        } else {
            r.via_count.to_string()
        };
        // Present only on a row the interface hop actually reached, so every
        // other row (and the whole line under `--no-iface`) carries no `via`
        // suffix.
        let iface_via = if r.iface_via.is_empty() {
            String::new()
        } else {
            format!("  via {}", r.iface_via.join(", "))
        };
        // The same conditional-suffix rule again: a hub file says so on its own
        // row, so the reader never has to join the trailer to the table to learn
        // which row the walk stopped at.
        let class_suffix = if r.infra { INFRA_SUFFIX } else { "" };
        let bus_only_suffix = bus_origin_suffix(r.bus_only, r.bus_origin.as_ref());
        out.push(format!(
            "{}  {}  {via}  {syms}{iface_via}{}{class_suffix}{bus_only_suffix}",
            r.file,
            r.hop,
            heuristic_suffix(r.heuristic, r.tier)
        ));
    }
    // A trailer, not a footnote: the rows above are NARROWER than the graph
    // allows, and the line names which contracts were held back, how broad each
    // one is, and the flag that lets them through. A narrowing nobody can see is
    // indistinguishable from a missing edge. Printed only when the brake fired.
    if !model.braked.is_empty() {
        let list = model
            .braked
            .iter()
            .map(|b| format!("{} (fan-in {})", b.iface, b.fanin))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("braked: {list} — raise --iface-max-fanin to widen"));
    }
    // Its own trailer line, not a term on the interface brake's: the two brakes
    // are undone by two different flags, and a line naming both would leave the
    // reader guessing which flag widens which name.
    if !model.braked_files.is_empty() {
        let list = model
            .braked_files
            .iter()
            .map(|b| format!("{} (in-degree {})", b.file, b.indegree))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "braked: {list} — raise --hub-max-indegree to widen"
        ));
    }
    if model.manifest_gap != 0 {
        out.push(format!(
            "manifest gap: {} graph file(s) not in manifest",
            model.manifest_gap
        ));
    }
    out.join("\n")
}

/// `--compact` `impact` rendering.
pub fn render_impact_compact(query: &str, model: &query::ImpactModel) -> String {
    let mut out: Vec<String> = Vec::new();
    let joined = model.seed_files.join(", ");
    let seeds = if joined.is_empty() {
        "-".to_string()
    } else {
        joined
    };
    out.push(format!(
        "impact: {query}  ({}, seed: {seeds})  hops<={}",
        seed_kind_str(model.kind),
        model.hops
    ));

    // The hop grouping preserves nothing order-relevant (the hop keys are
    // explicitly re-sorted right after), so a plain HashMap is fine. Row order
    // WITHIN a hop bucket must stay in `model.rows`' original
    // (score-desc/hop/file-sorted) order for `rle` to collapse the intended runs
    // -- `Vec::push` in a single forward pass preserves that.
    let mut by_hop: std::collections::HashMap<u32, Vec<&query::ImpactRow>> =
        std::collections::HashMap::new();
    for r in &model.rows {
        by_hop.entry(r.hop).or_default().push(r);
    }
    let mut hops: Vec<u32> = by_hop.keys().copied().collect();
    hops.sort_unstable();
    for hop in hops {
        let rows = &by_hop[&hop];
        out.push(format!("hop {hop} ({}):", rows.len()));
        let lines: Vec<String> = rows
            .iter()
            .map(|r| {
                // A heuristic-only row has a viaCount of zero by definition, so
                // printing `via=0` would say "reached by nothing". It reports
                // the guess count it was actually reached by instead, marked
                // with the same one-character tier marker the refs tables use.
                if r.heuristic {
                    return format!(
                        "{} via={}{}",
                        r.file,
                        r.heuristic_count,
                        compact_marker(r.heuristic, r.tier)
                    );
                }
                let via = if r.ambiguous_count != 0 {
                    format!("{}(+{}amb)", r.via_count, r.ambiguous_count)
                } else {
                    r.via_count.to_string()
                };
                // Same conditional-suffix rule as the default renderer's `via`.
                let iface_suffix = if r.iface_via.is_empty() {
                    String::new()
                } else {
                    format!(" iface={}", r.iface_via.join(","))
                };
                // Same conditional-suffix rule as the default renderer's class.
                let class_suffix = if r.infra { " class=infra" } else { "" };
                // The marker-plus-path-to-full-row shape `refs`' own compact
                // bus block established: compact has no room for the full
                // disclosure text or the origin identity, only a marker and
                // where to find them.
                let bus_only_suffix = if r.bus_only {
                    " ? (rerun without --compact for the full row)"
                } else {
                    ""
                };
                format!(
                    "{} via={via}{iface_suffix}{class_suffix}{bus_only_suffix}",
                    r.file
                )
            })
            .collect();
        for line in rle(&lines) {
            out.push(format!("  {line}"));
        }
    }

    let ambiguous: u32 = model.rows.iter().map(|r| r.ambiguous_count).sum();
    let gap = if model.manifest_gap != 0 {
        format!(" gap={}", model.manifest_gap)
    } else {
        String::new()
    };
    // `affected` counts precisely-reached files; the heuristic term joins it
    // only when there is one, and always BEFORE ` gap=`, so a graph with no
    // heuristic edges renders the same summary line.
    let heur = if model.heuristic_affected != 0 {
        format!(" heuristic={}", model.heuristic_affected)
    } else {
        String::new()
    };
    let tests = if model.tests_affected != 0 {
        format!(" tests={}", model.tests_affected)
    } else {
        String::new()
    };
    // The compact spelling of the default renderer's `braked:` trailer, appended
    // LAST so every summary line the brake never touched is unchanged. One
    // `braked=` term still, file entries after interface ones, each spelled
    // `name:number` exactly as an interface entry.
    let braked = if model.braked.is_empty() && model.braked_files.is_empty() {
        String::new()
    } else {
        let terms: Vec<String> = model
            .braked
            .iter()
            .map(|b| format!("{}:{}", b.iface, b.fanin))
            .chain(
                model
                    .braked_files
                    .iter()
                    .map(|b| format!("{}:{}", b.file, b.indegree)),
            )
            .collect();
        format!(" braked={}", terms.join(","))
    };
    out.push(format!(
        "summary: affected={} shown={} dropped={} ambiguous={ambiguous}{heur}{tests}{gap}{braked}",
        model.total_affected,
        model.rows.len(),
        model.dropped
    ));
    out.join("\n")
}

// Shared by both renderers below: a new block, not a per-row splice into the
// native table, which is what keeps the no-import case byte-identical --
// callers reach this only when `imported.rows` is non-empty.
fn render_imported_block(
    imported: &query::ImportedSection,
    provenance: &graph::Provenance,
) -> String {
    let mut out = vec![format!(
        "imported: {} reached  shown: {}  dropped: {}  provenance {}",
        imported.affected,
        imported.rows.len(),
        imported.dropped,
        provenance.id
    )];
    out.push("file  hop  repo  via".to_string());
    for r in &imported.rows {
        out.push(format!(
            "{}  {}  {}  {}",
            r.file, r.hop, r.repo, r.imported_kind
        ));
    }
    out.join("\n")
}

/// `render_impact_text`, plus the imported-edge section when an import is
/// configured.
///
/// Byte-identical to [`render_impact_text`] when `imported.rows` is empty:
/// the extra block is appended only when there is something to append.
pub fn render_impact_text_with_imports(
    query: &str,
    model: &query::ImpactModel,
    imported: &query::ImportedSection,
    provenance: &graph::Provenance,
) -> String {
    let mut out = render_impact_text(query, model);
    if !imported.rows.is_empty() {
        out.push('\n');
        out.push_str(&render_imported_block(imported, provenance));
    }
    out
}

/// `render_impact_compact`, plus the imported-edge section when an import is
/// configured. Byte-identical to [`render_impact_compact`] when
/// `imported.rows` is empty.
pub fn render_impact_compact_with_imports(
    query: &str,
    model: &query::ImpactModel,
    imported: &query::ImportedSection,
    provenance: &graph::Provenance,
) -> String {
    let mut out = render_impact_compact(query, model);
    if !imported.rows.is_empty() {
        out.push('\n');
        out.push_str(&render_imported_block(imported, provenance));
    }
    out
}
