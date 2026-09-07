use super::*;

// --- The per-file `names` list -------------------------------

fn names_of(e: &Extraction) -> Vec<(&str, &str, usize, &str)> {
    e.names
        .iter()
        .map(|n| (n.name.as_str(), n.kind.as_str(), n.line, n.owner.as_str()))
        .collect()
}

#[test]
fn names_record_every_member_kind_with_no_accessibility_filter() {
    let e = extract_src(
            "namespace Ns;\npublic class Ledger\n{\n\tprivate int _a, _b;\n\tpublic string Label { get; set; }\n\tpublic event EventHandler Retired;\n\tpublic event EventHandler Sealed { add { } remove { } }\n\tprivate void Hidden() { }\n}\n",
        );
    assert_eq!(
        names_of(&e),
        vec![
            ("_a", "field", 4, "Ns.Ledger"),
            ("_b", "field", 4, "Ns.Ledger"),
            ("Label", "property", 5, "Ns.Ledger"),
            ("Retired", "event", 6, "Ns.Ledger"),
            ("Sealed", "event", 7, "Ns.Ledger"),
            ("Hidden", "method", 8, "Ns.Ledger"),
        ]
    );
}

#[test]
fn a_names_line_is_the_name_token_row_not_the_attribute_the_declaration_starts_at() {
    let e = extract_src(
        "namespace Ns;\npublic class Ledger\n{\n\t[Obsolete]\n\tpublic int Total { get; }\n}\n",
    );
    assert_eq!(
        names_of(&e),
        vec![("Total", "property", 5, "Ns.Ledger")],
        "line 5 is the name, line 4 is the attribute"
    );
}

#[test]
fn overloads_are_two_names_and_a_nested_type_owns_its_own_members() {
    let e = extract_src(
            "namespace Ns;\npublic class Ledger\n{\n\tpublic void Add() { }\n\tpublic void Add(int n) { }\n\tpublic class Tally\n\t{\n\t\tpublic void Add(long n) { }\n\t}\n}\n",
        );
    assert_eq!(
            names_of(&e),
            vec![
                ("Add", "method", 4, "Ns.Ledger"),
                ("Add", "method", 5, "Ns.Ledger"),
                ("Add", "method", 8, "Ns.Ledger+Tally"),
            ],
            "two overloads are two declarations at two lines; the nested type's member is owned by the nested id"
        );
}

#[test]
fn an_interface_body_and_an_enum_body_contribute_what_they_declare_and_nothing_else() {
    let e = extract_src("namespace Ns;\npublic interface ISink\n{\n\tvoid Accept();\n}\npublic enum Mode\n{\n\tOn,\n}\n");
    assert_eq!(
        names_of(&e),
        vec![("Accept", "method", 4, "Ns.ISink")],
        "an enum's members are defs already, never names rows"
    );
}
