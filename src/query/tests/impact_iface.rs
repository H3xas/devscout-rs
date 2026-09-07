use super::*;

fn ctor_di_no_to(from_file: &str, from_line: usize, iface: &str, resolution: &str) -> graph::Edge {
    graph::Edge::CtorDi {
        from_file: from_file.into(),
        from_line,
        iface: iface.into(),
        resolution: resolution.into(),
        args: None,
        to: None,
        candidates: vec![],
    }
}

const IFACE_HOP_MANIFEST_FILES: &[&str] = &[
    "Pay/IPaymentGateway.cs",
    "Pay/StripeGateway.cs",
    "Pay/OrderService.cs",
    "Pay/RefundService.cs",
    "Pay/GatewayFactory.cs",
    "Pay/GatewayHolder.cs",
    "Pay/AuditLogger.cs",
];

/// `StripeGateway` is `IPaymentGateway`'s SOLE implementor. `OrderService`/
/// `RefundService` ctor-inject the interface (never naming the class) --
/// each also carries the COMPANION plain `uses-type` ref always emitted
/// alongside a ctor-param ref, at the identical from_file/from_line,
/// resolving to the INTERFACE (dedup-proving). `GatewayFactory` names
/// `StripeGateway` directly (a plain direct-name hit). `GatewayHolder`
/// references `IPaymentGateway` directly but NOT through a constructor (a
/// property type, distinct file/line from every ctor-di site). `AuditLogger`
/// carries an unrelated, unresolvable `ILogger` ctor-di edge (`infra`, no
/// `to`) that must never widen anything.
fn iface_hop_fixture_graph() -> graph::Graph {
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
                "App.Pay.OrderService",
                "OrderService",
                "App.Pay",
                "class",
                "Pay/OrderService.cs",
                3,
            ),
            def(
                "App.Pay.RefundService",
                "RefundService",
                "App.Pay",
                "class",
                "Pay/RefundService.cs",
                3,
            ),
            def(
                "App.Pay.GatewayFactory",
                "GatewayFactory",
                "App.Pay",
                "class",
                "Pay/GatewayFactory.cs",
                3,
            ),
            def(
                "App.Pay.GatewayHolder",
                "GatewayHolder",
                "App.Pay",
                "class",
                "Pay/GatewayHolder.cs",
                3,
            ),
            def(
                "App.Pay.AuditLogger",
                "AuditLogger",
                "App.Pay",
                "class",
                "Pay/AuditLogger.cs",
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
            ctor_di_to(
                "Pay/OrderService.cs",
                5,
                "IPaymentGateway",
                "plain",
                "App.Pay.StripeGateway",
            ),
            uses_type(
                "Pay/OrderService.cs",
                5,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            ctor_di_to(
                "Pay/RefundService.cs",
                5,
                "IPaymentGateway",
                "plain",
                "App.Pay.StripeGateway",
            ),
            uses_type(
                "Pay/RefundService.cs",
                5,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            uses_type(
                "Pay/GatewayFactory.cs",
                6,
                "App.Pay.StripeGateway",
                "Pay/StripeGateway.cs",
            ),
            uses_type(
                "Pay/GatewayHolder.cs",
                4,
                "App.Pay.IPaymentGateway",
                "Pay/IPaymentGateway.cs",
            ),
            ctor_di_no_to("Pay/AuditLogger.cs", 5, "ILogger", "infra"),
        ],
    )
}

fn iface_hop_fixture_root() -> PathBuf {
    let root = temp_repo_root("iface-hop");
    write_manifest_fixture(&root, IFACE_HOP_MANIFEST_FILES);
    root
}

