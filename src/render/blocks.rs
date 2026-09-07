use crate::query;

// Run-length encoding: collapses a run of `n > 1` identical consecutive strings
// into `"{value}x{n}"`; a run of length 1 passes through unchanged. Not
// order-sensitive beyond adjacency -- callers are responsible for handing it
// already-grouped/sorted input.
pub(crate) fn rle(values: &[String]) -> Vec<String> {
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
fn group_by_file<R>(
    rows: &[R],
    file_of: fn(&R) -> &str,
    line_fmt: impl Fn(&R) -> String,
) -> Vec<String> {
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
pub(crate) fn ref_kind_block<R>(
    out: &mut Vec<String>,
    label: &str,
    table: Option<&query::Table<R>>,
    row_fmt: impl Fn(&R) -> String,
) {
    let Some(t) = table else { return };
    let dropped_note = if t.dropped != 0 {
        format!(", {} dropped", t.dropped)
    } else {
        String::new()
    };
    out.push(format!("  {label} ({}{dropped_note}):", t.total));
    for r in &t.rows {
        out.push(format!("    {}", row_fmt(r)));
    }
}

// Used by `--compact`: a missing OR present-but-EMPTY table prints nothing at
// all, unlike `ref_kind_block` above.
pub(crate) fn compact_block<R>(
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
    let dropped_note = if t.dropped != 0 {
        format!(", {} dropped", t.dropped)
    } else {
        String::new()
    };
    out.push(format!("{label} ({}{dropped_note}):", t.total));
    for line in group_by_file(&t.rows, file_of, line_fmt) {
        out.push(format!("  {line}"));
    }
}
