use super::*;

// --- v8: the ref `outer_types` enclosing-type stack -------------------

fn find_ref<'a>(e: &'a Extraction, kind: &str, name: &str) -> Option<&'a RefRecord> {
    e.refs.iter().find(|r| r.kind == kind && r.name == name)
}

#[test]
fn v8_a_ref_inside_a_nested_type_records_outer_types_outermost_first() {
    let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    private Marker _m;\n  }\n}\n\npublic class Marker { }\n",
        );
    let r = find_ref(&e, "uses-type", "Marker").expect("the field type ref is recorded");
    // Outermost first is the order type_id joins with "+", so the resolver
    // rebuilds a nested id by prefix rather than by reversing.
    assert_eq!(
        r.outer_types,
        vec!["Outer".to_string(), "Inner".to_string()]
    );
}

#[test]
fn v8_a_namespace_level_ref_and_an_imports_ref_carry_no_outer_types() {
    let e = extract_src(
        "using App.Other;\n\nnamespace App.Core;\n\npublic class Widget : Marker { }\n",
    );
    assert!(find_ref(&e, "imports", "App.Other")
        .expect("using ref")
        .outer_types
        .is_empty());
    assert!(find_ref(&e, "inherits", "Marker")
        .expect("base ref")
        .outer_types
        .is_empty());
}

#[test]
fn v8_a_member_ref_carries_outer_types_alongside_every_other_receiver_fact() {
    let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    private Store<int> _s;\n\n    public void Go()\n    {\n      _s.Add(1);\n      Box<int>.Make();\n    }\n  }\n}\n",
        );
    let with_receiver = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Add"))
        .expect("receiver-fact ref");
    assert_eq!(with_receiver.receiver_type.as_deref(), Some("Store"));
    assert_eq!(with_receiver.receiver_args, Some(vec!["int".to_string()]));
    assert_eq!(
        with_receiver.outer_types,
        vec!["Outer".to_string(), "Inner".to_string()]
    );
    // A generic qualifier earns no receiver fact; the stack lands on it all
    // the same.
    let generic = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Make"))
        .expect("generic-qualifier ref");
    assert!(generic.generic);
    assert_eq!(
        generic.outer_types,
        vec!["Outer".to_string(), "Inner".to_string()]
    );
}

#[test]
fn v8_a_base_list_ref_carries_the_declaring_types_outer_stack_with_self_excluded() {
    let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner : Marker\n  {\n  }\n}\n",
        );
    // The same stack record_type_def used to build Inner's own id.
    assert_eq!(
        find_ref(&e, "inherits", "Marker")
            .expect("base ref")
            .outer_types,
        vec!["Outer".to_string()]
    );
}

#[test]
fn v8_a_dotted_chain_tail_ref_carries_the_sites_own_outer_types() {
    let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    public void Go() { Alpha.Beta.Gamma(); }\n  }\n}\n",
        );
    let tail = e
        .refs
        .iter()
        .find(|r| r.qualified.as_deref() == Some("Alpha.Beta"))
        .expect("chain-tail ref");
    // Positional: the stack of the SITE, never inherited from the head.
    assert_eq!(
        tail.outer_types,
        vec!["Outer".to_string(), "Inner".to_string()]
    );
}
