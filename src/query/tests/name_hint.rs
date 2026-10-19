use super::*;

use super::super::name_hint::{name_hint_rows, stem_matches, NAME_HINT_REFERENCE_CAP};
use crate::graph::Edge;

fn index_over<'g>(root: &Path, g: &'g graph::Graph) -> GraphIndex<'g> {
    load_graph_index(g, root)
}

// A `uses-type` edge as the extractor records a parameter or a field type.
fn refers(file: &str, line: usize, to: &str) -> Edge {
    Edge::UsesType {
        from_file: file.to_string(),
        from_line: line,
        to: to.to_string(),
        to_file: "Src/Target.cs".to_string(),
        heuristic: false,
    }
}

fn test_type(id: &str, file: &str) -> graph::Def {
    let mut d = def(id, last_segment(id), "Tests", "class", file, 1);
    d.test_methods = vec!["Runs".to_string()];
    d
}

fn last_segment(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

#[test]
fn stem_match_requires_one_affix_and_ignores_case_and_separators() {
    for file in [
        "Tests/FooTests.cs",
        "Tests/FooTest.cs",
        "Tests/footests.cs",
        "Tests/foo.spec.ts",
        "Tests/foo.test.ts",
    ] {
        assert!(stem_matches("Foo", file), "{file} names Foo");
    }
    for file in [
        "Tests/foo-bar.spec.ts",
        "Tests/foo_bar_test.rs",
        "Tests/FooBarTests.cs",
    ] {
        assert!(stem_matches("FooBar", file), "{file} names FooBar");
    }
}

#[test]
fn stem_match_refuses_a_file_that_only_contains_the_name() {
    for file in [
        "Tests/Foo.cs",
        "Tests/FooBarTests.cs",
        "Tests/BarFooTests.cs",
        "Tests/FooTestsHelper.cs",
        "Tests/Fooo.cs",
    ] {
        assert!(!stem_matches("Foo", file), "{file} does not name Foo");
    }
    // A name carrying its own separators is normalized the same way, so the
    // rule is symmetric rather than a file-name-only concession.
    assert!(stem_matches("Foo-Bar", "Tests/FooBarTest.cs"));
}

#[test]
fn stem_match_reads_the_last_segment_so_a_matching_directory_is_not_enough() {
    assert!(!stem_matches("Foo", "Foo/Helpers/Harness.cs"));
}

#[test]
fn stem_match_lets_only_a_name_that_normalizes_to_nothing_match_a_bare_affix() {
    // The rule's boundary. A name of separators alone normalizes to the empty
    // string, which is the one input shape a bare `Tests.cs`/`Spec.cs` stem
    // matches; no language admits a declared symbol of that name, so the
    // boundary is unreachable from a resolved query and every name an
    // extractor records leaves characters behind.
    assert!(stem_matches("-", "Tests/Tests.cs"));
    assert!(stem_matches("_", "Tests/Spec.cs"));
    assert!(!stem_matches("Foo", "Tests/Tests.cs"));
}

#[test]
fn stem_match_lowercases_the_whole_name_so_the_rule_is_not_ascii_only() {
    assert!(stem_matches("Ünicode", "Tests/ÜnicodeTests.cs"));
    assert!(stem_matches("Ünicode", "Tests/ünicodetests.cs"));
    assert!(!stem_matches("Ünicode", "Tests/UnicodeTests.cs"));
}

#[test]
fn the_rule_reads_the_def_simple_name_and_not_the_query_the_caller_typed() {
    let root = temp_repo_root("name-hint-qualified");
    let g = make_graph(
        vec![
            def("Outer.Foo", "Foo", "Outer", "class", "Src/Foo.cs", 1),
            test_type("Tests.FooTests", "Tests/FooTests.cs"),
        ],
        Vec::new(),
    );
    let index = index_over(&root, &g);

    // `tests Outer.Foo` resolves to the def whose simple name is `Foo`, so the
    // match is against `Foo` -- the query text itself matches no file stem.
    let TestsResult::Resolved(model) = build_tests_model(&index, "Outer.Foo") else {
        panic!("Outer.Foo resolves");
    };
    assert_eq!(model.name_hint.len(), 1, "{model:?}");
    assert_eq!(model.name_hint[0].file, "Tests/FooTests.cs");
}

#[test]
fn a_matching_test_file_names_the_production_symbols_it_references() {
    let root = temp_repo_root("name-hint-refs");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            def("Prod.Bar", "Bar", "Prod", "class", "Src/Bar.cs", 1),
            test_type("Tests.FooTests", "Tests/FooTests.cs"),
        ],
        vec![refers("Tests/FooTests.cs", 5, "Prod.Bar")],
    );
    let index = index_over(&root, &g);

    let rows = name_hint_rows(&index, "Prod.Foo");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].file, "Tests/FooTests.cs");
    assert_eq!(rows[0].references, vec!["Prod.Bar".to_string()]);
    assert_eq!(rows[0].reference_total, 1);
}

#[test]
fn a_symbol_its_own_test_file_cannot_hint_is_the_declaring_file() {
    let root = temp_repo_root("name-hint-own");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Tests/FooTests.cs", 1),
            def(
                "Tests.FooTests",
                "FooTests",
                "Tests",
                "class",
                "Tests/FooTests.cs",
                1,
            ),
        ],
        Vec::new(),
    );
    let index = index_over(&root, &g);

    assert!(
        name_hint_rows(&index, "Prod.Foo").is_empty(),
        "a symbol declared in the matching file is not hinted at itself"
    );
}

