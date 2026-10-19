use crate::query;

use super::blocks::{compact_block, ref_kind_block_if_any};
use super::markers::{compact_marker, count_suffix, heuristic_suffix, test_via_suffix};
use super::refs::bus_hop_line;

/// Default `devscout tests` output.
///
/// The zero case is a first-class answer, not
/// an empty table: "nothing tests this" is exactly what the caller asked, and it
/// is one line rather than a header over a void. A symbol reached ONLY over a
/// possible-route bus hop -- no precise or heuristic row at all -- still gets
/// the header and its `bus-hop` block; only a symbol with neither prints the
/// zero-case line.
///
/// Heuristic file lines carry the SAME word-suffix the refs/impact renderers
/// use, and the summary counts stay precise-only, so a file listed under a
/// covered count is a file a test really references.
pub fn render_tests_text(model: &query::TestsModel) -> String {
    // No row cap ever applies to a test file, so the counted and the printed
    // number are the same one -- and they are still both stated, because the
    // reader of the zero case has to see that the answer is empty rather than
    // truncated.
    let files = model.rows.len();
    let mut out: Vec<String> = vec![format!(
        "tests for {}{}",
        model.symbol,
        count_suffix("files", files, files)
    )];
    if model.rows.is_empty() && model.bus.total == 0 {
        out.push("no test references found".to_string());
        name_hint_lines(&mut out, &model.name_hint);
        return out.join("\n");
    }
    out.push(format!(
        "covered by {} test file(s), {} reference(s)",
        model.test_file_count, model.ref_count
    ));
    out.push(String::new());
    for r in &model.rows {
        out.push(format!(
            "{}{}{}",
            r.file,
            heuristic_suffix(r.heuristic, r.tier),
            test_via_suffix(r.via)
        ));
        let lines = r
            .lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if r.test_defs.is_empty() {
            // A `Project`-vouched row can carry no attributed def at all --
            // still one line, so the file's referencing lines are never
            // silently dropped.
            out.push(format!("  lines: {lines}"));
        } else {
            for def_id in &r.test_defs {
                out.push(format!("  {def_id}  lines: {lines}"));
            }
        }
    }
    // A test file that PUBLISHES to this handler over a bus hop: a possible
    // route, kept out of `test_file_count`/`ref_count` by construction (see
    // `TestsModel::bus`'s own doc comment), rendered through the exact same
    // row formatter `refs`' own `bus-hop` block uses so the two never state
    // the disclosure in different words.
    ref_kind_block_if_any(&mut out, "bus-hop", &model.bus, bus_hop_line);
    out.join("\n")
}

// The ` (+n more)` tail a capped reference list carries, and nothing when the
// list already holds every distinct id the file references.
fn more_suffix(total: usize, listed: usize) -> String {
    match total.checked_sub(listed) {
        Some(n) if n > 0 => format!(" (+{n} more)"),
        _ => String::new(),
    }
}

// The hint block under `no test references found`. The heading states what
// the block is IN the answer itself -- a file NAME, not a reference -- so a
// reader who skips the docs still cannot mistake it for coverage. An empty
// hint writes nothing at all, which is what keeps every other answer's bytes
// as they were.
fn name_hint_lines(out: &mut Vec<String>, hint: &[query::NameHintRow]) {
    if hint.is_empty() {
        return;
    }
    out.push("hint (file name only, not a test reference):".to_string());
    for row in hint {
        out.push(if row.references.is_empty() {
            format!("  {}  references no indexed production symbol", row.file)
        } else {
            format!(
                "  {}  references {}{}",
                row.file,
                row.references.join(", "),
                more_suffix(row.reference_total, row.references.len())
            )
        });
    }
}

/// `--compact` `devscout tests` output.
///
/// The same model with the def ids and the
/// per-file indentation dropped: one header line carrying every count, then one
/// line per file. The `x`/`h` line marker is the compact renderer's existing
/// convention for a guess.
pub fn render_tests_compact(model: &query::TestsModel) -> String {
    let heur = if model.heuristic_file_count != 0 {
        format!(" heuristic={}", model.heuristic_file_count)
    } else {
        String::new()
    };
    let files = model.rows.len();
    let mut out: Vec<String> = vec![format!(
        "tests {} files={} refs={}{heur}{}",
        model.symbol,
        model.test_file_count,
        model.ref_count,
        count_suffix("files", files, files)
    )];
    for row in &model.name_hint {
        let refs = if row.references.is_empty() {
            "none".to_string()
        } else {
            format!(
                "{}{}",
                row.references.join(","),
                more_suffix(row.reference_total, row.references.len())
            )
        };
        out.push(format!("hint {} -> {refs}", row.file));
    }
    for r in &model.rows {
        let lines = r
            .lines
            .iter()
            .map(|l| format!("{l}{}", compact_marker(r.heuristic, r.tier)))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("{} {lines}", r.file));
    }
    // Same marker-plus-path-to-full-row shape `refs`' own compact `bus-hop`
    // block established, reused verbatim rather than respelled here.
    let file_of_bus: fn(&query::BusHopRow) -> &str = |r| r.file.as_str();
    let line_bus = |r: &query::BusHopRow| format!("{}?", r.line);
    compact_block(
        &mut out,
        "bus-hop (? = possible route, rerun without --compact for the full row)",
        Some(&model.bus),
        file_of_bus,
        line_bus,
    );
    out.join("\n")
}