#[test]
fn build_impact_model_widens_through_ctor_injected_interface_consumers_and_direct_interface_references(
) {
    let graph = iface_hop_fixture_graph();
    let root = iface_hop_fixture_root();
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
    let files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
    assert!(
        files.contains(&"Pay/GatewayFactory.cs"),
        "direct-name reference must still be reached: {files:?}"
    );
    assert!(
        files.contains(&"Pay/OrderService.cs"),
        "ctor-injected consumer must be reached: {files:?}"
    );
    assert!(
        files.contains(&"Pay/RefundService.cs"),
        "ctor-injected consumer must be reached: {files:?}"
    );
    assert!(
        files.contains(&"Pay/GatewayHolder.cs"),
        "direct interface-name reference must be reached: {files:?}"
    );
    assert!(
        !files.contains(&"Pay/AuditLogger.cs"),
        "an unrelated infra ctor-di edge must never widen: {files:?}"
    );

    let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
    assert_eq!(
        row_of("Pay/OrderService.cs").hop,
        1,
        "the interface hop counts as ONE hop, same as a direct reference"
    );
    assert_eq!(
        row_of("Pay/OrderService.cs").iface_via,
        vec!["IPaymentGateway (ctor-di)".to_string()]
    );
    assert_eq!(
        row_of("Pay/RefundService.cs").iface_via,
        vec!["IPaymentGateway (ctor-di)".to_string()]
    );
    assert_eq!(
        row_of("Pay/GatewayHolder.cs").iface_via,
        vec!["IPaymentGateway".to_string()]
    );
    assert!(
        row_of("Pay/GatewayFactory.cs").iface_via.is_empty(),
        "a plain direct-name hit carries no iface_via label"
    );
    // The companion plain `uses-type` ref (same from_file/from_line as the
    // ctor-di edge) must be deduped away, not double-counted.
    assert_eq!(
        row_of("Pay/OrderService.cs").via_count,
        1,
        "the ctor-di hit and its companion ref are ONE hit, not two"
    );
}

#[test]
fn build_impact_model_no_iface_restores_the_pre_ds_0050_radius_byte_for_byte() {
    let graph = iface_hop_fixture_graph();
    let root = iface_hop_fixture_root();
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
    let files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(
        files,
        vec!["Pay/GatewayFactory.cs"],
        "only the direct-name reference survives with --no-iface"
    );
    assert!(
        model.rows.iter().all(|r| r.iface_via.is_empty()),
        "no row may carry an iface_via label with --no-iface"
    );
}

/// A second implementor with the SAME bare interface name in a different
/// namespace must never contribute its consumers to the first's radius --
/// the widen is def-id-matched (via the resolver's own confirmed `to`),
/// never name-matched. Also covers the truly ambiguous ctor-di shape
/// (two tied implementors, `to: None`): it must widen neither.
#[test]
fn build_impact_model_never_widens_through_a_same_named_interface_elsewhere_or_an_ambiguous_ctor_di_edge(
) {
    let graph = make_graph(
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
                "Other.Billing.IPaymentGateway",
                "IPaymentGateway",
                "Other.Billing",
                "interface",
                "Billing/IPaymentGateway.cs",
                3,
            ),
            def(
                "Other.Billing.LegacyGateway",
                "LegacyGateway",
                "Other.Billing",
                "class",
                "Billing/LegacyGateway.cs",
                3,
            ),
            def(
                "App.Pay.UnrelatedConsumer",
                "UnrelatedConsumer",
                "App.Pay",
                "class",
                "Pay/UnrelatedConsumer.cs",
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
            inherits(
                "Billing/LegacyGateway.cs",
                3,
                "Other.Billing.IPaymentGateway",
                "Billing/IPaymentGateway.cs",
            ),
            // Names the OTHER namespace's IPaymentGateway -- must never
            // reach StripeGateway just because the bare name matches.
            ctor_di_to(
                "Pay/UnrelatedConsumer.cs",
                5,
                "IPaymentGateway",
                "plain",
                "Other.Billing.LegacyGateway",
            ),
            // A tied ('ambiguous') ctor-di edge naming StripeGateway's
            // OWN interface -- no `to`, so it must not widen either.
            ctor_di_no_to(
                "Pay/UnrelatedConsumer.cs",
                9,
                "IPaymentGateway",
                "ambiguous",
            ),
        ],
    );
    let root = temp_repo_root("iface-hop-collision");
    write_manifest_fixture(
        &root,
        &[
            "Pay/IPaymentGateway.cs",
            "Pay/StripeGateway.cs",
            "Billing/IPaymentGateway.cs",
            "Billing/LegacyGateway.cs",
            "Pay/UnrelatedConsumer.cs",
        ],
    );
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
    assert!(
        model.rows.is_empty(),
        "a same-named interface elsewhere, and an ambiguous ctor-di edge, must never widen: {:?}",
        model.rows
    );
}
