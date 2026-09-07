use super::*;

// --- the per-kind referencing line on an impact row ---

const FROM_LINES_MANIFEST_FILES: &[&str] = &[
    "Pay/IPaymentGateway.cs",
    "Pay/StripeGateway.cs",
    "Pay/Mixed.cs",
    "Pay/GuessOnly.cs",
];

/// One consumer file reached by ALL FOUR kinds the walk distinguishes, each
/// at its own line: two direct `uses-type` refs (lines 5 and 12 -- the
/// lower one must win), a `ctor-di` edge (line 7) plus its companion plain
/// ref at the identical site (deduped away, never a second kind), a direct
/// interface-name ref (line 20), and a heuristic guess (line 30).
fn from_lines_fixture_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "App.Pay.IPaymentGateway",
                "IPaymentGateway",
                "App.Pay",
                "interface",
                "Pay/IPaymentGateway.cs",
                3,
            ),
            def(
                "App.Pay.StripeGateway",
                "StripeGateway",
                "App.Pay",
                "class",
                "Pay/StripeGateway.cs",
                3,
            ),
            def(
                "App.Pay.Mixed",
                "Mixed",
                "App.Pay",
                "class",
                "Pay/Mixed.cs",
                3,
            ),
            def(
                "App.Pay.GuessOnly",
                "GuessOnly",
                "App.Pay",
                "class",
                "Pay/GuessOnly.cs",
                3,
            ),
        ],
        vec![
            inherits(
                "Pay/StripeGateway.cs",
                3,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            uses_type(
                "Pay/Mixed.cs",
                12,
                "App.Pay.StripeGateway",
                "Pay/StripeGateway.cs",
            ),
            uses_type(
                "Pay/Mixed.cs",
                5,
                "App.Pay.StripeGateway",
                "Pay/StripeGateway.cs",
            ),
            ctor_di_to(
                "Pay/Mixed.cs",
                7,
                "IPaymentGateway",
                "plain",
                "App.Pay.StripeGateway",
            ),
            uses_type(
                "Pay/Mixed.cs",
                7,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            uses_type(
                "Pay/Mixed.cs",
                20,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            heuristic_uses_member(
                "Pay/Mixed.cs",
                30,
                "App.Pay.StripeGateway",
                "Pay/StripeGateway.cs",
            ),
            heuristic_uses_member(
                "Pay/GuessOnly.cs",
                9,
                "App.Pay.StripeGateway",
                "Pay/StripeGateway.cs",
            ),
        ],
    )
}

fn from_lines_fixture_root() -> PathBuf {
    let root = temp_repo_root("from-lines");
    write_manifest_fixture(&root, FROM_LINES_MANIFEST_FILES);
    root
}

#[test]
fn build_impact_model_names_one_referencing_line_per_edge_kind_lowest_line_per_kind() {
    let graph = from_lines_fixture_graph();
    let root = from_lines_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "StripeGateway",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
    assert_eq!(
        row_of("Pay/Mixed.cs").from_lines,
        vec![
            ("direct", 5),
            ("ctor-di", 7),
            ("heuristic", 30),
            ("iface", 20)
        ],
        "key order is the walk's own kind declaration order, never a map iteration"
    );
    assert_eq!(
        row_of("Pay/Mixed.cs").via_count,
        4,
        "the companion ref at the ctor-di site is still deduped, not a fifth hit"
    );
    assert_eq!(
        row_of("Pay/GuessOnly.cs").from_lines,
        vec![("heuristic", 9)]
    );
}

#[test]
fn build_impact_model_no_iface_drops_the_two_interface_hop_kinds_and_keeps_the_rest() {
    let graph = from_lines_fixture_graph();
    let root = from_lines_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "StripeGateway",
        1,
        DEFAULT_CAP,
        false,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let row = model
        .rows
        .iter()
        .find(|r| r.file == "Pay/Mixed.cs")
        .unwrap();
    assert_eq!(
        row.from_lines,
        vec![("direct", 5), ("heuristic", 30)],
        "no hop was attempted, so neither interface kind may claim a line"
    );
}

#[test]
fn build_impact_model_an_ambiguous_only_hit_still_names_its_line_under_the_direct_kind() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "One/Config.cs",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let row = model
        .rows
        .iter()
        .find(|r| r.file == "Three/Consumer.cs")
        .unwrap();
    assert_eq!(row.ambiguous_count, 1);
    assert_eq!(
        row.from_lines,
        vec![("direct", 4)],
        "refs' own tie-break: an ambiguous site is used only when no resolved one exists"
    );
}

#[test]
fn build_impact_model_a_row_no_kind_could_attribute_a_line_to_carries_no_from_lines_at_all() {
    let graph = make_graph(
        vec![
            def("App.A.Seed", "Seed", "App.A", "class", "A/Seed.cs", 1),
            def("App.A.User", "User", "App.A", "class", "A/User.cs", 1),
        ],
        vec![uses_type("A/User.cs", 0, "App.A.Seed", "A/Seed.cs")],
    );
    let root = temp_repo_root("from-lines-empty");
    write_manifest_fixture(&root, &["A/Seed.cs", "A/User.cs"]);
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Seed",
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
            .map(|r| r.file.as_str())
            .collect::<Vec<_>>(),
        vec!["A/User.cs"]
    );
    assert!(
        model.rows[0].from_lines.is_empty(),
        "a line-less edge adds no key, exactly like every other conditional field"
    );
}
