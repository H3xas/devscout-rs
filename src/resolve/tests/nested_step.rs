use super::*;

#[test]
fn v8_nested_step_resolves_a_type_declared_in_the_enclosing_type() {
    let files = vec![(
        "Core/Types.cs".to_string(),
        frag(
            vec![
                def("App.Core.Outer", "Outer", "App.Core", "class"),
                def("App.Core.Outer+Nested", "Nested", "App.Core", "class"),
                def("App.Core.Other+Nested", "Nested", "App.Core", "class"),
            ],
            vec![],
            vec![nested_ref("uses-type", "Nested", "App.Core", &["Outer"])],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Outer+Nested"),
        _ => unreachable!(),
    }
    // Two same-named nested defs: without the stack this was ambiguous.
    assert_eq!(g.stats.ambiguous_count, 0);
}

#[test]
fn v8_nested_step_beats_the_namespace_and_usings_steps() {
    let files = vec![
        (
            "Other/Beta.cs".to_string(),
            frag(
                vec![def("App.Other.Beta", "Beta", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Core/Types.cs".to_string(),
            frag(
                vec![
                    def("App.Core.Alpha", "Alpha", "App.Core", "class"),
                    def("App.Core.Outer+Alpha", "Alpha", "App.Core", "class"),
                    def("App.Core.Outer+Beta", "Beta", "App.Core", "class"),
                ],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![
                    nested_ref("uses-type", "Alpha", "App.Core", &["Outer"]),
                    nested_ref("uses-type", "Beta", "App.Core", &["Outer"]),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let targets: Vec<&str> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesType { to, .. } => Some(to.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(targets, vec!["App.Core.Outer+Alpha", "App.Core.Outer+Beta"]);
}

#[test]
fn v8_alias_still_short_circuits_above_the_nested_step() {
    // C# puts type scope above a using-alias; devscout keeps the alias first
    // by construction -- a documented deviation, pinned here.
    let files = vec![
        (
            "Other/Gamma.cs".to_string(),
            frag(
                vec![def("App.Other.Gamma", "Gamma", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Core/Types.cs".to_string(),
            frag(
                vec![def("App.Core.Outer+Gamma", "Gamma", "App.Core", "class")],
                vec![FragUsing::Alias {
                    alias: "Gamma".into(),
                    target: "App.Other.Gamma".into(),
                    global: false,
                }],
                vec![nested_ref("uses-type", "Gamma", "App.Core", &["Outer"])],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Other.Gamma"),
        _ => unreachable!(),
    }
}

#[test]
fn v8_the_innermost_enclosing_type_wins_over_an_outer_one() {
    let files = vec![(
        "Core/Nest.cs".to_string(),
        frag(
            vec![
                def("App.Core.Outer+Target", "Target", "App.Core", "class"),
                def("App.Core.Outer+Inner+Target", "Target", "App.Core", "class"),
            ],
            vec![],
            vec![nested_ref(
                "uses-type",
                "Target",
                "App.Core",
                &["Outer", "Inner"],
            )],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Outer+Inner+Target"),
        _ => unreachable!(),
    }
}

#[test]
fn v8_a_dotted_nested_ref_never_enters_the_nested_step() {
    // BOUNDS: "." is not "+", so a dotted "Outer.Nested" stays on the
    // qualified ladder and never enters step 0b. It is the dotted suffix
    // step that reads the text: only the def whose path ends in
    // `Outer.Nested` matches, so the same-named `Other+Nested` cannot
    // make it ambiguous.
    let files = vec![(
        "Core/Types.cs".to_string(),
        frag(
            vec![
                def("App.Core.Outer+Nested", "Nested", "App.Core", "class"),
                def("App.Core.Other+Nested", "Nested", "App.Core", "class"),
            ],
            vec![],
            vec![FragRef {
                outer_types: vec!["Outer".into()],
                ..type_ref("uses-type", "Nested", Some("Outer.Nested"), "App.Core")
            }],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Outer+Nested"),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.ambiguous_count, 0);
}

#[test]
fn v8_an_outer_types_naming_no_nested_id_falls_through_unchanged() {
    let files = vec![
        (
            "Other/Marker.cs".to_string(),
            frag(
                vec![def("App.Other.Marker", "Marker", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Core/Types.cs".to_string(),
            frag(
                vec![def("App.Core.Outer", "Outer", "App.Core", "class")],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![nested_ref("uses-type", "Marker", "App.Core", &["Outer"])],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present") {
        Edge::UsesType { to, .. } => assert_eq!(to, "App.Other.Marker"),
        _ => unreachable!(),
    }
}

#[test]
fn v8_tier_e_receiver_probe_carries_the_refs_outer_types() {
    // The whole defect: without the stack on the SYNTHETIC probe the
    // member access resolves against two same-named nested types and can
    // only ever be a guess.
    let files = vec![(
        "Core/Types.cs".to_string(),
        frag(
            vec![
                def("App.Core.Outer", "Outer", "App.Core", "class"),
                def_with(
                    "App.Core.Outer+Nested",
                    "Nested",
                    "App.Core",
                    "class",
                    &["Run"],
                    &[],
                    &[],
                ),
                def_with(
                    "App.Core.Other+Nested",
                    "Nested",
                    "App.Core",
                    "class",
                    &["Run"],
                    &[],
                    &[],
                ),
            ],
            vec![],
            vec![FragRef {
                outer_types: vec!["Outer".into()],
                ..receiver_ref("_n", "Run", "App.Core", "Nested", Some(0))
            }],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    let precise: Vec<&str> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                to,
                heuristic: false,
                ..
            } => Some(to.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(precise, vec!["App.Core.Outer+Nested"]);
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

// --- constructor-parameter DI resolution ---
//
// These tests cover the RESOLVED 'ctor-di' edge `resolve_graph` produces
// from the extraction-layer facts (a def's type_params/base_generic_args and
// the 'ctor-param' ref itself).
