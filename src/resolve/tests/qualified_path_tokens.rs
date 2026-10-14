use super::*;

// A name written with trivia must resolve exactly as its clean spelling
// does, so every case below is compared against the same source with the
// name spelled plainly: same target, same tier, same edges.

/// The same dotted name written four ways, always in the `App.Widgets`
/// namespace.
fn writings(name: &str) -> Vec<String> {
    vec![
        format!("Widgets . {name}"),
        format!("Widgets./*c*/{name}"),
        format!("Widgets. // c\n    {name}"),
        format!("Widgets.\n    {name}"),
    ]
}

const DECLARATIONS: &[(&str, &str)] = &[
    (
        "Widgets/Widget.cs",
        "namespace App.Widgets { public class Widget { public void Go() { } } }",
    ),
    (
        "Other/Widget.cs",
        "namespace App.Other { public class Widget { public void Go() { } } }",
    ),
    (
        "Widgets/Base.cs",
        "namespace App.Widgets { public class Base { } }",
    ),
];

fn graph_for(declaration: &str) -> Graph {
    let host = format!(
        "using App.Other;\n\nnamespace App;\n\npublic class Host\n{{\n    {declaration}\n\n    \
         public void Run()\n    {{\n        _field.Go();\n    }}\n}}\n"
    );
    let mut files: Vec<(&str, &str)> = DECLARATIONS.to_vec();
    files.push(("App/Host.cs", host.as_str()));
    resolve_graph(&no_git_root(), &fragments_for(&files))
}

fn inherits_targets_from<'a>(g: &'a Graph, from: &str) -> Vec<&'a str> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::Inherits { from_file, to, .. } if from_file == from => Some(to.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn qualified_path_tokens_ac_1_a_trivia_written_receiver_binds_its_written_type() {
    let plain = graph_for("Widgets.Widget _field;");
    let clean = member_edge_targets(&plain);
    assert_eq!(clean, vec!["App.Widgets.Widget"]);
    for written in writings("Widget") {
        let g = graph_for(&format!("{written} _field;"));
        assert_eq!(
            member_edge_targets(&g),
            clean,
            "{written:?} must bind what the clean spelling binds"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_2_a_trivia_written_type_reference_binds_its_written_type() {
    let plain = graph_for("Widgets.Widget _field;");
    let clean = type_edge_targets_from(&plain, "App/Host.cs");
    assert_eq!(clean, vec!["App.Widgets.Widget"]);
    for written in writings("Widget") {
        let g = graph_for(&format!("{written} _field;"));
        assert_eq!(
            type_edge_targets_from(&g, "App/Host.cs"),
            clean,
            "{written:?} must bind what the clean spelling binds"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_2_a_trivia_written_type_argument_binds_its_written_type() {
    let plain = graph_for("List<Widgets.Widget> _items;");
    let clean = type_edge_targets_from(&plain, "App/Host.cs");
    assert_eq!(clean, vec!["App.Widgets.Widget"]);
    for written in writings("Widget") {
        let g = graph_for(&format!("List<{written}> _items;"));
        assert_eq!(
            type_edge_targets_from(&g, "App/Host.cs"),
            clean,
            "{written:?} must bind what the clean spelling binds"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_2_a_trivia_written_base_binds_its_written_type() {
    let base_edges = |written: &str| {
        let source = format!("namespace App;\n\npublic class Derived : {written} {{ }}\n");
        let mut files: Vec<(&str, &str)> = DECLARATIONS.to_vec();
        files.push(("App/Derived.cs", source.as_str()));
        let g = resolve_graph(&no_git_root(), &fragments_for(&files));
        inherits_targets_from(&g, "App/Derived.cs")
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<String>>()
    };

    let clean = base_edges("Widgets.Base");
    assert_eq!(clean, vec!["App.Widgets.Base".to_string()]);
    for written in writings("Base") {
        assert_eq!(
            base_edges(&written),
            clean,
            "{written:?} must bind what the clean spelling binds"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_3_a_trivia_written_using_resolves_like_its_clean_spelling() {
    let host = |directive: &str, reference: &str| {
        format!(
            "{directive}\n\nnamespace App;\n\npublic class Host\n{{\n    private {reference} _w;\n}}\n"
        )
    };
    let resolves_to = |directive: &str, reference: &str| {
        let source = host(directive, reference);
        let files = fragments_for(&[
            (
                "Widgets/Widget.cs",
                "namespace App.Widgets { public class Widget { } }",
            ),
            ("App/Host.cs", source.as_str()),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let targets = type_edge_targets_from(&g, "App/Host.cs");
        assert_eq!(targets, vec!["App.Widgets.Widget"], "{directive:?}");
    };

    resolves_to("using App . Widgets;", "Widget");
    resolves_to("using App.Widgets;", "Widget");
    resolves_to("using W = App . Widgets.Widget;", "W");
    resolves_to("using W = App.Widgets.Widget;", "W");
}

#[test]
fn qualified_path_tokens_ac_2_every_shape_in_one_file_agrees_with_its_clean_spelling() {
    let source = |name: &str| {
        format!(
            "using App.Other;\n\nnamespace App;\n\npublic class Host\n{{\n    private {name} \
             _field;\n    private List<{name}> _items;\n\n    public void \
             Run(IServiceCollection services)\n    {{\n        _field.Go();\n        \
             services.AddScoped<{name}, {name}>();\n    }}\n}}\n\npublic class Derived : {name} {{ }}\n"
        )
    };
    let edges = |name: &str| {
        let source = source(name);
        let mut files: Vec<(&str, &str)> = DECLARATIONS.to_vec();
        files.push(("App/Host.cs", source.as_str()));
        let g = resolve_graph(&no_git_root(), &fragments_for(&files));
        g.edges
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<String>>()
    };

    let clean = edges("Widgets.Widget");
    assert!(
        clean.iter().any(|e| e.contains("App.Widgets.Widget")),
        "the clean spelling resolves to the written type: {clean:?}"
    );
    for written in ["Widgets . Widget", "Widgets./*c*/Widget"] {
        assert_eq!(edges(written), clean, "{written:?}");
    }
}
