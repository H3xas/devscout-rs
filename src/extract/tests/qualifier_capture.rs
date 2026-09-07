use super::*;

// --- member-access qualifier capture (identifier vs. deep chain) ----

#[test]
fn simple_identifier_qualifier_is_captured_without_dot() {
    let e = extract_src(
            "namespace Fixtures.Orders { public enum Priority { Low, High } public class Probe { public bool F() { var x = Priority.High; return true; } } }",
        );
    let m = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("High"))
        .expect("member ref present");
    assert_eq!(m.name, "Priority");
    assert!(m.qualified.is_none());
}

#[test]
fn deep_member_chain_captures_every_window_qualifier_flattened_at_each_level() {
    // `Fixtures.Orders.Priority.High` parses as nested
    // member_access_expression, not qualified_name, since this is an
    // expression position -- but member_qualifier_text now flattens a
    // member_access_expression chain into its full dotted text, so the
    // OUTER window ("Fixtures.Orders.Priority" -> "High") is captured
    // too, not just the innermost identifier.identifier pair. walk()
    // still recurses into every level regardless, so the middle and
    // innermost windows are ALSO captured, each as their own separate
    // candidate -- resolve.rs's ladder is what decides, per candidate,
    // whether any of them actually resolves to an enum.
    let e = extract_src(
            "namespace Fixtures.Orders { public class Probe { public void F() { var x = Fixtures.Orders.Priority.High; } } }",
        );
    let members: Vec<(&str, &str)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| (r.name.as_str(), r.member.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        members,
        vec![
            ("Priority", "High"),
            ("Orders", "Priority"),
            ("Fixtures", "Orders")
        ]
    );
    let outer = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("High"))
        .expect("outer window present");
    assert_eq!(outer.qualified.as_deref(), Some("Fixtures.Orders.Priority"));
}

#[test]
fn nested_enum_dotted_qualifier_is_captured_as_a_two_part_qualified_text() {
    // "Outer.Inner.On" -- the nested-enum-via-dotted-notation shape (not
    // the "+"-joined def id, which only exists on the def/id side, never
    // in source text). Extraction only needs to capture the RAW
    // qualifier text here; whether it resolves is resolve.rs's ladder.
    let e = extract_src(
            "namespace Fixtures.Widgets { public class Outer { public enum Inner { Off, On } } public class Probe { public void F() { var x = Outer.Inner.On; } } }",
        );
    let m = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("On"))
        .expect("outer window present");
    assert_eq!(m.name, "Inner");
    assert_eq!(m.qualified.as_deref(), Some("Outer.Inner"));
}

// --- declaration_expression (out-declarations) -> uses-type ref -------

#[test]
fn inline_out_declaration_emits_a_uses_type_ref_for_its_type() {
    // `Method(out SomeEnum x)` -- a declaration_expression{type, name}
    // pair in expression position, the same shape as an ordinary
    // `parameter`.
    let e = extract_src(
            "namespace Fixtures.Orders { public class Probe { public void F() { TryGet(out SomeEnum x); } } }",
        );
    let uses_type: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        uses_type.contains(&"SomeEnum"),
        "expected a uses-type ref for the out-declared type, got {uses_type:?}"
    );
}

#[test]
fn inline_out_declaration_with_var_type_yields_no_candidate() {
    // `out var x` -- implicit_type, same as an ordinary `var` parameter:
    // never a user-defined type reference, per outer_type_name.
    let e = extract_src("namespace Fixtures.Orders { public class Probe { public void F() { TryGet(out var x); } } }");
    assert!(e.refs.iter().all(|r| r.kind != "uses-type"));
}
