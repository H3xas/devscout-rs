use super::*;

#[test]
fn ds0012_property_hop_resolves_to_the_propertys_declared_type() {
    let files = vec![
        (
            "Other/Settings.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Settings",
                    "Settings",
                    "App.Other",
                    "class",
                    &["Reload"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![with_member_types(
                    def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &[],
                        &["Config"],
                        &[],
                    ),
                    &[],
                    &[("Config", "Settings")],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Hop.cs".to_string(),
            frag(
                vec![def("App.Consumers.Hop", "Hop", "App.Consumers", "class")],
                vec![FragUsing::Plain {
                    text: "App.Other".to_string(),
                    global: false,
                }],
                vec![property_hop_ref(
                    "Widget",
                    "Config",
                    "Reload",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_edge_targets(&g), vec!["App.Other.Settings"]);
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "a precise hop leaves nothing for the scored tier"
    );
}

#[test]
fn ds0012_property_hop_stops_on_an_unrecorded_property_a_missing_member_and_an_ambiguous_type() {
    let files = vec![
        (
            "Other/Settings.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Settings",
                    "Settings",
                    "App.Other",
                    "class",
                    &["Reload"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![with_member_types(
                    def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &[],
                        &["Label", "Config", "Price"],
                        &[],
                    ),
                    &[],
                    // `Label` is declared `string`: a predefined type
                    // records no fact at all, so it is absent here.
                    &[("Config", "Settings"), ("Price", "Money")],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Money/A.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Money.A.Money",
                    "Money",
                    "App.Money.A",
                    "class",
                    &["Round"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Money/B.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Money.B.Money",
                    "Money",
                    "App.Money.B",
                    "class",
                    &["Round"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Stops.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Stops",
                    "Stops",
                    "App.Consumers",
                    "class",
                )],
                vec![
                    FragUsing::Plain {
                        text: "App.Other".to_string(),
                        global: false,
                    },
                    FragUsing::Plain {
                        text: "App.Money.A".to_string(),
                        global: false,
                    },
                    FragUsing::Plain {
                        text: "App.Money.B".to_string(),
                        global: false,
                    },
                ],
                vec![
                    property_hop_ref("Widget", "Label", "Trim", "App.Consumers"),
                    property_hop_ref("Widget", "Config", "Missing", "App.Consumers"),
                    property_hop_ref("Widget", "Price", "Round", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edge_targets(&g).is_empty(),
        "no recorded type, no declared member, and an ambiguous type each end the hop"
    );
}

#[test]
fn ds0010_var_from_invocation_resolves_through_the_callees_recorded_return_type() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Factory.cs".to_string(),
            frag(
                vec![with_member_types(
                    def_with(
                        "App.Other.Factory",
                        "Factory",
                        "App.Other",
                        "class",
                        &["Make"],
                        &[],
                        &[],
                    ),
                    &[("Make", "Widget")],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/FromCall.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.FromCall",
                    "FromCall",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".to_string(),
                    global: false,
                }],
                vec![call_receiver_ref(
                    "made",
                    "Factory",
                    "Make",
                    "Render",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
}

#[test]
fn ds0010_ambiguous_out_of_graph_and_return_less_callees_stay_taken_but_unknown() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "A/Factory.cs".to_string(),
            frag(
                vec![with_member_types(
                    def_with(
                        "App.A.Factory",
                        "Factory",
                        "App.A",
                        "class",
                        &["Make"],
                        &[],
                        &[],
                    ),
                    &[("Make", "Widget")],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "B/Factory.cs".to_string(),
            frag(
                vec![with_member_types(
                    def_with(
                        "App.B.Factory",
                        "Factory",
                        "App.B",
                        "class",
                        &["Make"],
                        &[],
                        &[],
                    ),
                    &[("Make", "Widget")],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Silent.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Silent",
                    "Silent",
                    "App.Other",
                    "class",
                    &["Make"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Unknowns.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Unknowns",
                    "Unknowns",
                    "App.Consumers",
                    "class",
                )],
                vec![
                    FragUsing::Plain {
                        text: "App.Other".to_string(),
                        global: false,
                    },
                    FragUsing::Plain {
                        text: "App.A".to_string(),
                        global: false,
                    },
                    FragUsing::Plain {
                        text: "App.B".to_string(),
                        global: false,
                    },
                ],
                vec![
                    call_receiver_ref("ambiguous", "Factory", "Make", "Render", "App.Consumers"),
                    call_receiver_ref("external", "ThirdParty", "Make", "Render", "App.Consumers"),
                    // `Silent.Make` is declared but records no return type
                    // (a void method blocks its own name).
                    call_receiver_ref("silent", "Silent", "Make", "Render", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    // An owner the ladder refuses to pick, an owner it never finds, and an
    // owner whose `Make` records no return type all leave the local exactly
    // as unknown as the extractor left it.
    assert!(member_edge_targets(&g).is_empty());
}

// --- this/base receiver typing, awaited Task unwrap ---------------------
//
// All four run real C# through the extractor (`fragments_for`), the same
// choice the tier-(e) end-to-end block above makes: a `this.`/`base.`
// qualifier's `receiverBase`/`receiverAwaited` bits and a method's
// `methodReturnArgs` are extractor facts, so a test that hand-built the
// fragments would take the extractor's word for them rather than proving
// them.
