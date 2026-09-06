use super::*;

#[test]
fn dotted_suffix_picks_the_def_whose_path_ends_with_the_written_text() {
    let files = vec![
        (
            "Core/Outer.cs".to_string(),
            frag(
                vec![def("App.Core.Outer+Nested", "Nested", "App.Core", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Holder.cs".to_string(),
            frag(
                vec![def(
                    "App.Other.Holder+Nested",
                    "Nested",
                    "App.Other",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Picks.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Picks",
                    "Picks",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![type_ref(
                    "uses-type",
                    "Nested",
                    Some("Holder.Nested"),
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge = find_edge(&g, |e| matches!(e, Edge::UsesType { .. }))
        .expect("suffix match resolves to the def whose path ends with the written text");
    match edge {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Other.Holder+Nested"),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.ambiguous_count, 0);

    // Control: neither def's path ends with this unrelated written text.
    let files = vec![
        (
            "Core/Outer.cs".to_string(),
            frag(
                vec![def("App.Core.Outer+Nested", "Nested", "App.Core", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Holder.cs".to_string(),
            frag(
                vec![def(
                    "App.Other.Holder+Nested",
                    "Nested",
                    "App.Other",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Misses.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Misses",
                    "Misses",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![type_ref(
                    "uses-type",
                    "Nested",
                    Some("Elsewhere.Nested"),
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.unresolved_external_count, 1);
}

#[test]
fn two_defs_whose_paths_both_end_with_the_written_text_stay_ambiguous() {
    let files = vec![
        (
            "A/Outer.cs".to_string(),
            frag(
                vec![def("App.A.Outer+Nested", "Nested", "App.A", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "B/Outer.cs".to_string(),
            frag(
                vec![def("App.B.Outer+Nested", "Nested", "App.B", "class")],
                vec![],
                vec![],
            ),
        ),
        // A third same-named nested def whose path does NOT end with the
        // written text: the suffix rule drops it from the pool, which is
        // what makes the candidate list two rather than three.
        (
            "C/Holder.cs".to_string(),
            frag(
                vec![def("App.C.Holder+Nested", "Nested", "App.C", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Ambiguous.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Ambiguous",
                    "Ambiguous",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![type_ref(
                    "uses-type",
                    "Nested",
                    Some("Outer.Nested"),
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(g.edges.iter().all(|e| !matches!(e, Edge::UsesType { .. })));
    assert_eq!(g.stats.ambiguous_count, 1);
    match find_edge(&g, |e| matches!(e, Edge::Ambiguous { .. })).unwrap() {
        Edge::Ambiguous {
            candidate_count,
            candidates,
            ..
        } => {
            assert_eq!(*candidate_count, 2);
            assert_eq!(
                candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                vec!["App.A.Outer+Nested", "App.B.Outer+Nested"]
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn generic_outer_type_written_with_its_arguments_still_reaches_the_nested_type() {
    // The extractor strips type arguments off the TAIL only, so a nested
    // type under a generic outer arrives as `Box<string>.Slot`; the
    // suffix step reads it as `Box.Slot`.
    let files = vec![
        (
            "Core/Box.cs".to_string(),
            frag(
                vec![
                    FragDef {
                        type_params: vec!["T".into()],
                        ..def("App.Core.Box", "Box", "App.Core", "class")
                    },
                    def("App.Core.Box+Slot", "Slot", "App.Core", "class"),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Holder.cs".to_string(),
            frag(
                vec![def("App.Bus.Holder", "Holder", "App.Bus", "class")],
                vec![],
                vec![type_ref(
                    "uses-type",
                    "Slot",
                    Some("Box<string>.Slot"),
                    "App.Bus",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Box+Slot"),
        _ => unreachable!(),
    }
}

#[test]
fn global_alias_qualified_name_resolves_by_its_absolute_path() {
    let files = vec![
        (
            "Core/Widget.cs".to_string(),
            frag(
                vec![def("App.Core.Widget", "Widget", "App.Core", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Holder.cs".to_string(),
            frag(
                vec![def("App.Bus.Holder", "Holder", "App.Bus", "class")],
                vec![],
                vec![type_ref(
                    "uses-type",
                    "Widget",
                    Some("global::App.Core.Widget"),
                    "App.Bus",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Widget"),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.unresolved_external_count, 0);
}

#[test]
fn nested_type_named_through_a_derived_type_is_admitted_by_the_inheritance_closure() {
    // `Derived.Item` for an `Item` declared inside `Base` is legal C#, and
    // no def path ends with `Derived.Item`; the qualifier resolves as a
    // type of its own and the nested candidate's enclosing def must lie in
    // its inheritance closure. `Unrelated.Item` -- a type with no such
    // base -- stays external.
    let core = |refs: Vec<FragRef>| {
        vec![
            (
                "Core/Types.cs".to_string(),
                frag(
                    vec![
                        def("App.Core.Base", "Base", "App.Core", "class"),
                        def("App.Core.Base+Item", "Item", "App.Core", "class"),
                        FragDef {
                            bases: vec!["Base".into()],
                            ..def("App.Core.Derived", "Derived", "App.Core", "class")
                        },
                        def("App.Core.Unrelated", "Unrelated", "App.Core", "class"),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Bus/Holder.cs".to_string(),
                frag(
                    vec![def("App.Bus.Holder", "Holder", "App.Bus", "class")],
                    vec![FragUsing::Plain {
                        text: "App.Core".into(),
                        global: false,
                    }],
                    refs,
                ),
            ),
        ]
    };
    let g = resolve_graph(
        &no_git_root(),
        &core(vec![type_ref(
            "uses-type",
            "Item",
            Some("Derived.Item"),
            "App.Bus",
        )]),
    );
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Base+Item"),
        _ => unreachable!(),
    }
    let g = resolve_graph(
        &no_git_root(),
        &core(vec![type_ref(
            "uses-type",
            "Item",
            Some("Unrelated.Item"),
            "App.Bus",
        )]),
    );
    assert!(find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).is_none());
    assert_eq!(g.stats.unresolved_external_count, 1);
}

#[test]
fn e2e_foreign_qualified_enum_and_property_hop_emit_no_precise_edge() {
    // Real fixture, run through this crate's own extractor: a foreign
    // qualifier reading the SAME simple name as an in-tree enum
    // ("RabbitMQ.Client.ExchangeType.Fanout"), the in-tree enum reached
    // through its own full name ("App.Transports.Fabric.ExchangeType.
    // Topic"), and a property hop through an external intermediate
    // ("expr.Member.Name") that happens to share a tail with an in-tree
    // class named "Member".
    let files = fragments_for(&[
        (
            "Fabric/ExchangeType.cs",
            "namespace App.Transports.Fabric { public enum ExchangeType { Direct, Fanout, Topic } }",
        ),
        (
            "Model/Member.cs",
            "namespace App.Model { public class Member { public string Name { get; set; } } }",
        ),
        (
            "Bus/Configure.cs",
            "namespace App.Bus { public class Configure { public void Run(System.Linq.Expressions.MemberExpression expr) { var t = RabbitMQ.Client.ExchangeType.Fanout; var own = App.Transports.Fabric.ExchangeType.Topic; var n = expr.Member.Name; } } }",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Bus/Configure.cs")
            .into_iter()
            .map(|(to, _)| to)
            .collect::<Vec<_>>(),
        vec!["App.Transports.Fabric.ExchangeType.Topic"],
        "only the own-namespace fully-qualified access earns a precise edge"
    );
    assert!(
        member_edge_targets(&g)
            .iter()
            .all(|t| *t != "App.Model.Member"),
        "the property hop through the external \"expr\" receiver never binds the in-tree class"
    );
    // The scored tier is untouched by the suffix rule: "Name" is unique
    // in this graph (declared only by App.Model.Member), so the property
    // hop still earns a guess.
    assert_eq!(heuristic_member_edge_targets(&g), vec!["App.Model.Member"]);
}

// --- namespace-proximity (step 3, exact match, not a walk) ------------

#[test]
fn same_namespace_reference_resolves_without_any_using() {
    let files = vec![
        (
            "A/Widget.cs".to_string(),
            frag(
                vec![def("A.Widget", "Widget", "A", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "A/Holder.cs".to_string(),
            frag(
                vec![def("A.Holder", "Holder", "A", "class")],
                vec![],
                vec![type_ref("uses-type", "Widget", None, "A")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.ambiguous_count, 0);
    assert_eq!(g.stats.unresolved_external_count, 0);
    assert_eq!(g.stats.edges_by_kind.uses_type, 1);
}

#[test]
fn empty_namespace_is_treated_as_absent_not_as_a_matchable_prefix() {
    // A file-scope (no enclosing namespace) reference to another
    // file-scope type must resolve via step 4 (global uniqueness), NOT
    // step 3 -- an empty-string ns must behave as absent, not as a
    // matchable prefix.
    let files = vec![
        (
            "Root.cs".to_string(),
            frag(vec![def("Anchor", "Anchor", "", "class")], vec![], vec![]),
        ),
        (
            "Probe.cs".to_string(),
            frag(
                vec![def("Probe", "Probe", "", "class")],
                vec![],
                vec![type_ref("uses-type", "Anchor", None, "")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.ambiguous_count, 0);
    assert_eq!(g.stats.edges_by_kind.uses_type, 1);
}

// --- global using -----------------------------------------------------

#[test]
fn global_using_resolves_a_bare_name_from_an_unrelated_namespace() {
    let files = vec![
        (
            "Catalog/Status.cs".to_string(),
            frag(
                vec![def("Catalog.Status", "Status", "Catalog", "enum")],
                vec![],
                vec![],
            ),
        ),
        (
            "Globals.cs".to_string(),
            frag(
                vec![],
                vec![FragUsing::Plain {
                    text: "Catalog".into(),
                    global: true,
                }],
                vec![],
            ),
        ),
        (
            "Ops/View.cs".to_string(),
            frag(
                vec![def("Ops.View", "View", "Ops", "class")],
                vec![],
                vec![type_ref("uses-type", "Status", None, "Ops")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved via global using");
    match edge {
        Edge::UsesType { to, .. } => assert_eq!(to, "Catalog.Status"),
        _ => unreachable!(),
    }
}

// --- tier (a) widened: static property / field ------------------------
