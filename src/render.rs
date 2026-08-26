// Default / --compact rendering for `refs`/`impact`. This module owns
// `render_refs_text`/`render_impact_text` (default), `render_refs_compact`/
// `render_impact_compact` (`--compact`), and their shared helpers
// (`ref_kind_block`/`compact_block`/`rle`/`group_by_file`). Analytics/dev output
// (triage/analyze/report) is out of scope here.
//
// `--json` output is NOT built here -- it lives in cli.rs alongside the rest of
// the CLI-level `--json` flag handling, matching the module split.
//
// Rendering notes (`ref_kind_block`/`compact_block`):
//
// - `ref_kind_block` (default renderer) prints its header line whenever the
//   table itself is present, EVEN IF `total == 0` -- only a genuinely
//   missing/undefined table suppresses the header entirely. `compact_block`
//   additionally suppresses on `total == 0` (empty tables print nothing in
//   `--compact`, not even a header). This asymmetry is real and preserved:
//   see `ref_kind_block_present_but_empty_table_still_prints_header` below.
// - "Missing table" tolerance (the `Option<&Table<R>>` parameter): query.rs's
//   typed `RefsModel`/`ImpactModel` always supply a concrete (possibly
//   zero-total) `Table` for every kind, so the `None` branch is unreachable from
//   a real model. It is kept because the block helpers are tested directly with
//   an absent table to exercise exactly this path; it is not reachable through
//   the public render_* functions.

use crate::query;

// The ONE place a heuristic row is marked in the default text renderer, and the
// marker is a plain-language word rather than a symbol: the consumer is a model
// reading a wall of `file:line kind` rows, and a sigil it has to look up is the
// same as no marker at all. A row with no flag renders no suffix.
const HEURISTIC_SUFFIX: &str = " (heuristic)";

// The same shape of suffix as `HEURISTIC_SUFFIX`, for the hub-file class, so a
// row carries its own reason and the two never need to be told apart by
// position.
const INFRA_SUFFIX: &str = " (infra)";

// `HEURISTIC_SUFFIX` when the row is a guess, `""` otherwise. Applied inside each
// row formatter rather than inside `ref_kind_block`, because the two tables that
// can never hold a guess (imports, ambiguous) carry no flag to read -- they get
// no suffix, the same as any non-guess row.
fn heuristic_suffix(heuristic: bool) -> &'static str {
    if heuristic { HEURISTIC_SUFFIX } else { "" }
}

// The source snippet, two spaces then the text, appended AFTER the heuristic
// suffix by `ref_kind_block`. Only inbound rows ever carry a source line; every
// other table's rows leave it empty and render no suffix.
fn source_suffix(source: &str) -> String {
    if source.is_empty() { String::new() } else { format!("  {source}") }
}

/// The impact seed kind as a string (`"file"`/`"symbol"`), used in the impact
/// renderers' header line and in cli.rs's `--json` and error messages.
pub fn seed_kind_str(kind: query::SeedKind) -> &'static str {
    match kind {
        query::SeedKind::File => "file",
        query::SeedKind::Symbol => "symbol",
    }
}

// Run-length encoding: collapses a run of `n > 1` identical consecutive strings
// into `"{value}x{n}"`; a run of length 1 passes through unchanged. Not
// order-sensitive beyond adjacency -- callers are responsible for handing it
// already-grouped/sorted input.
fn rle(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let mut j = i + 1;
        while j < values.len() && values[j] == values[i] {
            j += 1;
        }
        if j - i > 1 {
            out.push(format!("{}x{}", values[i], j - i));
        } else {
            out.push(values[i].clone());
        }
        i = j;
    }
    out
}

// Rows arrive already sorted file-then-line (`build_refs_model`'s own `table()`
// sort), so consecutive rows sharing a file are adjacent -- this groups them
// into one `file:line1,line2,...` entry (RLE-collapsed) instead of repeating the
// file path per row.
// `file_of` is a plain `fn` pointer, not `impl Fn`: every caller passes a
// non-capturing closure (only ever reads its own parameter), and a `fn`
// pointer's elided input/output lifetimes are implicitly higher-ranked
// (`for<'a> fn(&'a R) -> &'a str`), which lets the SAME accessor be reused
// across several `compact_block` calls below without the compiler pinning
// it to one concrete lifetime (which is what happens if the parameter is
// `impl Fn(&R) -> &str` and the closure is bound to a `let` once and
// passed to multiple call sites).
fn group_by_file<R>(rows: &[R], file_of: fn(&R) -> &str, line_fmt: impl Fn(&R) -> String) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() && file_of(&rows[j]) == file_of(&rows[i]) {
            j += 1;
        }
        let lines: Vec<String> = rows[i..j].iter().map(|r| line_fmt(r)).collect();
        out.push(format!("{}:{}", file_of(&rows[i]), rle(&lines).join(",")));
        i = j;
    }
    out
}

// Used by the DEFAULT (non-compact) renderer: a missing table is skipped, but a
// present-but-empty table still prints its header, just with no row lines under
// it.
fn ref_kind_block<R>(out: &mut Vec<String>, label: &str, table: Option<&query::Table<R>>, row_fmt: impl Fn(&R) -> String) {
    let Some(t) = table else { return };
    let dropped_note = if t.dropped != 0 { format!(", {} dropped", t.dropped) } else { String::new() };
    out.push(format!("  {label} ({}{dropped_note}):", t.total));
    for r in &t.rows {
        out.push(format!("    {}", row_fmt(r)));
    }
}

// Used by `--compact`: a missing OR present-but-EMPTY table prints nothing at
// all, unlike `ref_kind_block` above.
fn compact_block<R>(
    out: &mut Vec<String>,
    label: &str,
    table: Option<&query::Table<R>>,
    file_of: fn(&R) -> &str,
    line_fmt: impl Fn(&R) -> String,
) {
    let Some(t) = table else { return };
    if t.total == 0 {
        return;
    }
    let dropped_note = if t.dropped != 0 { format!(", {} dropped", t.dropped) } else { String::new() };
    out.push(format!("{label} ({}{dropped_note}):", t.total));
    for line in group_by_file(&t.rows, file_of, line_fmt) {
        out.push(format!("  {line}"));
    }
}

