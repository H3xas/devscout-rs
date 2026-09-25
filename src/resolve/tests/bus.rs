use super::*;

// A `def()` carrying a base list plus the flattened and nested generic-arg
// facts a consumer/handler base records -- the two `bus.rs` reads that
// neither shared builder exposes. `nested` entries are optional per base
// name: only the bases this test wants to carry a `baseTypeArgs` fact pass
// one.
fn consumer_def(
    id: &str,
    name: &str,
    ns: &str,
    bases: &[&str],
    flat: &[(&str, &[&str])],
    nested: &[(&str, &[&str])],
) -> FragDef {
    let mut flat_map = OrderedMap::new();
    for (base, args) in flat {
        flat_map.insert(
            (*base).to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        );
    }
    let mut nested_map = OrderedMap::new();
    for (base, args) in nested {
        nested_map.insert(
            (*base).to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        );
    }
    FragDef {
        bases: bases.iter().map(|s| s.to_string()).collect(),
        base_generic_args: flat_map,
        base_type_args: nested_map,
        ..def(id, name, ns, "class")
    }
}

// `FragPublish` has no fixture builder of its own here (unlike `FragDef`'s
// `def()`) and this module does not import the type by name at all: a
// round trip through its own `Deserialize` impl, via `Fragment`'s, lets
// `Vec<FragPublish>` come back fully typed from a plain JSON value with no
// type name written on this side.
fn publish(verb: &str, message: &str, ns: &str, line: usize) -> serde_json::Value {
    serde_json::json!({
        "verb": verb,
        "message": message,
        "namespace": ns,
        "line": line,
    })
}

fn frag_with_publishes(defs: Vec<FragDef>, publishes: Vec<serde_json::Value>) -> Fragment {
    let mut value = serde_json::to_value(frag(defs, vec![], vec![])).unwrap();
    value["publishes"] = serde_json::Value::Array(publishes);
    serde_json::from_value(value).unwrap()
}

fn bus_hops(g: &Graph) -> Vec<&Edge> {
    g.edges
        .iter()
        .filter(|e| matches!(e, Edge::BusHop { .. }))
        .collect()
}

// --- the required end-to-end claim -----------------------------------

