use super::*;

fn empty_inbound() -> InboundTables {
    InboundTables {
        inherits: table(vec![], 0),
        uses_type: table(vec![], 0),
        uses_member: table(vec![], 0),
        implements: table(vec![], 0),
        overrides: table(vec![], 0),
    }
}
fn empty_outbound() -> OutboundTables {
    OutboundTables {
        inherits: table(vec![], 0),
        uses_type: table(vec![], 0),
        uses_member: table(vec![], 0),
        implements: table(vec![], 0),
        overrides: table(vec![], 0),
        imports: table(vec![], 0),
    }
}
fn empty_ambiguous() -> AmbiguousTables {
    AmbiguousTables {
        inbound: table(vec![], 0),
        outbound: table(vec![], 0),
    }
}

fn refs_model(
    inbound: InboundTables,
    outbound: OutboundTables,
    ambiguous: AmbiguousTables,
    manifest_gap: usize,
) -> RefsModel {
    RefsModel {
        query: "IFoo".to_string(),
        id: "App.IFoo".to_string(),
        kind: "interface".to_string(),
        sites: vec![DefSite {
            file: "src/IFoo.cs".to_string(),
            line: 3,
        }],
        inbound,
        outbound: Some(outbound),
        ambiguous,
        bus: table(vec![], 0),
        manifest_gap,
        member_refs: None,
    }
}

// --- render_refs_compact fixtures --------------------------------------

#[test]
fn refs_compact_one_header_per_kind_empty_kinds_print_nothing() {
    let mut inbound = empty_inbound();
    inbound.inherits = table(
        vec![InboundRow {
            file: "src/Foo.cs".into(),
            line: 5,
            heuristic: false,
            tier: None,
            source: String::new(),
            occurrence_index: None,
        }],
        0,
    );
    let mut outbound = empty_outbound();
    outbound.uses_type = table(
        vec![OutboundRow {
            file: "src/IFoo.cs".into(),
            line: 3,
            to_file: "src/Bar.cs".into(),
            to: "App.Bar".into(),
            heuristic: false,
            tier: None,
            source: String::new(),
            occurrence_index: None,
        }],
        0,
    );
    let model = refs_model(inbound, outbound, empty_ambiguous(), 0);
    let out = render_refs_compact(&model);
    assert!(out.contains("in:inherits (1):\n  src/Foo.cs:5"));
    assert!(out.contains("out:uses-type (1):\n  src/IFoo.cs:3"));
    assert!(
        !out.contains("in:uses-type"),
        "a kind with zero rows must not print a header"
    );
    assert!(!out.contains("out:inherits"));
    assert!(!out.contains("out:imports"));
    assert!(
        !out.contains("Bar.cs"),
        "outbound target file is dropped in compact mode -- only path:line survives"
    );
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
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            OutboundRow {
                file: "src/Widget.cs".into(),
                line: 5,
                to_file: "src/IGadget.cs".into(),
                to: "App.IGadget".into(),
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
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
            InboundRow {
                file: "src/Consumers/Big.cs".into(),
                line: 12,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/Consumers/Big.cs".into(),
                line: 40,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/Consumers/Big.cs".into(),
                line: 40,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/Consumers/Small.cs".into(),
                line: 3,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
        ],
        0,
    );
    let model = refs_model(inbound, empty_outbound(), empty_ambiguous(), 0);
    let out = render_refs_compact(&model);
    assert!(out
        .contains("in:uses-type (4):\n  src/Consumers/Big.cs:12,40x2\n  src/Consumers/Small.cs:3"));
    assert_eq!(
        out.matches("src/Consumers/Big.cs").count(),
        1,
        "the repeated file path must appear exactly once"
    );
}

