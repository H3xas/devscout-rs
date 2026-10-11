// The four TS-family tables of a `refs`/`read` answer's `inbound` or
// `outbound` object, split out of `json.rs` to keep that file under its flat
// line limit. Each joins the object after every older key, and only when its
// own total is non-zero -- the rule `implements`/`overrides` follow -- so an
// answer with no TS edge keeps the exact bytes it had before these existed.

use crate::query;
use crate::query::why::Why;

use super::{j_table, RowKind, J};

// The edge-kind word each table is keyed by, and the `why` its rows carry.
fn why_of(kind: &str) -> Why {
    match kind {
        "call" => Why::Call,
        "jsx-use" => Why::JsxUse,
        "dispatch" => Why::Dispatch,
        _ => Why::Import,
    }
}

pub(super) fn ts_table_fields<R>(
    tables: &query::TsTables<R>,
    row: impl Fn(&R, RowKind) -> J + Copy,
) -> Vec<(&'static str, J)> {
    tables
        .named()
        .into_iter()
        .filter(|(_, table)| table.total != 0)
        .map(|(kind, table)| {
            let why = why_of(kind);
            (kind, j_table(table, move |r| row(r, RowKind::Ts(why))))
        })
        .collect()
}
