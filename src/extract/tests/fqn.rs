use super::*;

// --- FQN building (type_id / typeStack "+"-joining) ---------------

#[test]
fn namespace_level_type_fqn_is_dotted() {
    let e = extract_src("namespace Fixtures.Widgets { public class Gadget {} }");
    assert!(find_def(&e, "Fixtures.Widgets.Gadget").is_some());
}

#[test]
fn top_level_type_fqn_has_no_namespace_prefix() {
    let e = extract_src("public class Gadget {}");
    let d = find_def(&e, "Gadget").expect("Gadget def present");
    assert_eq!(d.namespace, "");
}

#[test]
fn nested_type_fqn_uses_plus_not_dot() {
    let e =
        extract_src("namespace Fixtures.Widgets { public class Outer { public class Inner {} } }");
    assert!(find_def(&e, "Fixtures.Widgets.Outer+Inner").is_some());
    // The "+"-joined id must never collide with a literal dotted path.
    assert!(find_def(&e, "Fixtures.Widgets.Outer.Inner").is_none());
}

#[test]
fn doubly_nested_type_fqn_chains_plus_joins() {
    let e = extract_src(
        "namespace Fixtures.Widgets { public class A { public class B { public class C {} } } }",
    );
    assert!(find_def(&e, "Fixtures.Widgets.A+B+C").is_some());
}

#[test]
fn enum_member_id_appends_dot_even_under_nested_type() {
    let e = extract_src(
        "namespace Fixtures.Gadgets { public class Controller { public enum State { Off, On } } }",
    );
    assert!(find_def(&e, "Fixtures.Gadgets.Controller+State").is_some());
    assert!(find_def(&e, "Fixtures.Gadgets.Controller+State.Off").is_some());
    assert!(find_def(&e, "Fixtures.Gadgets.Controller+State.On").is_some());
}

#[test]
fn file_scoped_namespace_siblings_get_the_namespace() {
    let e = extract_src(
        "namespace Fixtures.Gadgets;\npublic class Registry {}\npublic enum State { Off, On }\n",
    );
    assert!(find_def(&e, "Fixtures.Gadgets.Registry").is_some());
    assert!(find_def(&e, "Fixtures.Gadgets.State").is_some());
    assert!(find_def(&e, "Fixtures.Gadgets.State.Off").is_some());
}

#[test]
fn nested_regular_namespace_dots_accumulate() {
    let e = extract_src("namespace A { namespace B { public class Widget {} } }");
    assert!(find_def(&e, "A.B.Widget").is_some());
}
