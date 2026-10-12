use super::*;

// A plain using's `namespace` is the level whose type lookup it joins, so it
// must equal the `namespace` the same block's refs record.

fn plain_usings(e: &Extraction) -> Vec<(&str, &str, bool)> {
    e.usings
        .iter()
        .filter_map(|u| match u {
            UsingRecord::Plain {
                text,
                global,
                namespace,
            } => Some((text.as_str(), namespace.as_str(), *global)),
            UsingRecord::Alias { .. } => None,
        })
        .collect()
}

fn type_ref_namespace<'a>(e: &'a Extraction, name: &str) -> Option<&'a str> {
    e.refs
        .iter()
        .find(|r| r.kind == "uses-type" && r.name == name)
        .and_then(|r| r.namespace.as_deref())
}

#[test]
fn using_inside_a_dotted_namespace_block_records_that_namespace() {
    let e = extract_src("namespace A.B { using X; class C { Widget w; } }");
    assert_eq!(plain_usings(&e), vec![("X", "A.B", false)]);
    assert_eq!(type_ref_namespace(&e, "Widget"), Some("A.B"));
}

#[test]
fn using_inside_nested_namespace_blocks_records_the_joined_namespace() {
    let e = extract_src("namespace A { namespace B { using X; class C { Widget w; } } }");
    assert_eq!(plain_usings(&e), vec![("X", "A.B", false)]);
    assert_eq!(type_ref_namespace(&e, "Widget"), Some("A.B"));
}

#[test]
fn using_after_a_file_scoped_namespace_records_that_namespace() {
    let e = extract_src("namespace A.B;\nusing X;\nclass C { Widget w; }\n");
    assert_eq!(plain_usings(&e), vec![("X", "A.B", false)]);
    assert_eq!(type_ref_namespace(&e, "Widget"), Some("A.B"));
}

#[test]
fn using_above_a_file_scoped_namespace_is_at_compilation_unit_level() {
    let e = extract_src("using X;\nnamespace A.B;\nclass C { Widget w; }\n");
    assert_eq!(plain_usings(&e), vec![("X", "", false)]);
    assert_eq!(type_ref_namespace(&e, "Widget"), Some("A.B"));
}

#[test]
fn compilation_unit_using_records_the_empty_namespace() {
    let e = extract_src("using X;\nnamespace A.B { class C { } }\n");
    assert_eq!(plain_usings(&e), vec![("X", "", false)]);
}

#[test]
fn global_using_records_the_empty_namespace() {
    let e = extract_src("global using X;\nnamespace A.B;\nclass C { }\n");
    assert_eq!(plain_usings(&e), vec![("X", "", true)]);
}

#[test]
fn sibling_namespace_blocks_keep_their_own_usings() {
    let e = extract_src(
        "namespace A { using X; class C { Widget w; } }\n\
         namespace B { using Y; class D { Gadget g; } }\n",
    );
    assert_eq!(plain_usings(&e), vec![("X", "A", false), ("Y", "B", false)]);
    assert_eq!(type_ref_namespace(&e, "Widget"), Some("A"));
    assert_eq!(type_ref_namespace(&e, "Gadget"), Some("B"));
}

#[test]
fn alias_directive_inside_a_namespace_is_unchanged() {
    let e = extract_src("namespace A { using W = X.Widget; }");
    assert!(plain_usings(&e).is_empty());
    assert!(matches!(
        &e.usings[0],
        UsingRecord::Alias { alias, target, global: false } if alias == "W" && target == "X.Widget"
    ));
}

#[test]
fn extract_dump_prints_the_namespace_only_when_set() {
    let json = extraction_to_json(&extract_src(
        "using Outer;\nnamespace A.B { using Inner; }\n",
    ));
    let dump: serde_json::Value = serde_json::from_str(&json).expect("dump is JSON");
    assert_eq!(
        dump["usings"],
        serde_json::json!([
            {"text": "Outer", "global": false},
            {"text": "Inner", "global": false, "namespace": "A.B"},
        ])
    );
}
