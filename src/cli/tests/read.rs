use super::*;

#[test]
fn read_json_leads_with_schema_version_and_keeps_span_right_after_kind() {
    let model = query::ReadModel {
        refs: json_refs_model(Vec::new(), false),
        span: Some(query::ReadSpan {
            file: "src/Widget.cs".to_string(),
            start_line: 3,
            end_line: 5,
            source: "class Widget {}".to_string(),
        }),
    };
    let json = read_model_to_json(&model);
    assert!(
        json.starts_with(
            r#"{"schema_version":1,"status":"resolved","query":"Widget","id":"App.Widget","kind":"class","span":{"#
        ),
        "schema_version leads, and span still sits right after kind, unmoved: {json}"
    );
}
