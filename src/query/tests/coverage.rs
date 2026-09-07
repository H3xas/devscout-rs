use super::*;

fn resolved_tests(model: TestsResult) -> TestsModel {
    match model {
        TestsResult::Resolved(m) => m,
        other => panic!("expected a resolved tests model, got {other:?}"),
    }
}

/// `OrderService` declared in production unit `App`; test unit
/// `App.Tests` (referencing `App`) holds one attribute-vouched test file
/// and one un-attributed harness file, both referencing `OrderService`
/// twice and once respectively.
fn project_tests_fixture_defs_and_edges() -> (Vec<graph::Def>, Vec<graph::Edge>) {
    (
        vec![
            def(
                "App.Orders.OrderService",
                "OrderService",
                "App.Orders",
                "class",
                "src/App/OrderService.cs",
                3,
            ),
            test_def(
                "App.Orders.Tests.OrderServiceTests",
                "OrderServiceTests",
                "tests/App.Tests/OrderServiceTests.cs",
                5,
                &["Totals"],
            ),
        ],
        vec![
            uses_type(
                "tests/App.Tests/OrderServiceTests.cs",
                10,
                "App.Orders.OrderService",
                "src/App/OrderService.cs",
            ),
            uses_type(
                "tests/App.Tests/FakeServer.cs",
                12,
                "App.Orders.OrderService",
                "src/App/OrderService.cs",
            ),
            uses_type(
                "tests/App.Tests/FakeServer.cs",
                34,
                "App.Orders.OrderService",
                "src/App/OrderService.cs",
            ),
        ],
    )
}

fn project_tests_fixture_units() -> Vec<graph::GraphUnit> {
    vec![
        graph::GraphUnit {
            id: "src/App/App.csproj".to_string(),
            name: "App".to_string(),
            refs: Vec::new(),
            test: false,
        },
        graph::GraphUnit {
            id: "tests/App.Tests/App.Tests.csproj".to_string(),
            name: "App.Tests".to_string(),
            refs: vec!["src/App/App.csproj".to_string()],
            test: true,
        },
    ]
}

#[test]
fn build_tests_model_names_the_test_file_its_test_defs_and_every_referencing_line() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());
    let m = resolved_tests(build_tests_model(&index, "OrderService"));

    assert_eq!(m.symbol, "App.Orders.OrderService");
    assert_eq!(m.def_files, vec!["src/OrderService.cs".to_string()]);
    assert_eq!(
        m.test_file_count, 1,
        "the non-test neighbour is not a test file"
    );
    assert_eq!(
        m.ref_count, 3,
        "lines keep duplicates -- refCount is the reference count, not the distinct-line count"
    );
    assert_eq!(m.rows.len(), 2, "one precise row, then the heuristic one");

    let precise = &m.rows[0];
    assert_eq!(precise.file, "tests/OrderServiceTests.cs");
    assert_eq!(
        precise.test_defs,
        vec!["App.Orders.Tests.OrderServiceTests".to_string()]
    );
    assert_eq!(
        precise.lines,
        vec![10, 10, 11],
        "ascending, duplicates kept"
    );
    assert_eq!(precise.ref_count, 3);
    assert!(!precise.heuristic);
}

#[test]
fn build_tests_model_puts_heuristic_rows_after_every_precise_one_and_counts_them_separately() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());
    let m = resolved_tests(build_tests_model(&index, "OrderService"));

    let guessed = &m.rows[1];
    assert!(
        guessed.heuristic,
        "a guess never sits inside the list of facts"
    );
    assert_eq!(guessed.file, "tests/Partial.Extra.cs");
    assert_eq!(
        guessed.test_defs,
        vec!["App.Orders.Tests.PartialTests".to_string()]
    );
    assert_eq!(guessed.lines, vec![9]);
    assert_eq!(m.heuristic_file_count, 1);
    assert_eq!(m.heuristic_ref_count, 1);
    assert_eq!(m.test_file_count, 1, "files= and refs= stay PRECISE-only");
}