#[test]
fn refs_compact_summary_line_folds_all_counts() {
    let mut inbound = empty_inbound();
    inbound.inherits = table(
        vec![
            InboundRow {
                file: "src/A.cs".into(),
                line: 1,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/B.cs".into(),
                line: 2,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
        ],
        3,
    );
    let mut ambiguous = empty_ambiguous();
    ambiguous.inbound = table(
        vec![AmbiguousRow {
            file: "src/C.cs".into(),
            line: 9,
            origin: "uses-type".into(),
            raw: "Config".into(),
            candidate_count: 2,
        }],
        0,
    );
    let model = refs_model(inbound, empty_outbound(), ambiguous, 4);
    let out = render_refs_compact(&model);
    let last_line = out.lines().last().unwrap();
    assert_eq!(
        last_line,
        "summary: edges=5 shown=2 dropped=3 ambiguous=1 gap=4"
    );
    assert!(out.contains("amb:in (1):\n  src/C.cs:9(candidates=2)"));
    assert!(
        !out.contains("Config"),
        "the raw ambiguous token is not a minimal column and must not appear"
    );
}

// --- the enum member-count line ----------------------------------------

#[test]
fn member_refs_line_renders_after_the_inbound_block_and_caps_the_named_members() {
    let mut model = refs_model(empty_inbound(), empty_outbound(), empty_ambiguous(), 0);
    model.member_refs = Some(query::MemberRefs {
        total: 3,
        member_count: 2,
        members: vec![
            query::MemberRefEntry {
                name: "EnableX".into(),
                count: 2,
            },
            query::MemberRefEntry {
                name: "EnableY".into(),
                count: 1,
            },
        ],
        dropped: 0,
    });
    let out = render_refs_text(&model);
    assert!(
        out.contains("\nmember refs: 3 across 2 member(s): EnableX 2, EnableY 1"),
        "{out}"
    );
    assert!(
        render_refs_compact(&model).contains("\nmem: EnableX=2,EnableY=1\nsummary:"),
        "{}",
        render_refs_compact(&model)
    );

    model.member_refs = Some(query::MemberRefs {
        total: 9,
        member_count: 7,
        members: vec![query::MemberRefEntry {
            name: "A".into(),
            count: 9,
        }],
        dropped: 6,
    });
    assert!(
        render_refs_text(&model).contains("member refs: 9 across 7 member(s): A 9 +6 more"),
        "{}",
        render_refs_text(&model)
    );
    assert!(
        render_refs_compact(&model).contains("mem: A=9,+6"),
        "{}",
        render_refs_compact(&model)
    );
}

#[test]
fn a_model_with_no_member_refs_renders_exactly_as_before() {
    let out = render_refs_text(&refs_model(
        empty_inbound(),
        empty_outbound(),
        empty_ambiguous(),
        0,
    ));
    assert!(!out.contains("member refs:"), "{out}");
    assert!(!render_refs_compact(&refs_model(
        empty_inbound(),
        empty_outbound(),
        empty_ambiguous(),
        0
    ))
    .contains("mem: "));
}

// --- heuristic markers: the render literals ----------------------------

#[test]
fn refs_text_suffixes_only_tagged_rows_and_never_an_imports_row() {
    let mut inbound = empty_inbound();
    inbound.uses_member = table(
        vec![
            InboundRow {
                file: "src/Fact.cs".into(),
                line: 4,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/Guess.cs".into(),
                line: 9,
                heuristic: true,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
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
            tier: None,
            source: String::new(),
            occurrence_index: None,
        }],
        0,
    );
    // An imports row carries no flag at all -- JS reads `r.heuristic` as
    // `undefined` there, which renders no suffix; this side has no field
    // to read, which is the same thing.
    outbound.imports = table(
        vec![ImportRow {
            file: "src/Guess.cs".into(),
            line: 1,
            target: "App.Core".into(),
            source: String::new(),
        }],
        0,
    );

    let out = render_refs_text(&refs_model(inbound, outbound, empty_ambiguous(), 0));
    assert!(out.contains("    src/Fact.cs:4  uses-member\n"), "{out}");
    assert!(
        out.contains("    src/Guess.cs:9  uses-member (heuristic)"),
        "{out}"
    );
    assert!(
        out.contains("    src/Guess.cs:9  uses-member  -> src/Widget.cs (heuristic)"),
        "{out}"
    );
    assert!(
        out.ends_with("    src/Guess.cs:1  imports  -> App.Core"),
        "an imports row is never suffixed\n{out}"
    );
    assert_eq!(out.matches("(heuristic)").count(), 2);
}

#[test]
fn refs_text_marks_an_extension_row_and_a_guess_row_with_their_own_words() {
    let row = |file: &str, line: usize, tier: Option<HeuristicTier>| InboundRow {
        file: file.into(),
        line,
        heuristic: tier.is_some(),
        tier,
        source: String::new(),
        occurrence_index: None,
    };
    let mut inbound = empty_inbound();
    inbound.uses_member = table(
        vec![
            row("src/Fact.cs", 4, None),
            row("src/Ext.cs", 7, Some(HeuristicTier::Ext)),
            row("src/Guess.cs", 9, Some(HeuristicTier::Guess)),
        ],
        0,
    );
    let out = render_refs_text(&refs_model(inbound, empty_outbound(), empty_ambiguous(), 0));
    assert!(out.contains("    src/Fact.cs:4  uses-member\n"), "{out}");
    assert!(
        out.contains("    src/Ext.cs:7  uses-member (extension)"),
        "{out}"
    );
    assert!(
        out.contains("    src/Guess.cs:9  uses-member (guess)"),
        "{out}"
    );
    // The two tiers are told apart by their own words, and neither borrows
    // the umbrella one: a row saying `(heuristic)` now means only "guessed,
    // tier unknown", which no row built from a schema-2 graph can be.
    assert_eq!(out.matches("(heuristic)").count(), 0, "{out}");
    // The HEADER keeps the umbrella word regardless -- one vocabulary with
    // two levels of detail, not two competing labels.
    assert!(out.contains("  uses-member (3):"), "{out}");
}

#[test]
fn refs_compact_marks_a_heuristic_row_with_a_trailing_h_and_rle_keeps_them_distinct() {
    let mut inbound = empty_inbound();
    inbound.uses_member = table(
        vec![
            InboundRow {
                file: "src/A.cs".into(),
                line: 5,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/A.cs".into(),
                line: 5,
                heuristic: true,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            InboundRow {
                file: "src/A.cs".into(),
                line: 5,
                heuristic: true,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
        ],
        0,
    );
    let out = render_refs_compact(&refs_model(inbound, empty_outbound(), empty_ambiguous(), 0));
    assert!(
        out.contains("in:uses-member (3):\n  src/A.cs:5,5hx2"),
        "`5` and `5h` are distinct RLE entries\n{out}"
    );
}

#[test]
fn refs_compact_marks_ext_with_x_and_guess_with_h_and_rle_keeps_them_distinct() {
    let row = |line: usize, tier: Option<HeuristicTier>| InboundRow {
        file: "src/A.cs".into(),
        line,
        heuristic: tier.is_some(),
        tier,
        source: String::new(),
        occurrence_index: None,
    };
    let mut inbound = empty_inbound();
    inbound.uses_member = table(
        vec![
            row(5, None),
            row(5, Some(HeuristicTier::Ext)),
            row(5, Some(HeuristicTier::Ext)),
            row(5, Some(HeuristicTier::Guess)),
        ],
        0,
    );
    let out = render_refs_compact(&refs_model(inbound, empty_outbound(), empty_ambiguous(), 0));
    // Three distinct RLE entries off ONE line number: the marker belongs to
    // the value, so the run-length `x2` on `5x` reads `5xx2` and still
    // collapses only rows that agree on both line AND tier.
    assert!(
        out.contains("in:uses-member (4):\n  src/A.cs:5,5xx2,5h"),
        "{out}"
    );
}
