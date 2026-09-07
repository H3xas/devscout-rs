use super::*;

fn stage4_impact_defs() -> Vec<graph::Def> {
    vec![
        widget_def(),
        def(
            "App.Direct.Direct",
            "Direct",
            "App.Direct",
            "class",
            "Consumers/Direct.cs",
            3,
        ),
        def(
            "App.Guessed.Guessed",
            "Guessed",
            "App.Guessed",
            "class",
            "Consumers/Guessed.cs",
            3,
        ),
    ]
}

#[test]
fn stage4_cli_refs_lists_every_precise_row_before_any_heuristic_row_and_suffixes_only_the_guesses()
{
    // Alphabetically Guess.cs sorts BEFORE Precise.cs, so a single
    // by-location sort over the union would interleave them. The split is
    // what puts the facts first, not the sort.
    let defs = vec![widget_def()];
    let edges = vec![
        uses_member(
            "Consumers/Precise.cs",
            10,
            "App.Core.Widget",
            "Core/Widget.cs",
        ),
        heuristic_uses_member("Consumers/Guess.cs", 7, "App.Core.Widget", "Core/Widget.cs"),
    ];
    let root = stage4_root(&defs, &edges, "stage4-order");
    let g = make_graph(defs, edges);
    let index = load_graph_index(&g, &root);
    let model = match build_refs_model(
        &index,
        "Widget",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected a resolved model, got {other:?}"),
    };

    let out = crate::render::render_refs_text(&model);
    let lines: Vec<&str> = out.lines().collect();
    let start = lines
        .iter()
        .position(|l| *l == "  uses-member (2):")
        .expect("a uses-member block counting both rows");
    assert_eq!(
        &lines[start + 1..start + 3],
        [
            "    Consumers/Precise.cs:10  uses-member",
            "    Consumers/Guess.cs:7  uses-member (guess)"
        ]
    );

    // --compact marks the same row with the one-character form.
    let compact = crate::render::render_refs_compact(&model);
    assert!(
        compact.contains("in:uses-member (2):\n  Consumers/Precise.cs:10\n  Consumers/Guess.cs:7h"),
        "{compact}"
    );
}

#[test]
fn stage4_cli_refs_precise_rows_filling_the_cap_leave_no_room_for_heuristic_rows() {
    let defs = vec![widget_def()];
    let mut edges: Vec<graph::Edge> = (0..31)
        .map(|i| {
            uses_member(
                &format!("Consumers/C{i:02}.cs"),
                4,
                "App.Core.Widget",
                "Core/Widget.cs",
            )
        })
        .collect();
    edges.push(heuristic_uses_member(
        "Consumers/Zzz.cs",
        9,
        "App.Core.Widget",
        "Core/Widget.cs",
    ));
    let root = stage4_root(&defs, &edges, "stage4-cap");
    let g = make_graph(defs, edges);
    let index = load_graph_index(&g, &root);
    let model = match build_refs_model(
        &index,
        "Widget",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected a resolved model, got {other:?}"),
    };

    let out = crate::render::render_refs_text(&model);
    assert!(
        out.contains("  uses-member (32, 2 dropped):"),
        "the cap is shared: 32 rows, 30 shown, 2 dropped\n{out}"
    );
    assert!(
        out.contains("\n  +2 more\n"),
        "one trailer carries the exact count the call did not return\n{out}"
    );
    assert!(
        !out.contains("(guess)"),
        "precise rows have priority -- a full cap shows zero guesses"
    );
    assert_eq!(
        out.lines()
            .filter(|l| l.starts_with("    Consumers/"))
            .count(),
        30
    );
}

