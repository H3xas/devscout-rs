use super::*;

#[test]
fn tests_json_appends_via_project_last_and_omits_it_for_an_attribute_row() {
    let attribute_row = query::TestRow {
        file: "tests/OrderServiceTests.cs".to_string(),
        test_defs: vec!["App.Orders.Tests.OrderServiceTests".to_string()],
        lines: vec![10],
        ref_count: 1,
        heuristic: false,
        tier: None,
        via: query::TestVia::Attribute,
    };
    let project_row = query::TestRow {
        file: "tests/App.Tests/FakeServer.cs".to_string(),
        test_defs: vec![],
        lines: vec![12, 34],
        ref_count: 2,
        heuristic: false,
        tier: None,
        via: query::TestVia::Project,
    };
    let model = query::TestsModel {
        query: "Order".to_string(),
        symbol: "App.Orders.Order".to_string(),
        def_files: vec!["src/Order.cs".to_string()],
        rows: vec![attribute_row, project_row],
        test_file_count: 2,
        ref_count: 3,
        heuristic_file_count: 0,
        heuristic_ref_count: 0,
        bus: query::Table {
            total: 0,
            dropped: 0,
            rows: vec![],
        },
        name_hint: vec![],
    };
    let json = tests_model_to_json(&model);
    assert!(
        json.contains(
            r#"{"file":"tests/OrderServiceTests.cs","testDefs":["App.Orders.Tests.OrderServiceTests"],"lines":[10],"refCount":1,"why":"test-attribute"}"#
        ),
        "an attribute row carries no via key at all: {json}"
    );
    assert!(
        json.contains(
            r#"{"file":"tests/App.Tests/FakeServer.cs","testDefs":[],"lines":[12,34],"refCount":2,"via":"project","why":"test-project"}"#
        ),
        "a project row appends via LAST (no heuristic/tier on this row): {json}"
    );
}

#[test]
fn tests_json_leads_with_schema_version_ahead_of_status() {
    let model = query::TestsModel {
        query: "Order".to_string(),
        symbol: "App.Orders.Order".to_string(),
        def_files: vec!["src/Order.cs".to_string()],
        rows: vec![],
        test_file_count: 0,
        ref_count: 0,
        heuristic_file_count: 0,
        heuristic_ref_count: 0,
        bus: query::Table {
            total: 0,
            dropped: 0,
            rows: vec![],
        },
        name_hint: vec![],
    };
    let json = tests_model_to_json(&model);
    assert!(
        json.starts_with(r#"{"schema_version":1,"status":"resolved","query":"Order""#),
        "schema_version leads, ahead of the status key that used to be first: {json}"
    );
}

fn untested_model() -> query::TestsModel {
    query::TestsModel {
        query: "Order".to_string(),
        symbol: "App.Orders.Order".to_string(),
        def_files: vec!["src/Order.cs".to_string()],
        rows: vec![],
        test_file_count: 0,
        ref_count: 0,
        heuristic_file_count: 0,
        heuristic_ref_count: 0,
        bus: query::Table {
            total: 0,
            dropped: 0,
            rows: vec![],
        },
        name_hint: vec![],
    }
}

fn hint_row(file: &str, references: &[&str], reference_total: usize) -> query::NameHintRow {
    query::NameHintRow {
        file: file.to_string(),
        references: references.iter().map(|r| (*r).to_string()).collect(),
        reference_total,
    }
}

#[test]
fn tests_json_puts_name_hint_between_the_heuristic_pair_and_next() {
    let mut model = untested_model();
    model.name_hint = vec![hint_row("tests/OrderTests.cs", &["App.Orders.Other"], 1)];
    let next = vec![vec!["impact".to_string(), "src/Order.cs".to_string()]];
    let json = tests_model_to_json_with_next(&model, &next);

    assert!(
        json.contains(
            r#""heuristicRefCount":0,"nameHint":[{"file":"tests/OrderTests.cs","references":["App.Orders.Other"],"referenceTotal":1}],"next":[["impact","src/Order.cs"]],"outcome":"hit"}"#
        ),
        "{json}"
    );
    assert!(
        json.starts_with(r#"{"schema_version":1,"#),
        "the key is additive and bumps no schema version: {json}"
    );
}

#[test]
fn tests_json_lists_name_hint_rows_in_path_order() {
    let mut model = untested_model();
    model.name_hint = vec![
        hint_row(
            "tests/A/OrderTests.cs",
            &["App.Orders.A", "App.Orders.B"],
            3,
        ),
        hint_row("tests/B/OrderTests.cs", &[], 0),
    ];
    let json = tests_model_to_json_with_next(&model, &[]);

    let a = json.find("tests/A/OrderTests.cs").expect("first row");
    let b = json.find("tests/B/OrderTests.cs").expect("second row");
    assert!(a < b, "{json}");
    assert!(
        json.contains(r#""references":[],"referenceTotal":0"#),
        "{json}"
    );
}

#[test]
fn tests_json_leaves_the_name_hint_key_out_when_no_file_matches() {
    let model = untested_model();
    let json = tests_model_to_json_with_next(&model, &[]);

    assert!(!json.contains("nameHint"), "{json}");
    assert!(
        json.contains(r#""heuristicRefCount":0,"next":[],"outcome":"hit"}"#),
        "{json}"
    );
}
