use super::*;
use crate::graph::FragRegistration;

// A `def()`/`def_with()` carrying explicit per-method arity ranges -- the one
// fact the dispatch resolver's arity-uniqueness test reads, and the one
// neither shared builder exposes.
fn def_with_arities(
    id: &str,
    name: &str,
    ns: &str,
    kind: &str,
    methods: &[&str],
    bases: &[&str],
    arities: &[(&str, &[(usize, i64)])],
) -> FragDef {
    let mut m = OrderedMap::new();
    for (member, ranges) in arities {
        m.insert(member.to_string(), ranges.to_vec());
    }
    FragDef {
        bases: bases.iter().map(|s| s.to_string()).collect(),
        method_arities: m,
        ..def_with(id, name, ns, kind, methods, &[], &[])
    }
}

fn frag_with_registrations(defs: Vec<FragDef>, registrations: Vec<FragRegistration>) -> Fragment {
    Fragment {
        defs,
        usings: vec![],
        refs: vec![],
        names: vec![],
        registrations,
    }
}

fn registration(service: &str, implementation: &str, ns: &str, line: usize) -> FragRegistration {
    FragRegistration {
        service: service.to_string(),
        implementation: implementation.to_string(),
        namespace: ns.to_string(),
        line,
    }
}

// --- B2: the type-level `implements` edge ---------------------------------

