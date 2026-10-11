use super::*;

fn in_row(file: &str, line: usize, source: &str) -> InboundRow {
    InboundRow {
        file: file.into(),
        line,
        heuristic: false,
        tier: None,
        source: source.into(),
        occurrence_index: None,
    }
}

fn out_row(line: usize, to: &str) -> OutboundRow {
    OutboundRow {
        file: "src/page.tsx".into(),
        line,
        to_file: to.split('#').next().unwrap().into(),
        to: to.into(),
        heuristic: false,
        tier: None,
        source: String::new(),
        occurrence_index: None,
    }
}

fn model(
    inbound: query::TsTables<InboundRow>,
    outbound: query::TsTables<OutboundRow>,
) -> RefsModel {
    let empty_in = || table(Vec::<InboundRow>::new(), 0);
    let empty_out = || table(Vec::<OutboundRow>::new(), 0);
    RefsModel {
        query: "useThing".into(),
        id: "src/hooks.ts#useThing".into(),
        kind: "function".into(),
        sites: vec![DefSite {
            file: "src/hooks.ts".into(),
            line: 1,
        }],
        inbound: InboundTables {
            inherits: empty_in(),
            uses_type: empty_in(),
            uses_member: empty_in(),
            implements: empty_in(),
            overrides: empty_in(),
            ts: inbound,
        },
        outbound: Some(OutboundTables {
            inherits: empty_out(),
            uses_type: empty_out(),
            uses_member: empty_out(),
            implements: empty_out(),
            overrides: empty_out(),
            imports: table(Vec::<ImportRow>::new(), 0),
            ts: outbound,
        }),
        ambiguous: AmbiguousTables {
            inbound: table(vec![], 0),
            outbound: table(vec![], 0),
        },
        bus: table(vec![], 0),
        manifest_gap: 0,
        member_refs: None,
    }
}

fn sample() -> RefsModel {
    let inbound = query::TsTables {
        call: table(
            vec![
                in_row("src/a.ts", 4, "useThing();"),
                in_row("src/b.ts", 9, ""),
            ],
            2,
        ),
        import: table(
            vec![in_row("src/a.ts", 1, "import { useThing } from './hooks';")],
            0,
        ),
        ..Default::default()
    };
    let outbound = query::TsTables {
        import: table(
            vec![
                out_row(2, "src/model.ts#Item"),
                out_row(2, "src/model.ts#ItemId"),
            ],
            0,
        ),
        ..Default::default()
    };
    model(inbound, outbound)
}

#[test]
fn text_shows_only_non_empty_ts_tables_after_the_older_kinds_and_counts_their_drops() {
    let text = render_refs_text(&sample());
    assert_eq!(
        text,
        "src/hooks.ts#useThing  (function)  5 inbound (showing 3)\n\
         def: src/hooks.ts:1\n\
         inbound:\n  inherits (0):\n  uses-type (0):\n  uses-member (0):\n  \
         call (4, 2 dropped):\n    src/a.ts:4  call  useThing();\n    src/b.ts:9  call\n  \
         import (1):\n    src/a.ts:1  import  import { useThing } from './hooks';\n  \
         +2 more\n\
         outbound:\n  inherits (0):\n  uses-type (0):\n  uses-member (0):\n  imports (0):\n  \
         import (2):\n    src/page.tsx:2  import  -> src/model.ts#Item\n    \
         src/page.tsx:2  import  -> src/model.ts#ItemId"
    );
}

#[test]
fn compact_labels_each_ts_table_by_direction_and_sums_them_in_the_summary() {
    let compact = render_refs_compact(&sample());
    assert_eq!(
        compact,
        "src/hooks.ts#useThing  (function)  5 inbound (showing 3)\n\
         def: src/hooks.ts:1\n\
         in:call (4, 2 dropped):\n  src/a.ts:4\n  src/b.ts:9\n\
         in:import (1):\n  src/a.ts:1\n\
         out:import (2):\n  src/page.tsx:2x2\n\
         summary: edges=7 shown=5 dropped=2 ambiguous=0"
    );
}

#[test]
fn an_answer_without_ts_rows_renders_no_ts_header() {
    let text = render_refs_text(&model(Default::default(), Default::default()));
    for word in ["call", "jsx-use", "dispatch", "import ("] {
        assert!(!text.contains(word), "{word} in {text}");
    }
}
