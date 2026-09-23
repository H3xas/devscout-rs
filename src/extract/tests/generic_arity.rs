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

// --- member-qualifier arity (uses-member's own type_arg_count) --------

#[test]
fn a_member_qualifiers_own_leaf_arity_is_recorded_on_the_wire() {
    let e = extract_src(
        r#"
namespace App.Arity;

public class A { }
public class B { }
public class Bar<X, Y> { }
public class Widget { }

public class Host
{
    private Widget local;
    private Widget a;

    public void Run()
    {
        Foo<A>.M1();
        Foo<A, B>.M2();
        Foo<Bar<A, B>>.M3();
        Ns.Foo<A>.M4();
        Foo.M5();
        this.M6();
        base.M7();
        var chained = a.B2().C();
        local.M8();
    }
}
"#,
    );
    let arity_of = |member: &str| -> Option<Option<usize>> {
        e.refs
            .iter()
            .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some(member))
            .map(|r| r.type_arg_count)
    };
    assert_eq!(arity_of("M1"), Some(Some(1)), "Foo<A>.M1()");
    assert_eq!(arity_of("M2"), Some(Some(2)), "Foo<A, B>.M2()");
    assert_eq!(
        arity_of("M3"),
        Some(Some(1)),
        "Foo<Bar<A, B>>.M3(): a nested generic argument's own comma must not count"
    );
    assert_eq!(arity_of("M4"), Some(Some(1)), "Ns.Foo<A>.M4()");
    assert_eq!(arity_of("M5"), Some(Some(0)), "bare Foo.M5()");
    assert_eq!(
        arity_of("M6"),
        Some(None),
        "this.M6() keeps an arity-blind lookup"
    );
    assert_eq!(
        arity_of("M7"),
        Some(None),
        "base.M7() keeps an arity-blind lookup"
    );
    assert_eq!(
        arity_of("C"),
        Some(None),
        "a chain-tail window's qualifier is an invocation's own source, never a type name"
    );
    assert_eq!(
        arity_of("M8"),
        Some(None),
        "a bare qualifier the extractor already holds a field fact for carries no type arity"
    );
}

#[test]
fn uses_type_arity_stays_untouched_by_the_member_qualifier_rule() {
    let e = extract_src(
        "namespace App.Arity; public class A {} public class Bar<X, Y> {} public class Host { private Bar<A, A> Field; }",
    );
    let bar = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-type" && r.name == "Bar")
        .expect("Bar<A, A> field type records a uses-type ref");
    assert_eq!(bar.type_arg_count, Some(2));
}
