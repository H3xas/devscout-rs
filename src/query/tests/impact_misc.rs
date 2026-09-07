use super::*;

#[test]
fn build_impact_model_on_an_enum_file_reaches_files_that_use_only_its_members() {
    let graph = make_graph(
        vec![
            def(
                "App.Flags.Toggles",
                "Toggles",
                "App.Flags",
                "enum",
                "Flags/Toggles.cs",
                3,
            ),
            def(
                "App.Flags.Toggles.EnableX",
                "EnableX",
                "App.Flags",
                "enum-member",
                "Flags/Toggles.cs",
                5,
            ),
            def(
                "App.Run.Runner",
                "Runner",
                "App.Run",
                "class",
                "Run/Runner.cs",
                3,
            ),
        ],
        vec![uses_member(
            "Run/Runner.cs",
            6,
            "App.Flags.Toggles.EnableX",
            "Flags/Toggles.cs",
        )],
    );
    let root = temp_repo_root("enum-member-impact");
    write_manifest_fixture(&root, &["Flags/Toggles.cs", "Run/Runner.cs"]);
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Flags/Toggles.cs",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model.rows.iter().map(|r| (r.file.as_str(), r.top_symbols.clone())).collect::<Vec<_>>(),
        vec![("Run/Runner.cs", vec!["EnableX".to_string()])],
        "the member def is a def of the seed FILE, so a member-only consumer is still in the blast radius"
    );
}

// --- 18: uses-member edge counts toward 1-hop blast radius ---

#[test]
fn build_impact_model_uses_member_edge_counts_toward_one_hop_blast_radius() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Question",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model
            .rows
            .iter()
            .map(|r| r.file.clone())
            .collect::<Vec<_>>(),
        vec!["Consumers/Reader.cs".to_string()]
    );
    assert_eq!(model.rows[0].hop, 1);
}

// --- 19: uses-member edge propagates a second hop through the ordinary type-ref graph ---

#[test]
fn build_impact_model_uses_member_edge_propagates_second_hop() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Question",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
    files.sort();
    assert_eq!(files, vec!["Consumers/Reader.cs", "Consumers/TwoHop.cs"]);
    assert_eq!(
        model
            .rows
            .iter()
            .find(|r| r.file == "Consumers/TwoHop.cs")
            .unwrap()
            .hop,
        2
    );
}

// --- 20: impact_walk + personalized_page_rank -- finite, non-negative, never NaN ---

#[test]
fn impact_walk_and_ppr_never_negative_or_nan() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let walk = impact_walk(
        &index,
        &["App.Widgets.IWidget".to_string()],
        2,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    );
    let mut nodes: SeqSet<String> = SeqSet::new();
    for f in walk.seed_files.iter() {
        nodes.insert(f.clone());
    }
    for f in walk.visited.keys() {
        nodes.insert(f.clone());
    }
    let seeds: Vec<String> = walk.seed_files.iter().cloned().collect();
    let rank = personalized_page_rank(
        &nodes.into_vec(),
        &walk.fwd_adj,
        &seeds,
        DEFAULT_DAMPING,
        DEFAULT_ITERATIONS,
    );
    for v in rank.values() {
        assert!(
            v.is_finite() && *v >= 0.0,
            "every rank must be a finite, non-negative number, got {v}"
        );
    }
}

// --- extra: looks_like_file_path pinned directly (small and worth
// documenting explicitly, incl. the qualified-id quirk). ---

#[test]
fn looks_like_file_path_matches_js_regex_semantics() {
    assert!(looks_like_file_path("Widgets/IWidget.cs"));
    assert!(looks_like_file_path("IWidget.cs"));
    assert!(!looks_like_file_path("IWidget"));
    assert!(
        looks_like_file_path("App.Widgets.IWidget"),
        "qualified id with alnum trailing segment matches the regex, same as JS"
    );
    assert!(
        !looks_like_file_path("Foo."),
        "trailing dot with nothing after it does not match (empty extension)"
    );
    assert!(
        !looks_like_file_path("Foo!"),
        "no dot-then-alnum-to-end anywhere"
    );
}
