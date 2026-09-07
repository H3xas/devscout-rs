use super::*;

// --- 3: build_refs_model basic inbound/outbound/imports/manifest-gap ---

#[test]
fn build_refs_model_def_site_inbound_outbound_grouped_imports_manifest_gap() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "IWidget",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };

    assert_eq!(model.id, "App.Widgets.IWidget");
    assert_eq!(
        model.sites,
        vec![DefSite {
            file: "Widgets/IWidget.cs".into(),
            line: 3
        }]
    );

    assert_eq!(model.inbound.inherits.total, 2);
    let mut files: Vec<&str> = model
        .inbound
        .inherits
        .rows
        .iter()
        .map(|r| r.file.as_str())
        .collect();
    files.sort();
    assert_eq!(
        files,
        vec!["Widgets/Impl/OtherImpl.cs", "Widgets/Impl/WidgetImpl.cs"]
    );
    assert_eq!(model.inbound.uses_type.total, 1);
    assert_eq!(model.inbound.uses_type.rows[0].file, "Consumers/Holder.cs");

    // IWidget's own file makes no outbound reference in the fixture.
    assert_eq!(
        model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .inherits
            .total,
        0
    );
    assert_eq!(
        model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .imports
            .total,
        0
    );

    assert_eq!(
        model.manifest_gap, 1,
        "the one flagged def file must surface in every refs call, not just the loader"
    );
}

// --- 4: outbound inherits+imports from own file; partial-class def sites ---

#[test]
fn build_refs_model_outbound_own_file_and_partial_class_sites() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);

    let impl_model = match build_refs_model(
        &index,
        "WidgetImpl",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        impl_model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .inherits
            .total,
        1
    );
    assert_eq!(
        impl_model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .inherits
            .rows[0]
            .to_file,
        "Widgets/IWidget.cs"
    );
    assert_eq!(
        impl_model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .imports
            .total,
        1
    );
    assert_eq!(
        impl_model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .imports
            .rows[0]
            .target,
        "App.Widgets"
    );
    assert_eq!(
        impl_model.inbound.uses_type.total, 1,
        "TwoHop.cs references WidgetImpl"
    );

    let container = match build_refs_model(
        &index,
        "App.Outer.Container",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        container.sites,
        vec![
            DefSite {
                file: "Outer/Container.cs".into(),
                line: 3
            },
            DefSite {
                file: "Outer/Container.Extra.cs".into(),
                line: 1
            }
        ]
    );
}

// --- 5: partial class, second site same file as first -- no outbound double-count ---

#[test]
fn build_refs_model_partial_class_same_file_second_site_no_double_count() {
    let graph = make_graph(
        vec![def_also(
            "App.Split.Combo",
            "Combo",
            "App.Split",
            "class",
            "Split/Combo.cs",
            3,
            vec![("Split/Combo.cs", 20)],
        )],
        vec![
            uses_type("Split/Combo.cs", 5, "App.Split.Combo", "Split/Combo.cs"),
            imports("Split/Combo.cs", 1, "System"),
        ],
    );
    let root = temp_repo_root("partial-same-file");
    write_manifest_fixture(&root, &["Split/Combo.cs"]);
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "Combo",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .uses_type
            .total,
        1,
        "the single outbound uses-type edge must not be counted twice"
    );
    assert_eq!(
        model
            .outbound
            .as_ref()
            .expect("built with out=true")
            .imports
            .total,
        1,
        "the single outbound imports edge must not be counted twice"
    );
}

// --- 6: ambiguous edges land in a separate trailing section ---

#[test]
fn build_refs_model_ambiguous_edges_never_guessed_into_inbound() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "App.One.Config",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };

    assert_eq!(model.inbound.inherits.total, 0);
    assert_eq!(
        model.inbound.uses_type.total, 0,
        "the ambiguous ref to \"Config\" must not be counted as a resolved inbound uses-type hit"
    );
    assert_eq!(model.ambiguous.inbound.total, 1);
    assert_eq!(model.ambiguous.inbound.rows[0].raw, "Config");
    assert_eq!(model.ambiguous.inbound.rows[0].candidate_count, 2);
}

// --- 7: an ambiguous QUERY name returns candidates, never a guess ---

