use super::*;

// --- test-coverage stage: `devscout tests` bytes --------------------------

fn tests_model(rows: Vec<TestRow>) -> TestsModel {
    let precise: Vec<&TestRow> = rows.iter().filter(|r| !r.heuristic).collect();
    let guessed: Vec<&TestRow> = rows.iter().filter(|r| r.heuristic).collect();
    TestsModel {
        query: "OrderService".to_string(),
        symbol: "App.Orders.OrderService".to_string(),
        def_files: vec!["src/OrderService.cs".to_string()],
        test_file_count: precise.len(),
        ref_count: precise.iter().map(|r| r.ref_count).sum(),
        heuristic_file_count: guessed.len(),
        heuristic_ref_count: guessed.iter().map(|r| r.ref_count).sum(),
        rows,
    }
}

fn test_row(file: &str, test_defs: &[&str], lines: &[usize], heuristic: bool) -> TestRow {
    TestRow {
        file: file.into(),
        test_defs: test_defs.iter().map(|s| (*s).to_string()).collect(),
        lines: lines.to_vec(),
        ref_count: lines.len(),
        heuristic,
        tier: None,
        via: query::TestVia::Attribute,
    }
}

#[test]
fn tests_text_heads_the_file_and_indents_one_line_per_test_def() {
    let model = tests_model(vec![test_row(
        "tests/OrderServiceTests.cs",
        &["App.Orders.Tests.OrderServiceTests"],
        &[10, 11],
        false,
    )]);
    assert_eq!(
        render_tests_text(&model),
        "tests for App.Orders.OrderService\ncovered by 1 test file(s), 2 reference(s)\n\ntests/OrderServiceTests.cs\n  App.Orders.Tests.OrderServiceTests  lines: 10, 11"
    );
}

#[test]
fn tests_text_answers_the_zero_case_in_one_line_instead_of_a_header_over_a_void() {
    let mut model = tests_model(vec![]);
    model.symbol = "App.Orders.Untested".to_string();
    assert_eq!(
        render_tests_text(&model),
        "tests for App.Orders.Untested\nno test references found"
    );
}

#[test]
fn tests_text_marks_a_guessed_file_with_the_shared_heuristic_suffix() {
    let model = tests_model(vec![
        test_row(
            "tests/OrderServiceTests.cs",
            &["App.Orders.Tests.OrderServiceTests"],
            &[10],
            false,
        ),
        test_row(
            "tests/Partial.Extra.cs",
            &["App.Orders.Tests.PartialTests"],
            &[9],
            true,
        ),
    ]);
    let out = render_tests_text(&model);
    assert!(
        out.contains("covered by 1 test file(s), 1 reference(s)"),
        "counts stay precise-only\n{out}"
    );
    assert!(
        out.ends_with(
            "tests/Partial.Extra.cs (heuristic)\n  App.Orders.Tests.PartialTests  lines: 9"
        ),
        "{out}"
    );
}

#[test]
fn tests_text_suffix_follows_the_row_tier() {
    let tiered = |file: &str, tier: Option<HeuristicTier>| TestRow {
        tier,
        heuristic: tier.is_some(),
        ..test_row(file, &["App.Orders.Tests.T"], &[9], tier.is_some())
    };
    let model = tests_model(vec![
        tiered("tests/Precise.cs", None),
        tiered("tests/Ext.cs", Some(HeuristicTier::Ext)),
        tiered("tests/Guess.cs", Some(HeuristicTier::Guess)),
    ]);
    let out = render_tests_text(&model);
    assert!(out.contains("\ntests/Precise.cs\n"), "{out}");
    assert!(out.contains("\ntests/Ext.cs (extension)\n"), "{out}");
    assert!(out.contains("\ntests/Guess.cs (guess)\n"), "{out}");
    // The header's counts stay precise-only and keep the umbrella word, the
    // same split the refs and impact renderers make.
    assert!(
        out.contains("covered by 1 test file(s), 1 reference(s)"),
        "{out}"
    );
    assert_eq!(
        render_tests_compact(&model),
        "tests App.Orders.OrderService files=1 refs=1 heuristic=2\ntests/Precise.cs 9\ntests/Ext.cs 9x\ntests/Guess.cs 9h"
    );
}

#[test]
fn tests_text_marks_a_project_vouched_row_with_the_test_project_suffix_and_lists_its_lines_without_a_def_id(
) {
    let harness = TestRow {
        via: query::TestVia::Project,
        ..test_row("tests/App.Tests/FakeServer.cs", &[], &[12, 34], false)
    };
    let model = tests_model(vec![
        test_row(
            "tests/OrderServiceTests.cs",
            &["App.Orders.Tests.OrderServiceTests"],
            &[10],
            false,
        ),
        harness,
    ]);
    let out = render_tests_text(&model);
    assert!(
        out.ends_with("tests/App.Tests/FakeServer.cs (test project)\n  lines: 12, 34"),
        "{out}"
    );
    // The attribute-vouched row above it carries no such suffix.
    assert!(
        out.contains(
            "tests/OrderServiceTests.cs\n  App.Orders.Tests.OrderServiceTests  lines: 10\n"
        ),
        "{out}"
    );
}

#[test]
fn tests_compact_folds_the_counts_into_the_header_and_drops_the_def_ids() {
    let model = tests_model(vec![test_row(
        "tests/OrderServiceTests.cs",
        &["App.Orders.Tests.OrderServiceTests"],
        &[10, 11],
        false,
    )]);
    assert_eq!(
        render_tests_compact(&model),
        "tests App.Orders.OrderService files=1 refs=2\ntests/OrderServiceTests.cs 10,11"
    );

    let mut empty = tests_model(vec![]);
    empty.symbol = "App.Orders.Untested".to_string();
    assert_eq!(
        render_tests_compact(&empty),
        "tests App.Orders.Untested files=0 refs=0"
    );
}

#[test]
fn tests_compact_declares_heuristic_files_in_the_header_and_marks_their_lines() {
    let model = tests_model(vec![
        test_row(
            "tests/OrderServiceTests.cs",
            &["App.Orders.Tests.OrderServiceTests"],
            &[10],
            false,
        ),
        test_row(
            "tests/Partial.Extra.cs",
            &["App.Orders.Tests.PartialTests"],
            &[9, 12],
            true,
        ),
    ]);
    assert_eq!(
        render_tests_compact(&model),
        "tests App.Orders.OrderService files=1 refs=1 heuristic=1\ntests/OrderServiceTests.cs 10\ntests/Partial.Extra.cs 9h,12h"
    );
}