#[test]
fn a_matching_file_no_vouch_reaches_is_not_a_candidate() {
    let root = temp_repo_root("name-hint-unvouched");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            def("Other.Bar", "Bar", "Other", "class", "Other/Bar.cs", 1),
        ],
        vec![refers("Tests/FooTests.cs", 5, "Other.Bar")],
    );
    let index = index_over(&root, &g);

    assert!(name_hint_rows(&index, "Prod.Foo").is_empty());
}

#[test]
fn references_rank_by_count_then_id_and_report_the_total_behind_the_cap() {
    let root = temp_repo_root("name-hint-cap");
    let mut defs = vec![def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1)];
    let mut edges: Vec<Edge> = Vec::new();
    // Seven production targets at descending counts, with one tie broken by
    // id, plus a helper inside the test file and a heuristic edge.
    for (i, name) in (0..7).rev().enumerate() {
        let id = format!("Prod.T{name}");
        defs.push(def(
            &id,
            &format!("T{name}"),
            "Prod",
            "class",
            "Src/T.cs",
            1,
        ));
        for _ in 0..=i {
            edges.push(refers("Tests/FooTests.cs", 5, &id));
        }
    }
    let tie_a = "Prod.A".to_string();
    let tie_b = "Prod.B".to_string();
    defs.push(def(&tie_a, "A", "Prod", "class", "Src/A.cs", 1));
    defs.push(def(&tie_b, "B", "Prod", "class", "Src/B.cs", 1));
    edges.push(refers("Tests/FooTests.cs", 5, &tie_a));
    edges.push(refers("Tests/FooTests.cs", 5, &tie_b));
    defs.push(test_type("Tests.FooTests", "Tests/FooTests.cs"));
    defs.push(test_type("Tests.Helper", "Tests/Helper.cs"));
    edges.push(refers("Tests/FooTests.cs", 6, "Tests.Helper"));
    edges.push(Edge::UsesType {
        from_file: "Tests/FooTests.cs".to_string(),
        from_line: 7,
        to: "Prod.Guessed".to_string(),
        to_file: "Src/Guessed.cs".to_string(),
        heuristic: true,
    });
    defs.push(def(
        "Prod.Guessed",
        "Guessed",
        "Prod",
        "class",
        "Src/Guessed.cs",
        1,
    ));

    let g = make_graph(defs, edges);
    let index = index_over(&root, &g);
    let rows = name_hint_rows(&index, "Prod.Foo");

    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row.references.len(), NAME_HINT_REFERENCE_CAP, "{row:?}");
    assert_eq!(
        row.references,
        vec![
            "Prod.T0".to_string(),
            "Prod.T1".to_string(),
            "Prod.T2".to_string(),
            "Prod.T3".to_string(),
            "Prod.T4".to_string(),
        ]
    );
    assert_eq!(
        row.reference_total, 9,
        "the two tied ids count too: {row:?}"
    );
    assert!(!row.references.contains(&"Tests.Helper".to_string()));
    assert!(!row.references.contains(&"Prod.Guessed".to_string()));
}

#[test]
fn a_matching_file_with_no_production_reference_is_listed_empty() {
    let root = temp_repo_root("name-hint-empty");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            test_type("Tests.FooTests", "Tests/FooTests.cs"),
            test_type("Tests.Helper", "Tests/Helper.cs"),
        ],
        vec![refers("Tests/FooTests.cs", 5, "Tests.Helper")],
    );
    let index = index_over(&root, &g);

    let rows = name_hint_rows(&index, "Prod.Foo");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].references.is_empty(), "{rows:?}");
    assert_eq!(rows[0].reference_total, 0);
}

#[test]
fn rows_follow_path_order() {
    let root = temp_repo_root("name-hint-order");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            test_type("Tests.BFooTests", "Tests/B/FooTests.cs"),
            test_type("Tests.AFooTests", "Tests/A/FooTests.cs"),
        ],
        Vec::new(),
    );
    let index = index_over(&root, &g);

    let rows = name_hint_rows(&index, "Prod.Foo");
    assert_eq!(
        rows.iter().map(|r| r.file.as_str()).collect::<Vec<_>>(),
        vec!["Tests/A/FooTests.cs", "Tests/B/FooTests.cs"]
    );
}

#[test]
fn an_answer_with_a_row_or_a_possible_route_carries_no_hint() {
    let root = temp_repo_root("name-hint-suppressed");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            test_type("Tests.FooTests", "Tests/FooTests.cs"),
        ],
        vec![refers("Tests/FooTests.cs", 5, "Prod.Foo")],
    );
    let index = index_over(&root, &g);

    let TestsResult::Resolved(model) = build_tests_model(&index, "Prod.Foo") else {
        panic!("Foo resolves");
    };
    assert_eq!(model.test_file_count, 1);
    assert!(
        model.name_hint.is_empty(),
        "a real reference is an answer, so no name observation joins it"
    );
}

#[test]
fn a_test_file_reached_only_over_a_bus_hop_suppresses_the_hint() {
    let root = temp_repo_root("name-hint-bus");
    let g = make_graph(
        vec![
            def("Prod.Foo", "Foo", "Prod", "class", "Src/Foo.cs", 1),
            test_type("Tests.FooTests", "Tests/FooTests.cs"),
        ],
        vec![Edge::BusHop {
            from_file: "Tests/FooTests.cs".to_string(),
            from_line: 3,
            message: "Ping".to_string(),
            to: "Prod.Foo".to_string(),
            to_file: "Src/Foo.cs".to_string(),
            evidence: "publish-site".to_string(),
        }],
    );
    let index = index_over(&root, &g);

    let TestsResult::Resolved(model) = build_tests_model(&index, "Prod.Foo") else {
        panic!("Foo resolves");
    };
    assert_eq!(model.bus.total, 1);
    assert!(model.rows.is_empty());
    assert!(
        model.name_hint.is_empty(),
        "a possible route is still an answer, so no name observation joins it"
    );
}
