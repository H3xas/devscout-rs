use super::*;

#[test]
fn module_resolution_ac_3_manifest_preserves_condition_order() {
    let package = parse_package(
        "lib",
        r#"{
        "name":"lib", "type":"module",
        "exports":{".":{"default":"./fallback.js","import":"./esm.js"}}
    }"#,
    )
    .unwrap();
    let root = package.exports.unwrap();
    let conditions = root
        .as_object()
        .unwrap()
        .get(".")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(
        conditions
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        vec!["default", "import"]
    );
}

#[test]
fn module_resolution_ac_6_bad_manifest_is_refused_whole() {
    for (document, field) in [
        (r#"{"name":2,"type":"module"}"#, "name"),
        (r#"{"name":"lib","type":"esm"}"#, "type"),
        (
            r#"{"name":"lib","exports":{".":"./x.js","default":"./y.js"}}"#,
            "exports",
        ),
        (
            r#"{"name":"lib","workspaces":{"packages":"apps/*"}}"#,
            "workspaces",
        ),
        ("{", "document"),
    ] {
        assert_eq!(parse_package("lib", document).unwrap_err(), field);
    }
}
