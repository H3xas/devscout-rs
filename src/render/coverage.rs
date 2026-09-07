use crate::query;

use super::markers::{compact_marker, heuristic_suffix, test_via_suffix};

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
    out.join("\n")
}

/// `--compact` `devscout tests` output. The same model with the def ids and the
/// per-file indentation dropped: one header line carrying every count, then one
/// line per file. The `x`/`h` line marker is the compact renderer's existing
/// convention for a guess.
pub fn render_tests_compact(model: &query::TestsModel) -> String {
    let heur = if model.heuristic_file_count != 0 {
        format!(" heuristic={}", model.heuristic_file_count)
    } else {
        String::new()
    };
    let mut out: Vec<String> = vec![format!(
        "tests {} files={} refs={}{heur}",
        model.symbol, model.test_file_count, model.ref_count
    )];
    for r in &model.rows {
        let lines = r
            .lines
            .iter()
            .map(|l| format!("{l}{}", compact_marker(r.heuristic, r.tier)))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("{} {lines}", r.file));
    }
    out.join("\n")
}
