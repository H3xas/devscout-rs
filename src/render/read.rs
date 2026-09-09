use crate::query;

use super::refs::{render_refs_compact, render_refs_text};

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
    if out.len() > 1 {
        out[1] = read_def_line(model);
    }
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
    if out.len() > 1 {
        out[1] = read_def_line(model);
    }
    out.join("\n")
}