#[test]
fn build_refs_model_ambiguous_query_name_returns_candidates() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    match build_refs_model(
        &index,
        "Config",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Ambiguous(mut ids) => {
            ids.sort();
            assert_eq!(
                ids,
                vec!["App.One.Config".to_string(), "App.Two.Config".to_string()]
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

// --- 8: caps a table, always reports dropped ---

#[test]
fn build_refs_model_caps_a_table_and_reports_dropped() {
    let edges: Vec<graph::Edge> = (0..5)
        .map(|i| {
            uses_type(
                &format!("Consumers/C{i}.cs"),
                1,
                "App.Hot.Popular",
                "Hot/Popular.cs",
            )
        })
        .collect();
    let graph = make_graph(
        vec![def(
            "App.Hot.Popular",
            "Popular",
            "App.Hot",
            "class",
            "Hot/Popular.cs",
            1,
        )],
        edges,
    );
    let root = temp_repo_root("cap-table");
    let mut files: Vec<String> = (0..5).map(|i| format!("Consumers/C{i}.cs")).collect();
    files.push("Hot/Popular.cs".to_string());
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    write_manifest_fixture(&root, &file_refs);
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(&index, "Popular", true, 2, 2, OUTBOUND_CAP, false) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(model.inbound.uses_type.total, 5);
    assert_eq!(
        model.inbound.uses_type.rows.len(),
        2,
        "must respect the cap"
    );
    assert_eq!(
        model.inbound.uses_type.dropped, 3,
        "must always report how many rows were dropped"
    );
}

// --- 8b: the outbound tables exist only when asked for ---

#[test]
fn build_refs_model_outbound_tables_exist_only_when_asked_for() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);
    let resolved = |out: bool| match build_refs_model(
        &index,
        "WidgetImpl",
        out,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert!(
        resolved(false).outbound.is_none(),
        "the default model has no outbound tables at all"
    );
    assert!(resolved(true).outbound.is_some(), "--out brings them back");
}

// --- 8c: one cap over the three inbound kinds, ranked ---

#[test]
fn build_refs_model_one_inbound_cap_ranked_resolved_then_same_project() {
    // 3 uses-member (2 of them guesses) + 2 uses-type, cap 3. A per-kind
    // cap would show all three uses-member rows; the shared one spends the
    // budget on the facts first, and among facts on the def's own project.
    let graph = make_graph(
        vec![def(
            "App.Hot.Popular",
            "Popular",
            "App.Hot",
            "class",
            "Hot/Popular.cs",
            1,
        )],
        vec![
            uses_member("Hot/Near.cs", 3, "App.Hot.Popular", "Hot/Popular.cs"),
            heuristic_uses_member("Cold/Guess.cs", 4, "App.Hot.Popular", "Hot/Popular.cs"),
            heuristic_uses_member("Hot/Guess.cs", 5, "App.Hot.Popular", "Hot/Popular.cs"),
            uses_type("Cold/Far.cs", 6, "App.Hot.Popular", "Hot/Popular.cs"),
            uses_type("Hot/AlsoNear.cs", 7, "App.Hot.Popular", "Hot/Popular.cs"),
        ],
    );
    let root = temp_repo_root("inbound-rank");
    write_manifest_fixture(
        &root,
        &[
            "Hot/Popular.cs",
            "Hot/Near.cs",
            "Hot/Guess.cs",
            "Hot/AlsoNear.cs",
            "Cold/Guess.cs",
            "Cold/Far.cs",
        ],
    );
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "Popular",
        false,
        DEFAULT_CAP,
        3,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };

    fn files(t: &Table<InboundRow>) -> Vec<&str> {
        t.rows.iter().map(|r| r.file.as_str()).collect()
    }
    assert_eq!(
        files(&model.inbound.uses_type),
        vec!["Hot/AlsoNear.cs", "Cold/Far.cs"]
    );
    assert_eq!(files(&model.inbound.uses_member), vec!["Hot/Near.cs"]);
    assert_eq!(model.inbound.uses_member.total, 3);
    assert_eq!(
        model.inbound.uses_member.dropped, 2,
        "both guesses lost the budget to the facts"
    );
    assert_eq!(model.inbound.uses_type.dropped, 0);
}

// --- 8d: one trimmed source line per shown hit ---

#[test]
fn build_refs_model_shown_hit_carries_its_trimmed_source_line_and_a_missing_file_carries_none() {
    let graph = make_graph(
        vec![def(
            "App.Hot.Popular",
            "Popular",
            "App.Hot",
            "class",
            "Hot/Popular.cs",
            1,
        )],
        vec![
            uses_type("Hot/Real.cs", 2, "App.Hot.Popular", "Hot/Popular.cs"),
            uses_type("Hot/Absent.cs", 2, "App.Hot.Popular", "Hot/Popular.cs"),
        ],
    );
    let root = temp_repo_root("source-line");
    write_manifest_fixture(&root, &["Hot/Popular.cs", "Hot/Real.cs", "Hot/Absent.cs"]);
    std::fs::create_dir_all(root.join("Hot")).expect("fixture dir");
    std::fs::write(
        root.join("Hot/Real.cs"),
        "class Real\n\t{\tpublic Popular P { get; set; }\t}\n",
    )
    .expect("fixture file");

    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "Popular",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let row = |file: &str| {
        model
            .inbound
            .uses_type
            .rows
            .iter()
            .find(|r| r.file == file)
            .expect("row")
            .clone()
    };
    assert_eq!(
        row("Hot/Real.cs").source,
        "{ public Popular P { get; set; } }",
        "tabs collapse to single spaces, the indent is trimmed off"
    );
    assert_eq!(
        row("Hot/Absent.cs").source,
        "",
        "a file that is not on disk yields no line, never a partial one"
    );
}

// --- --out mirrors of 8c/8d above, over the four outbound kinds ---

#[test]
fn build_refs_model_one_outbound_cap_ranked_resolved_then_same_project_imports_foreign() {
    // A same-project resolved uses-type, an always-foreign imports edge, a
    // foreign resolved inherits edge and a same-project heuristic
    // uses-member, cap 2. Imports is never a guess so it beats both the
    // foreign inherits row and the heuristic row; among the two foreign,
    // non-heuristic rows (imports line 1, inherits line 6) the file/line
    // tiebreak decides, since project ties them.
    let graph = make_graph(
        vec![
            def(
                "App.Hot.Consumer",
                "Consumer",
                "App.Hot",
                "class",
                "Hot/Consumer.cs",
                1,
            ),
            def(
                "App.Hot.Local",
                "Local",
                "App.Hot",
                "class",
                "Hot/Local.cs",
                1,
            ),
            def("App.Cold.Far", "Far", "App.Cold", "class", "Cold/Far.cs", 1),
        ],
        vec![
            uses_type("Hot/Consumer.cs", 5, "App.Hot.Local", "Hot/Local.cs"),
            imports("Hot/Consumer.cs", 1, "System"),
            inherits("Hot/Consumer.cs", 6, "App.Cold.Far", "Cold/Far.cs"),
            heuristic_uses_member("Hot/Consumer.cs", 7, "App.Hot.Local", "Hot/Local.cs"),
        ],
    );
    let root = temp_repo_root("outbound-rank");
    write_manifest_fixture(&root, &["Hot/Consumer.cs", "Hot/Local.cs", "Cold/Far.cs"]);
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, false)
    {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let ob = model.outbound.as_ref().expect("built with out=true");

    assert_eq!(
        ob.uses_type.rows.len(),
        1,
        "the same-project resolved edge spends the shared budget first"
    );
    assert_eq!(ob.uses_type.rows[0].line, 5);
    assert_eq!(
        ob.imports.rows.len(),
        1,
        "imports is never a guess, so it beats both the foreign inherits and the heuristic row"
    );
    assert_eq!(ob.imports.rows[0].line, 1);
    assert_eq!(
        ob.inherits.rows.len(),
        0,
        "the foreign inherits edge lost the shared budget"
    );
    assert_eq!(ob.inherits.dropped, 1);
    assert_eq!(
        ob.uses_member.rows.len(),
        0,
        "the heuristic guess lost the budget too, project notwithstanding"
    );
    assert_eq!(ob.uses_member.dropped, 1);
    assert_eq!(ob.imports.dropped, 0);
    assert_eq!(ob.uses_type.dropped, 0);
}

#[test]
fn build_refs_model_shown_outbound_hit_carries_its_trimmed_source_line_and_a_missing_file_carries_none(
) {
    let graph = make_graph(
        vec![
            def_also(
                "App.Hot.Consumer",
                "Consumer",
                "App.Hot",
                "class",
                "Hot/Consumer.cs",
                1,
                vec![("Hot/Consumer.Extra.cs", 1)],
            ),
            def(
                "App.Hot.Local",
                "Local",
                "App.Hot",
                "class",
                "Hot/Local.cs",
                1,
            ),
        ],
        vec![
            uses_type("Hot/Consumer.cs", 2, "App.Hot.Local", "Hot/Local.cs"),
            uses_type("Hot/Consumer.Extra.cs", 3, "App.Hot.Local", "Hot/Local.cs"),
        ],
    );
    let root = temp_repo_root("outbound-source-line");
    write_manifest_fixture(
        &root,
        &["Hot/Consumer.cs", "Hot/Consumer.Extra.cs", "Hot/Local.cs"],
    );
    std::fs::create_dir_all(root.join("Hot")).expect("fixture dir");
    std::fs::write(
        root.join("Hot/Consumer.cs"),
        "x\n\t{\tpublic Local L { get; set; }\t}\n",
    )
    .expect("fixture file");
    // Hot/Consumer.Extra.cs is deliberately never written to disk.

    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "Consumer",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let rows = &model
        .outbound
        .as_ref()
        .expect("built with out=true")
        .uses_type
        .rows;
    let row = |line: usize| rows.iter().find(|r| r.line == line).expect("row").clone();
    assert_eq!(
        row(2).source,
        "{ public Local L { get; set; } }",
        "tabs collapse to single spaces, the indent is trimmed off"
    );
    assert_eq!(
        row(3).source,
        "",
        "a file that is not on disk yields no line, never a partial one"
    );
}

#[test]
fn build_refs_model_all_out_lifts_the_outbound_cap() {
    let edges: Vec<graph::Edge> = (0..5)
        .map(|i| imports("Hot/Consumer.cs", i + 1, &format!("Ns{i}")))
        .collect();
    let graph = make_graph(
        vec![def(
            "App.Hot.Consumer",
            "Consumer",
            "App.Hot",
            "class",
            "Hot/Consumer.cs",
            1,
        )],
        edges,
    );
    let root = temp_repo_root("outbound-all");
    write_manifest_fixture(&root, &["Hot/Consumer.cs"]);
    let index = load_graph_index(&graph, &root);

    let capped =
        match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
    let capped_ob = capped.outbound.as_ref().expect("built with out=true");
    assert_eq!(
        capped_ob.imports.rows.len(),
        2,
        "must respect the outbound cap"
    );
    assert_eq!(capped_ob.imports.dropped, 3);

    let all = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, true) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let all_ob = all.outbound.as_ref().expect("built with out=true");
    assert_eq!(
        all_ob.imports.rows.len(),
        5,
        "--all lifts the outbound cap entirely"
    );
    assert_eq!(all_ob.imports.dropped, 0);
}

