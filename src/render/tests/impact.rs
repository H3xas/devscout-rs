use super::*;

fn impact_model(
    rows: Vec<ImpactRow>,
    total_affected: usize,
    dropped: usize,
    manifest_gap: usize,
) -> ImpactModel {
    let heuristic_affected = rows.iter().filter(|r| r.heuristic).count();
    ImpactModel {
        kind: SeedKind::File,
        seed_files: vec!["src/IFoo.cs".to_string()],
        hops: 2,
        total_affected,
        rows,
        dropped,
        manifest_gap,
        heuristic_affected,
        tests_affected: 0,
        braked: vec![],
        braked_files: vec![],
    }
}

/// A precisely-reached impact row (both heuristic flags off).
fn impact_row(
    file: &str,
    hop: u32,
    via_count: u32,
    ambiguous_count: u32,
    top_symbols: &[&str],
    top_symbols_more: usize,
    score: f64,
) -> ImpactRow {
    ImpactRow {
        file: file.into(),
        hop,
        via_count,
        ambiguous_count,
        top_symbols: top_symbols.iter().map(|s| (*s).to_string()).collect(),
        top_symbols_more,
        score,
        heuristic_count: 0,
        heuristic: false,
        tier: None,
        iface_via: vec![],
        from_lines: vec![],
        infra: false,
        why: Why::UsesMemberPrecise,
    }
}

/// A heuristic-ONLY row: reached only by guesses, the only shape flagged.
fn heuristic_impact_row(
    file: &str,
    hop: u32,
    heuristic_count: u32,
    top_symbols: &[&str],
    score: f64,
) -> ImpactRow {
    ImpactRow {
        heuristic_count,
        heuristic: true,
        ..impact_row(file, hop, 0, 0, top_symbols, 0, score)
    }
}

#[test]
fn impact_compact_groups_rows_into_ascending_hop_buckets() {
    let model = impact_model(
        vec![
            impact_row("src/Two.cs", 2, 1, 0, &["Foo"], 0, 0.1),
            impact_row("src/One.cs", 1, 2, 1, &["Foo", "Bar"], 5, 0.5),
        ],
        3,
        0,
        0,
    );
    let out = render_impact_compact("IFoo", &model);
    let hop1 = out.find("hop 1").unwrap();
    let hop2 = out.find("hop 2").unwrap();
    assert!(
        hop1 < hop2,
        "hop 1 bucket must precede hop 2 regardless of row order in the model"
    );
    assert!(out.contains("hop 1 (1):\n  src/One.cs via=2(+1amb)"));
    assert!(out.contains("hop 2 (1):\n  src/Two.cs via=1"));
    assert!(!out.contains("Bar"), "compact drops top-symbols entirely");
    assert!(out.contains("summary: affected=3 shown=2 dropped=0 ambiguous=1"));
}

#[test]
fn impact_compact_manifest_gap_folds_into_summary() {
    let model = impact_model(vec![], 0, 0, 2);
    let out = render_impact_compact("IFoo", &model);
    let last_line = out.lines().last().unwrap();
    assert_eq!(
        last_line,
        "summary: affected=0 shown=0 dropped=0 ambiguous=0 gap=2"
    );
}

#[test]
fn impact_text_declares_heuristic_reached_files_beside_the_count_only_when_there_are_some() {
    let with_guess = impact_model(
        vec![
            impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5),
            heuristic_impact_row("src/Guessed.cs", 1, 2, &["Widget"], 0.0),
        ],
        1,
        0,
        0,
    );
    let out = render_impact_text("Widget", &with_guess);
    assert!(
        out.contains("affected files: 1 (+1 heuristic)  shown: 2  dropped: 0"),
        "{out}"
    );
    assert!(out.contains("src/Direct.cs  1  1  Widget\n"), "{out}");
    assert!(
        out.contains("src/Guessed.cs  1  2  Widget (heuristic)"),
        "the via column reports the GUESS count, never the zero viaCount\n{out}"
    );

    let without = impact_model(
        vec![impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5)],
        1,
        0,
        0,
    );
    let out = render_impact_text("Widget", &without);
    assert!(
        out.contains("affected files: 1  shown: 1  dropped: 0"),
        "byte-unchanged when there is nothing to declare\n{out}"
    );
    assert!(!out.contains("heuristic"), "{out}");
}

