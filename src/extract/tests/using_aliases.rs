use super::*;

// --- alias table (using directive parsing) -------------------------

#[test]
fn using_alias_directive_captures_alias_and_target() {
    let e = extract_src("using Widgets = Fixtures.Widgets.Catalog;\n");
    assert_eq!(e.usings.len(), 1);
    match &e.usings[0] {
        UsingRecord::Alias {
            alias,
            target,
            global,
        } => {
            assert_eq!(alias, "Widgets");
            assert_eq!(target, "Fixtures.Widgets.Catalog");
            assert!(!global);
        }
        UsingRecord::Plain { .. } => panic!("expected alias form"),
    }
}

#[test]
fn plain_using_directive_has_no_alias() {
    let e = extract_src("using System.Collections.Generic;\n");
    match &e.usings[0] {
        UsingRecord::Plain { text, global } => {
            assert_eq!(text, "System.Collections.Generic");
            assert!(!global);
        }
        UsingRecord::Alias { .. } => panic!("expected plain form"),
    }
}

#[test]
fn global_using_directive_sets_global_flag() {
    let e = extract_src("global using System;\n");
    match &e.usings[0] {
        UsingRecord::Plain { text, global } => {
            assert_eq!(text, "System");
            assert!(*global);
        }
        UsingRecord::Alias { .. } => panic!("expected plain form"),
    }
}

#[test]
fn static_using_directive_is_plain_form_not_flagged() {
    // `using static` is not distinguished from a plain `using` -- both are
    // 1-named-child directives.
    let e = extract_src("using static System.Math;\n");
    match &e.usings[0] {
        UsingRecord::Plain { text, global } => {
            assert_eq!(text, "System.Math");
            assert!(!global);
        }
        UsingRecord::Alias { .. } => panic!("expected plain form"),
    }
}

#[test]
fn using_directive_also_pushes_an_imports_ref_with_null_namespace() {
    let e = extract_src("using System.Collections.Generic;\n");
    let import_ref = e
        .refs
        .iter()
        .find(|r| r.kind == "imports")
        .expect("imports ref present");
    assert_eq!(import_ref.name, "System.Collections.Generic");
    assert!(import_ref.namespace.is_none());
}