#[test]
fn registration_creates_a_type_level_implements_edge_from_implementation_to_service() {
    let files = vec![
        (
            "Dispatch/IContract.cs".to_string(),
            frag(
                vec![def(
                    "App.Dispatch.IContract",
                    "IContract",
                    "App.Dispatch",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Widget.cs".to_string(),
            frag(
                vec![def(
                    "App.Dispatch.Widget",
                    "Widget",
                    "App.Dispatch",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Startup.cs".to_string(),
            frag_with_registrations(
                vec![def(
                    "App.Dispatch.Startup",
                    "Startup",
                    "App.Dispatch",
                    "class",
                )],
                vec![registration("IContract", "Widget", "App.Dispatch", 12)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let hits: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| matches!(e, Edge::Implements { member: None, .. }))
        .collect();
    assert_eq!(hits.len(), 1, "exactly one type-level implements edge");
    match hits[0] {
        Edge::Implements {
            from_file,
            to,
            to_file,
            ..
        } => {
            assert_eq!(
                from_file, "Dispatch/Widget.cs",
                "recorded at the implementation's own site"
            );
            assert_eq!(to, "App.Dispatch.IContract");
            assert_eq!(to_file, "Dispatch/IContract.cs");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.implements, 1);
    assert_eq!(g.stats.edges_by_kind.overrides, 0);
}

#[test]
fn a_registration_naming_a_type_outside_the_graph_emits_no_implements_edge() {
    let files = vec![
        (
            "Dispatch/Widget.cs".to_string(),
            frag(
                vec![def(
                    "App.Dispatch.Widget",
                    "Widget",
                    "App.Dispatch",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Startup.cs".to_string(),
            frag_with_registrations(
                vec![def(
                    "App.Dispatch.Startup",
                    "Startup",
                    "App.Dispatch",
                    "class",
                )],
                // "IUnknown" resolves nowhere in this corpus.
                vec![registration("IUnknown", "Widget", "App.Dispatch", 9)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        !g.edges.iter().any(|e| matches!(e, Edge::Implements { .. })),
        "an unresolved service type must emit no edge at all"
    );
    assert_eq!(g.stats.edges_by_kind.implements, 0);
}

#[test]
fn a_repository_with_no_registration_facts_gains_no_dispatch_edges() {
    let files = vec![
        (
            "Dispatch/IContract.cs".to_string(),
            frag(
                vec![def(
                    "App.Dispatch.IContract",
                    "IContract",
                    "App.Dispatch",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Widget.cs".to_string(),
            frag(
                vec![FragDef {
                    bases: vec!["IContract".to_string()],
                    ..def("App.Dispatch.Widget", "Widget", "App.Dispatch", "class")
                }],
                vec![],
                vec![],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        g.stats.edges_by_kind.implements, 0,
        "an ordinary base-list `IContract` with no registration earns no implements edge"
    );
    assert_eq!(g.stats.edges_by_kind.overrides, 0);
    assert!(!g
        .edges
        .iter()
        .any(|e| matches!(e, Edge::Implements { .. } | Edge::Overrides { .. })));
}

// --- B3: member-level `implements`/`overrides` -----------------------------

#[test]
fn a_registered_implementation_gains_a_member_level_implements_edge_for_its_matching_method() {
    let files = vec![
        (
            "Dispatch/IContract.cs".to_string(),
            frag(
                vec![def_with_arities(
                    "App.Dispatch.IContract",
                    "IContract",
                    "App.Dispatch",
                    "interface",
                    &["Handle"],
                    &[],
                    &[("Handle", &[(1, 1)])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Widget.cs".to_string(),
            frag(
                vec![def_with_arities(
                    "App.Dispatch.Widget",
                    "Widget",
                    "App.Dispatch",
                    "class",
                    &["Handle"],
                    &[],
                    &[("Handle", &[(1, 1)])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Startup.cs".to_string(),
            frag_with_registrations(
                vec![def(
                    "App.Dispatch.Startup",
                    "Startup",
                    "App.Dispatch",
                    "class",
                )],
                vec![registration("IContract", "Widget", "App.Dispatch", 5)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let member_hits: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| {
            matches!(
                e,
                Edge::Implements {
                    member: Some(_),
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        member_hits.len(),
        1,
        "exactly one member-level implements edge"
    );
    match member_hits[0] {
        Edge::Implements {
            member,
            to,
            from_file,
            ..
        } => {
            assert_eq!(member.as_deref(), Some("Handle"));
            assert_eq!(to, "App.Dispatch.IContract");
            assert_eq!(from_file, "Dispatch/Widget.cs");
        }
        _ => unreachable!(),
    }
    // The type-level fact plus the member-level fact -- both counted under
    // the same `implements` stat.
    assert_eq!(g.stats.edges_by_kind.implements, 2);
}

#[test]
fn two_arity_tied_overloads_on_the_implementation_leave_the_member_level_implements_edge_unemitted()
{
    let files = vec![
        (
            "Dispatch/IContract.cs".to_string(),
            frag(
                vec![def_with_arities(
                    "App.Dispatch.IContract",
                    "IContract",
                    "App.Dispatch",
                    "interface",
                    &["Handle"],
                    &[],
                    &[("Handle", &[(1, 1)])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Ambiguous.cs".to_string(),
            frag(
                vec![def_with_arities(
                    "App.Dispatch.Ambiguous",
                    "Ambiguous",
                    "App.Dispatch",
                    "class",
                    &["Handle"],
                    &[],
                    // Two overloads of `Handle` tie at arity (1,1): the
                    // resolver cannot tell which one the interface member
                    // pairs with, so it emits nothing for this member.
                    &[("Handle", &[(1, 1), (1, 1)])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Startup.cs".to_string(),
            frag_with_registrations(
                vec![def(
                    "App.Dispatch.Startup",
                    "Startup",
                    "App.Dispatch",
                    "class",
                )],
                vec![registration("IContract", "Ambiguous", "App.Dispatch", 5)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        !g.edges.iter().any(|e| matches!(e, Edge::Implements { member: Some(_), .. })),
        "an arity tie must emit no member-level implements edge, matching the ladder's own ambiguity rule"
    );
    // The type-level fact still resolves: it is registration-driven only.
    assert_eq!(g.stats.edges_by_kind.implements, 1);
}

#[test]
fn an_override_member_connects_to_the_nearest_in_graph_base_member_of_the_same_name_and_arity() {
    let files = vec![
        (
            "Dispatch/IContract.cs".to_string(),
            frag(
                vec![def(
                    "App.Dispatch.IContract",
                    "IContract",
                    "App.Dispatch",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Base.cs".to_string(),
            frag(
                vec![def_with_arities(
                    "App.Dispatch.Base",
                    "Base",
                    "App.Dispatch",
                    "class",
                    &["Notify"],
                    &[],
                    &[("Notify", &[(0, 0)])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Widget.cs".to_string(),
            frag(
                vec![FragDef {
                    override_methods: vec!["Notify".to_string()],
                    ..def_with_arities(
                        "App.Dispatch.Widget",
                        "Widget",
                        "App.Dispatch",
                        "class",
                        &["Notify"],
                        &["Base"],
                        &[("Notify", &[(0, 0)])],
                    )
                }],
                vec![],
                vec![],
            ),
        ),
        (
            "Dispatch/Startup.cs".to_string(),
            frag_with_registrations(
                vec![def(
                    "App.Dispatch.Startup",
                    "Startup",
                    "App.Dispatch",
                    "class",
                )],
                vec![registration("IContract", "Widget", "App.Dispatch", 7)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let overrides: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| matches!(e, Edge::Overrides { .. }))
        .collect();
    assert_eq!(overrides.len(), 1, "exactly one overrides edge");
    match overrides[0] {
        Edge::Overrides {
            member,
            to,
            from_file,
            ..
        } => {
            assert_eq!(member.as_deref(), Some("Notify"));
            assert_eq!(to, "App.Dispatch.Base");
            assert_eq!(from_file, "Dispatch/Widget.cs");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.overrides, 1);
}
