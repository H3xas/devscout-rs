use super::def_member_facts::member_facts;
use super::*;

// C# member lookup on a type never finds that type's explicit interface
// implementations: they are reachable only through the interface. The
// ordinary-lookup lists therefore leave them out, while the per-overload
// tables and `names` keep every declaration.

fn property_type_names(d: &DefRecord) -> Vec<(&str, &str)> {
    d.property_types
        .iter()
        .map(|(n, f)| (n.as_str(), f.type_name.as_str()))
        .collect()
}

fn names_of(e: &Extraction) -> Vec<(&str, &str, usize)> {
    e.names
        .iter()
        .map(|n| (n.name.as_str(), n.kind.as_str(), n.line))
        .collect()
}

#[test]
fn an_explicit_property_declared_first_never_takes_the_public_propertys_slot() {
    let e = extract_src(
        "namespace Ns;\n\
         public class Bus : IBus\n\
         {\n\
         \tIBase IBus.Host => Host;\n\
         \tpublic IDerived Host { get; }\n\
         \tpublic void Run()\n\
         \t{\n\
         \t\tHost.Start();\n\
         \t}\n\
         }\n",
    );
    let d = find_def(&e, "Ns.Bus").expect("Bus def present");
    assert_eq!(
        d.properties,
        vec!["Host"],
        "the public property is listed once"
    );
    assert_eq!(
        property_type_names(d),
        vec![("Host", "IDerived")],
        "the public property's type owns the name"
    );
    assert_eq!(
        member_facts(&e),
        vec![("Start", Some("IDerived"))],
        "a bare qualifier reads the public property's type instead of a conflict"
    );
    assert_eq!(
        names_of(&e),
        vec![
            ("Host", "property", 4),
            ("Host", "property", 5),
            ("Run", "method", 6)
        ],
        "both declarations stay findable by name"
    );
}

#[test]
fn an_explicit_only_property_leaves_its_bare_name_free_to_name_a_type() {
    let e = extract_src(
        "namespace Ns;\n\
         public class Switch : ISwitch\n\
         {\n\
         \tILogContext ISwitch.LogContext => _context;\n\
         \tpublic void Trip()\n\
         \t{\n\
         \t\tLogContext.Info();\n\
         \t}\n\
         }\n",
    );
    let d = find_def(&e, "Ns.Switch").expect("Switch def present");
    assert!(d.properties.is_empty(), "no ordinary property is declared");
    assert!(d.property_types.is_empty(), "no property type is recorded");
    assert_eq!(
        member_facts(&e),
        vec![("Info", None)],
        "the bare name records no receiver fact"
    );
    assert_eq!(
        names_of(&e),
        vec![("LogContext", "property", 4), ("Trip", "method", 5)]
    );
}

#[test]
fn explicit_only_methods_are_not_non_public_methods_but_keep_their_overload_tables() {
    let e = extract_src(
        "namespace Ns;\n\
         public class Factory : IFactory\n\
         {\n\
         \tvoid IFactory.Produce(string topic) { }\n\
         \tvoid IFactory.Produce(string topic, Config config) { }\n\
         \tint IFactory.Count() => 0;\n\
         }\n",
    );
    let d = find_def(&e, "Ns.Factory").expect("Factory def present");
    assert!(d.methods.is_empty());
    assert!(d.non_public_methods.is_empty());
    assert!(d.method_returns.is_empty());
    assert_eq!(
        d.method_arities,
        vec![
            ("Produce".to_string(), vec![(1, 1), (2, 2)]),
            ("Count".to_string(), vec![(0, 0)])
        ],
        "every explicit overload keeps its arity"
    );
    assert_eq!(
        d.method_params,
        vec![
            (
                "Produce".to_string(),
                vec![
                    vec!["string".to_string()],
                    vec!["string".to_string(), "Config".to_string()]
                ]
            ),
            ("Count".to_string(), vec![vec![]])
        ],
        "every explicit overload keeps its parameter descriptors"
    );
    assert_eq!(
        names_of(&e),
        vec![
            ("Produce", "method", 4),
            ("Produce", "method", 5),
            ("Count", "method", 6)
        ]
    );
}

#[test]
fn an_interface_implementing_a_base_member_explicitly_does_not_list_it() {
    let e = extract_src(
        "namespace Ns;\n\
         public interface IDerived : IBase\n\
         {\n\
         \tWidget Make();\n\
         \tGadget IBase.Build() => null;\n\
         \tBox<Gadget> IBase.Pack() => null;\n\
         }\n",
    );
    let d = find_def(&e, "Ns.IDerived").expect("IDerived def present");
    assert_eq!(d.methods, vec!["Make"]);
    assert_eq!(
        d.method_returns,
        vec![("Make".to_string(), "Widget".to_string())]
    );
    assert!(d.method_return_args.is_empty());
    assert!(d.non_public_methods.is_empty());
    assert_eq!(
        d.method_arities
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["Make", "Build", "Pack"]
    );
}

#[test]
fn ordinary_members_beside_explicit_ones_are_recorded_as_before() {
    let e = extract_src(
        "namespace Ns;\n\
         public class Store : IStore\n\
         {\n\
         \tvoid IStore.Save() { }\n\
         \tpublic void Save() { }\n\
         \tprivate void Flush() { }\n\
         \tvoid IStore.Flush() { }\n\
         \tpublic Widget Current { get; }\n\
         \tWidget IStore.Current => Current;\n\
         \tprivate Gadget Spare { get; }\n\
         \tpublic void Run()\n\
         \t{\n\
         \t\tCurrent.Render();\n\
         \t\tSpare.Render();\n\
         \t}\n\
         }\n",
    );
    let d = find_def(&e, "Ns.Store").expect("Store def present");
    assert_eq!(d.methods, vec!["Save", "Run"]);
    assert_eq!(d.non_public_methods, vec!["Flush"]);
    assert_eq!(d.properties, vec!["Current", "Spare"]);
    assert_eq!(
        property_type_names(d),
        vec![("Current", "Widget"), ("Spare", "Gadget")]
    );
    assert_eq!(
        member_facts(&e),
        vec![("Render", Some("Widget")), ("Render", Some("Gadget"))]
    );
    assert_eq!(
        d.method_arities
            .iter()
            .map(|(n, o)| (n.as_str(), o.len()))
            .collect::<Vec<_>>(),
        vec![("Save", 2), ("Flush", 2), ("Run", 1)],
        "the overload table still counts the explicit declarations"
    );
}
