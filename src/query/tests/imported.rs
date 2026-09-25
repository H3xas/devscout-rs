use super::*;

fn end(repo: &str, file: Option<&str>, line: Option<u64>) -> graph::EdgeEnd {
    graph::EdgeEnd {
        repo: Some(repo.to_string()),
        reference: None,
        file: file.map(str::to_string),
        line,
    }
}

fn message_end(key_ref: &str) -> graph::EdgeEnd {
    graph::EdgeEnd {
        repo: Some("message".to_string()),
        reference: Some(key_ref.to_string()),
        file: None,
        line: None,
    }
}

fn record(
    kind: &str,
    from: graph::EdgeEnd,
    to: graph::EdgeEnd,
    key: Option<&str>,
) -> graph::ImportRecord {
    graph::ImportRecord {
        kind: kind.to_string(),
        from,
        to,
        key: key.map(str::to_string),
    }
}

fn edges(mapped_repo: &str, records: Vec<graph::ImportRecord>) -> graph::ImportedEdges {
    graph::ImportedEdges {
        schema_version: 1,
        mapped_repo: mapped_repo.to_string(),
        provenance: graph::Provenance {
            id: "prov1".to_string(),
            producer: "test".to_string(),
            format_version: 1,
        },
        edges: records,
    }
}

fn empty_model(seed_files: Vec<&str>, rows: Vec<(&str, u32)>) -> ImpactModel {
    ImpactModel {
        kind: SeedKind::File,
        seed_files: seed_files.iter().map(|s| s.to_string()).collect(),
        hops: 2,
        total_affected: rows.len(),
        rows: rows
            .into_iter()
            .map(|(file, hop)| ImpactRow {
                file: file.to_string(),
                hop,
                via_count: 1,
                ambiguous_count: 0,
                top_symbols: vec![],
                top_symbols_more: 0,
                score: 0.0,
                heuristic_count: 0,
                heuristic: false,
                tier: None,
                iface_via: vec![],
                from_lines: vec![],
                infra: false,
                why: Why::UsesType,
                bus_only: false,
                bus_origin: None,
            })
            .collect(),
        dropped: 0,
        manifest_gap: 0,
        heuristic_affected: 0,
        tests_affected: 0,
        braked: vec![],
        braked_files: vec![],
    }
}

#[test]
fn direct_case_reaches_the_foreign_from_file_when_to_resolves_into_the_mapped_repo() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let export = edges(
        "storefront",
        vec![record(
            "calls",
            end("ledger", Some("caller.ts"), Some(1)),
            end("storefront", Some("A.cs"), Some(5)),
            None,
        )],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.affected, 1);
    assert_eq!(section.rows.len(), 1);
    assert_eq!(section.rows[0].file, "caller.ts");
    assert_eq!(section.rows[0].repo, "ledger");
    assert_eq!(section.rows[0].hop, 1);
    assert_eq!(section.rows[0].imported_kind, "calls");
    assert_eq!(section.rows[0].why, Why::ImportedEdge);
}

#[test]
fn a_record_whose_from_end_is_also_the_mapped_repo_is_never_reported() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let export = edges(
        "storefront",
        vec![record(
            "calls",
            end("storefront", Some("Other.cs"), Some(1)),
            end("storefront", Some("A.cs"), Some(5)),
            None,
        )],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.affected, 0);
}

#[test]
fn composed_case_joins_publishes_to_consumes_through_the_message_key_in_one_hop() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let export = edges(
        "storefront",
        vec![
            record(
                "publishes",
                end("storefront", Some("A.cs"), Some(5)),
                message_end("order-placed"),
                Some("order-placed"),
            ),
            record(
                "consumes",
                message_end("order-placed"),
                end("ledger", Some("consumer.ts"), Some(1)),
                Some("order-placed"),
            ),
        ],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.rows.len(), 1, "{:?}", section.rows);
    assert_eq!(section.rows[0].file, "consumer.ts");
    assert_eq!(section.rows[0].repo, "ledger");
    assert_eq!(section.rows[0].hop, 1);
    assert_eq!(section.rows[0].imported_kind, "consumes");
}

#[test]
fn composed_case_never_reports_the_publishers_own_arriving_site_as_its_consumer() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let export = edges(
        "storefront",
        vec![
            record(
                "publishes",
                end("storefront", Some("A.cs"), Some(5)),
                message_end("order-placed"),
                Some("order-placed"),
            ),
            // Foreign, so not filtered by the mapped-repo check -- but the
            // exact site (file, line) that arrived as the publisher's own,
            // which the exclusion rule must still refuse to echo back.
            record(
                "consumes",
                message_end("order-placed"),
                end("ledger", Some("A.cs"), Some(5)),
                Some("order-placed"),
            ),
        ],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.affected, 0, "{:?}", section.rows);
}

#[test]
fn a_foreign_edge_landing_on_a_native_row_reports_one_hop_further_out() {
    let model = empty_model(vec!["A.cs"], vec![("B.cs", 2)]);
    let export = edges(
        "storefront",
        vec![record(
            "calls",
            end("ledger", Some("caller.ts"), Some(1)),
            end("storefront", Some("B.cs"), Some(9)),
            None,
        )],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.rows[0].hop, 3);
}

#[test]
fn duplicate_records_landing_on_the_same_foreign_file_collapse_to_one_row() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let export = edges(
        "storefront",
        vec![
            record(
                "calls",
                end("ledger", Some("caller.ts"), Some(1)),
                end("storefront", Some("A.cs"), Some(5)),
                None,
            ),
            record(
                "calls",
                end("ledger", Some("caller.ts"), Some(2)),
                end("storefront", Some("A.cs"), Some(6)),
                None,
            ),
        ],
    );
    let section = build_imported_section(&model, &export, 50);
    assert_eq!(section.affected, 1);
    assert_eq!(section.rows.len(), 1);
}

#[test]
fn cap_bounds_rows_and_reports_the_rest_as_dropped_independent_of_native_rows() {
    let model = empty_model(vec!["A.cs"], vec![]);
    let records = (0..5)
        .map(|i| {
            record(
                "calls",
                end("ledger", Some(&format!("caller{i}.ts")), Some(1)),
                end("storefront", Some("A.cs"), Some(5)),
                None,
            )
        })
        .collect();
    let section = build_imported_section(&model, &edges("storefront", records), 2);
    assert_eq!(section.affected, 5);
    assert_eq!(section.rows.len(), 2);
    assert_eq!(section.dropped, 3);
}
