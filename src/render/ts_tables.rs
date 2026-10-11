// The four TS-family tables in the default and `--compact` `refs`/`read`
// renderers. Each prints only when its own total is non-zero, after every
// older table of its direction, so an answer with no TS edge renders exactly
// as it did before these tables existed.

use crate::query;

use super::blocks::{compact_block, ref_kind_block_if_any};
use super::markers::source_suffix;

pub(super) fn inbound_blocks(out: &mut Vec<String>, tables: &query::TsTables<query::InboundRow>) {
    for (kind, table) in tables.named() {
        ref_kind_block_if_any(out, kind, table, |r: &query::InboundRow| {
            format!("{}:{}  {kind}{}", r.file, r.line, source_suffix(&r.source))
        });
    }
}

// An outbound TS row names the target definition, not only its file: one
// import statement binding two names is two rows into the same file, which
// would otherwise read as one row printed twice.
pub(super) fn outbound_blocks(out: &mut Vec<String>, tables: &query::TsTables<query::OutboundRow>) {
    for (kind, table) in tables.named() {
        ref_kind_block_if_any(out, kind, table, |r: &query::OutboundRow| {
            format!(
                "{}:{}  {kind}  -> {}{}",
                r.file,
                r.line,
                r.to,
                source_suffix(&r.source)
            )
        });
    }
}

pub(super) fn compact_blocks<R>(
    out: &mut Vec<String>,
    direction: &str,
    tables: &query::TsTables<R>,
    file_of: fn(&R) -> &str,
    line_of: fn(&R) -> usize,
) {
    for (kind, table) in tables.named() {
        compact_block(
            out,
            &format!("{direction}:{kind}"),
            Some(table),
            file_of,
            |r| line_of(r).to_string(),
        );
    }
}

/// `(total, shown, dropped)` summed over the four tables, for the trailer
/// and summary lines that count every table of an answer.
pub(super) fn sums<R>(tables: &query::TsTables<R>) -> (usize, usize, usize) {
    tables
        .named()
        .into_iter()
        .fold((0, 0, 0), |(total, shown, dropped), (_, t)| {
            (total + t.total, shown + t.rows.len(), dropped + t.dropped)
        })
}
