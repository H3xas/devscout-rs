use super::*;

/// A seed file (`Widget.cs`) referenced by one ordinary file (`Handler.cs`,
/// reached at hop 1) which is itself the `bus-hop` handler `publisher_count`
/// distinct publish sites reach (`Publisher0.cs`..`PublisherN.cs`). Those
/// publish sites are what widening past Handler.cs would reach at hop 2 --
/// the same set that determines Handler's own bus-hop-derived in-degree, so
/// whether they show up in the model IS the hub-brake question. Every
/// publisher def is always present, whether or not `with_bus_edges` -- only
/// the `bus-hop` edges themselves come and go, the same "graph that never
/// recorded a bus-hop edge at all" shape the suppressor guarantee is about.
fn hub_via_bus_hop_graph(publisher_count: usize, with_bus_edges: bool) -> graph::Graph {
    let mut defs = vec![
        def(
            "App.Core.Widget",
            "Widget",
            "App.Core",
            "class",
            "Core/Widget.cs",
            3,
        ),
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
    ];
    let mut edges = vec![
        // Reaches Handler.cs at hop 1 from the Widget seed -- unrelated to
        // the bus, purely how the walk gets there.
        uses_type("Bus/Handler.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
    ];
    for n in 0..publisher_count {
        let file = format!("Bus/Publisher{n}.cs");
        defs.push(def(
            &format!("App.Bus.Publisher{n}"),
            &format!("Publisher{n}"),
            "App.Bus",
            "class",
            &file,
            3,
        ));
        if with_bus_edges {
            edges.push(bus_hop(
                &file,
                10,
                "App.Bus.LoanRequested",
                "App.Bus.Handler",
                "Bus/Handler.cs",
            ));
        }
    }
    make_graph(defs, edges)
}

fn hub_via_bus_hop_root(publisher_count: usize) -> PathBuf {
    let root = temp_repo_root("hub-via-bus-hop");
    let mut files: Vec<String> = vec![
        "Core/Widget.cs".into(),
        "Bus/Handler.cs".into(),
        "Bus/Messages.cs".into(),
    ];
    for n in 0..publisher_count {
        files.push(format!("Bus/Publisher{n}.cs"));
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    write_manifest_fixture(&root, &refs);
    root
}

fn hub_files_of(model: &ImpactModel) -> Vec<String> {
    let mut files: Vec<String> = model.rows.iter().map(|r| r.file.clone()).collect();
    files.sort();
    files
}

#[test]
fn bus_hops_below_the_shared_hub_boundary_still_expand() {
    let publisher_count = DEFAULT_HUB_MAX_INDEGREE - 1;
    let graph = hub_via_bus_hop_graph(publisher_count, true);
    let root = hub_via_bus_hop_root(publisher_count);
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let files = hub_files_of(&model);
    assert!(
        files.contains(&"Bus/Publisher0.cs".to_string()),
        "Handler.cs sits one under the default hub in-degree threshold ({publisher_count} \
         referring publish sites), so widening continues past it to reach its own publish sites"
    );
    assert_eq!(
        files
            .iter()
            .filter(|f| f.starts_with("Bus/Publisher"))
            .count(),
        publisher_count,
        "every publish site widened through, not merely one"
    );
    assert!(
        model.braked_files.is_empty(),
        "nothing crosses the threshold at {publisher_count} referrers"
    );
}

#[test]
fn bus_hops_reuse_the_shared_hub_brake_at_its_default_boundary() {
    let publisher_count = DEFAULT_HUB_MAX_INDEGREE;
    let graph = hub_via_bus_hop_graph(publisher_count, true);
    let root = hub_via_bus_hop_root(publisher_count);
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert!(
        hub_files_of(&model)
            .iter()
            .all(|f| !f.starts_with("Bus/Publisher")),
        "Handler.cs's bus-hop-derived in-degree reaches the default threshold \
         ({publisher_count} referring publish sites), so the shared hub brake stops \
         widening through it exactly as it would for any other edge kind"
    );
    assert_eq!(
        model.braked_files,
        vec![BrakedFile {
            file: "Bus/Handler.cs".to_string(),
            indegree: publisher_count,
        }],
        "a bus hop's referring file counts toward the same brake, at the same threshold"
    );
}

#[test]
fn no_bus_matches_a_graph_without_hops_even_when_hops_make_a_hub() {
    let publisher_count = DEFAULT_HUB_MAX_INDEGREE;
    let graph = hub_via_bus_hop_graph(publisher_count, true);
    let root = hub_via_bus_hop_root(publisher_count);

    // The un-suppressed answer over the same graph, with `--no-bus`'s
    // `IndexOptions`: bus-hop edges are excluded from every adjacency,
    // including the hub in-degree bookkeeping they would otherwise cross
    // the threshold in.
    let suppressed_index = load_graph_index_with(
        &graph,
        &root,
        IndexOptions {
            include_bus: false,
            ..IndexOptions::default()
        },
    );
    let suppressed = match build_impact_model(
        &suppressed_index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };

    // The ground truth: a graph that never carried a bus-hop edge at all
    // (every publisher def is still present; only the `bus-hop` edges are
    // gone), so the hub brake sees no referrers on Handler.cs at all.
    let bus_free_graph = hub_via_bus_hop_graph(publisher_count, false);
    let bus_free_root = hub_via_bus_hop_root(publisher_count);
    let bus_free_index = load_graph_index(&bus_free_graph, &bus_free_root);
    let bus_free = match build_impact_model(
        &bus_free_index,
        "Widget",
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
        suppressed, bus_free,
        "--no-bus reproduces exactly the answer a graph that never had bus hops would give, \
         even at a publisher count that would otherwise cross the hub brake threshold"
    );
}
