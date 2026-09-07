use super::*;

// --- 9: build_impact_model hop limit ---

#[test]
fn build_impact_model_hop_limit_honored() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);

    let one_hop = match build_impact_model(
        &index,
        "IWidget",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let mut files: Vec<&str> = one_hop.rows.iter().map(|r| r.file.as_str()).collect();
    files.sort();
    assert_eq!(
        files,
        vec![
            "Consumers/Holder.cs",
            "Widgets/Impl/OtherImpl.cs",
            "Widgets/Impl/WidgetImpl.cs"
        ]
    );
    assert!(
        !one_hop.rows.iter().any(|r| r.file == "Consumers/TwoHop.cs"),
        "TwoHop.cs is 2 hops away and must not appear at hops=1"
    );

    let two_hop = match build_impact_model(
        &index,
        "IWidget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let two_hop_row = two_hop
        .rows
        .iter()
        .find(|r| r.file == "Consumers/TwoHop.cs")
        .expect("TwoHop.cs must appear at hops=2");
    assert_eq!(two_hop_row.hop, 2);
    assert_eq!(
        two_hop_row.top_symbols,
        vec!["WidgetImpl".to_string()],
        "reached via WidgetImpl, not IWidget directly"
    );
}

// --- 10: file-path seed ---

#[test]
fn build_impact_model_accepts_a_file_path_seeding_every_def_in_it() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Widgets/IWidget.cs",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(model.kind, SeedKind::File);
    assert_eq!(model.seed_files, vec!["Widgets/IWidget.cs".to_string()]);
    let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
    files.sort();
    assert_eq!(
        files,
        vec![
            "Consumers/Holder.cs",
            "Widgets/Impl/OtherImpl.cs",
            "Widgets/Impl/WidgetImpl.cs"
        ]
    );
}

// --- 11: unknown file path -> notfound, not treated as a symbol ---

#[test]
fn build_impact_model_unknown_file_path_is_reported_not_silently_a_symbol() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    match build_impact_model(
        &index,
        "Nowhere/Missing.cs",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::NotFound { kind } => assert_eq!(kind, SeedKind::File),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// --- 12: ranking: 1-hop outranks 2-hop; nothing dropped below cap ---

#[test]
fn build_impact_model_ranking_orders_direct_ahead_of_indirect_never_drops_below_cap() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "IWidget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let score_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap().score;
    let hop_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap().hop;
    assert!(
        score_of("Widgets/Impl/WidgetImpl.cs") > score_of("Consumers/TwoHop.cs"),
        "1-hop dependent must outrank the 2-hop one"
    );
    assert_eq!(hop_of("Consumers/TwoHop.cs"), 2);
    assert_eq!(model.dropped, 0);
    assert_eq!(
        model.total_affected,
        model.rows.len(),
        "nothing filtered beyond the (unhit) cap"
    );
}

// --- 13: ranking determinism across repeated runs ---

#[test]
fn build_impact_model_ranking_deterministic_across_repeated_runs() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let a: Vec<String> = match build_impact_model(
        &index,
        "IWidget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m.rows.into_iter().map(|r| r.file).collect(),
        other => panic!("expected Resolved, got {other:?}"),
    };
    let b: Vec<String> = match build_impact_model(
        &index,
        "IWidget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m.rows.into_iter().map(|r| r.file).collect(),
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(a, b);
}
