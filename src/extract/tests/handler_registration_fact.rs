use super::*;
use crate::bus_seed::{INSTALL_PREFIXES, ROLE_SUFFIXES};

// A registration call's own name is the only signal available before
// resolution, so the list-entry rule and the exact name boundary it draws are
// what decide whether a repository's handler vocabulary is found at all. A
// name the rule misses drops every hop that hangs off the bases the missed
// registration would have taught, and a name it admits too liberally costs a
// fragment byte until the resolver's own base check discards it. These cases
// therefore pin both sides at their current outcome: the spellings admitted
// today, and the ones that are not.

// One invoked member call on line 7 of a source that is otherwise fixed, so a
// fact's line is asserted against the layout rather than restated by hand.
fn source_with(call: &str) -> String {
    format!(
        "namespace App.Wiring;\n\nclass Startup\n{{\n  void Configure()\n  {{\n    {call}\n  }}\n}}\n"
    )
}

fn facts_of(call: &str) -> Vec<(String, String, usize)> {
    let e = extract_src(&source_with(call));
    e.handler_registrations
        .iter()
        .map(|r| (r.handler.clone(), r.namespace.clone(), r.line))
        .collect()
}

fn admitted_entry_facts(method: &str) -> Vec<(String, String, usize)> {
    facts_of(&format!("registry.{method}<DeskHandler>();"))
}

#[test]
fn handler_registration_admits_each_list_entry() {
    for prefix in INSTALL_PREFIXES {
        // A lead word ending in no role suffix, so the prefix is the only
        // reason the composed name can be admitted.
        let method = format!("{prefix}Loom");
        assert_eq!(
            admitted_entry_facts(&method),
            vec![("DeskHandler".to_string(), "App.Wiring".to_string(), 7)],
            "{method}"
        );
    }
    for suffix in ROLE_SUFFIXES {
        // A lead word starting with no prefix, so the suffix alone carries it.
        let method = format!("Kiln{suffix}");
        assert_eq!(
            admitted_entry_facts(&method),
            vec![("DeskHandler".to_string(), "App.Wiring".to_string(), 7)],
            "{method}"
        );
    }
}

#[test]
fn handler_registration_name_boundary() {
    // The rule is a bare starts_with/ends_with with no word boundary, so a
    // name that merely BEGINS with a prefix is admitted, and one whose first
    // letter after the prefix is lower case is not: case is significant.
    for method in ["Address", "User", "Mapping"] {
        assert_eq!(
            admitted_entry_facts(method).len(),
            1,
            "{method} only begins the prefix word"
        );
    }
    // Further letters after a role suffix end the match: the suffix has to be
    // the end of the name, not a word inside it.
    for method in [
        "addShelf",
        "Shelfworker",
        "ShelfWorkers",
        "ShelfHandlerFactory",
        "Configure",
        "With",
        "Endpoint",
    ] {
        assert!(
            admitted_entry_facts(method).is_empty(),
            "{method}: {:?}",
            admitted_entry_facts(method)
        );
    }
}

#[test]
fn handler_registration_argument_and_call_shapes() {
    // Every refused shape sits in one source beside the two shapes that DO
    // record, so a negative cannot pass because the source recorded nothing
    // at all. Only the invoked member call and the naming of a method's own
    // type parameter record, in source order.
    let lines = [
        "namespace App.Wiring;",
        "",
        "class Startup<TEnclosing>",
        "{",
        "  void Configure<TMethod>()",
        "  {",
        "    registry.AddLoom();",
        "    registry.AddLoom(typeof(DeskHandler));",
        "    registry.AddLoom<DeskHandler, DeskState>();",
        "    registry.AddLoom<TEnclosing>();",
        "    var group = registry.AddLoom<DeskHandler>;",
        "    AddLoom<DeskHandler>();",
        "    registry?.AddLoom<DeskHandler>();",
        "    registry.AddLoom<TMethod>();",
        "    registry.AddLoom<DeskHandler>();",
        "  }",
        "}",
    ];
    let e = extract_src(&format!("{}\n", lines.join("\n")));
    let facts: Vec<(String, usize)> = e
        .handler_registrations
        .iter()
        .map(|r| (r.handler.clone(), r.line))
        .collect();
    assert_eq!(
        facts,
        vec![("TMethod".to_string(), 14), ("DeskHandler".to_string(), 15),],
        "a type argument naming the enclosing method's own parameter is recorded under that \
         name because the extraction scope carries only the enclosing type's parameters; the \
         enclosing type's own parameter, a receiver-less call, a conditional-access call, a \
         non-invoked method group and a zero- or two-argument list all record nothing"
    );
    assert!(
        e.handler_registrations
            .iter()
            .all(|r| r.namespace == "App.Wiring"),
        "{:?}",
        e.handler_registrations
    );
}
