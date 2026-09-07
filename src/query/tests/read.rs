use super::*;

#[test]
fn read_excludes_references_inside_the_target_declaration_span() {
    let root = temp_repo_root("read-self-inbound");
    fs::create_dir_all(root.join("Core")).unwrap();
    fs::create_dir_all(root.join("Consumers")).unwrap();
    fs::write(
        root.join("Core/Widget.cs"),
        "namespace App;\npublic class Widget\n{\n    Widget Again() => new Widget();\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Consumers/Reader.cs"),
        "namespace App;\npublic class Reader { Widget Value; }\n",
    )
    .unwrap();
    write_manifest_fixture(&root, &["Core/Widget.cs", "Consumers/Reader.cs"]);
    let mut target = def("App.Widget", "Widget", "App", "class", "Core/Widget.cs", 2);
    target.end_line = 5;
    let graph = make_graph(
        vec![target],
        vec![
            uses_type("Core/Widget.cs", 4, "App.Widget", "Core/Widget.cs"),
            uses_type("Consumers/Reader.cs", 2, "App.Widget", "Core/Widget.cs"),
        ],
    );
    let index = load_graph_index(&graph, &root);
    let ReadResult::Resolved(model) = build_read_model(&index, "Widget") else {
        panic!("Widget must resolve")
    };
    assert_eq!(model.refs.inbound.uses_type.total, 1);
    assert_eq!(
        model.refs.inbound.uses_type.rows[0].file,
        "Consumers/Reader.cs"
    );
}
