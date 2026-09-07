use std::path::Path;

use super::index::GraphIndex;
use super::refs::{build_refs_model_inner, RefsModel, RefsResult};
use super::refs_tables::{DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP};

// ============================================================================
// build_read_model -- the declaration span plus the same inbound answer.
// ============================================================================

/// The declaration span of a resolved def: the file, its 1-based start and
/// end lines, and the VERBATIM source text of those lines (no trim, no clip
///
/// -- a span is quoted, not summarized). `end_line` is what tree-sitter
/// delimited as the whole declaration node; when the file has changed since
/// the map, `end_line` is clamped to the file's current last line rather
/// than guessed upward, and a file that shrank below the span's start yields
/// no span at all.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadSpan {
    /// The file value.
    pub file: String,
    /// The start line value.
    pub start_line: usize,
    /// The end line value.
    pub end_line: usize,
    /// The source value.
    pub source: String,
}

/// The resolved `read` result for one symbol: everything `refs` answers,
///
/// plus the declaration span when one is on record. `span` is `None` for a
/// def with no recorded end (TS defs, a graph written before end lines were
/// extracted) and never faked from a start line alone.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadModel {
    /// The refs value.
    pub refs: RefsModel,
    /// The span value.
    pub span: Option<ReadSpan>,
}

/// The outcome of a `read` query -- the `refs` outcome set unchanged, so the
/// ambiguity and zero-hit discipline are literally the same code path's.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadResult {
    /// Represents `Resolved`.
    Resolved(Box<ReadModel>),
    /// A bare-member answer: one resolved-shaped model per declaring type.
    Members(Vec<RefsModel>),
    /// Represents `Ambiguous`.
    Ambiguous(Vec<String>),
    /// Represents `NotFound`.
    NotFound,
}

// Slices the mapped span out of the current file text. Fails closed: an
// unreadable file or a span past EOF degrades to "no span", which the
// renderer shows as a start-line-only answer, never as invented text.
fn read_span(root: &Path, model: &RefsModel, end_line: usize) -> Option<ReadSpan> {
    let site = model.sites.first()?;
    if end_line < site.line {
        return None;
    }
    let body = std::fs::read_to_string(root.join(&site.file)).ok()?;
    let lines: Vec<&str> = body.split('\n').collect();
    if site.line < 1 || site.line > lines.len() {
        return None;
    }
    // The map may be behind the working tree; quoting past EOF would invent
    // lines, so the end clamps to what the file actually has.
    let end = end_line.min(lines.len());
    let mut start_text = lines[site.line - 1];
    if site.line == 1 {
        start_text = start_text.strip_prefix('\u{feff}').unwrap_or(start_text);
    }
    let mut source = String::from(start_text);
    for line in &lines[site.line..end] {
        source.push('\n');
        source.push_str(line);
    }
    Some(ReadSpan {
        file: site.file.clone(),
        start_line: site.line,
        end_line: end,
        source,
    })
}

/// `read` = `refs` resolution + capped-and-ranked inbound machinery, reused
///
/// wholesale (`out` stays off -- the verb answers "what declares this and
/// what points at it"), plus the one new fact: the declaration span.
pub fn build_read_model(index: &GraphIndex, query: &str) -> ReadResult {
    match build_refs_model_inner(
        index,
        query,
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
        true,
    ) {
        RefsResult::Resolved(m) => {
            let end_line = index.def(&m.id).map(|d| d.end_line).unwrap_or(0);
            let span = if end_line > 0 {
                read_span(&index.root, &m, end_line)
            } else {
                None
            };
            ReadResult::Resolved(Box::new(ReadModel { refs: m, span }))
        }
        RefsResult::Members(models) => ReadResult::Members(models),
        RefsResult::Ambiguous(ids) => ReadResult::Ambiguous(ids),
        RefsResult::NotFound => ReadResult::NotFound,
    }
}