// The one line that splits an enum's inbound member edges by which MEMBER they
// land on. `refs Toggles` already counted them all under `uses-member`; this
// says how many of that total were member-level and which members carried them,
// which is the half of the answer a caller otherwise has to run a second query
// (or a grep) to get. Printed only for an enum with at least one member-level
// reference, so no other symbol's output moves.
fn member_refs_line(m: &query::MemberRefs) -> String {
    let named = m.members.iter().map(|e| format!("{} {}", e.name, e.count)).collect::<Vec<_>>().join(", ");
    let more = if m.dropped != 0 { format!(" +{} more", m.dropped) } else { String::new() };
    format!("member refs: {} across {} member(s): {named}{more}", m.total, m.member_count)
}

pub fn render_refs_text(model: &query::RefsModel) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{}  ({})", model.id, model.kind));
    out.push(format!(
        "def: {}",
        model.sites.iter().map(|s| format!("{}:{}", s.file, s.line)).collect::<Vec<_>>().join("  ")
    ));

    out.push("inbound:".to_string());
    ref_kind_block(&mut out, "inherits", Some(&model.inbound.inherits), |r: &query::InboundRow| {
        format!("{}:{}  inherits{}{}", r.file, r.line, heuristic_suffix(r.heuristic), source_suffix(&r.source))
    });
    ref_kind_block(&mut out, "uses-type", Some(&model.inbound.uses_type), |r: &query::InboundRow| {
        format!("{}:{}  uses-type{}{}", r.file, r.line, heuristic_suffix(r.heuristic), source_suffix(&r.source))
    });
    ref_kind_block(&mut out, "uses-member", Some(&model.inbound.uses_member), |r: &query::InboundRow| {
        format!("{}:{}  uses-member{}{}", r.file, r.line, heuristic_suffix(r.heuristic), source_suffix(&r.source))
    });
    // One trailer for the three kinds, because they share one cap: the
    // per-kind headers say how much each kind lost, this says what the call as
    // a whole did not return. `--all` lifts this cap too, the same lever the
    // outbound trailer below names, but the text here already reports the true
    // drop count and is unchanged whether or not `--all` is set.
    let inbound_dropped = model.inbound.inherits.dropped + model.inbound.uses_type.dropped + model.inbound.uses_member.dropped;
    if inbound_dropped != 0 {
        out.push(format!("  +{inbound_dropped} more"));
    }
    if let Some(m) = &model.member_refs {
        out.push(member_refs_line(m));
    }

    if let Some(ob) = &model.outbound {
        out.push("outbound:".to_string());
        ref_kind_block(&mut out, "inherits", Some(&ob.inherits), |r: &query::OutboundRow| {
            format!("{}:{}  inherits  -> {}{}{}", r.file, r.line, r.to_file, heuristic_suffix(r.heuristic), source_suffix(&r.source))
        });
        ref_kind_block(&mut out, "uses-type", Some(&ob.uses_type), |r: &query::OutboundRow| {
            format!("{}:{}  uses-type  -> {}{}{}", r.file, r.line, r.to_file, heuristic_suffix(r.heuristic), source_suffix(&r.source))
        });
        ref_kind_block(&mut out, "uses-member", Some(&ob.uses_member), |r: &query::OutboundRow| {
            format!("{}:{}  uses-member  -> {}{}{}", r.file, r.line, r.to_file, heuristic_suffix(r.heuristic), source_suffix(&r.source))
        });
        ref_kind_block(&mut out, "imports", Some(&ob.imports), |r: &query::ImportRow| {
            format!("{}:{}  imports  -> {}{}", r.file, r.line, r.target, source_suffix(&r.source))
        });
        // One trailer for the four outbound kinds, because they share one cap
        // (mirrors the inbound trailer above). Printing only ever happens when
        // `--all` would actually return more rows, so the hint is never dead
        // advice -- true of the inbound trailer above too, which is why that one
        // stays text-only rather than gaining the same hint.
        let outbound_dropped = ob.inherits.dropped + ob.uses_type.dropped + ob.uses_member.dropped + ob.imports.dropped;
        if outbound_dropped != 0 {
            out.push(format!("  +{outbound_dropped} more, use --all"));
        }
    }

    let amb_in = &model.ambiguous.inbound;
    let amb_out = &model.ambiguous.outbound;
    if amb_in.total != 0 || amb_out.total != 0 {
        out.push("ambiguous:".to_string());
        let amb_row = |r: &query::AmbiguousRow| {
            format!("{}:{}  {}  raw=\"{}\"  candidates={}", r.file, r.line, r.origin, r.raw, r.candidate_count)
        };
        if amb_in.total != 0 {
            ref_kind_block(&mut out, "inbound", Some(amb_in), amb_row);
        }
        if amb_out.total != 0 {
            ref_kind_block(&mut out, "outbound", Some(amb_out), amb_row);
        }
    }

    if model.manifest_gap != 0 {
        out.push(format!("manifest gap: {} graph file(s) not in manifest", model.manifest_gap));
    }
    out.join("\n")
}

