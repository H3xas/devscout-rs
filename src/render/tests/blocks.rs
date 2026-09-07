use super::*;

// --- Missing-table tolerance, tested directly on the helpers (see module
// header: unreachable via the typed RefsModel/ImpactModel, but ported). ---

#[test]
fn ref_kind_block_missing_table_prints_nothing() {
    let mut out: Vec<String> = vec!["before".to_string()];
    ref_kind_block::<InboundRow>(&mut out, "inherits", None, |r| {
        format!("{}:{}", r.file, r.line)
    });
    assert_eq!(out, vec!["before".to_string()]);
}

#[test]
fn compact_block_missing_table_prints_nothing() {
    let mut out: Vec<String> = vec!["before".to_string()];
    compact_block::<InboundRow>(
        &mut out,
        "in:inherits",
        None,
        |r| r.file.as_str(),
        |r| r.line.to_string(),
    );
    assert_eq!(out, vec!["before".to_string()]);
}

#[test]
fn ref_kind_block_present_but_empty_table_still_prints_header() {
    let mut out: Vec<String> = Vec::new();
    let t: Table<InboundRow> = table(vec![], 0);
    ref_kind_block(&mut out, "inherits", Some(&t), |r: &InboundRow| {
        format!("{}:{}", r.file, r.line)
    });
    assert_eq!(out, vec!["  inherits (0):".to_string()]);
}

#[test]
fn compact_block_present_but_empty_table_prints_nothing_unlike_ref_kind_block() {
    let mut out: Vec<String> = Vec::new();
    let t: Table<InboundRow> = table(vec![], 0);
    compact_block(
        &mut out,
        "in:inherits",
        Some(&t),
        |r: &InboundRow| r.file.as_str(),
        |r: &InboundRow| r.line.to_string(),
    );
    assert!(out.is_empty());
}

#[test]
fn ref_kind_block_dropped_note_only_appears_when_nonzero() {
    let mut out: Vec<String> = Vec::new();
    let t = table(
        vec![InboundRow {
            file: "a.cs".into(),
            line: 1,
            heuristic: false,
            tier: None,
            source: String::new(),
        }],
        2,
    );
    ref_kind_block(&mut out, "inherits", Some(&t), |r: &InboundRow| {
        format!("{}:{}", r.file, r.line)
    });
    assert_eq!(out[0], "  inherits (3, 2 dropped):");
}

#[test]
fn rle_collapses_consecutive_equal_runs_only() {
    let values = vec![
        "5".to_string(),
        "5".to_string(),
        "3".to_string(),
        "3".to_string(),
        "3".to_string(),
        "1".to_string(),
    ];
    assert_eq!(
        rle(&values),
        vec!["5x2".to_string(), "3x3".to_string(), "1".to_string()]
    );
}
