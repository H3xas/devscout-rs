use super::*;

// --- test-coverage stage: stats gains test_def_count, LAST -------------

#[test]
fn stats_appends_test_def_count_last_and_always_writes_it() {
    let stats = Stats {
        def_count: 2,
        file_count: 1,
        edges_by_kind: EdgesByKind::default(),
        ambiguous_count: 0,
        ambiguous_pct: Percent1::zero(),
        unresolved_external_count: 0,
        heuristic_edge_count: 0,
        test_def_count: 0,
        heuristic_by_tier: HeuristicByTier::default(),
        ts: None,
    };
    let json = serde_json::to_string(&stats).unwrap();
    assert!(
        json.ends_with(
            r#""heuristic_edge_count":0,"test_def_count":0,"heuristic_by_tier":{"ext":0,"guess":0}}"#
        ),
        "heuristic_by_tier is LAST, after test_def_count, and both its keys are always written: {json}"
    );
}

/// The same append-last rule for a TS repo, which is the only tree where
/// the two optional tail keys can both appear: `ts` was added first and
/// `heuristic_by_tier` after it, so `heuristic_by_tier` still ends the
/// block and `ts` sits between it and `test_def_count`. A reader diffing a
/// TS graph against an older one sees each new fact appended, never
/// inserted ahead of an older key.
#[test]
fn stats_keeps_heuristic_by_tier_last_even_when_a_ts_block_is_present() {
    let stats = Stats {
        def_count: 0,
        file_count: 0,
        edges_by_kind: EdgesByKind::default(),
        ambiguous_count: 0,
        ambiguous_pct: Percent1::zero(),
        unresolved_external_count: 0,
        heuristic_edge_count: 0,
        test_def_count: 0,
        heuristic_by_tier: HeuristicByTier::default(),
        ts: Some(crate::tsgraph::TsStats {
            ts_file_count: 1,
            ts_def_count: 2,
            external_import_count: 3,
            unresolved_ref_count: 4,
        }),
    };
    let json = serde_json::to_string(&stats).unwrap();
    assert!(
        json.ends_with(concat!(
            r#""test_def_count":0,"#,
            r#""ts":{"ts_file_count":1,"ts_def_count":2,"external_import_count":3,"unresolved_ref_count":4},"#,
            r#""heuristic_by_tier":{"ext":0,"guess":0}}"#
        )),
        "key order must be test_def_count, ts, heuristic_by_tier: {json}"
    );
}

// --- graph.json: `units` is appended after `names` --------------------

fn empty_stats() -> Stats {
    Stats {
        def_count: 0,
        file_count: 0,
        edges_by_kind: EdgesByKind::default(),
        ambiguous_count: 0,
        ambiguous_pct: Percent1::zero(),
        unresolved_external_count: 0,
        heuristic_edge_count: 0,
        test_def_count: 0,
        heuristic_by_tier: HeuristicByTier::default(),
        ts: None,
    }
}

// The two halves of the `units` contract in one place: a graph whose
// repo declares no project must serialize with NO `units` key at all
// (that is what keeps every csproj-less tree byte-identical to what it
// was), and one that does must carry `units` LAST -- after `names` --
// with each row keyed `id`, `name`, `refs`, `test` in that order and the
// last two omitted at their empty/false value. Byte literals on purpose:
// a golden recomputed by the code under test proves nothing.
#[test]
fn graph_omits_units_when_empty_and_appends_them_after_names_otherwise() {
    let mut g = Graph {
        schema_version: GRAPH_SCHEMA_VERSION,
        built_at_head: None,
        defs: Vec::new(),
        edges: Vec::new(),
        stats: empty_stats(),
        names: vec![GraphName {
            name: "A".to_string(),
            kind: "class".to_string(),
            file: "A/A.cs".to_string(),
            line: 1,
            owner: String::new(),
        }],
        units: Vec::new(),
    };

    const WITHOUT_UNITS: &str = concat!(
        r#"{"schema_version":2,"built_at_head":null,"defs":[],"edges":[],"stats":{"def_count":0,"#,
        r#""file_count":0,"edges_by_kind":{"inherits":0,"uses-type":0,"imports":0,"uses-member":0,"#,
        r#""ctor-di":0},"ambiguous_count":0,"ambiguous_pct":0,"unresolved_external_count":0,"#,
        r#""heuristic_edge_count":0,"test_def_count":0,"heuristic_by_tier":{"ext":0,"guess":0}},"#,
        r#""names":[{"name":"A","kind":"class","file":"A/A.cs","line":1}]}"#,
    );
    assert_eq!(
        serde_json::to_string(&g).unwrap(),
        WITHOUT_UNITS,
        "an empty unit list must not emit a `units` key at all"
    );

    g.units = vec![
        GraphUnit {
            id: "A/A.csproj".to_string(),
            name: "A".to_string(),
            refs: vec!["B/B.csproj".to_string()],
            test: false,
        },
        GraphUnit {
            id: "B/B.csproj".to_string(),
            name: "B".to_string(),
            refs: Vec::new(),
            test: false,
        },
        GraphUnit {
            id: "T/T.Tests.csproj".to_string(),
            name: "T.Tests".to_string(),
            refs: vec!["A/A.csproj".to_string()],
            test: true,
        },
    ];

    const WITH_UNITS: &str = concat!(
        r#""names":[{"name":"A","kind":"class","file":"A/A.cs","line":1}],"#,
        r#""units":[{"id":"A/A.csproj","name":"A","refs":["B/B.csproj"]},"#,
        r#"{"id":"B/B.csproj","name":"B"},"#,
        r#"{"id":"T/T.Tests.csproj","name":"T.Tests","refs":["A/A.csproj"],"test":true}]}"#,
    );
    let json = serde_json::to_string(&g).unwrap();
    assert_eq!(
        json,
        format!(
            "{}{}",
            WITHOUT_UNITS
                .strip_suffix(r#""names":[{"name":"A","kind":"class","file":"A/A.cs","line":1}]}"#)
                .unwrap(),
            WITH_UNITS
        ),
        "`units` is appended after `names` and changes nothing before it"
    );

    // And a graph.json written before `units` existed still reads back --
    // the field defaults rather than failing the parse.
    let reparsed: Graph = serde_json::from_str(WITHOUT_UNITS).unwrap();
    assert!(reparsed.units.is_empty());
    let round_tripped: Graph = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped.units, g.units);
}
