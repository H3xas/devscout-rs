use super::*;

// --- generic arity (type_argument_list -> one uses-type ref per arg) --

#[test]
fn type_def_end_line_is_the_closing_brace_line_of_the_whole_declaration() {
    let e = extract_src("namespace N;\npublic class Widget\n{\n    int n;\n}\n");
    let d = find_def(&e, "N.Widget").unwrap();
    assert_eq!((d.line, d.end_line), (2, 5));
}

#[test]
fn enum_member_end_line_is_its_own_single_line() {
    let e = extract_src("public enum State\n{\n    Off,\n    On,\n}\n");
    assert_eq!(
        (
            find_def(&e, "State.Off").unwrap().line,
            find_def(&e, "State.Off").unwrap().end_line
        ),
        (3, 3)
    );
    assert_eq!(
        (
            find_def(&e, "State.On").unwrap().line,
            find_def(&e, "State.On").unwrap().end_line
        ),
        (4, 4)
    );
}

#[test]
fn nested_type_span_stays_within_its_own_node() {
    let e = extract_src("public class Outer\n{\n    public class Inner { }\n}\n");
    assert_eq!(
        (
            find_def(&e, "Outer").unwrap().line,
            find_def(&e, "Outer").unwrap().end_line
        ),
        (1, 4)
    );
    assert_eq!(
        (
            find_def(&e, "Outer+Inner").unwrap().line,
            find_def(&e, "Outer+Inner").unwrap().end_line
        ),
        (3, 3)
    );
}

#[test]
fn generic_name_records_base_and_each_type_argument() {
    let e = extract_src(
            "using System.Collections.Generic;\nnamespace Fixtures.Generics { public class Store { public Dictionary<Key, Value> Items { get; set; } } public class Key {} public class Value {} }",
        );
    let uses_type_names: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .map(|r| r.name.as_str())
        .collect();
    // Dictionary itself (generic_name's base identifier) plus both type
    // arguments (arity 2) -- three distinct uses-type refs total from
    // one property type.
    assert!(uses_type_names.contains(&"Dictionary"));
    assert!(uses_type_names.contains(&"Key"));
    assert!(uses_type_names.contains(&"Value"));
}

#[test]
fn nested_generic_type_arguments_are_all_recorded() {
    let e = extract_src(
            "using System.Collections.Generic;\nnamespace Fixtures.Generics { public class Store { public List<Dictionary<string, Gadget>> Items { get; set; } } public class Gadget {} }",
        );
    let uses_type_names: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .map(|r| r.name.as_str())
        .collect();
    assert!(uses_type_names.contains(&"List"));
    assert!(uses_type_names.contains(&"Dictionary"));
    assert!(uses_type_names.contains(&"Gadget"));
    // "string" is a predefined_type -- never a candidate.
    assert!(!uses_type_names.contains(&"string"));
}
