use super::*;

// --- 2: resolve_symbol ladder ---

#[test]
fn resolve_symbol_exact_id_unique_name_case_insensitive_ambiguous_notfound() {
    let graph = base_fixture_graph();
    let root = base_fixture_root();
    let index = load_graph_index(&graph, &root);

    assert_eq!(
        resolve_symbol(&index, "App.Outer.Container+Item"),
        Resolution::Resolved("App.Outer.Container+Item".into())
    );
    assert_eq!(
        resolve_symbol(&index, "IWidget"),
        Resolution::Resolved("App.Widgets.IWidget".into())
    );
    assert_eq!(
        resolve_symbol(&index, "iwidget"),
        Resolution::Resolved("App.Widgets.IWidget".into()),
        "case-insensitive unique match must resolve"
    );

    match resolve_symbol(&index, "Config") {
        Resolution::Ambiguous(mut ids) => {
            ids.sort();
            assert_eq!(
                ids,
                vec!["App.One.Config".to_string(), "App.Two.Config".to_string()]
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }

    assert_eq!(
        resolve_symbol(&index, "NoSuchSymbolAnywhere"),
        Resolution::NotFound
    );
}

// --- 14: enum-member resolve_symbol by id and by unique simple name ---

#[test]
fn resolve_symbol_finds_enum_member_by_id_and_unique_simple_name() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    assert_eq!(
        resolve_symbol(&index, "App.Enums.PostType.Question"),
        Resolution::Resolved("App.Enums.PostType.Question".into())
    );
    assert_eq!(
        resolve_symbol(&index, "Question"),
        Resolution::Resolved("App.Enums.PostType.Question".into())
    );
}

// --- the Enum.Member tail and the member-count split ---

#[test]
fn resolve_symbol_accepts_a_dotted_tail_of_a_def_id_and_refuses_a_shared_one() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    assert_eq!(
        resolve_symbol(&index, "PostType.Question"),
        Resolution::Resolved("App.Enums.PostType.Question".into()),
        "the spelling a caller reaches for -- never the namespace-qualified id the graph keys it under"
    );
    assert_eq!(
        resolve_symbol(&index, "Enums.PostType"),
        Resolution::Resolved("App.Enums.PostType".into()),
        "the tail rule is not enum-specific: any dotted suffix of exactly one def id resolves"
    );
    assert_eq!(
        resolve_symbol(&index, "PostType.Missing"),
        Resolution::NotFound
    );

    let two = make_graph(
        vec![
            def("App.One.Mode", "Mode", "App.One", "enum", "One/Mode.cs", 1),
            def(
                "App.One.Mode.Fast",
                "Fast",
                "App.One",
                "enum-member",
                "One/Mode.cs",
                2,
            ),
            def("App.Two.Mode", "Mode", "App.Two", "enum", "Two/Mode.cs", 1),
            def(
                "App.Two.Mode.Fast",
                "Fast",
                "App.Two",
                "enum-member",
                "Two/Mode.cs",
                2,
            ),
        ],
        vec![],
    );
    let two_root = temp_repo_root("enum-tail-ambiguous");
    write_manifest_fixture(&two_root, &["One/Mode.cs", "Two/Mode.cs"]);
    let two_index = load_graph_index(&two, &two_root);
    assert_eq!(
        resolve_symbol(&two_index, "Mode.Fast"),
        Resolution::Ambiguous(vec!["App.One.Mode.Fast".into(), "App.Two.Mode.Fast".into()])
    );
}
