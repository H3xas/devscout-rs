use super::*;

fn resolved_refs(index: &GraphIndex, seed: &str) -> RefsModel {
    match build_refs_model(
        index,
        seed,
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(model) => model,
        other => panic!("expected a resolved refs model, got {other:?}"),
    }
}

fn empty_bus() -> Table<BusHopRow> {
    Table {
        total: 0,
        dropped: 0,
        rows: Vec::new(),
    }
}

// One interface referenced from one other file: enough for both suggestions to
// have a target.
fn one_interface_index<'g>(root: &Path, graph: &'g graph::Graph) -> GraphIndex<'g> {
    let _ = root;
    load_graph_index(graph, root)
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

fn interface_graph(extra_defs: Vec<graph::Def>, extra_edges: Vec<graph::Edge>) -> graph::Graph {
    let mut defs = vec![def(
        "App.IFoo",
        "IFoo",
        "App",
        "interface",
        "src/IFoo.cs",
        3,
    )];
    defs.extend(extra_defs);
    let mut edges = vec![uses_type("src/Bar.cs", 9, "App.IFoo", "src/IFoo.cs")];
    edges.extend(extra_edges);
    make_graph(defs, edges)
}

#[test]
fn refs_suggests_the_tests_that_reach_it_then_the_blast_radius_of_its_file() {
    let root = temp_repo_root("next-refs");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);
    let model = resolved_refs(&index, "IFoo");

    assert_eq!(
        refs_next(&index, &model),
        vec![
            argv(&["tests", "App.IFoo"]),
            argv(&["impact", "src/IFoo.cs"]),
        ]
    );
}

// The suggestion filter runs per candidate, not per answer: a seed that
// resolves as one name can still be the wrong next query when it is spelled
// as another, and that candidate alone is the one to drop.
#[test]
fn a_candidate_whose_target_is_ambiguous_is_dropped_and_its_sibling_is_kept() {
    let root = temp_repo_root("next-ambiguous");
    let g = interface_graph(
        vec![
            def("A.Widget", "Widget", "A", "class", "src/A.cs", 3),
            def("B.Widget", "Widget", "B", "class", "src/B.cs", 5),
        ],
        Vec::new(),
    );
    let index = one_interface_index(&root, &g);
    let mut model = resolved_refs(&index, "IFoo");
    // The bare name two types declare: resolving it again is exactly the
    // ambiguity the answer itself refused to guess between.
    model.id = "Widget".to_string();

    assert_eq!(
        refs_next(&index, &model),
        vec![argv(&["impact", "src/IFoo.cs"])]
    );
}

#[test]
fn a_candidate_that_no_longer_resolves_is_dropped() {
    let root = temp_repo_root("next-not-found");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);
    let mut model = resolved_refs(&index, "IFoo");
    model.id = "NothingDeclaresThis".to_string();

    assert_eq!(
        refs_next(&index, &model),
        vec![argv(&["impact", "src/IFoo.cs"])]
    );
}

// A member answers under the type that declares it, so its own dotted id is
// not a def id at all -- the member fallback is what keeps the suggestion.
#[test]
fn a_member_seed_survives_the_filter_through_the_member_fallback() {
    let root = temp_repo_root("next-member");
    let mut g = interface_graph(Vec::new(), Vec::new());
    g.names = vec![graph::GraphName {
        name: "Stow".to_string(),
        kind: "method".to_string(),
        file: "src/Bin.cs".to_string(),
        line: 7,
        owner: "App.IFoo".to_string(),
    }];
    let index = one_interface_index(&root, &g);
    let mut model = resolved_refs(&index, "IFoo");
    model.id = "App.IFoo.Stow".to_string();

    assert_eq!(
        refs_next(&index, &model),
        vec![
            argv(&["tests", "App.IFoo.Stow"]),
            argv(&["impact", "src/IFoo.cs"]),
        ]
    );
}

// `impact` reads a file argument through its own resolver, so a row whose
// blast radius names a file no def declares offers nothing to run.
#[test]
fn an_impact_candidate_naming_a_file_the_index_does_not_hold_is_dropped() {
    let root = temp_repo_root("next-impact-file");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);
    let mut model = resolved_refs(&index, "IFoo");
    model.sites = vec![DefSite {
        file: "src/Elsewhere.cs".to_string(),
        line: 1,
    }];

    assert_eq!(
        refs_next(&index, &model),
        vec![argv(&["tests", "App.IFoo"])]
    );
}

