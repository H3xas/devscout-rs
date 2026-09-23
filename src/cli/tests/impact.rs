use super::*;

#[test]
fn impact_json_leads_with_schema_version_ahead_of_query() {
    let model = query::ImpactModel {
        kind: query::SeedKind::Symbol,
        seed_files: vec!["src/Widget.cs".to_string()],
        hops: 2,
        total_affected: 0,
        rows: vec![],
        dropped: 0,
        manifest_gap: 0,
        heuristic_affected: 0,
        tests_affected: 0,
        braked: vec![],
        braked_files: vec![],
    };
    let json = impact_model_to_json("Widget", &model);
    assert!(
        json.starts_with(r#"{"schema_version":1,"query":"Widget","status":"resolved""#),
        "schema_version leads, ahead of the query key that used to be first: {json}"
    );
}

#[test]
fn impact_json_appends_heuristic_count_then_heuristic_after_score_and_heuristic_affected_after_manifest_gap(
) {
    let row = |file: &str, heuristic: bool| query::ImpactRow {
        file: file.to_string(),
        hop: 1,
        via_count: if heuristic { 0 } else { 1 },
        ambiguous_count: 0,
        top_symbols: vec!["Widget".to_string()],
        top_symbols_more: 0,
        score: 0.5,
        heuristic_count: if heuristic { 2 } else { 0 },
        heuristic,
        tier: None,
        iface_via: vec![],
        from_lines: vec![],
        infra: false,
        why: if heuristic {
            query::Why::UsesMemberGuess
        } else {
            query::Why::UsesMemberPrecise
        },
    };
    let model = query::ImpactModel {
        kind: query::SeedKind::Symbol,
        seed_files: vec!["src/Widget.cs".to_string()],
        hops: 2,
        total_affected: 1,
        rows: vec![row("src/Direct.cs", false), row("src/Guessed.cs", true)],
        dropped: 0,
        manifest_gap: 0,
        heuristic_affected: 1,
        tests_affected: 0,
        braked: vec![],
        braked_files: vec![],
    };
    let json = impact_model_to_json("Widget", &model);
    assert!(
        json.contains(
            r#"{"file":"src/Direct.cs","hop":1,"viaCount":1,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"why":"uses-member-precise"}"#
        ),
        "{json}"
    );
    assert!(
        json.contains(
            r#"{"file":"src/Guessed.cs","hop":1,"viaCount":0,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"heuristicCount":2,"heuristic":true,"why":"uses-member-guess"}"#
        ),
        "{json}"
    );
    assert!(
        json.ends_with(
            r#","dropped":0,"manifestGap":0,"heuristicAffected":1,"testsAffected":0,"outcome":"hit"}"#
        ),
        "{json}"
    );
}

#[test]
fn impact_json_braked_array_carries_hub_file_entries_after_every_interface_entry() {
    let row = |file: &str, infra: bool| query::ImpactRow {
        file: file.to_string(),
        hop: 1,
        via_count: 1,
        ambiguous_count: 0,
        top_symbols: vec!["Widget".to_string()],
        top_symbols_more: 0,
        score: 0.5,
        heuristic_count: 0,
        heuristic: false,
        tier: None,
        iface_via: vec![],
        from_lines: vec![],
        infra,
        why: query::Why::UsesMemberPrecise,
    };
    let model = query::ImpactModel {
        kind: query::SeedKind::Symbol,
        seed_files: vec!["src/Widget.cs".to_string()],
        hops: 2,
        total_affected: 3,
        rows: vec![
            row("src/Direct.cs", false),
            row("Api/Startup.cs", true),
            row("Core/Shared.cs", true),
        ],
        dropped: 0,
        manifest_gap: 0,
        heuristic_affected: 0,
        tests_affected: 0,
        braked: vec![query::BrakedIface {
            iface: "IWidget".to_string(),
            fanin: 12,
        }],
        braked_files: vec![
            query::BrakedFile {
                file: "Api/Startup.cs".to_string(),
                indegree: 40,
            },
            query::BrakedFile {
                file: "Core/Shared.cs".to_string(),
                indegree: 34,
            },
        ],
    };
    let json = impact_model_to_json("Widget", &model);
    // The hub file entries ride the SAME `braked` array as the interface
    // entry, appended after it in `braked_files` order -- one array, no
    // second top-level key.
    assert!(
        json.contains(
            r#""braked":[{"iface":"IWidget","fanin":12},{"file":"Api/Startup.cs","indegree":40},{"file":"Core/Shared.cs","indegree":34}]"#
        ),
        "{json}"
    );
    // `class` is appended LAST on a hub row, right before `why`, exactly like
    // every other conditional key this module pins.
    assert!(
        json.contains(
            r#"{"file":"Api/Startup.cs","hop":1,"viaCount":1,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"class":"infra","why":"uses-member-precise"}"#
        ),
        "{json}"
    );
    // A non-hub row carries no `class` key at all.
    assert!(
        json.contains(
            r#"{"file":"src/Direct.cs","hop":1,"viaCount":1,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"why":"uses-member-precise"}"#
        ),
        "{json}"
    );
}

#[test]
fn impact_json_tier_sits_between_heuristic_and_iface_via() {
    let row = |file: &str, tier: graph::HeuristicTier| query::ImpactRow {
        file: file.to_string(),
        hop: 1,
        via_count: 0,
        ambiguous_count: 0,
        top_symbols: vec!["Widget".to_string()],
        top_symbols_more: 0,
        score: 0.5,
        heuristic_count: 2,
        heuristic: true,
        tier: Some(tier),
        iface_via: vec!["IWidget".to_string()],
        from_lines: vec![],
        infra: false,
        why: match tier {
            graph::HeuristicTier::Ext => query::Why::UsesMemberExt,
            graph::HeuristicTier::Guess => query::Why::UsesMemberGuess,
        },
    };
    let model = query::ImpactModel {
        kind: query::SeedKind::Symbol,
        seed_files: vec!["src/Widget.cs".to_string()],
        hops: 2,
        total_affected: 0,
        rows: vec![
            row("src/Extended.cs", graph::HeuristicTier::Ext),
            row("src/Guessed.cs", graph::HeuristicTier::Guess),
        ],
        dropped: 0,
        manifest_gap: 0,
        heuristic_affected: 2,
        tests_affected: 0,
        braked: vec![],
        braked_files: vec![],
    };
    let json = impact_model_to_json("Widget", &model);
    // `tier` takes the slot right after the flag it refines, which on this
    // row shape means BEFORE `ifaceVia` -- every key that was already
    // appended last stays appended last.
    assert!(
        json.contains(
            r#""score":0.5,"heuristicCount":2,"heuristic":true,"tier":"ext","ifaceVia":["IWidget"],"why":"uses-member-ext"}"#
        ),
        "{json}"
    );
    assert!(
        json.contains(
            r#""score":0.5,"heuristicCount":2,"heuristic":true,"tier":"guess","ifaceVia":["IWidget"],"why":"uses-member-guess"}"#
        ),
        "{json}"
    );
}
