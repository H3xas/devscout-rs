use super::*;

fn implements_edge(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::Implements {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        member: None,
    }
}

fn member_implements_edge(
    from_file: &str,
    from_line: usize,
    to: &str,
    to_file: &str,
    member: &str,
) -> graph::Edge {
    graph::Edge::Implements {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        member: Some(member.into()),
    }
}

// One interface (`IContract`, called by `Consumers/Caller.cs`) and one
// registered implementation (`Widget`), plus the type-level and
// member-level `implements` edges the resolver would have produced for it.
fn dispatch_fixture_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "App.Dispatch.IContract",
                "IContract",
                "App.Dispatch",
                "interface",
                "Widgets/IContract.cs",
                3,
            ),
            def(
                "App.Dispatch.Widget",
                "Widget",
                "App.Dispatch",
                "class",
                "Widgets/Widget.cs",
                5,
            ),
            def(
                "App.Dispatch.Caller",
                "Caller",
                "App.Dispatch",
                "class",
                "Consumers/Caller.cs",
                3,
            ),
        ],
        vec![
            implements_edge(
                "Widgets/Widget.cs",
                5,
                "App.Dispatch.IContract",
                "Widgets/IContract.cs",
            ),
            member_implements_edge(
                "Widgets/Widget.cs",
                5,
                "App.Dispatch.IContract",
                "Widgets/IContract.cs",
                "Handle",
            ),
            uses_member(
                "Consumers/Caller.cs",
                8,
                "App.Dispatch.IContract",
                "Widgets/IContract.cs",
            ),
        ],
    )
}

fn dispatch_fixture_root() -> PathBuf {
    let root = temp_repo_root("dispatch");
    write_manifest_fixture(
        &root,
        &[
            "Widgets/IContract.cs",
            "Widgets/Widget.cs",
            "Consumers/Caller.cs",
        ],
    );
    root
}

// --- refs/impact/tests expand through the new edges by default -----------

#[test]
fn refs_on_the_service_interface_lists_its_implementation_by_default() {
    let g = dispatch_fixture_graph();
    let root = dispatch_fixture_root();
    let index = load_graph_index(&g, &root);
    let model = match build_refs_model(
        &index,
        "IContract",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected a resolved refs model, got {other:?}"),
    };
    // Both the type-level fact (registration-driven) and the member-level
    // fact (the `Handle` method satisfying the interface) land here -- the
    // same `implements` kind, one row each, both at the implementation's own
    // site.
    assert_eq!(model.inbound.implements.total, 2);
    assert!(model
        .inbound
        .implements
        .rows
        .iter()
        .all(|r| r.file == "Widgets/Widget.cs"));
}

#[test]
fn impact_on_a_registered_implementation_reaches_the_interfaces_callers() {
    let g = dispatch_fixture_graph();
    let root = dispatch_fixture_root();
    let index = load_graph_index(&g, &root);
    let model = match build_impact_model(
        &index,
        "Widgets/Widget.cs",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        crate::query::DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected a resolved impact model, got {other:?}"),
    };
    assert!(
        model.rows.iter().any(|r| r.file == "Consumers/Caller.cs"),
        "the widened interface hop must reach IContract's own caller: {model:?}"
    );
}

#[test]
fn tests_expands_through_a_member_level_implements_edge() {
    let root = temp_repo_root("dispatch-tests");
    write_manifest_fixture(
        &root,
        &[
            "Widgets/IContract.cs",
            "Widgets/Widget.cs",
            "tests/WidgetTests.cs",
        ],
    );
    let mut widget_tests = test_def(
        "App.Dispatch.Tests.WidgetTests",
        "WidgetTests",
        "tests/WidgetTests.cs",
        3,
        &["Handles"],
    );
    widget_tests.namespace = "App.Dispatch.Tests".to_string();
    let g = make_graph(
        vec![
            def(
                "App.Dispatch.IContract",
                "IContract",
                "App.Dispatch",
                "interface",
                "Widgets/IContract.cs",
                3,
            ),
            widget_tests,
        ],
        vec![member_implements_edge(
            "tests/WidgetTests.cs",
            3,
            "App.Dispatch.IContract",
            "Widgets/IContract.cs",
            "Handle",
        )],
    );
    let index = load_graph_index(&g, &root);
    let model = match build_tests_model(&index, "IContract") {
        TestsResult::Resolved(m) => m,
        other => panic!("expected a resolved tests model, got {other:?}"),
    };
    assert_eq!(model.test_file_count, 1);
    assert_eq!(model.rows[0].file, "tests/WidgetTests.cs");
}

// --- `--no-dispatch` restores the pre-change answer ---------------------

#[test]
fn no_dispatch_excludes_implements_edges_from_refs_and_the_impact_interface_hop() {
    let g = dispatch_fixture_graph();
    let root = dispatch_fixture_root();
    let narrowed = load_graph_index_with(
        &g,
        &root,
        IndexOptions {
            include_guesses: true,
            include_dispatch: false,
        },
    );

    let model = match build_refs_model(
        &narrowed,
        "IContract",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected a resolved refs model, got {other:?}"),
    };
    assert_eq!(
        model.inbound.implements.total, 0,
        "--no-dispatch must suppress the implements table entirely"
    );

    let impact_model = match build_impact_model(
        &narrowed,
        "Widgets/Widget.cs",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        crate::query::DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        ImpactResult::NotFound { .. } => return,
        other => panic!("expected resolved or not-found, got {other:?}"),
    };
    assert!(
        !impact_model
            .rows
            .iter()
            .any(|r| r.file == "Consumers/Caller.cs"),
        "--no-dispatch must remove the widening the implements edge provided: {impact_model:?}"
    );
}
