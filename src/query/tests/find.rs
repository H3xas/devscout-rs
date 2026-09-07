use super::*;

// --- the per-file first-declaration line find's manifest-pool block reads ---

fn graph_name(name: &str, kind: &str, file: &str, line: usize) -> graph::GraphName {
    graph::GraphName {
        name: name.to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        owner: String::new(),
    }
}

// --- file_inbound_counts -------------------------------------------------
//
// The map `find`'s tie-break reads. Every claim it makes about which edges
// are facts is pinned here: one wrong kind in (or out) silently reorders
// every text-tied find answer.

#[test]
fn file_inbound_counts_counts_precise_reference_edges_by_target_file() {
    let g = make_graph(
        vec![def(
            "App.IWidget",
            "IWidget",
            "App",
            "interface",
            "Widgets/IWidget.cs",
            3,
        )],
        vec![
            inherits("Widgets/Impl/A.cs", 5, "App.IWidget", "Widgets/IWidget.cs"),
            uses_type("Consumers/B.cs", 4, "App.IWidget", "Widgets/IWidget.cs"),
            uses_member("Consumers/C.cs", 9, "App.IWidget", "Widgets/IWidget.cs"),
        ],
    );
    let counts = file_inbound_counts(&g);
    assert_eq!(
        counts.get("Widgets/IWidget.cs"),
        Some(&3),
        "every precise reference kind counts"
    );
    assert_eq!(counts.len(), 1);
}

#[test]
fn file_inbound_counts_excludes_heuristic_edges_and_non_reference_kinds() {
    let g = make_graph(
        vec![def(
            "App.IWidget",
            "IWidget",
            "App",
            "interface",
            "Widgets/IWidget.cs",
            3,
        )],
        vec![
            heuristic_uses_type("Guessy/Guesser.cs", 7, "App.IWidget", "Widgets/IWidget.cs"),
            heuristic_uses_member("Guessy/Guesser2.cs", 8, "App.IWidget", "Widgets/IWidget.cs"),
            // Names a namespace, never a definition.
            imports("Widgets/IWidget.cs", 1, "App.Widgets"),
            // DI wiring and an unresolved name: neither is a reference to
            // this file's definitions.
            graph::Edge::CtorDi {
                from_file: "App/Program.cs".into(),
                from_line: 4,
                iface: "App.IWidget".into(),
                resolution: "plain".into(),
                args: None,
                to: None,
                candidates: vec![],
            },
            ambiguous(
                "Ambig/User.cs",
                6,
                "IWidget",
                vec![("App.IWidget", "Widgets/IWidget.cs")],
            ),
            // A module import that resolves to the file itself is still an
            // import, not a reference to a declaration.
            graph::Edge::Import {
                from_file: "Consumers/Importer.ts".into(),
                from_line: 1,
                target: "./widgets".into(),
                to_file: "Widgets/IWidget.ts".into(),
                via: None,
            },
        ],
    );
    assert!(
        file_inbound_counts(&g).is_empty(),
        "guesses, imports, ctor-di wiring, and ambiguity earn no count"
    );
}

#[test]
fn file_inbound_counts_counts_the_ts_reference_kinds() {
    // On a TS repo call/jsx-use/dispatch ARE the reference graph; a count
    // blind to them would rank every TS file at zero.
    let g = make_graph(
        vec![def(
            "ui.Button",
            "Button",
            "",
            "function",
            "src/Button.tsx",
            1,
        )],
        vec![
            graph::Edge::Call {
                from_file: "src/App.ts".into(),
                from_line: 10,
                to: "ui.Button".into(),
                to_file: "src/Button.tsx".into(),
            },
            graph::Edge::JsxUse {
                from_file: "src/Page.tsx".into(),
                from_line: 20,
                to: "ui.Button".into(),
                to_file: "src/Button.tsx".into(),
            },
            graph::Edge::Dispatch {
                from_file: "src/store.ts".into(),
                from_line: 30,
                to: "ui.Button".into(),
                to_file: "src/Button.tsx".into(),
            },
        ],
    );
    let counts = file_inbound_counts(&g);
    assert_eq!(counts.get("src/Button.tsx"), Some(&3));
    assert!(
        !counts.contains_key("src/App.ts"),
        "keyed by the TARGET file only"
    );
}

#[test]
fn file_inbound_counts_excludes_a_files_references_to_itself() {
    // This repository treats only references from other files as inbound
    // interest; a file's self-references do not count.
    let g = make_graph(
        vec![def("App.Hub", "Hub", "App", "class", "src/Hub.cs", 3)],
        vec![
            uses_type("src/Hub.cs", 5, "App.Hub", "src/Hub.cs"),
            inherits("src/Hub.cs", 7, "App.Hub", "src/Hub.cs"),
            uses_type("src/Other.cs", 9, "App.Hub", "src/Hub.cs"),
        ],
    );
    let counts = file_inbound_counts(&g);
    assert_eq!(
        counts.get("src/Hub.cs"),
        Some(&1),
        "self-references earn nothing"
    );
    assert_eq!(counts.len(), 1);
}

#[test]
fn file_inbound_counts_on_an_edgeless_graph_is_an_empty_map() {
    assert!(file_inbound_counts(&make_graph(vec![], vec![])).is_empty());
}

#[test]
fn first_decl_line_by_file_keeps_the_minimum_line_per_file_across_the_whole_name_index_not_just_the_last_one_seen(
) {
    let mut graph = make_graph(vec![], vec![]);
    graph.names = vec![
        graph_name("Widget", "class", "a.cs", 5),
        graph_name("Render", "method", "a.cs", 12),
        graph_name("Widget", "class", "a.cs", 1),
        graph_name("Other", "class", "b.cs", 8),
    ];
    let mut rows: Vec<(String, usize)> = first_decl_line_by_file(&graph).into_iter().collect();
    rows.sort();
    assert_eq!(rows, vec![("a.cs".to_string(), 1), ("b.cs".to_string(), 8)]);
}

#[test]
fn first_decl_line_by_file_on_a_graph_with_no_names_index_is_empty() {
    assert!(first_decl_line_by_file(&make_graph(vec![], vec![])).is_empty());
}