/// Default (non-compact) `impact` rendering.
pub fn render_impact_text(query: &str, model: &query::ImpactModel) -> String {
    let mut out: Vec<String> = Vec::new();
    let joined = model.seed_files.join(", ");
    let seeds = if joined.is_empty() { "-".to_string() } else { joined };
    out.push(format!("impact: {query}  ({}, seed files: {seeds})  hops<={}", seed_kind_str(model.kind), model.hops));
    // The affected count stays the count of files reached by real edges, and
    // the guesses are declared beside it rather than folded into it. The
    // parenthetical appears ONLY when there is something to declare, so a
    // graph with no heuristic edges renders this line byte-for-byte unchanged.
    let heuristic_note =
        if model.heuristic_affected != 0 { format!(" (+{} heuristic)", model.heuristic_affected) } else { String::new() };
    // Test-coverage stage -- the term joins the summary only when the blast
    // radius actually reaches a test file, so a graph built before this stage
    // (and every query that reaches none) renders the line byte-for-byte as it
    // always did. Its ABSENCE is the gap signal, which is why it is never
    // printed as `tests=0`.
    let tests_note = if model.tests_affected != 0 { format!(" tests={}", model.tests_affected) } else { String::new() };
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
        let iface_via =
            if r.iface_via.is_empty() { String::new() } else { format!("  via {}", r.iface_via.join(", ")) };
        // The same conditional-suffix rule again: a hub file says so on its own
        // row, so the reader never has to join the trailer to the table to learn
        // which row the walk stopped at.
        let class_suffix = if r.infra { INFRA_SUFFIX } else { "" };
        out.push(format!(
            "{}  {}  {via}  {syms}{iface_via}{}{class_suffix}",
            r.file,
            r.hop,
            heuristic_suffix(r.heuristic)
        ));
    }
    // A trailer, not a footnote: the rows above are NARROWER than the graph
    // allows, and the line names which contracts were held back, how broad each
    // one is, and the flag that lets them through. A narrowing nobody can see is
    // indistinguishable from a missing edge. Printed only when the brake fired.
    if !model.braked.is_empty() {
        let list =
            model.braked.iter().map(|b| format!("{} (fan-in {})", b.iface, b.fanin)).collect::<Vec<_>>().join(", ");
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
        out.push(format!("braked: {list} — raise --hub-max-indegree to widen"));
    }
    if model.manifest_gap != 0 {
        out.push(format!("manifest gap: {} graph file(s) not in manifest", model.manifest_gap));
    }
    out.join("\n")
}

/// `--compact` `refs` rendering.
pub fn render_refs_compact(model: &query::RefsModel) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{}  ({})", model.id, model.kind));
    out.push(format!(
        "def: {}",
        model.sites.iter().map(|s| format!("{}:{}", s.file, s.line)).collect::<Vec<_>>().join("  ")
    ));

    // `file_of_*` are explicitly typed as `fn(&R) -> &str` (not left to
    // inference) so the closure is assigned its higher-ranked signature
    // (`for<'a> fn(&'a R) -> &'a str`) at the point of declaration --
    // without the annotation, an unannotated `let` binds the closure to
    // one concrete (non-reusable) lifetime instead, which then fails to
    // coerce back down at `compact_block`'s `fn` parameter on the second
    // and later call sites below.
    // Compact mode exists to spend as few bytes as possible, so the marker
    // here is one character rather than the default renderer's word. It is
    // still mandatory: an unmarked guess sitting in a list of facts is the
    // exact failure this resolver is built to avoid, and a small output is no
    // defence. `5h` never collides with a line number, and the run-length
    // collapse treats `5` and `5h` as the distinct entries they are.
    //
    // Compact groups a file's hits into one `path:line,line` entry, which
    // leaves no slot for a per-hit source line -- the snippet is a
    // default-renderer and `--json` affordance only.
    let file_of_ib: fn(&query::InboundRow) -> &str = |r| r.file.as_str();
    let line_ib = |r: &query::InboundRow| if r.heuristic { format!("{}h", r.line) } else { r.line.to_string() };
    compact_block(&mut out, "in:inherits", Some(&model.inbound.inherits), file_of_ib, line_ib);
    compact_block(&mut out, "in:uses-type", Some(&model.inbound.uses_type), file_of_ib, line_ib);
    compact_block(&mut out, "in:uses-member", Some(&model.inbound.uses_member), file_of_ib, line_ib);

    if let Some(ob) = &model.outbound {
        let file_of_ob: fn(&query::OutboundRow) -> &str = |r| r.file.as_str();
        let line_ob = |r: &query::OutboundRow| if r.heuristic { format!("{}h", r.line) } else { r.line.to_string() };
        compact_block(&mut out, "out:inherits", Some(&ob.inherits), file_of_ob, line_ob);
        compact_block(&mut out, "out:uses-type", Some(&ob.uses_type), file_of_ob, line_ob);
        compact_block(&mut out, "out:uses-member", Some(&ob.uses_member), file_of_ob, line_ob);

        let file_of_imp: fn(&query::ImportRow) -> &str = |r| r.file.as_str();
        let line_imp = |r: &query::ImportRow| r.line.to_string();
        compact_block(&mut out, "out:imports", Some(&ob.imports), file_of_imp, line_imp);
    }

    let amb_in = &model.ambiguous.inbound;
    let amb_out = &model.ambiguous.outbound;
    let file_of_amb: fn(&query::AmbiguousRow) -> &str = |r| r.file.as_str();
    let line_amb = |r: &query::AmbiguousRow| format!("{}(candidates={})", r.line, r.candidate_count);
    compact_block(&mut out, "amb:in", Some(amb_in), file_of_amb, line_amb);
    compact_block(&mut out, "amb:out", Some(amb_out), file_of_amb, line_amb);

    // The missing-table tolerance documented at the top of this file is what an
    // absent `outbound` falls through here: the three outbound tables plus
    // imports contribute nothing to any of the three sums when the caller did not
    // ask for them.
    let ob = model.outbound.as_ref();
    let ob_sum = |f: fn(&query::OutboundTables) -> usize| ob.map_or(0, f);
    let edges = model.inbound.inherits.total
        + model.inbound.uses_type.total
        + model.inbound.uses_member.total
        + ob_sum(|o| o.inherits.total + o.uses_type.total + o.uses_member.total + o.imports.total);
    let shown = model.inbound.inherits.rows.len()
        + model.inbound.uses_type.rows.len()
        + model.inbound.uses_member.rows.len()
        + ob_sum(|o| o.inherits.rows.len() + o.uses_type.rows.len() + o.uses_member.rows.len() + o.imports.rows.len());
    let dropped = model.inbound.inherits.dropped
        + model.inbound.uses_type.dropped
        + model.inbound.uses_member.dropped
        + ob_sum(|o| o.inherits.dropped + o.uses_type.dropped + o.uses_member.dropped + o.imports.dropped);
    let ambiguous = amb_in.total + amb_out.total;
    let gap = if model.manifest_gap != 0 { format!(" gap={}", model.manifest_gap) } else { String::new() };
    // Same fact as the default renderer's line, spelled the way every other
    // compact block is: `name=count`, no prose, and absent entirely when there is
    // nothing to say.
    if let Some(m) = &model.member_refs {
        let named = m.members.iter().map(|e| format!("{}={}", e.name, e.count)).collect::<Vec<_>>().join(",");
        let more = if m.dropped != 0 { format!(",+{}", m.dropped) } else { String::new() };
        out.push(format!("mem: {named}{more}"));
    }
    out.push(format!("summary: edges={edges} shown={shown} dropped={dropped} ambiguous={ambiguous}{gap}"));
    out.join("\n")
}

