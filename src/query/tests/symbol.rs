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

// --- the `Type::item` path of a Rust item ---

fn rust_path_index_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "src/shelf.rs#Gadget",
                "Gadget",
                "src/shelf.rs",
                "struct",
                "src/shelf.rs",
                1,
            ),
            def(
                "src/shelf.rs#Gadget::fmt",
                "fmt",
                "src/shelf.rs#Gadget",
                "fn",
                "src/shelf.rs",
                3,
            ),
            def(
                "src/shelf.rs#Gadget::fmt-1",
                "fmt",
                "src/shelf.rs#Gadget",
                "fn",
                "src/shelf.rs",
                6,
            ),
            def(
                "src/cabinet.rs#cabinet::Hinge::oil",
                "oil",
                "src/cabinet.rs#cabinet::Hinge",
                "fn",
                "src/cabinet.rs",
                9,
            ),
            def(
                "we#b/x.rs#Tool::run",
                "run",
                "we#b/x.rs#Tool",
                "fn",
                "we#b/x.rs",
                2,
            ),
        ],
        vec![],
    )
}

#[test]
fn a_rust_path_resolves_by_its_tail_and_a_shared_one_is_ambiguous() {
    let graph = rust_path_index_graph();
    let root = temp_repo_root("rust-path");
    write_manifest_fixture(&root, &["src/shelf.rs", "src/cabinet.rs", "we#b/x.rs"]);
    let index = load_graph_index(&graph, &root);

    assert_eq!(
        resolve_symbol(&index, "Gadget::fmt"),
        Resolution::Ambiguous(vec![
            "src/shelf.rs#Gadget::fmt".into(),
            "src/shelf.rs#Gadget::fmt-1".into()
        ])
    );
    assert_eq!(
        resolve_symbol(&index, "Gadget::fmt-1"),
        Resolution::Resolved("src/shelf.rs#Gadget::fmt-1".into())
    );
    assert_eq!(
        resolve_symbol(&index, "src/shelf.rs#Gadget::fmt-1"),
        Resolution::Resolved("src/shelf.rs#Gadget::fmt-1".into())
    );
    assert_eq!(
        resolve_symbol(&index, "cabinet::Hinge::oil"),
        Resolution::Resolved("src/cabinet.rs#cabinet::Hinge::oil".into())
    );
    assert_eq!(
        resolve_symbol(&index, "Hinge::oil"),
        Resolution::Resolved("src/cabinet.rs#cabinet::Hinge::oil".into())
    );
    assert_eq!(
        resolve_symbol(&index, "Tool::run"),
        Resolution::Resolved("we#b/x.rs#Tool::run".into()),
        "only the part after the last `#` is the path"
    );
    assert_eq!(
        resolve_symbol(&index, "dget::fmt"),
        Resolution::NotFound,
        "a tail must start at a segment boundary"
    );
    assert_eq!(resolve_symbol(&index, "Gadget::oil"), Resolution::NotFound);
}

#[test]
fn a_colon_path_never_answers_for_csharp_or_typescript_ids() {
    let graph = make_graph(
        vec![
            def(
                "App.Shop.Widget",
                "Widget",
                "App.Shop",
                "class",
                "Shop/Widget.cs",
                1,
            ),
            def(
                "App.Shop.Widget.Save",
                "Save",
                "App.Shop",
                "enum-member",
                "Shop/Widget.cs",
                2,
            ),
            def(
                "web/a.ts#render",
                "render",
                "web/a.ts",
                "function",
                "web/a.ts",
                1,
            ),
        ],
        vec![],
    );
    let root = temp_repo_root("colon-path-cs-ts");
    write_manifest_fixture(&root, &["Shop/Widget.cs", "web/a.ts"]);
    let index = load_graph_index(&graph, &root);
    for query in [
        "Shop::Widget",
        "App::Shop::Widget",
        "Widget::Save",
        "a.ts::render",
        "web/a.ts::render",
    ] {
        assert_eq!(
            resolve_symbol(&index, query),
            Resolution::NotFound,
            "{query}"
        );
    }
}
