use super::*;

#[test]
fn qualified_reference_resolves_at_an_outer_enclosing_namespace_prefix() {
    // From within `Fixtures.Billing` (2 segments), a reference to
    // `Common.IIdentifiable` must find `Fixtures.Common.IIdentifiable`
    // by trying prefix "Fixtures" (outer), after "Fixtures.Billing"
    // (innermost) fails -- exercises the walk, not just a literal or
    // innermost-only match.
    let files = vec![
        (
            "Common/IIdentifiable.cs".to_string(),
            frag(
                vec![def(
                    "Fixtures.Common.IIdentifiable",
                    "IIdentifiable",
                    "Fixtures.Common",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Billing/Invoice.cs".to_string(),
            frag(
                vec![def(
                    "Fixtures.Billing.Invoice",
                    "Invoice",
                    "Fixtures.Billing",
                    "class",
                )],
                vec![],
                vec![type_ref(
                    "inherits",
                    "IIdentifiable",
                    Some("Common.IIdentifiable"),
                    "Fixtures.Billing",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::Inherits { .. })).expect("resolved via namespace walk");
    match edge {
        Edge::Inherits { to, .. } => assert_eq!(to, "Fixtures.Common.IIdentifiable"),
        _ => unreachable!(),
    }
}

#[test]
fn qualified_reference_with_no_matching_prefix_and_no_literal_match_is_external() {
    let files = vec![(
        "A/Probe.cs".to_string(),
        frag(
            vec![def("A.Probe", "Probe", "A", "class")],
            vec![],
            vec![type_ref("uses-type", "Y", Some("X.Y"), "A")],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.unresolved_external_count, 1);
}

// --- qualified (dotted) resolution: the suffix fallback -----------------

#[test]
fn foreign_qualified_enum_member_never_binds_to_a_same_named_in_tree_enum() {
    // The RabbitMQ.Client.ExchangeType shape: an in-tree enum shares its
    // bare name with a foreign one, and only the dotted TEXT tells them
    // apart -- step 1b must reject it rather than let the enum's own
    // unconditional-emission rule wave it through.
    let files = vec![
        (
            "Fabric/ExchangeType.cs".to_string(),
            frag(
                vec![
                    def(
                        "App.Transports.Fabric.ExchangeType",
                        "ExchangeType",
                        "App.Transports.Fabric",
                        "enum",
                    ),
                    def(
                        "App.Transports.Fabric.ExchangeType.Fanout",
                        "Fanout",
                        "App.Transports.Fabric",
                        "enum-member",
                    ),
                    def(
                        "App.Transports.Fabric.ExchangeType.Topic",
                        "Topic",
                        "App.Transports.Fabric",
                        "enum-member",
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Configure.cs".to_string(),
            frag(
                vec![def("App.Bus.Configure", "Configure", "App.Bus", "class")],
                vec![],
                vec![member_ref(
                    "ExchangeType",
                    Some("RabbitMQ.Client.ExchangeType"),
                    "Fanout",
                    "App.Bus",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edge_targets(&g).is_empty());
    // An enum MEMBER never lands in `member_name_to_defs` (built from
    // methods/properties/fields only, see the module header's asymmetry
    // note) -- so the scored tier has no pool to draw from either. The
    // foreign qualifier is dropped silently, not guessed.
    assert!(heuristic_member_edge_targets(&g).is_empty());
}

#[test]
fn own_namespace_relative_and_bare_qualified_enum_uses_still_resolve() {
    // The suffix rule only forecloses a FOREIGN dotted qualifier -- every
    // shape C# actually uses to name the SAME enum (an exact match, a
    // relative qualification resolved by the enclosing-prefix walk at
    // step 1, and a bare name through a using) must keep resolving.
    let enum_defs = vec![
        def(
            "App.Transports.Fabric.ExchangeType",
            "ExchangeType",
            "App.Transports.Fabric",
            "enum",
        ),
        def(
            "App.Transports.Fabric.ExchangeType.Fanout",
            "Fanout",
            "App.Transports.Fabric",
            "enum-member",
        ),
        def(
            "App.Transports.Fabric.ExchangeType.Topic",
            "Topic",
            "App.Transports.Fabric",
            "enum-member",
        ),
    ];
    let files = vec![
        (
            "Fabric/ExchangeType.cs".to_string(),
            frag(enum_defs, vec![], vec![]),
        ),
        (
            "Bus/Exact.cs".to_string(),
            frag(
                vec![def("App.Bus.Exact", "Exact", "App.Bus", "class")],
                vec![],
                vec![member_ref(
                    "ExchangeType",
                    Some("App.Transports.Fabric.ExchangeType"),
                    "Topic",
                    "App.Bus",
                )],
            ),
        ),
        (
            "Relative.cs".to_string(),
            frag(
                vec![def("App.Relative", "Relative", "App", "class")],
                vec![],
                vec![member_ref(
                    "ExchangeType",
                    Some("Transports.Fabric.ExchangeType"),
                    "Topic",
                    "App",
                )],
            ),
        ),
        (
            "Bus/Bare.cs".to_string(),
            frag(
                vec![def("App.Bus.Bare", "Bare", "App.Bus", "class")],
                vec![FragUsing::Plain {
                    text: "App.Transports.Fabric".into(),
                    global: false,
                }],
                vec![member_ref("ExchangeType", None, "Fanout", "App.Bus")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let mut targets = member_edge_targets(&g);
    targets.sort_unstable();
    let mut expected = vec![
        "App.Transports.Fabric.ExchangeType.Fanout",
        "App.Transports.Fabric.ExchangeType.Topic",
        "App.Transports.Fabric.ExchangeType.Topic",
    ];
    expected.sort_unstable();
    assert_eq!(targets, expected);
}

#[test]
fn foreign_qualified_static_call_never_binds_to_a_same_named_in_tree_class() {
    let json = def_with(
        "App.Infra.JsonSerializer",
        "JsonSerializer",
        "App.Infra",
        "class",
        &["Serialize"],
        &[],
        &[],
    );
    let foreign_ref = FragRef {
        arg_count: Some(1),
        ..member_ref(
            "JsonSerializer",
            Some("System.Text.Json.JsonSerializer"),
            "Serialize",
            "App.Svc",
        )
    };
    let files = vec![
        (
            "Infra/JsonSerializer.cs".to_string(),
            frag(vec![json.clone()], vec![], vec![]),
        ),
        (
            "Svc/Foreign.cs".to_string(),
            frag(
                vec![def("App.Svc.Foreign", "Foreign", "App.Svc", "class")],
                vec![],
                vec![foreign_ref],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edge_targets(&g).is_empty(),
        "the foreign qualifier must never bind the precise edge to the in-tree class"
    );
    // The suffix rule only tightens the PRECISE ladder (step 1b); the
    // scored tier's member-name uniqueness pool is a wholly separate
    // path, and "Serialize" is unique in this graph -- so the guess
    // still fires. This is deliberate: the suffix rule narrows certainty,
    // it does not widen what the scored tier is willing to guess.
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.Infra.JsonSerializer"]
    );

    let own_ref = FragRef {
        arg_count: Some(1),
        ..member_ref(
            "JsonSerializer",
            Some("App.Infra.JsonSerializer"),
            "Serialize",
            "App.Svc",
        )
    };
    let files = vec![
        (
            "Infra/JsonSerializer.cs".to_string(),
            frag(vec![json], vec![], vec![]),
        ),
        (
            "Svc/Own.cs".to_string(),
            frag(
                vec![def("App.Svc.Own", "Own", "App.Svc", "class")],
                vec![],
                vec![own_ref],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_edge_targets(&g), vec!["App.Infra.JsonSerializer"]);
}

#[test]
fn property_hop_through_an_external_intermediate_never_binds_a_same_named_type() {
    // "expr.Member.Name" (an Expression-tree walk): the extractor
    // flattens the qualifier to "expr.Member", which happens to share
    // its tail with an in-tree type named "Member". Neither a top-level
    // nor a NESTED same-named type may answer for it -- "expr" is not a
    // namespace prefix at all, and the suffix rule only cares whether the
    // def's own path ends with the written text.
    let top_level = def_with(
        "App.Model.Member",
        "Member",
        "App.Model",
        "class",
        &[],
        &["Name"],
        &[],
    );
    let files = vec![
        (
            "Model/Member.cs".to_string(),
            frag(vec![top_level], vec![], vec![]),
        ),
        (
            "Svc/Hop.cs".to_string(),
            frag(
                vec![def("App.Svc.Hop", "Hop", "App.Svc", "class")],
                vec![],
                vec![member_ref("Member", Some("expr.Member"), "Name", "App.Svc")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edge_targets(&g).is_empty());

    let nested = def_with(
        "App.Model.Outer+Member",
        "Member",
        "App.Model",
        "class",
        &[],
        &["Name"],
        &[],
    );
    let files = vec![
        (
            "Model/Outer.cs".to_string(),
            frag(vec![nested], vec![], vec![]),
        ),
        (
            "Svc/Hop.cs".to_string(),
            frag(
                vec![def("App.Svc.Hop", "Hop", "App.Svc", "class")],
                vec![],
                vec![member_ref("Member", Some("expr.Member"), "Name", "App.Svc")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edge_targets(&g).is_empty());
}

#[test]
fn foreign_qualified_base_type_is_external_not_an_inherits_edge() {
    let files = vec![
        (
            "Messaging/DefaultBasicConsumer.cs".to_string(),
            frag(
                vec![def(
                    "App.Messaging.DefaultBasicConsumer",
                    "DefaultBasicConsumer",
                    "App.Messaging",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Consumer.cs".to_string(),
            frag(
                vec![def("App.Bus.Consumer", "Consumer", "App.Bus", "class")],
                vec![],
                vec![type_ref(
                    "inherits",
                    "DefaultBasicConsumer",
                    Some("RabbitMQ.Client.DefaultBasicConsumer"),
                    "App.Bus",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(g.edges.iter().all(|e| !matches!(e, Edge::Inherits { .. })));
    assert_eq!(g.stats.unresolved_external_count, 1);
}

#[test]
fn alias_qualified_name_whose_target_lacks_the_type_is_external_even_when_another_namespace_has_it()
{
    // The alias rewrite (step 1a) hands step 1b the EXPANDED text, not
    // the literal "Ns.MyEnum" -- an alias pointed at the wrong namespace
    // must not fall back to matching some unrelated namespace's
    // same-named enum by suffix.
    let files = vec![
        (
            "Other/MyEnum.cs".to_string(),
            frag(
                vec![
                    def("App.Other.MyEnum", "MyEnum", "App.Other", "enum"),
                    def("App.Other.MyEnum.On", "On", "App.Other", "enum-member"),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/AliasMiss.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.AliasMiss",
                    "AliasMiss",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Alias {
                    alias: "Ns".into(),
                    target: "Some.Namespace".into(),
                    global: false,
                }],
                vec![member_ref(
                    "MyEnum",
                    Some("Ns.MyEnum"),
                    "On",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edge_targets(&g).is_empty());
}