// The `def:` line both `read` renderers open with: the declaring sites in
// refs' own order, with the PRIMARY site carrying the recorded span
// (`file:start-end`) when one is on record. A site without a span fact
// prints start-only -- an absent end is never rendered as a range. When the
// model carries a source block it goes under this line, verbatim.
fn read_def_line(model: &query::ReadModel) -> String {
    let sites = &model.refs.sites;
    if sites.is_empty() {
        return "def: -".to_string();
    }
    let rendered: Vec<String> = sites
        .iter()
        .enumerate()
        .map(|(i, s)| match (i, &model.span) {
            (0, Some(sp)) => format!("{}:{}-{}", sp.file, sp.start_line, sp.end_line),
            _ => format!("{}:{}", s.file, s.line),
        })
        .collect();
    format!("def: {}", rendered.join("  "))
}

/// Default (non-compact) `read` rendering: the declaration span and its
///
/// verbatim source, then exactly the inbound answer `refs` gives -- same
/// tables, same cap trailer, same ambiguous section and manifest-gap note.
pub fn render_read_text(model: &query::ReadModel) -> String {
    let rendered = render_refs_text(&model.refs);
    let mut out: Vec<String> = rendered.split('\n').map(str::to_string).collect();
    if out.len() > 1 { out[1] = read_def_line(model); }
    if let Some(sp) = &model.span {
        out.splice(2..2, sp.source.split('\n').map(str::to_string));
    }
    out.join("\n")
}

/// `--compact` `read` rendering: the span on the def line, no source block,
///
/// then the compact inbound blocks and summary. Everything `refs --compact`
/// prints minus the outbound side, which `read` never computes.
pub fn render_read_compact(model: &query::ReadModel) -> String {
    let rendered = render_refs_compact(&model.refs);
    let mut out: Vec<String> = rendered.split('\n').map(str::to_string).collect();
    if out.len() > 1 { out[1] = read_def_line(model); }
    out.join("\n")
}

/// `--compact` `impact` rendering.
pub fn render_impact_compact(query: &str, model: &query::ImpactModel) -> String {
    let mut out: Vec<String> = Vec::new();
    let joined = model.seed_files.join(", ");
    let seeds = if joined.is_empty() { "-".to_string() } else { joined };
    out.push(format!("impact: {query}  ({}, seed: {seeds})  hops<={}", seed_kind_str(model.kind), model.hops));

    // The hop grouping preserves nothing order-relevant (the hop keys are
    // explicitly re-sorted right after), so a plain HashMap is fine. Row order
    // WITHIN a hop bucket must stay in `model.rows`' original
    // (score-desc/hop/file-sorted) order for `rle` to collapse the intended runs
    // -- `Vec::push` in a single forward pass preserves that.
    let mut by_hop: std::collections::HashMap<u32, Vec<&query::ImpactRow>> = std::collections::HashMap::new();
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
                // with the same `h`.
                if r.heuristic {
                    return format!("{} via={}h", r.file, r.heuristic_count);
                }
                let via = if r.ambiguous_count != 0 { format!("{}(+{}amb)", r.via_count, r.ambiguous_count) } else { r.via_count.to_string() };
                // Same conditional-suffix rule as the default renderer's `via`.
                let iface_suffix = if r.iface_via.is_empty() { String::new() } else { format!(" iface={}", r.iface_via.join(",")) };
                // Same conditional-suffix rule as the default renderer's class.
                let class_suffix = if r.infra { " class=infra" } else { "" };
                format!("{} via={via}{iface_suffix}{class_suffix}", r.file)
            })
            .collect();
        for line in rle(&lines) {
            out.push(format!("  {line}"));
        }
    }

    let ambiguous: u32 = model.rows.iter().map(|r| r.ambiguous_count).sum();
    let gap = if model.manifest_gap != 0 { format!(" gap={}", model.manifest_gap) } else { String::new() };
    // `affected` counts precisely-reached files; the heuristic term joins it
    // only when there is one, and always BEFORE ` gap=`, so a graph with no
    // heuristic edges renders the same summary line.
    let heur = if model.heuristic_affected != 0 { format!(" heuristic={}", model.heuristic_affected) } else { String::new() };
    let tests = if model.tests_affected != 0 { format!(" tests={}", model.tests_affected) } else { String::new() };
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
            .chain(model.braked_files.iter().map(|b| format!("{}:{}", b.file, b.indegree)))
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

/// Default `devscout tests` output. The zero case is a first-class answer, not
/// an empty table: "nothing tests this" is exactly what the caller asked, and it
/// is one line rather than a header over a void.
///
/// Heuristic file lines carry the SAME word-suffix the refs/impact renderers
/// use, and the summary counts stay precise-only, so a file listed under a
/// covered count is a file a test really references.
pub fn render_tests_text(model: &query::TestsModel) -> String {
    let mut out: Vec<String> = vec![format!("tests for {}", model.symbol)];
    if model.rows.is_empty() {
        out.push("no test references found".to_string());
        return out.join("\n");
    }
    out.push(format!("covered by {} test file(s), {} reference(s)", model.test_file_count, model.ref_count));
    out.push(String::new());
    for r in &model.rows {
        out.push(format!("{}{}", r.file, heuristic_suffix(r.heuristic)));
        let lines = r.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ");
        for def_id in &r.test_defs {
            out.push(format!("  {def_id}  lines: {lines}"));
        }
    }
    out.join("\n")
}