#[test]
fn impact_text_marks_a_row_reached_only_by_an_extension_guess_as_extension() {
    let tiered = |file: &str, count: u32, tier: HeuristicTier| ImpactRow {
        heuristic_count: count,
        heuristic: true,
        tier: Some(tier),
        ..impact_row(file, 1, 0, 0, &["Widget"], 0, 0.0)
    };
    let model = impact_model(
        vec![
            impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5),
            tiered("src/Extended.cs", 2, HeuristicTier::Ext),
            tiered("src/Guessed.cs", 3, HeuristicTier::Guess),
        ],
        1,
        0,
        0,
    );
    let out = render_impact_text("Widget", &model);
    // The SUMMARY keeps the umbrella word and the umbrella count -- both
    // tiers are still "not a fact" as far as `affected` is concerned.
    assert!(
        out.contains("affected files: 1 (+2 heuristic)  shown: 3  dropped: 0"),
        "{out}"
    );
    assert!(
        out.contains("src/Extended.cs  1  2  Widget (extension)"),
        "{out}"
    );
    assert!(
        out.contains("src/Guessed.cs  1  3  Widget (guess)"),
        "{out}"
    );
    // Compact says the same thing in one character each.
    let compact = render_impact_compact("Widget", &model);
    assert!(compact.contains("src/Extended.cs via=2x"), "{compact}");
    assert!(compact.contains("src/Guessed.cs via=3h"), "{compact}");
}

#[test]
fn impact_compact_summary_inserts_heuristic_before_gap() {
    let model = impact_model(
        vec![heuristic_impact_row(
            "src/Guessed.cs",
            1,
            3,
            &["Widget"],
            0.0,
        )],
        0,
        0,
        2,
    );
    let out = render_impact_compact("Widget", &model);
    assert!(out.contains("hop 1 (1):\n  src/Guessed.cs via=3h"), "{out}");
    assert_eq!(
        out.lines().last().unwrap(),
        "summary: affected=0 shown=1 dropped=0 ambiguous=0 heuristic=1 gap=2"
    );

    // And it disappears entirely at zero.
    let none = impact_model(
        vec![impact_row("src/Direct.cs", 1, 1, 0, &["Widget"], 0, 0.5)],
        1,
        0,
        2,
    );
    assert_eq!(
        render_impact_compact("Widget", &none)
            .lines()
            .last()
            .unwrap(),
        "summary: affected=1 shown=1 dropped=0 ambiguous=0 gap=2"
    );
}

#[test]
fn impact_summaries_append_tests_only_when_the_blast_radius_reaches_one() {
    let reached = ImpactModel {
        tests_affected: 1,
        ..impact_model(
            vec![impact_row(
                "tests/OrderServiceTests.cs",
                1,
                1,
                0,
                &["OrderService"],
                0,
                0.5,
            )],
            1,
            0,
            0,
        )
    };
    assert!(
        render_impact_text("OrderService", &reached)
            .contains("affected files: 1  shown: 1  dropped: 0 tests=1"),
        "{}",
        render_impact_text("OrderService", &reached)
    );
    assert_eq!(
        render_impact_compact("OrderService", &reached)
            .lines()
            .last()
            .unwrap(),
        "summary: affected=1 shown=1 dropped=0 ambiguous=0 tests=1"
    );

    let none = impact_model(
        vec![impact_row(
            "src/Other.cs",
            1,
            1,
            0,
            &["OrderService"],
            0,
            0.5,
        )],
        1,
        0,
        0,
    );
    assert!(
        render_impact_text("OrderService", &none)
            .contains("affected files: 1  shown: 1  dropped: 0\n"),
        "its ABSENCE is the gap signal -- never printed as tests=0"
    );
    assert_eq!(
        render_impact_compact("OrderService", &none)
            .lines()
            .last()
            .unwrap(),
        "summary: affected=1 shown=1 dropped=0 ambiguous=0"
    );
}

#[test]
fn impact_compact_orders_tests_after_heuristic_and_before_gap() {
    let model = ImpactModel {
        tests_affected: 1,
        ..impact_model(
            vec![
                impact_row(
                    "tests/OrderServiceTests.cs",
                    1,
                    1,
                    0,
                    &["OrderService"],
                    0,
                    0.5,
                ),
                heuristic_impact_row("src/Guessed.cs", 1, 3, &["OrderService"], 0.0),
            ],
            1,
            0,
            2,
        )
    };
    assert_eq!(
        render_impact_compact("OrderService", &model)
            .lines()
            .last()
            .unwrap(),
        "summary: affected=1 shown=2 dropped=0 ambiguous=0 heuristic=1 tests=1 gap=2"
    );
}
