use super::*;

fn valid_export() -> &'static str {
    r#"{
        "schemaVersion": 1,
        "format": "flowtrace-edges",
        "tool": { "name": "flowtrace-cli", "version": "0.2.0" },
        "provenance": { "id": "abc123", "producer": "flowtrace-cli 0.2.0", "formatVersion": 1 },
        "edges": [
            {
                "kind": "calls",
                "from": { "repo": "ledger", "ref": "a", "file": "a.ts", "line": 1 },
                "to": { "repo": "storefront", "ref": "b", "file": "b.cs", "line": 2 }
            }
        ]
    }"#
}

#[test]
fn parse_imported_edges_accepts_a_well_formed_export() {
    let parsed = parse_imported_edges(valid_export().as_bytes(), "storefront").unwrap();
    assert_eq!(parsed.mapped_repo, "storefront");
    assert_eq!(parsed.provenance.id, "abc123");
    assert_eq!(parsed.edges.len(), 1);
    assert_eq!(parsed.edges[0].kind, "calls");
    assert_eq!(parsed.edges[0].from.repo.as_deref(), Some("ledger"));
    assert_eq!(parsed.edges[0].to.file.as_deref(), Some("b.cs"));
}

#[test]
fn parse_imported_edges_rejects_malformed_json() {
    let err = parse_imported_edges(b"not json", "storefront").unwrap_err();
    assert!(err.contains("malformed JSON"), "{err}");
}

#[test]
fn parse_imported_edges_rejects_a_format_mismatch_naming_the_offending_value() {
    let body = r#"{"schemaVersion":1,"format":"other","provenance":{"id":"x"},"edges":[]}"#;
    let err = parse_imported_edges(body.as_bytes(), "storefront").unwrap_err();
    assert!(err.contains("other"), "{err}");
}

#[test]
fn parse_imported_edges_rejects_a_schema_version_mismatch_naming_the_offending_value() {
    let body =
        r#"{"schemaVersion":7,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[]}"#;
    let err = parse_imported_edges(body.as_bytes(), "storefront").unwrap_err();
    assert!(err.contains('7'), "{err}");
}

#[test]
fn parse_imported_edges_rejects_a_record_missing_kind_from_or_to() {
    let missing_kind = r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"from":{},"to":{}}]}"#;
    assert!(parse_imported_edges(missing_kind.as_bytes(), "storefront")
        .unwrap_err()
        .contains("kind"));

    let missing_from = r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"calls","to":{}}]}"#;
    assert!(parse_imported_edges(missing_from.as_bytes(), "storefront")
        .unwrap_err()
        .contains("from"));

    let missing_to = r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"calls","from":{}}]}"#;
    assert!(parse_imported_edges(missing_to.as_bytes(), "storefront")
        .unwrap_err()
        .contains("to"));
}

#[test]
fn parse_imported_edges_rejects_a_kind_outside_the_closed_set() {
    let body = r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"deletes","from":{},"to":{}}]}"#;
    let err = parse_imported_edges(body.as_bytes(), "storefront").unwrap_err();
    assert!(err.contains("deletes"), "{err}");
}

#[test]
fn write_then_read_imported_edges_round_trips() {
    let dir = temp_dir("imported-edges-round-trip");
    let parsed = parse_imported_edges(valid_export().as_bytes(), "storefront").unwrap();
    write_imported_edges(&dir, &parsed).unwrap();
    let read_back = read_imported_edges(&dir).unwrap();
    assert_eq!(read_back, parsed);
}

#[test]
fn read_imported_edges_is_none_when_the_artifact_is_absent() {
    let dir = temp_dir("imported-edges-absent");
    assert!(read_imported_edges(&dir).is_none());
}