/// `--compact` `devscout tests` output. The same model with the def ids and the
/// per-file indentation dropped: one header line carrying every count, then one
/// line per file. The `h` line marker is the compact renderer's existing
/// convention for a guess.
pub fn render_tests_compact(model: &query::TestsModel) -> String {
    let heur = if model.heuristic_file_count != 0 { format!(" heuristic={}", model.heuristic_file_count) } else { String::new() };
    let mut out: Vec<String> =
        vec![format!("tests {} files={} refs={}{heur}", model.symbol, model.test_file_count, model.ref_count)];
    for r in &model.rows {
        let lines = r
            .lines
            .iter()
            .map(|l| if r.heuristic { format!("{l}h") } else { l.to_string() })
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("{} {lines}", r.file));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{
        AmbiguousRow, AmbiguousTables, DefSite, ImpactModel, ImpactRow, ImportRow, InboundRow, InboundTables,
        OutboundRow, OutboundTables, RefsModel, SeedKind, Table, TestRow, TestsModel,
    };

    fn table<R>(rows: Vec<R>, dropped: usize) -> Table<R> {
        Table { total: rows.len() + dropped, dropped, rows }
    }

    fn empty_inbound() -> InboundTables {
        InboundTables { inherits: table(vec![], 0), uses_type: table(vec![], 0), uses_member: table(vec![], 0) }
    }
    fn empty_outbound() -> OutboundTables {
        OutboundTables {
            inherits: table(vec![], 0),
            uses_type: table(vec![], 0),
            uses_member: table(vec![], 0),
            imports: table(vec![], 0),
        }
    }
    fn empty_ambiguous() -> AmbiguousTables {
        AmbiguousTables { inbound: table(vec![], 0), outbound: table(vec![], 0) }
    }

    fn refs_model(inbound: InboundTables, outbound: OutboundTables, ambiguous: AmbiguousTables, manifest_gap: usize) -> RefsModel {
        RefsModel {
            query: "IFoo".to_string(),
            id: "App.IFoo".to_string(),
            kind: "interface".to_string(),
            sites: vec![DefSite { file: "src/IFoo.cs".to_string(), line: 3 }],
            inbound,
            outbound: Some(outbound),
            ambiguous,
            manifest_gap,
            member_refs: None,
        }
    }

    // --- render_refs_compact fixtures --------------------------------------

    #[test]
    fn refs_compact_one_header_per_kind_empty_kinds_print_nothing() {
        let mut inbound = empty_inbound();
        inbound.inherits = table(vec![InboundRow { file: "src/Foo.cs".into(), line: 5, heuristic: false, source: String::new() }], 0);
        let mut outbound = empty_outbound();
        outbound.uses_type = table(
            vec![OutboundRow {
                file: "src/IFoo.cs".into(),
                line: 3,
                to_file: "src/Bar.cs".into(),
                to: "App.Bar".into(),
                heuristic: false,
                source: String::new(),
            }],
            0,
        );
        let model = refs_model(inbound, outbound, empty_ambiguous(), 0);
        let out = render_refs_compact(&model);
        assert!(out.contains("in:inherits (1):\n  src/Foo.cs:5"));
        assert!(out.contains("out:uses-type (1):\n  src/IFoo.cs:3"));
        assert!(!out.contains("in:uses-type"), "a kind with zero rows must not print a header");
        assert!(!out.contains("out:inherits"));
        assert!(!out.contains("out:imports"));
        assert!(!out.contains("Bar.cs"), "outbound target file is dropped in compact mode -- only path:line survives");
    }

    #[test]
    fn refs_compact_same_file_line_collapses_to_nxn() {
        let mut outbound = empty_outbound();
        outbound.inherits = table(
            vec![
                OutboundRow {
                    file: "src/Widget.cs".into(),
                    line: 5,
                    to_file: "src/IWidget.cs".into(),
                    to: "App.IWidget".into(),
                    heuristic: false,
                    source: String::new(),
                },
                OutboundRow {
                    file: "src/Widget.cs".into(),
                    line: 5,
                    to_file: "src/IGadget.cs".into(),
                    to: "App.IGadget".into(),
                    heuristic: false,
                    source: String::new(),
                },
            ],
            0,
        );
        let model = refs_model(empty_inbound(), outbound, empty_ambiguous(), 0);
        let out = render_refs_compact(&model);
        assert!(out.contains("out:inherits (2):\n  src/Widget.cs:5x2"));
    }

    #[test]
    fn refs_compact_groups_multiple_lines_under_one_file_mention() {
        let mut inbound = empty_inbound();
        inbound.uses_type = table(
            vec![
                InboundRow { file: "src/Consumers/Big.cs".into(), line: 12, heuristic: false, source: String::new() },
                InboundRow { file: "src/Consumers/Big.cs".into(), line: 40, heuristic: false, source: String::new() },
                InboundRow { file: "src/Consumers/Big.cs".into(), line: 40, heuristic: false, source: String::new() },
                InboundRow { file: "src/Consumers/Small.cs".into(), line: 3, heuristic: false, source: String::new() },
            ],
            0,
        );
        let model = refs_model(inbound, empty_outbound(), empty_ambiguous(), 0);
        let out = render_refs_compact(&model);
        assert!(out.contains("in:uses-type (4):\n  src/Consumers/Big.cs:12,40x2\n  src/Consumers/Small.cs:3"));
        assert_eq!(out.matches("src/Consumers/Big.cs").count(), 1, "the repeated file path must appear exactly once");
    }

    #[test]
    fn refs_compact_summary_line_folds_all_counts() {
        let mut inbound = empty_inbound();
        inbound.inherits =
            table(vec![InboundRow { file: "src/A.cs".into(), line: 1, heuristic: false, source: String::new() }, InboundRow { file: "src/B.cs".into(), line: 2, heuristic: false, source: String::new() }], 3);
        let mut ambiguous = empty_ambiguous();
        ambiguous.inbound = table(
            vec![AmbiguousRow { file: "src/C.cs".into(), line: 9, origin: "uses-type".into(), raw: "Config".into(), candidate_count: 2 }],
            0,
        );
        let model = refs_model(inbound, empty_outbound(), ambiguous, 4);
        let out = render_refs_compact(&model);
        let last_line = out.lines().last().unwrap();
        assert_eq!(last_line, "summary: edges=5 shown=2 dropped=3 ambiguous=1 gap=4");
        assert!(out.contains("amb:in (1):\n  src/C.cs:9(candidates=2)"));
        assert!(!out.contains("Config"), "the raw ambiguous token is not a minimal column and must not appear");
    }

    fn impact_model(rows: Vec<ImpactRow>, total_affected: usize, dropped: usize, manifest_gap: usize) -> ImpactModel {
        let heuristic_affected = rows.iter().filter(|r| r.heuristic).count();
        ImpactModel {
            kind: SeedKind::File,
            seed_files: vec!["src/IFoo.cs".to_string()],
            hops: 2,
            total_affected,
            rows,
            dropped,
            manifest_gap,
            heuristic_affected,
            tests_affected: 0,
            braked: vec![],
            braked_files: vec![],
        }
    }

    /// A precisely-reached impact row (both heuristic flags off).
    fn impact_row(
        file: &str,
        hop: u32,
        via_count: u32,
        ambiguous_count: u32,
        top_symbols: &[&str],
        top_symbols_more: usize,
        score: f64,
    ) -> ImpactRow {
        ImpactRow {
            file: file.into(),
            hop,
            via_count,
            ambiguous_count,
            top_symbols: top_symbols.iter().map(|s| (*s).to_string()).collect(),
            top_symbols_more,
            score,
            heuristic_count: 0,
            heuristic: false,
            iface_via: vec![],
            from_lines: vec![],
            infra: false,
        }
    }

    /// A heuristic-ONLY row: reached by `heuristic_count` guesses and nothing
    /// else, which is the only shape `buildImpactModel` ever flags.
    fn heuristic_impact_row(file: &str, hop: u32, heuristic_count: u32, top_symbols: &[&str], score: f64) -> ImpactRow {
        ImpactRow { heuristic_count, heuristic: true, ..impact_row(file, hop, 0, 0, top_symbols, 0, score) }
    }

    #[test]
    fn impact_compact_groups_rows_into_ascending_hop_buckets() {
        let model = impact_model(
            vec![
                impact_row("src/Two.cs", 2, 1, 0, &["Foo"], 0, 0.1),
                impact_row("src/One.cs", 1, 2, 1, &["Foo", "Bar"], 5, 0.5),
            ],
            3,
            0,
            0,
        );
        let out = render_impact_compact("IFoo", &model);
        let hop1 = out.find("hop 1").unwrap();
        let hop2 = out.find("hop 2").unwrap();
        assert!(hop1 < hop2, "hop 1 bucket must precede hop 2 regardless of row order in the model");
        assert!(out.contains("hop 1 (1):\n  src/One.cs via=2(+1amb)"));
        assert!(out.contains("hop 2 (1):\n  src/Two.cs via=1"));
        assert!(!out.contains("Bar"), "compact drops top-symbols entirely");
        assert!(out.contains("summary: affected=3 shown=2 dropped=0 ambiguous=1"));
    }

    #[test]
    fn impact_compact_manifest_gap_folds_into_summary() {
        let model = impact_model(vec![], 0, 0, 2);
        let out = render_impact_compact("IFoo", &model);
        let last_line = out.lines().last().unwrap();
        assert_eq!(last_line, "summary: affected=0 shown=0 dropped=0 ambiguous=0 gap=2");
    }

    // --- Missing-table tolerance, tested directly on the helpers (see module
    // header: unreachable via the typed RefsModel/ImpactModel, but ported). ---

    #[test]
    fn ref_kind_block_missing_table_prints_nothing() {
        let mut out: Vec<String> = vec!["before".to_string()];
        ref_kind_block::<InboundRow>(&mut out, "inherits", None, |r| format!("{}:{}", r.file, r.line));
        assert_eq!(out, vec!["before".to_string()]);
    }

    #[test]
    fn compact_block_missing_table_prints_nothing() {
        let mut out: Vec<String> = vec!["before".to_string()];
        compact_block::<InboundRow>(&mut out, "in:inherits", None, |r| r.file.as_str(), |r| r.line.to_string());
        assert_eq!(out, vec!["before".to_string()]);
    }

    #[test]
    fn ref_kind_block_present_but_empty_table_still_prints_header() {
        let mut out: Vec<String> = Vec::new();
        let t: Table<InboundRow> = table(vec![], 0);
        ref_kind_block(&mut out, "inherits", Some(&t), |r: &InboundRow| format!("{}:{}", r.file, r.line));
        assert_eq!(out, vec!["  inherits (0):".to_string()]);
    }

    #[test]
    fn compact_block_present_but_empty_table_prints_nothing_unlike_ref_kind_block() {
        let mut out: Vec<String> = Vec::new();
        let t: Table<InboundRow> = table(vec![], 0);
        compact_block(&mut out, "in:inherits", Some(&t), |r: &InboundRow| r.file.as_str(), |r: &InboundRow| r.line.to_string());
        assert!(out.is_empty());
    }

    #[test]
    fn ref_kind_block_dropped_note_only_appears_when_nonzero() {
        let mut out: Vec<String> = Vec::new();
        let t = table(vec![InboundRow { file: "a.cs".into(), line: 1, heuristic: false, source: String::new() }], 2);
        ref_kind_block(&mut out, "inherits", Some(&t), |r: &InboundRow| format!("{}:{}", r.file, r.line));
        assert_eq!(out[0], "  inherits (3, 2 dropped):");
    }

    #[test]
    fn rle_collapses_consecutive_equal_runs_only() {
        let values = vec!["5".to_string(), "5".to_string(), "3".to_string(), "3".to_string(), "3".to_string(), "1".to_string()];
        assert_eq!(rle(&values), vec!["5x2".to_string(), "3x3".to_string(), "1".to_string()]);
    }

    #[test]
    fn seed_kind_str_matches_js_string_literals() {
        assert_eq!(seed_kind_str(SeedKind::File), "file");
        assert_eq!(seed_kind_str(SeedKind::Symbol), "symbol");
    }

    // --- the enum member-count line ----------------------------------------

    #[test]
    fn member_refs_line_renders_after_the_inbound_block_and_caps_the_named_members() {
        let mut model = refs_model(empty_inbound(), empty_outbound(), empty_ambiguous(), 0);
        model.member_refs = Some(query::MemberRefs {
            total: 3,
            member_count: 2,
            members: vec![
                query::MemberRefEntry { name: "EnableX".into(), count: 2 },
                query::MemberRefEntry { name: "EnableY".into(), count: 1 },
            ],
            dropped: 0,
        });
        let out = render_refs_text(&model);
        assert!(out.contains("\nmember refs: 3 across 2 member(s): EnableX 2, EnableY 1"), "{out}");
        assert!(render_refs_compact(&model).contains("\nmem: EnableX=2,EnableY=1\nsummary:"), "{}", render_refs_compact(&model));

        model.member_refs = Some(query::MemberRefs {
            total: 9,
            member_count: 7,
            members: vec![query::MemberRefEntry { name: "A".into(), count: 9 }],
            dropped: 6,
        });
        assert!(render_refs_text(&model).contains("member refs: 9 across 7 member(s): A 9 +6 more"), "{}", render_refs_text(&model));
        assert!(render_refs_compact(&model).contains("mem: A=9,+6"), "{}", render_refs_compact(&model));
    }

    #[test]
    fn a_model_with_no_member_refs_renders_exactly_as_before() {
        let out = render_refs_text(&refs_model(empty_inbound(), empty_outbound(), empty_ambiguous(), 0));
        assert!(!out.contains("member refs:"), "{out}");
        assert!(!render_refs_compact(&refs_model(empty_inbound(), empty_outbound(), empty_ambiguous(), 0)).contains("mem: "));
    }

    // --- heuristic markers: the render literals ----------------------------

    #[test]
    fn refs_text_suffixes_only_tagged_rows_and_never_an_imports_row() {
        let mut inbound = empty_inbound();
        inbound.uses_member = table(
            vec![
                InboundRow { file: "src/Fact.cs".into(), line: 4, heuristic: false, source: String::new() },
                InboundRow { file: "src/Guess.cs".into(), line: 9, heuristic: true, source: String::new() },
            ],
            0,
        );
        let mut outbound = empty_outbound();
        outbound.uses_member = table(
            vec![OutboundRow {
                file: "src/Guess.cs".into(),
                line: 9,
                to_file: "src/Widget.cs".into(),
                to: "App.Widget".into(),
                heuristic: true,
                source: String::new(),
            }],
            0,
        );
        // An imports row carries no flag at all -- JS reads `r.heuristic` as
        // `undefined` there, which renders no suffix; this side has no field
        // to read, which is the same thing.
        outbound.imports =
            table(vec![ImportRow { file: "src/Guess.cs".into(), line: 1, target: "App.Core".into(), source: String::new() }], 0);

        let out = render_refs_text(&refs_model(inbound, outbound, empty_ambiguous(), 0));
        assert!(out.contains("    src/Fact.cs:4  uses-member\n"), "{out}");
        assert!(out.contains("    src/Guess.cs:9  uses-member (heuristic)"), "{out}");
        assert!(out.contains("    src/Guess.cs:9  uses-member  -> src/Widget.cs (heuristic)"), "{out}");
        assert!(out.ends_with("    src/Guess.cs:1  imports  -> App.Core"), "an imports row is never suffixed\n{out}");
        assert_eq!(out.matches("(heuristic)").count(), 2);
    }

    #[test]
    fn refs_compact_marks_a_heuristic_row_with_a_trailing_h_and_rle_keeps_them_distinct() {
        let mut inbound = empty_inbound();
        inbound.uses_member = table(
            vec![
                InboundRow { file: "src/A.cs".into(), line: 5, heuristic: false, source: String::new() },
                InboundRow { file: "src/A.cs".into(), line: 5, heuristic: true, source: String::new() },
                InboundRow { file: "src/A.cs".into(), line: 5, heuristic: true, source: String::new() },
            ],
            0,
        );
        let out = render_refs_compact(&refs_model(inbound, empty_outbound(), empty_ambiguous(), 0));
        assert!(out.contains("in:uses-member (3):\n  src/A.cs:5,5hx2"), "`5` and `5h` are distinct RLE entries\n{out}");
    }

    #[test]
    fn impact_text_declares_heuristic_reached_files_beside_the_count_only_when_there_are_some() {
        let with_guess = impact_model(
            vec![impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5), heuristic_impact_row("src/Guessed.cs", 1, 2, &["Widget"], 0.0)],
            1,
            0,
            0,
        );
        let out = render_impact_text("Widget", &with_guess);
        assert!(out.contains("affected files: 1 (+1 heuristic)  shown: 2  dropped: 0"), "{out}");
        assert!(out.contains("src/Direct.cs  1  1  Widget\n"), "{out}");
        assert!(
            out.contains("src/Guessed.cs  1  2  Widget (heuristic)"),
            "the via column reports the GUESS count, never the zero viaCount\n{out}"
        );

        let without = impact_model(vec![impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5)], 1, 0, 0);
        let out = render_impact_text("Widget", &without);
        assert!(out.contains("affected files: 1  shown: 1  dropped: 0"), "byte-unchanged when there is nothing to declare\n{out}");
        assert!(!out.contains("heuristic"), "{out}");
    }

    #[test]
    fn impact_compact_summary_inserts_heuristic_before_gap() {
        let model = impact_model(vec![heuristic_impact_row("src/Guessed.cs", 1, 3, &["Widget"], 0.0)], 0, 0, 2);
        let out = render_impact_compact("Widget", &model);
        assert!(out.contains("hop 1 (1):\n  src/Guessed.cs via=3h"), "{out}");
        assert_eq!(out.lines().last().unwrap(), "summary: affected=0 shown=1 dropped=0 ambiguous=0 heuristic=1 gap=2");

        // And it disappears entirely at zero.
        let none = impact_model(vec![impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5)], 1, 0, 2);
        assert_eq!(
            render_impact_compact("Widget", &none).lines().last().unwrap(),
            "summary: affected=1 shown=1 dropped=0 ambiguous=0 gap=2"
        );
    }

    // --- test-coverage stage: `devscout tests` bytes --------------------------

    fn tests_model(rows: Vec<TestRow>) -> TestsModel {
        let precise: Vec<&TestRow> = rows.iter().filter(|r| !r.heuristic).collect();
        let guessed: Vec<&TestRow> = rows.iter().filter(|r| r.heuristic).collect();
        TestsModel {
            query: "OrderService".to_string(),
            symbol: "App.Orders.OrderService".to_string(),
            def_files: vec!["src/OrderService.cs".to_string()],
            test_file_count: precise.len(),
            ref_count: precise.iter().map(|r| r.ref_count).sum(),
            heuristic_file_count: guessed.len(),
            heuristic_ref_count: guessed.iter().map(|r| r.ref_count).sum(),
            rows,
        }
    }

    fn test_row(file: &str, test_defs: &[&str], lines: &[usize], heuristic: bool) -> TestRow {
        TestRow {
            file: file.into(),
            test_defs: test_defs.iter().map(|s| (*s).to_string()).collect(),
            lines: lines.to_vec(),
            ref_count: lines.len(),
            heuristic,
        }
    }

    #[test]
    fn tests_text_heads_the_file_and_indents_one_line_per_test_def() {
        let model = tests_model(vec![test_row("tests/OrderServiceTests.cs", &["App.Orders.Tests.OrderServiceTests"], &[10, 11], false)]);
        assert_eq!(
            render_tests_text(&model),
            "tests for App.Orders.OrderService\ncovered by 1 test file(s), 2 reference(s)\n\ntests/OrderServiceTests.cs\n  App.Orders.Tests.OrderServiceTests  lines: 10, 11"
        );
    }

    #[test]
    fn tests_text_answers_the_zero_case_in_one_line_instead_of_a_header_over_a_void() {
        let mut model = tests_model(vec![]);
        model.symbol = "App.Orders.Untested".to_string();
        assert_eq!(render_tests_text(&model), "tests for App.Orders.Untested\nno test references found");
    }

    #[test]
    fn tests_text_marks_a_guessed_file_with_the_shared_heuristic_suffix() {
        let model = tests_model(vec![
            test_row("tests/OrderServiceTests.cs", &["App.Orders.Tests.OrderServiceTests"], &[10], false),
            test_row("tests/Partial.Extra.cs", &["App.Orders.Tests.PartialTests"], &[9], true),
        ]);
        let out = render_tests_text(&model);
        assert!(out.contains("covered by 1 test file(s), 1 reference(s)"), "counts stay precise-only\n{out}");
        assert!(out.ends_with("tests/Partial.Extra.cs (heuristic)\n  App.Orders.Tests.PartialTests  lines: 9"), "{out}");
    }

    #[test]
    fn tests_compact_folds_the_counts_into_the_header_and_drops_the_def_ids() {
        let model = tests_model(vec![test_row("tests/OrderServiceTests.cs", &["App.Orders.Tests.OrderServiceTests"], &[10, 11], false)]);
        assert_eq!(
            render_tests_compact(&model),
            "tests App.Orders.OrderService files=1 refs=2\ntests/OrderServiceTests.cs 10,11"
        );

        let mut empty = tests_model(vec![]);
        empty.symbol = "App.Orders.Untested".to_string();
        assert_eq!(render_tests_compact(&empty), "tests App.Orders.Untested files=0 refs=0");
    }

    #[test]
    fn tests_compact_declares_heuristic_files_in_the_header_and_marks_their_lines() {
        let model = tests_model(vec![
            test_row("tests/OrderServiceTests.cs", &["App.Orders.Tests.OrderServiceTests"], &[10], false),
            test_row("tests/Partial.Extra.cs", &["App.Orders.Tests.PartialTests"], &[9, 12], true),
        ]);
        assert_eq!(
            render_tests_compact(&model),
            "tests App.Orders.OrderService files=1 refs=1 heuristic=1\ntests/OrderServiceTests.cs 10\ntests/Partial.Extra.cs 9h,12h"
        );
    }

    #[test]
    fn impact_summaries_append_tests_only_when_the_blast_radius_reaches_one() {
        let reached = ImpactModel {
            tests_affected: 1,
            ..impact_model(vec![impact_row("tests/OrderServiceTests.cs", 1, 1, 0, &["OrderService"], 0, 0.5)], 1, 0, 0)
        };
        assert!(
            render_impact_text("OrderService", &reached).contains("affected files: 1  shown: 1  dropped: 0 tests=1"),
            "{}",
            render_impact_text("OrderService", &reached)
        );
        assert_eq!(
            render_impact_compact("OrderService", &reached).lines().last().unwrap(),
            "summary: affected=1 shown=1 dropped=0 ambiguous=0 tests=1"
        );

        let none = impact_model(vec![impact_row("src/Other.cs", 1, 1, 0, &["OrderService"], 0, 0.5)], 1, 0, 0);
        assert!(
            render_impact_text("OrderService", &none).contains("affected files: 1  shown: 1  dropped: 0\n"),
            "its ABSENCE is the gap signal -- never printed as tests=0"
        );
        assert_eq!(
            render_impact_compact("OrderService", &none).lines().last().unwrap(),
            "summary: affected=1 shown=1 dropped=0 ambiguous=0"
        );
    }

    #[test]
    fn impact_compact_orders_tests_after_heuristic_and_before_gap() {
        let model = ImpactModel {
            tests_affected: 1,
            ..impact_model(
                vec![
                    impact_row("tests/OrderServiceTests.cs", 1, 1, 0, &["OrderService"], 0, 0.5),
                    heuristic_impact_row("src/Guessed.cs", 1, 3, &["OrderService"], 0.0),
                ],
                1,
                0,
                2,
            )
        };
        assert_eq!(
            render_impact_compact("OrderService", &model).lines().last().unwrap(),
            "summary: affected=1 shown=2 dropped=0 ambiguous=0 heuristic=1 tests=1 gap=2"
        );
    }
}
