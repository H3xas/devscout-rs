use super::*;

#[test]
fn uses_member_edge_resolves_only_when_qualifier_is_an_enum() {
    let files = vec![
        (
            "A/Status.cs".to_string(),
            frag(
                vec![
                    def("A.Status", "Status", "A", "enum"),
                    def("A.Status.Active", "Active", "A", "enum-member"),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "B/Bundle.cs".to_string(),
            frag(
                vec![def("B.Bundle", "Bundle", "B", "class")],
                vec![],
                vec![member_ref("Status", None, "Active", "A")], // same namespace as the enum
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { to, to_file, .. } => {
            assert_eq!(to, "A.Status.Active");
            assert_eq!(to_file, "A/Status.cs");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.uses_member, 1);
}

#[test]
fn uses_member_on_a_non_enum_qualifier_is_dropped_silently() {
    let files = vec![
        (
            "A/Constants.cs".to_string(),
            frag(
                vec![def("A.Constants", "Constants", "A", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "B/View.cs".to_string(),
            frag(
                vec![def("B.View", "View", "B", "class")],
                vec![FragUsing::Plain {
                    text: "A".into(),
                    global: false,
                }],
                vec![member_ref("Constants", None, "MaxRetries", "B")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(g
        .edges
        .iter()
        .all(|e| !matches!(e, Edge::UsesMember { .. })));
    // Silently dropped -- not counted as ambiguous or external.
    assert_eq!(g.stats.ambiguous_count, 0);
    assert_eq!(g.stats.unresolved_external_count, 0);
}

// --- multi-part (qualified) member-access qualifiers ---

#[test]
fn qualified_multipart_member_qualifier_resolves_via_exact_fqn_ladder_step() {
    let files = vec![
        (
            "Enums/MyEnum.cs".to_string(),
            frag(
                vec![
                    def("Some.Namespace.MyEnum", "MyEnum", "Some.Namespace", "enum"),
                    def(
                        "Some.Namespace.MyEnum.Member",
                        "Member",
                        "Some.Namespace",
                        "enum-member",
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Reader.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Reader",
                    "Reader",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![member_ref(
                    "MyEnum",
                    Some("Some.Namespace.MyEnum"),
                    "Member",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { to, to_file, .. } => {
            assert_eq!(to, "Some.Namespace.MyEnum.Member");
            assert_eq!(to_file, "Enums/MyEnum.cs");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.uses_member, 1);
}

#[test]
fn namespace_alias_qualified_member_ref_resolves_through_the_alias_target() {
    // Step 0 (the alias short-circuit) only ever fires for a BARE,
    // non-dotted ref. "Ns.MyEnum" is dotted, so it reaches step 1a
    // instead, which rewrites the aliased head to its target and looks
    // the whole name up exactly -- genuine alias resolution, not the
    // bare-tail uniqueness a dotted ref no longer gets.
    let files = vec![
        (
            "Enums/MyEnum.cs".to_string(),
            frag(
                vec![
                    def("Some.Namespace.MyEnum", "MyEnum", "Some.Namespace", "enum"),
                    def(
                        "Some.Namespace.MyEnum.Member",
                        "Member",
                        "Some.Namespace",
                        "enum-member",
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/AliasNsUser.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.AliasNsUser",
                    "AliasNsUser",
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
                    "Member",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
        .expect("resolves via the Ns alias rewritten to its target");
    match edge {
        Edge::UsesMember { to, .. } => assert_eq!(to, "Some.Namespace.MyEnum.Member"),
        _ => unreachable!(),
    }
}

// --- non-enum emission tiers ---

#[test]
fn dotted_exact_qualified_member_access_to_a_static_class_emits_uses_member_edge() {
    let files = vec![
        (
            "Other/Utils.cs".to_string(),
            frag(
                vec![def("App.Other.Utils", "Utils", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UsesNonEnum.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UsesNonEnum",
                    "UsesNonEnum",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![member_ref(
                    "Utils",
                    Some("App.Other.Utils"),
                    "MaxRetries",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
        .expect("exact-qualified static access emits");
    match edge {
        Edge::UsesMember { to, .. } => assert_eq!(
            to, "App.Other.Utils",
            "targets the type def -- member defs exist only for enums"
        ),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.uses_member, 1);
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "uses-member misses must never be reported as ambiguous"
    );
    assert_eq!(g.stats.unresolved_external_count, 0);
}

#[test]
fn bare_qualifier_to_a_class_emits_only_when_the_member_is_a_recorded_method() {
    let urn = def_with(
        "App.Other.MessageUrn",
        "MessageUrn",
        "App.Other",
        "class",
        &["ForType"],
        &[],
        &[],
    );
    let files = vec![
        (
            "Other/MessageUrn.cs".to_string(),
            frag(vec![urn], vec![], vec![]),
        ),
        (
            "Consumers/CallsStatic.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.CallsStatic",
                    "CallsStatic",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![
                    member_ref("MessageUrn", None, "ForType", "App.Consumers"),
                    member_ref("MessageUrn", None, "SomeUnknownField", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 1,
        "the method call emits; the unknown-member access does not (could be a same-named property)"
    );
    match find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).unwrap() {
        Edge::UsesMember { to, .. } => assert_eq!(to, "App.Other.MessageUrn"),
        _ => unreachable!(),
    }
}

#[test]
fn generic_qualifier_emits_because_type_argument_syntax_cannot_be_a_local_or_property() {
    let mut generic_ref = member_ref("TypeCache", None, "Cached", "App.Consumers");
    generic_ref.generic = true;
    let files = vec![
        (
            "Other/TypeCache.cs".to_string(),
            frag(
                vec![def(
                    "App.Other.TypeCache",
                    "TypeCache",
                    "App.Other",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UsesCache.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UsesCache",
                    "UsesCache",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![generic_ref],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 1,
        "a generic qualifier is type-certain even for a property member"
    );
    match find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).unwrap() {
        Edge::UsesMember { to, .. } => assert_eq!(to, "App.Other.TypeCache"),
        _ => unreachable!(),
    }
}

#[test]
fn dotted_chain_with_inherited_generic_flag_does_not_emit_via_tail_name_match() {
    // "EqualityComparer<TSaga>.Default.GetHashCode(...)" shape: the
    // flattened qualifier "EqualityComparer.Default" carries generic=true
    // from its inner segment; its TAIL name "Default" happens to match a
    // real type. Gate-audit regression: no edge.
    let mut chain_ref = member_ref(
        "Default",
        Some("EqualityComparer.Default"),
        "GetHashCode",
        "App.Consumers",
    );
    chain_ref.generic = true;
    let files = vec![
        (
            "Other/Default.cs".to_string(),
            frag(
                vec![def("App.Other.Default", "Default", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Chain.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Chain",
                    "Chain",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![chain_ref],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        g.edges
            .iter()
            .all(|e| !matches!(e, Edge::UsesMember { .. })),
        "chain-tail name match must not emit"
    );
}

#[test]
fn bare_non_method_member_on_a_non_enum_qualifier_is_still_dropped_silently() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def("App.Other.Widget", "Widget", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/PropLike.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.PropLike",
                    "PropLike",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![member_ref("Widget", None, "Name", "App.Consumers")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        g.edges
            .iter()
            .all(|e| !matches!(e, Edge::UsesMember { .. })),
        "no certainty signal -> no edge (could be an instance property named Widget)"
    );
    assert_eq!(g.stats.ambiguous_count, 0);
    assert_eq!(g.stats.unresolved_external_count, 0);
}

#[test]
fn nested_enum_dotted_qualifier_resolves_via_global_uniqueness_not_the_plus_joined_id() {
    // The "+"-joined nested-type id ("App.Widgets.Outer+Inner") never
    // matches the literal dotted source text ("Outer.Inner") at ladder
    // step 1 -- same as an ordinary nested TYPE reference, this only
    // resolves via step 4 (globally unique simple name "Inner").
    let files = vec![
        (
            "Enums/Container.cs".to_string(),
            frag(
                vec![
                    def("App.Widgets.Outer", "Outer", "App.Widgets", "class"),
                    def("App.Widgets.Outer+Inner", "Inner", "App.Widgets", "enum"),
                    def(
                        "App.Widgets.Outer+Inner.On",
                        "On",
                        "App.Widgets",
                        "enum-member",
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/NestedUser.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.NestedUser",
                    "NestedUser",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![member_ref(
                    "Inner",
                    Some("Outer.Inner"),
                    "On",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { to, to_file, .. } => {
            assert_eq!(to, "App.Widgets.Outer+Inner.On");
            assert_eq!(to_file, "Enums/Container.cs");
        }
        _ => unreachable!(),
    }
}

// --- declaration_expression -> uses-type ref, same ladder ---

#[test]
fn declaration_expression_type_ref_resolves_through_the_normal_uses_type_ladder() {
    let files = vec![
        (
            "Enums/PostType.cs".to_string(),
            frag(
                vec![def("App.Enums.PostType", "PostType", "App.Enums", "enum")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/OutUser.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.OutUser",
                    "OutUser",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Enums".into(),
                    global: false,
                }],
                vec![type_ref("uses-type", "PostType", None, "App.Consumers")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present");
    match edge {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Enums.PostType"),
        _ => unreachable!(),
    }
}

#[test]
fn declaration_expression_type_ref_with_ambiguous_simple_name_is_marked_ambiguous() {
    let files = vec![
        (
            "A/Status.cs".to_string(),
            frag(
                vec![def("A.Status", "Status", "A", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "B/Status.cs".to_string(),
            frag(
                vec![def("B.Status", "Status", "B", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/AmbiguousOutUser.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.AmbiguousOutUser",
                    "AmbiguousOutUser",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![type_ref("uses-type", "Status", None, "App.Consumers")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::Ambiguous { .. })).expect("ambiguous edge present");
    match edge {
        Edge::Ambiguous {
            origin,
            raw,
            candidate_count,
            ..
        } => {
            assert_eq!(origin, "uses-type");
            assert_eq!(raw, "Status");
            assert_eq!(*candidate_count, 2);
        }
        _ => unreachable!(),
    }
}

// --- qualified (dotted) resolution: enclosing-namespace walk ----------
