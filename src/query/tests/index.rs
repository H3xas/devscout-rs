use super::*;

// --- 1: load_graph_index + manifest flagging ---

#[test]
fn load_graph_index_joins_def_files_against_the_manifest_and_flags_a_graph_file_missing_from_it() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    assert!(
        index.by_id.contains_key("App.Ghost.NotInManifest"),
        "the def must still be indexed, not dropped"
    );
    assert!(
        index.flagged_files.contains("Ghost/NotInManifest.cs"),
        "its file must be flagged as missing from the manifest"
    );
    assert_eq!(
        index.flagged_files.len(),
        1,
        "every other def/edge file is in the manifest and must not be flagged"
    );
    assert!(index.manifest_present);
}

#[test]
fn load_graph_index_hub_indegree_counts_distinct_referring_files_across_both_edge_kinds() {
    let graph = hub_fixture_graph();
    let root = hub_fixture_root();
    let index = load_graph_index(&graph, &root);
    assert_eq!(index.hub_indegree.get("Api/Startup.cs").copied(), Some(4));
    assert_eq!(
        index.hub_indegree.get("Core/Shared.cs").copied(),
        Some(5),
        "the heuristic referrer counts too -- a hub is reached by either kind"
    );
    assert_eq!(index.hub_indegree.get("Core/Plain.cs").copied(), Some(1));
    assert!(
        index.hub_indegree.get("Api/A.cs").is_none(),
        "a file nothing references carries no entry at all"
    );
}

#[test]
fn no_guess_keeps_the_extension_tier_drops_the_scored_one_and_leaves_the_hub_indegree_alone() {
    let defs = vec![widget_def()];
    let edges = vec![
        uses_member(
            "Consumers/Precise.cs",
            10,
            "App.Core.Widget",
            "Core/Widget.cs",
        ),
        ext_uses_member("Consumers/Ext.cs", 4, "App.Core.Widget", "Core/Widget.cs"),
        heuristic_uses_member("Consumers/Guess.cs", 7, "App.Core.Widget", "Core/Widget.cs"),
    ];
    let root = stage4_root(&defs, &edges, "no-guess-index");
    let g = make_graph(defs, edges);

    let full = load_graph_index(&g, &root);
    let narrowed = load_graph_index_with(
        &g,
        &root,
        IndexOptions {
            include_guesses: false,
            include_dispatch: true,
            include_bus: true,
        },
    );
    assert_eq!(
        full.heuristic_inbound["App.Core.Widget"].uses_member.len(),
        2,
        "the default index carries both tiers"
    );
    let kept = &narrowed.heuristic_inbound["App.Core.Widget"].uses_member;
    assert_eq!(kept.len(), 1, "only the extension tier survives --no-guess");
    assert_eq!(
        edge_loc(&g.edges[kept[0]]).0,
        "Consumers/Ext.cs",
        "and it is the extension edge that survived, not whichever came first"
    );

    // The hub brake counts a referring FILE, not a believed edge: a guess
    // still proves the two files touch, so narrowing the ANSWER must not
    // quietly widen the WALK by making a hub look less connected.
    assert_eq!(full.hub_indegree.get("Core/Widget.cs").copied(), Some(3));
    assert_eq!(
        narrowed.hub_indegree.get("Core/Widget.cs").copied(),
        Some(3),
        "--no-guess drops rows, never the in-degree the hub brake reads"
    );

    let text = |index: &GraphIndex| {
        let model = match build_refs_model(
            index,
            "Widget",
            false,
            DEFAULT_CAP,
            INBOUND_CAP,
            OUTBOUND_CAP,
            false,
        ) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };
        crate::render::render_refs_text(&model)
    };
    let out = text(&full);
    assert!(
        out.contains("    Consumers/Ext.cs:4  uses-member (extension)"),
        "{out}"
    );
    assert!(
        out.contains("    Consumers/Guess.cs:7  uses-member (guess)"),
        "{out}"
    );

    let out = text(&narrowed);
    assert!(out.contains("  uses-member (2):"), "{out}");
    assert!(
        out.contains("    Consumers/Ext.cs:4  uses-member (extension)"),
        "the surviving tier still says which tier it is\n{out}"
    );
    assert!(
        !out.contains("Consumers/Guess.cs"),
        "a refused guess leaves no row behind\n{out}"
    );
}

/// The heuristic adjacency really is SEPARATE: a graph whose only
/// uses-type edge is tagged leaves the precise inbound table empty, so no
/// consumer that never asked for guesses can see one.
#[test]
fn stage4_heuristic_edges_never_enter_the_precise_adjacency() {
    let defs = vec![widget_def()];
    let edges = vec![heuristic_uses_type(
        "Consumers/Guess.cs",
        7,
        "App.Core.Widget",
        "Core/Widget.cs",
    )];
    let root = stage4_root(&defs, &edges, "stage4-adjacency");
    let g = make_graph(defs, edges);
    let index = load_graph_index(&g, &root);
    assert!(
        index.inbound.get("App.Core.Widget").is_none(),
        "the precise inbound table never sees a tagged edge"
    );
    assert_eq!(
        index
            .heuristic_inbound
            .get("App.Core.Widget")
            .map(|e| e.uses_type.len()),
        Some(1),
        "and the heuristic one holds it, keyed by the same def id"
    );
    assert!(index.outbound_by_file.get("Consumers/Guess.cs").is_none());
    assert_eq!(
        index
            .heuristic_outbound_by_file
            .get("Consumers/Guess.cs")
            .map(|e| e.uses_type.len()),
        Some(1)
    );
}

#[test]
fn test_defs_by_file_registers_every_declaring_site_and_never_a_file_without_a_test_def() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());
    assert!(index
        .test_defs_by_file
        .contains_key("tests/OrderServiceTests.cs"));
    assert!(
        index
            .test_defs_by_file
            .contains_key("tests/Partial.Extra.cs"),
        "a partial test class registers its second declaring file too"
    );
    assert!(
        !index.test_defs_by_file.contains_key("tests/Fakes.cs"),
        "a file is a test file only because of the attribute"
    );
    assert!(!index.test_defs_by_file.contains_key("src/OrderService.cs"));
}