#[test]
fn tests_suggests_the_blast_radius_of_the_declaring_file() {
    let root = temp_repo_root("next-tests");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);
    let model = TestsModel {
        query: "IFoo".to_string(),
        symbol: "App.IFoo".to_string(),
        def_files: vec!["src/IFoo.cs".to_string()],
        rows: Vec::new(),
        test_file_count: 0,
        ref_count: 0,
        heuristic_file_count: 0,
        heuristic_ref_count: 0,
        bus: empty_bus(),
    };

    assert_eq!(
        tests_next(&index, &model),
        vec![argv(&["impact", "src/IFoo.cs"])]
    );
}

#[test]
fn impact_suggests_the_references_of_the_highest_ranked_rows_first_top_symbol() {
    let root = temp_repo_root("next-impact");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);
    let model = match build_impact_model(
        &index,
        "IFoo",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(model) => model,
        other => panic!("expected a resolved impact model, got {other:?}"),
    };
    let top = model.rows[0]
        .top_symbol_id
        .clone()
        .expect("a reached file kept its symbol");

    assert_eq!(impact_next(&index, &model), vec![argv(&["refs", &top])]);
}

#[test]
fn impact_reaching_nothing_suggests_nothing() {
    let root = temp_repo_root("next-impact-empty");
    let g = make_graph(
        vec![def(
            "App.Island",
            "Island",
            "App",
            "class",
            "src/Island.cs",
            3,
        )],
        Vec::new(),
    );
    let index = one_interface_index(&root, &g);
    let model = match build_impact_model(
        &index,
        "Island",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(model) => model,
        other => panic!("expected a resolved impact model, got {other:?}"),
    };

    assert!(model.rows.is_empty(), "the seed reaches nothing");
    assert!(impact_next(&index, &model).is_empty());
}

fn name_row(name: &str, kind: &str, file: &str, line: usize, owner: &str) -> graph::GraphName {
    graph::GraphName {
        name: name.to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        owner: owner.to_string(),
    }
}

#[test]
fn find_reads_the_declaration_its_first_row_names() {
    let root = temp_repo_root("next-find");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);

    assert_eq!(
        find_next(&index, &name_row("IFoo", "interface", "src/IFoo.cs", 3, "")),
        vec![argv(&["read", "App.IFoo"])]
    );
}

#[test]
fn find_reads_the_declaring_type_of_a_member_row() {
    let root = temp_repo_root("next-find-member");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);

    // A member is not a def of its own, so the row's owner is the declaration
    // there is to read.
    assert_eq!(
        find_next(
            &index,
            &name_row("Stow", "method", "src/Bin.cs", 7, "App.IFoo")
        ),
        vec![argv(&["read", "App.IFoo"])]
    );
}

#[test]
fn find_suggests_nothing_for_a_name_that_declares_nothing() {
    let root = temp_repo_root("next-find-none");
    let g = interface_graph(Vec::new(), Vec::new());
    let index = one_interface_index(&root, &g);

    // A markup name is a name the index holds and nothing more.
    assert!(find_next(
        &index,
        &name_row("Panel", "markup-class", "src/Panel.xaml", 2, "")
    )
    .is_empty());
    // A code name the def table does not carry at that site.
    assert!(find_next(
        &index,
        &name_row("IFoo", "interface", "src/IFoo.cs", 99, "")
    )
    .is_empty());
}

// A Rust item's id carries the path characters a shell would otherwise read
// as its own, and it still has to survive the exact-id rung of the resolver.
#[test]
fn find_reads_a_rust_items_own_id() {
    let root = temp_repo_root("next-find-rust");
    let g = make_graph(
        vec![def(
            "src/shelf.rs#Widget::new",
            "new",
            "shelf",
            "fn",
            "src/shelf.rs",
            5,
        )],
        Vec::new(),
    );
    let index = one_interface_index(&root, &g);

    assert_eq!(
        find_next(&index, &name_row("new", "fn", "src/shelf.rs", 5, "")),
        vec![argv(&["read", "src/shelf.rs#Widget::new"])]
    );
}

// A path or id beginning with `--` is the one target the query verbs cannot be
// handed as a positional: they read it as a flag and come back a usage error.
// Suggesting it would hand the caller a command that cannot run.
#[test]
fn a_target_no_verb_would_read_as_a_seed_is_dropped() {
    let root = temp_repo_root("next-dashed");
    let mut g = interface_graph(Vec::new(), Vec::new());
    g.defs.push(def(
        "Dash.Dashed",
        "Dashed",
        "Dash",
        "class",
        "--dashed.cs",
        3,
    ));
    let index = one_interface_index(&root, &g);

    assert!(merge_suggestions(vec![vec![argv(&["impact", "--dashed.cs"])]]).is_empty());
    // A single leading dash is an ordinary argument and survives.
    assert_eq!(
        merge_suggestions(vec![vec![argv(&["impact", "-dashed.cs"])]]),
        vec![argv(&["impact", "-dashed.cs"])]
    );
}