#[test]
fn a_publish_site_reaches_its_handler_only_when_both_message_identities_resolve() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Bus.LoanRequested",
                    "LoanRequested",
                    "App.Bus",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Consumers.cs".to_string(),
            frag(
                vec![
                    consumer_def(
                        "App.Bus.WidgetA",
                        "WidgetA",
                        "App.Bus",
                        &["IConsumer"],
                        &[("IConsumer", &["LoanRequested"])],
                        &[],
                    ),
                    // Same shape as WidgetA, but its own base argument names
                    // a type this corpus never declares -- the consumer
                    // side's own message never resolves, so this def must
                    // earn no hop despite matching WidgetA's base name.
                    consumer_def(
                        "App.Bus.WidgetB",
                        "WidgetB",
                        "App.Bus",
                        &["IConsumer"],
                        &[("IConsumer", &["Ghost"])],
                        &[],
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Publisher.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Publisher", "Publisher", "App.Bus", "class")],
                vec![publish("Publish", "LoanRequested", "App.Bus", 10)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let hops = bus_hops(&g);
    assert_eq!(hops.len(), 1, "WidgetB's own message never resolves");
    match hops[0] {
        Edge::BusHop {
            from_file,
            from_line,
            message,
            to,
            to_file,
            evidence,
        } => {
            assert_eq!(from_file, "Bus/Publisher.cs");
            assert_eq!(*from_line, 10);
            assert_eq!(message, "App.Bus.LoanRequested");
            assert_eq!(to, "App.Bus.WidgetA");
            assert_eq!(to_file, "Bus/Consumers.cs");
            assert_eq!(evidence, "base-arg");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.bus_hop, Some(1));
}

// --- fan-out ------------------------------------------------------------

#[test]
fn one_message_with_two_handlers_emits_one_edge_per_handler() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Bus.OverdueReminder",
                    "OverdueReminder",
                    "App.Bus",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Consumers.cs".to_string(),
            frag(
                vec![
                    consumer_def(
                        "App.Bus.LedgerConsumer",
                        "LedgerConsumer",
                        "App.Bus",
                        &["IConsumer"],
                        &[("IConsumer", &["OverdueReminder"])],
                        &[],
                    ),
                    consumer_def(
                        "App.Bus.NotifierConsumer",
                        "NotifierConsumer",
                        "App.Bus",
                        &["IConsumer"],
                        &[("IConsumer", &["OverdueReminder"])],
                        &[],
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Trigger.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Trigger", "Trigger", "App.Bus", "class")],
                vec![publish("Publish", "OverdueReminder", "App.Bus", 7)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let mut targets: Vec<&str> = bus_hops(&g)
        .iter()
        .map(|e| match e {
            Edge::BusHop { to, .. } => to.as_str(),
            _ => unreachable!(),
        })
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec!["App.Bus.LedgerConsumer", "App.Bus.NotifierConsumer"],
        "one edge per handler, not one edge per message"
    );
    assert_eq!(g.stats.edges_by_kind.bus_hop, Some(2));
}

// --- batch consumers ------------------------------------------------------

#[test]
fn a_batch_consumers_inner_type_argument_is_the_message_not_the_wrapper() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Bus.LoanRequested",
                    "LoanRequested",
                    "App.Bus",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/BatchConsumer.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.Bus.LoanBatchConsumer",
                    "LoanBatchConsumer",
                    "App.Bus",
                    &["IConsumer"],
                    &[("IConsumer", &["Batch"])],
                    &[("IConsumer", &["Batch<LoanRequested>"])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Publisher.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Publisher", "Publisher", "App.Bus", "class")],
                vec![publish("PublishAsync", "LoanRequested", "App.Bus", 3)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let hops = bus_hops(&g);
    assert_eq!(hops.len(), 1);
    match hops[0] {
        Edge::BusHop {
            message,
            to,
            evidence,
            ..
        } => {
            assert_eq!(
                message, "App.Bus.LoanRequested",
                "the message is the wrapped argument, never the Batch wrapper itself"
            );
            assert_eq!(to, "App.Bus.LoanBatchConsumer");
            assert_eq!(evidence, "nested-base-arg");
        }
        _ => unreachable!(),
    }
}

// --- a base's own second generic argument earns its own evidence word -----

#[test]
fn a_two_argument_handler_base_takes_its_message_from_the_first_argument() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![
                    def("App.Bus.LookupRequest", "LookupRequest", "App.Bus", "class"),
                    def("App.Bus.LookupResult", "LookupResult", "App.Bus", "class"),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Handler.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.Bus.LookupHandler",
                    "LookupHandler",
                    "App.Bus",
                    &["IRequestHandler"],
                    &[("IRequestHandler", &["LookupRequest", "LookupResult"])],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Router.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Router", "Router", "App.Bus", "class")],
                vec![publish("Send", "LookupRequest", "App.Bus", 5)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let hops = bus_hops(&g);
    assert_eq!(hops.len(), 1);
    match hops[0] {
        Edge::BusHop {
            message, evidence, ..
        } => {
            assert_eq!(message, "App.Bus.LookupRequest");
            assert_eq!(evidence, "mediator-request");
        }
        _ => unreachable!(),
    }
}

// --- inherited consumer bases ---------------------------------------------

#[test]
fn a_subclass_of_a_consumer_base_receives_the_base_classes_message() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Bus.CatalogueEntryPublished",
                    "CatalogueEntryPublished",
                    "App.Bus",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Consumer.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.Bus.CatalogueEntryConsumer",
                    "CatalogueEntryConsumer",
                    "App.Bus",
                    &["BaseConsumer"],
                    &[("BaseConsumer", &["CatalogueEntryPublished"])],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Publisher.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Publisher", "Publisher", "App.Bus", "class")],
                vec![publish(
                    "PublishAsync",
                    "CatalogueEntryPublished",
                    "App.Bus",
                    2,
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let hops = bus_hops(&g);
    assert_eq!(hops.len(), 1);
    match hops[0] {
        Edge::BusHop { to, evidence, .. } => {
            assert_eq!(to, "App.Bus.CatalogueEntryConsumer");
            assert_eq!(evidence, "base-arg");
        }
        _ => unreachable!(),
    }
}

// --- negatives --------------------------------------------------------

#[test]
fn two_same_named_messages_in_different_namespaces_never_link() {
    let files = vec![
        (
            "Bus/OneMessage.cs".to_string(),
            frag(
                vec![def("App.One.Ghost", "Ghost", "App.One", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/TwoMessage.cs".to_string(),
            frag(
                vec![def("App.Two.Ghost", "Ghost", "App.Two", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/OneConsumer.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.One.GhostConsumer",
                    "GhostConsumer",
                    "App.One",
                    &["IConsumer"],
                    &[("IConsumer", &["Ghost"])],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/TwoPublisher.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Two.Publisher", "Publisher", "App.Two", "class")],
                // Resolves through the SAME ladder against ITS OWN namespace
                // (App.Two), landing on App.Two.Ghost -- a different def
                // than the one App.One.GhostConsumer's own base resolved
                // to -- so the shared bare name must never bridge them.
                vec![publish("Publish", "Ghost", "App.Two", 4)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        bus_hops(&g).is_empty(),
        "a bare name shared by two unrelated namespaced types must never link a hop"
    );
    assert_eq!(g.stats.edges_by_kind.bus_hop, None);
}

#[test]
fn a_publish_site_naming_an_unhandled_message_earns_no_edge_even_beside_a_handled_one() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![
                    def("App.Bus.LoanRequested", "LoanRequested", "App.Bus", "class"),
                    def(
                        "App.Bus.ExternalNotice",
                        "ExternalNotice",
                        "App.Bus",
                        "class",
                    ),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Consumer.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.Bus.LoanConsumer",
                    "LoanConsumer",
                    "App.Bus",
                    &["IConsumer"],
                    &[("IConsumer", &["LoanRequested"])],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Decoy.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Decoy", "Decoy", "App.Bus", "class")],
                // Publishes a type this corpus declares but nobody consumes
                // -- an in-graph resolution with no registered handler, the
                // resolve-layer shape a same-line decoy reference collapses
                // to once extraction has already picked the actually
                // published type.
                vec![publish("Publish", "ExternalNotice", "App.Bus", 9)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        bus_hops(&g).is_empty(),
        "ExternalNotice has no registered consumer, and LoanConsumer's own message was never published here"
    );
}

#[test]
fn a_wildcard_base_argument_supplies_no_concrete_message() {
    let files = vec![
        (
            "Bus/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Bus.LoanRequested",
                    "LoanRequested",
                    "App.Bus",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/OpenConsumer.cs".to_string(),
            frag(
                vec![consumer_def(
                    "App.Bus.OpenConsumer",
                    "OpenConsumer",
                    "App.Bus",
                    &["IConsumer"],
                    // An unbound pass-through, the shape `base_generic_args`
                    // records for a type parameter of the declaring def
                    // itself rather than a closed argument.
                    &[("IConsumer", &["*"])],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Bus/Publisher.cs".to_string(),
            frag_with_publishes(
                vec![def("App.Bus.Publisher", "Publisher", "App.Bus", "class")],
                vec![publish("Publish", "LoanRequested", "App.Bus", 1)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        bus_hops(&g).is_empty(),
        "a wildcard names no concrete message, so nothing can ever match it"
    );
}

#[test]
fn a_repository_with_no_publish_site_gains_no_bus_hop_edges_at_all() {
    let files = vec![(
        "Bus/Consumer.cs".to_string(),
        frag(
            vec![consumer_def(
                "App.Bus.LoanConsumer",
                "LoanConsumer",
                "App.Bus",
                &["IConsumer"],
                &[("IConsumer", &["LoanRequested"])],
                &[],
            )],
            vec![],
            vec![],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(bus_hops(&g).is_empty());
    assert_eq!(g.stats.edges_by_kind.bus_hop, None);
}
