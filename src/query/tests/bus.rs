use super::*;

/// The seed (`Handler`) is reached at hop 1 by `Publisher.cs` through a
/// `bus-hop` edge alone, and at hop 1 by `SiblingHandler.cs` through an
/// ordinary reference. `Deep.cs` is reached at hop 2 by BOTH: once from
/// `Publisher`'s own def (a tainted ancestor) and, in the mixed-evidence
/// case, once from `SiblingHandler`'s (an untainted one).
fn taint_graph(deep_also_references_sibling: bool) -> graph::Graph {
    let mut edges = vec![
        bus_hop(
            "Bus/Publisher.cs",
            5,
            "App.Bus.LoanRequested",
            "App.Bus.Handler",
            "Bus/Handler.cs",
        ),
        uses_type(
            "Bus/SiblingHandler.cs",
            5,
            "App.Bus.Handler",
            "Bus/Handler.cs",
        ),
        uses_type("Bus/Deep.cs", 10, "App.Bus.Publisher", "Bus/Publisher.cs"),
    ];
    if deep_also_references_sibling {
        edges.push(uses_type(
            "Bus/Deep.cs",
            20,
            "App.Bus.SiblingHandler",
            "Bus/SiblingHandler.cs",
        ));
    }
    make_graph(
        vec![
            def(
                "App.Bus.Handler",
                "Handler",
                "App.Bus",
                "class",
                "Bus/Handler.cs",
                3,
            ),
            def(
                "App.Bus.LoanRequested",
                "LoanRequested",
                "App.Bus",
                "class",
                "Bus/Messages.cs",
                3,
            ),
            def(
                "App.Bus.Publisher",
                "Publisher",
                "App.Bus",
                "class",
                "Bus/Publisher.cs",
                3,
            ),
            def(
                "App.Bus.SiblingHandler",
                "SiblingHandler",
                "App.Bus",
                "class",
                "Bus/SiblingHandler.cs",
                3,
            ),
        ],
        edges,
    )
}

fn taint_root() -> PathBuf {
    let root = temp_repo_root("bus-taint");
    write_manifest_fixture(
        &root,
        &[
            "Bus/Handler.cs",
            "Bus/Messages.cs",
            "Bus/Publisher.cs",
            "Bus/SiblingHandler.cs",
            "Bus/Deep.cs",
        ],
    );
    root
}

fn row_of<'a>(model: &'a ImpactModel, file: &str) -> &'a ImpactRow {
    model
        .rows
        .iter()
        .find(|r| r.file == file)
        .unwrap_or_else(|| panic!("no row for {file}: {model:?}"))
}

#[test]
fn impact_reached_only_through_a_bus_hop_keeps_the_possible_route_marker_downstream() {
    let graph = taint_graph(false);
    let root = taint_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Handler",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let publisher = row_of(&model, "Bus/Publisher.cs");
    assert_eq!(publisher.hop, 1);
    assert!(
        publisher.bus_only,
        "Publisher.cs's only path back to Handler is the bus hop itself"
    );
    let publisher_origin = publisher
        .bus_origin
        .as_ref()
        .expect("a bus-only row must carry the origin hop's own identity");
    assert_eq!(publisher_origin.file, "Bus/Publisher.cs");
    assert_eq!(publisher_origin.line, 5);
    assert_eq!(publisher_origin.message, "App.Bus.LoanRequested");
    assert_eq!(publisher_origin.to, "App.Bus.Handler");
    assert_eq!(publisher_origin.to_file, "Bus/Handler.cs");
    let deep = row_of(&model, "Bus/Deep.cs");
    assert_eq!(deep.hop, 2, "reached only by widening past Publisher.cs");
    assert!(
        deep.bus_only,
        "Deep.cs's only path back to Handler runs through the tainted Publisher.cs, \
         even though the edge that reached Deep.cs is an ordinary reference"
    );
    assert_eq!(
        deep.bus_origin.as_ref(),
        Some(publisher_origin),
        "Deep.cs's own row discloses the ORIGINATING hop's identity, carried forward through \
         Publisher.cs's inherited taint, not merely a bare flag that a hop exists somewhere"
    );
}

#[test]
fn impact_reached_by_both_a_bus_hop_and_an_independent_reference_keeps_the_stronger_evidence() {
    let graph = taint_graph(true);
    let root = taint_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Handler",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let sibling = row_of(&model, "Bus/SiblingHandler.cs");
    assert_eq!(sibling.hop, 1);
    assert!(
        !sibling.bus_only,
        "SiblingHandler.cs reaches Handler by an ordinary reference, never a bus hop"
    );
    assert_eq!(
        sibling.bus_origin, None,
        "an ordinary reference discloses no bus-hop identity at all"
    );
    let deep = row_of(&model, "Bus/Deep.cs");
    assert_eq!(deep.hop, 2);
    assert!(
        !deep.bus_only,
        "Deep.cs also references the untainted SiblingHandler.cs, so its stronger \
         evidence stands even though a tainted path through Publisher.cs exists too"
    );
    assert_eq!(
        deep.why,
        Why::UsesType,
        "the ordinary reference explains the row, not the bus hop"
    );
    assert_eq!(
        deep.bus_origin, None,
        "a row with an independent non-bus path drops the origin identity entirely, \
         never a stale one from the tainted path it also has"
    );
}
