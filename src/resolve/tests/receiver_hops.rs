use super::*;

fn same_name_property_hop(generic_declares_select: bool, recorded_args: bool) -> Graph {
    let plain = def_with(
        "App.Lists.ItemList",
        "ItemList",
        "App.Lists",
        "interface",
        &["Select"],
        &[],
        &[],
    );
    let generic = FragDef {
        type_params: vec!["T".into()],
        ..def_with(
            "App.Lists.ItemList",
            "ItemList",
            "App.Lists",
            "interface",
            if generic_declares_select {
                &["Select"]
            } else {
                &[]
            },
            &[],
            &[],
        )
    };
    let mut property_types = OrderedMap::new();
    property_types.insert(
        "Consumed".into(),
        FragFact {
            type_name: "ItemList".into(),
            args: recorded_args.then(|| vec!["*".into()]),
        },
    );
    let holder = FragDef {
        property_types,
        ..def_with(
            "App.Lists.Holder",
            "Holder",
            "App.Lists",
            "class",
            &[],
            &["Consumed"],
            &[],
        )
    };
    let files = vec![
        (
            "Lists/Plain.cs".to_string(),
            frag(vec![plain], vec![], vec![]),
        ),
        (
            "Lists/Generic.cs".to_string(),
            frag(vec![generic], vec![], vec![]),
        ),
        (
            "Lists/Holder.cs".to_string(),
            frag(vec![holder], vec![], vec![]),
        ),
        (
            "Consumers/Hop.cs".to_string(),
            frag(
                vec![def("App.Consumers.Hop", "Hop", "App.Consumers", "class")],
                vec![FragUsing::Plain {
                    text: "App.Lists".into(),
                    global: false,
                    namespace: String::new(),
                }],
                vec![property_hop_ref(
                    "Holder",
                    "Consumed",
                    "Select",
                    "App.Consumers",
                )],
            ),
        ),
    ];
    resolve_graph(&no_git_root(), &files)
}

#[test]
fn recorded_property_args_choose_the_generic_sibling() {
    let g = same_name_property_hop(true, true);
    let targets: Vec<&str> = g
        .edges
        .iter()
        .filter_map(|edge| match edge {
            Edge::UsesMember {
                to_file,
                heuristic: false,
                ..
            } => Some(to_file.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(targets, vec!["Lists/Generic.cs"]);
}

#[test]
fn recorded_property_args_refuse_the_unrelated_plain_sibling() {
    let g = same_name_property_hop(false, true);
    assert!(member_edge_targets(&g).is_empty());
}

#[test]
fn absent_property_args_keep_the_existing_fallback() {
    let g = same_name_property_hop(false, false);
    assert_eq!(member_edge_targets(&g), vec!["App.Lists.ItemList"]);
}

#[test]
fn property_hop_applies_the_typed_receiver_delegate_and_visibility_gate() {
    let definitions = r#"namespace App.Lists
{
  public delegate bool Filter<T>(T value);
  public class Entry { }
  public class ItemList
  {
    public void Select(Action<Entry> action) { }
    public void Select<T>(Filter<T> filter) { }
  }
  public class Holder { public ItemList Consumed { get; } }
}"#;
    let graph_for = |defs: &str, call: &str| {
        let consumer = format!(
            "using App.Lists;\nnamespace App.Consumers;\npublic class Hop {{ public void Run(Holder h) {{ {call}; }} }}"
        );
        resolve_graph(
            &no_git_root(),
            &fragments_for(&[
                ("Lists/Types.cs", defs),
                ("Consumers/Hop.cs", consumer.as_str()),
            ]),
        )
    };
    let targets = |g: &Graph| {
        member_edges_named(g, "Consumers/Hop.cs", "Select")
            .into_iter()
            .map(|(to, _)| to.to_string())
            .collect::<Vec<_>>()
    };
    let refused = graph_for(definitions, "h.Consumed.Select(x => true)");
    assert!(
        targets(&refused).is_empty(),
        "no overload accepts the value lambda"
    );
    let admitted = graph_for(definitions, "h.Consumed.Select(x => x.ToString())");
    assert_eq!(targets(&admitted), vec!["App.Lists.ItemList"]);
    let private_only = definitions
        .replace(
            "public void Select(Action<Entry> action)",
            "private void Select(Action<Entry> action)",
        )
        .replace("public void Select<T>", "private void Select<T>");
    let hidden = graph_for(&private_only, "h.Consumed.Select(x => x.ToString())");
    assert!(
        targets(&hidden).is_empty(),
        "ordinary property access is not this-shaped"
    );
}

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
                    namespace: String::new(),
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
                        namespace: String::new(),
                    },
                    FragUsing::Plain {
                        text: "App.Money.A".to_string(),
                        global: false,
                        namespace: String::new(),
                    },
                    FragUsing::Plain {
                        text: "App.Money.B".to_string(),
                        global: false,
                        namespace: String::new(),
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
                    namespace: String::new(),
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
                        namespace: String::new(),
                    },
                    FragUsing::Plain {
                        text: "App.A".to_string(),
                        global: false,
                        namespace: String::new(),
                    },
                    FragUsing::Plain {
                        text: "App.B".to_string(),
                        global: false,
                        namespace: String::new(),
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