#[test]
fn build_tests_model_on_a_symbol_no_test_references_resolves_with_no_rows() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());
    let m = resolved_tests(build_tests_model(&index, "Untested"));
    assert_eq!(m.symbol, "App.Orders.Untested");
    assert!(m.rows.is_empty());
    assert_eq!(
        (
            m.test_file_count,
            m.ref_count,
            m.heuristic_file_count,
            m.heuristic_ref_count
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn build_tests_model_uses_the_same_resolve_symbol_ladder_refs_does() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());
    assert_eq!(
        build_tests_model(&index, "NoSuchSymbol"),
        TestsResult::NotFound
    );
    assert_eq!(
        resolved_tests(build_tests_model(&index, "orderservice")).symbol,
        "App.Orders.OrderService",
        "case-insensitive unique name is the ladder's last rung, same as refs"
    );

    let ambiguous_graph = base_fixture_graph();
    let ambiguous_index = load_graph_index(&ambiguous_graph, &base_fixture_root());
    assert_eq!(
        build_tests_model(&ambiguous_index, "Config"),
        TestsResult::Ambiguous(vec![
            "App.One.Config".to_string(),
            "App.Two.Config".to_string()
        ])
    );
}

#[test]
fn build_impact_model_counts_precisely_affected_test_files_only() {
    let g = tests_fixture_graph();
    let index = load_graph_index(&g, &tests_fixture_root());

    let ImpactResult::Resolved(covered) = build_impact_model(
        &index,
        "src/OrderService.cs",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) else {
        panic!("file seed resolves");
    };
    assert_eq!(
        covered.tests_affected, 1,
        "the guessed test file is not coverage; the non-test neighbour is not a test file"
    );

    let ImpactResult::Resolved(untouched) = build_impact_model(
        &index,
        "src/Untested.cs",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) else {
        panic!("file seed resolves");
    };
    assert_eq!(
        untouched.tests_affected, 0,
        "zero is the interesting answer -- a blast radius reaching no test file at all"
    );
}

#[test]
fn tests_lists_a_harness_file_in_a_test_project_with_a_project_via_and_no_test_defs() {
    let (defs, edges) = project_tests_fixture_defs_and_edges();
    let g = make_graph_with_units(defs, edges, project_tests_fixture_units());
    let root = temp_repo_root("project-tests-model");
    let index = load_graph_index(&g, &root);
    assert!(
        index.project.is_some(),
        "a non-empty `graph.units` builds a project model"
    );

    let m = resolved_tests(build_tests_model(&index, "OrderService"));
    assert_eq!(m.rows.len(), 2, "the harness file earns its own row too");

    let attributed = m
        .rows
        .iter()
        .find(|r| r.file == "tests/App.Tests/OrderServiceTests.cs")
        .expect("the attribute-vouched file is listed");
    assert_eq!(attributed.via, TestVia::Attribute);
    assert_eq!(
        attributed.test_defs,
        vec!["App.Orders.Tests.OrderServiceTests".to_string()]
    );

    let harness = m
        .rows
        .iter()
        .find(|r| r.file == "tests/App.Tests/FakeServer.cs")
        .expect("a harness file in a test project is listed even with no attributed def");
    assert_eq!(harness.via, TestVia::Project);
    assert!(
        harness.test_defs.is_empty(),
        "no attribute vouches for this file -- test_defs stays empty"
    );
    assert_eq!(harness.lines, vec![12, 34]);
    assert!(!harness.heuristic);

    assert_eq!(
        m.test_file_count, 2,
        "the project-vouched harness counts toward the precise total"
    );
}

#[test]
fn tests_without_a_project_model_is_unchanged() {
    let (defs, edges) = project_tests_fixture_defs_and_edges();
    let g = make_graph(defs, edges); // no `units` at all -- no project model
    let root = temp_repo_root("project-tests-model-none");
    let index = load_graph_index(&g, &root);
    assert!(index.project.is_none());

    let m = resolved_tests(build_tests_model(&index, "OrderService"));
    assert_eq!(
        m.rows.len(),
        1,
        "with no project model the un-attributed harness file stays invisible, exactly as before"
    );
    assert_eq!(m.rows[0].file, "tests/App.Tests/OrderServiceTests.cs");
    assert_eq!(m.rows[0].via, TestVia::Attribute);
}
