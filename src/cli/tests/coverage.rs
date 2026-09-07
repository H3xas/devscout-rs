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
    };
    let json = tests_model_to_json(&model);
    assert!(
        json.starts_with(r#"{"schema_version":1,"status":"resolved","query":"Order""#),
        "schema_version leads, ahead of the status key that used to be first: {json}"
    );
}