#[test]
fn stage4_cli_impact_declares_heuristic_reached_files_beside_the_affected_count_only_when_there_are_some(
) {
    let precise = uses_type(
        "Consumers/Direct.cs",
        4,
        "App.Core.Widget",
        "Core/Widget.cs",
    );

    let defs = stage4_impact_defs();
    let edges = vec![
        precise.clone(),
        heuristic_uses_member(
            "Consumers/Guessed.cs",
            8,
            "App.Core.Widget",
            "Core/Widget.cs",
        ),
    ];
    let root = stage4_root(&defs, &edges, "stage4-impact-with");
    let g = make_graph(defs, edges);
    let index = load_graph_index(&g, &root);
    let model = match build_impact_model(
        &index,
        "Core/Widget.cs",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected a resolved model, got {other:?}"),
    };
    let out = crate::render::render_impact_text("Core/Widget.cs", &model);
    assert!(
        out.contains("affected files: 1 (+1 heuristic)  shown: 2  dropped: 0"),
        "{out}"
    );
    let lines: Vec<&str> = out.lines().collect();
    let header = lines
        .iter()
        .position(|l| *l == "file  hops  via  top-symbols")
        .expect("row header present");
    assert_eq!(
        &lines[header + 1..header + 3],
        [
            "Consumers/Direct.cs  1  1  Widget",
            "Consumers/Guessed.cs  1  1  Widget (guess)"
        ],
        "heuristic-reached files are listed after every precise one, never ranked among them"
    );
    assert!(
        crate::render::render_impact_compact("Core/Widget.cs", &model)
            .contains("Consumers/Guessed.cs via=1h"),
        "compact reports the guess count, never `via=0`"
    );

    // The same query on a graph with no heuristic edges must render the
    // count line byte-for-byte as it did before this stage -- no empty
    // parenthetical anywhere.
    let defs = stage4_impact_defs();
    let edges = vec![precise];
    let root = stage4_root(&defs, &edges, "stage4-impact-without");
    let g = make_graph(defs, edges);
    let index = load_graph_index(&g, &root);
    let model = match build_impact_model(
        &index,
        "Core/Widget.cs",
        DEFAULT_HOPS,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected a resolved model, got {other:?}"),
    };
    let out = crate::render::render_impact_text("Core/Widget.cs", &model);
    assert!(
        out.contains("affected files: 1  shown: 1  dropped: 0"),
        "{out}"
    );
    assert!(!out.contains("heuristic"), "{out}");
    assert!(!crate::render::render_impact_compact("Core/Widget.cs", &model).contains("heuristic"));
}

#[test]
fn stage4_cli_impact_stops_at_a_heuristic_edge_instead_of_walking_through_it_to_a_second_hop() {
    // Core/Widget.cs <- Mid/Middle.cs <- Far/Far.cs. The first link is the
    // one under test; the second is always precise, so whether Far/Far.cs
    // shows up depends purely on whether the walk was allowed to continue
    // through link 1.
    let impact_out = |heuristic: bool, label: &str| {
        let defs = vec![
            widget_def(),
            def(
                "App.Mid.Middle",
                "Middle",
                "App.Mid",
                "class",
                "Mid/Middle.cs",
                3,
            ),
            def("App.Far.Far", "Far", "App.Far", "class", "Far/Far.cs", 3),
        ];
        let first = if heuristic {
            heuristic_uses_member("Mid/Middle.cs", 8, "App.Core.Widget", "Core/Widget.cs")
        } else {
            uses_member("Mid/Middle.cs", 8, "App.Core.Widget", "Core/Widget.cs")
        };
        let edges = vec![
            first,
            uses_type("Far/Far.cs", 4, "App.Mid.Middle", "Mid/Middle.cs"),
        ];
        let root = stage4_root(&defs, &edges, label);
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        let model = match build_impact_model(
            &index,
            "Core/Widget.cs",
            2,
            DEFAULT_CAP,
            true,
            DEFAULT_IFACE_MAX_FANIN,
            DEFAULT_HUB_MAX_INDEGREE,
        ) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };
        crate::render::render_impact_text("Core/Widget.cs", &model)
    };

    let control = impact_out(false, "stage4-walk-control");
    assert!(
        control.contains("Far/Far.cs"),
        "control: with a precise first link the walk reaches the second hop\n{control}"
    );
    assert!(control.contains("affected files: 2  "), "{control}");

    let guessed = impact_out(true, "stage4-walk-guess");
    assert!(
        guessed.contains("Mid/Middle.cs  1  1  Widget (guess)"),
        "the guessed file itself is still reported\n{guessed}"
    );
    assert!(
        !guessed.contains("Far/Far.cs"),
        "a guess may reach a file and must never become the premise of the next hop -- compounding guesses is how blast radius turns into fiction\n{guessed}"
    );
    assert!(
        guessed.contains("affected files: 0 (+1 heuristic)  shown: 1  dropped: 0"),
        "{guessed}"
    );
}
