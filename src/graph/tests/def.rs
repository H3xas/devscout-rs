use super::*;

// --- Def: also_in omission ------------------------------------------

#[test]
fn def_without_also_in_omits_the_key() {
    let d = Def {
        id: "Ns.Type".into(),
        name: "Type".into(),
        namespace: "Ns".into(),
        kind: "class".into(),
        file: "Ns/Type.cs".into(),
        line: 3,
        methods: vec![],
        test_methods: vec![],
        also_in: vec![],
        end_line: 0,
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(
        !json.contains("also_in"),
        "empty also_in must be omitted: {json}"
    );
}

#[test]
fn def_end_line_serializes_last_and_defaults_when_absent() {
    let mut d = Def {
        id: "Ns.Type".into(),
        name: "Type".into(),
        namespace: "Ns".into(),
        kind: "class".into(),
        file: "Ns/Type.cs".into(),
        line: 3,
        methods: vec![],
        test_methods: vec![],
        also_in: vec![],
        end_line: 0,
    };
    assert!(!serde_json::to_string(&d).unwrap().contains("endLine"));
    d.end_line = 9;
    assert!(serde_json::to_string(&d)
        .unwrap()
        .ends_with(",\"endLine\":9}"));
}

#[test]
fn def_with_also_in_includes_it_after_methods() {
    let d = Def {
        id: "Ns.Type".into(),
        name: "Type".into(),
        namespace: "Ns".into(),
        kind: "class".into(),
        file: "Ns/Type.cs".into(),
        line: 3,
        methods: vec!["M".into()],
        test_methods: vec![],
        also_in: vec![AlsoIn {
            file: "Ns/Type.Extra.cs".into(),
            line: 5,
        }],
        end_line: 0,
    };
    let json = serde_json::to_string(&d).unwrap();
    let methods_pos = json.find("\"methods\"").unwrap();
    let also_in_pos = json.find("\"also_in\"").unwrap();
    assert!(
        methods_pos < also_in_pos,
        "also_in must come after methods: {json}"
    );
}

// --- test-coverage stage: the def ROW keeps testMethods, between
// methods and also_in ---------------------------------------------------

#[test]
fn def_row_places_test_methods_between_methods_and_also_in() {
    let d = Def {
        id: "Ns.TypeTests".into(),
        name: "TypeTests".into(),
        namespace: "Ns".into(),
        kind: "class".into(),
        file: "Ns/TypeTests.cs".into(),
        line: 3,
        methods: vec!["M".into()],
        test_methods: vec!["Fact1".into()],
        also_in: vec![AlsoIn {
            file: "Ns/TypeTests.Extra.cs".into(),
            line: 5,
        }],
        end_line: 0,
    };
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(
        json,
        r#"{"id":"Ns.TypeTests","name":"TypeTests","namespace":"Ns","kind":"class","file":"Ns/TypeTests.cs","line":3,"methods":["M"],"testMethods":["Fact1"],"also_in":[{"file":"Ns/TypeTests.Extra.cs","line":5}]}"#
    );
}

#[test]
fn def_row_omits_test_methods_when_the_def_declares_no_tests() {
    let d = Def {
        id: "Ns.Type".into(),
        name: "Type".into(),
        namespace: "Ns".into(),
        kind: "class".into(),
        file: "Ns/Type.cs".into(),
        line: 3,
        methods: vec!["M".into()],
        test_methods: vec![],
        also_in: vec![],
        end_line: 0,
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(
        !json.contains("testMethods"),
        "empty testMethods must be omitted: {json}"
    );
}