// `--all` (`all_out`) must lift the INBOUND cap, not just the outbound one:
// without it a caller reaching for `--all` on a truncated inbound table got
// nothing back for it. Five distinct referring files against a cap of 2
// proves the file-loss shape, not just a row-count-under-cap shape.
#[test]
fn build_refs_model_all_out_now_lifts_the_inbound_cap_too() {
    let files = [
        "Cold/ConsumerA.cs",
        "Cold/ConsumerB.cs",
        "Cold/ConsumerC.cs",
        "Cold/ConsumerD.cs",
        "Cold/ConsumerE.cs",
    ];
    let edges: Vec<graph::Edge> = files
        .iter()
        .map(|f| uses_type(f, 1, "App.Hot.Widget", "Hot/Widget.cs"))
        .collect();
    let graph = make_graph(
        vec![def(
            "App.Hot.Widget",
            "Widget",
            "App.Hot",
            "class",
            "Hot/Widget.cs",
            1,
        )],
        edges,
    );
    let root = temp_repo_root("inbound-all");
    let mut manifest_files: Vec<&str> = vec!["Hot/Widget.cs"];
    manifest_files.extend_from_slice(&files);
    write_manifest_fixture(&root, &manifest_files);
    let index = load_graph_index(&graph, &root);

    let capped =
        match build_refs_model(&index, "Widget", false, DEFAULT_CAP, 2, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
    assert_eq!(
        capped.inbound.uses_type.rows.len(),
        2,
        "must respect the inbound cap"
    );
    assert_eq!(
        capped.inbound.uses_type.dropped, 3,
        "the other 3 referring files are lost without --all"
    );

    let all = match build_refs_model(&index, "Widget", false, DEFAULT_CAP, 2, OUTBOUND_CAP, true) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        all.inbound.uses_type.rows.len(),
        5,
        "--all lifts the inbound cap too, not just the outbound one"
    );
    assert_eq!(all.inbound.uses_type.dropped, 0);
}
