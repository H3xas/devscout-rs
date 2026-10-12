use super::*;

// C# looks a bare type name up one namespace level at a time, innermost
// first: the level's own types, then the using directives declared at that
// level. Compilation-unit and `global using` directives sit at the global
// level, after every enclosing namespace's types. Every fixture below
// declares at least two same-named types, so the graph-wide unique-name
// fallback can never be the step that answers.

fn member_targets<'a>(g: &'a Graph, from: &str, member: &str) -> Vec<&'a str> {
    member_edges_named(g, from, member)
        .into_iter()
        .map(|(to, _)| to)
        .collect()
}

#[test]
fn a_type_in_the_enclosing_namespace_answers_before_a_using_declared_there() {
    let files = fragments_for(&[
        (
            "Framework/TestInstance.cs",
            "namespace App.Framework.Sagas { public class TestInstance { public int Key { get; set; } } }",
        ),
        (
            "Tests/Container_Specs.cs",
            "namespace App.Tests\n{\n    namespace ContainerTests\n    {\n        using Framework.Sagas;\n\n        public class TestInstance { public int Key { get; set; } }\n\n        public class Spec\n        {\n            TestInstance _instance;\n\n            public int Read(TestInstance x)\n            {\n                return x.Key;\n            }\n        }\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let types = type_edge_targets_from(&g, "Tests/Container_Specs.cs");
    assert!(
        types.contains(&"App.Tests.ContainerTests.TestInstance"),
        "{types:?}"
    );
    assert!(
        !types.contains(&"App.Framework.Sagas.TestInstance"),
        "{types:?}"
    );
    assert_eq!(
        member_targets(&g, "Tests/Container_Specs.cs", "Key"),
        vec!["App.Tests.ContainerTests.TestInstance"]
    );
}

#[test]
fn a_using_declared_inside_a_namespace_answers_before_a_shallower_namespace_type() {
    let files = fragments_for(&[
        ("App/Event.cs", "namespace App { public class Event { } }"),
        (
            "Contracts/Event.cs",
            "namespace App.Contracts { public class Event { } }",
        ),
        (
            "Feature/Handler.cs",
            "namespace App.Feature\n{\n    using App.Contracts;\n\n    public class Handler\n    {\n        Event _event;\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, "Feature/Handler.cs"),
        vec!["App.Contracts.Event"]
    );
}

#[test]
fn an_enclosing_namespace_type_answers_before_a_compilation_unit_using() {
    let files = fragments_for(&[
        ("Lib/Widget.cs", "namespace Lib { public class Widget { } }"),
        ("App/Widget.cs", "namespace App { public class Widget { } }"),
        (
            "Core/Block.cs",
            "using Lib;\n\nnamespace App.Core\n{\n    public class Block\n    {\n        Widget _widget;\n    }\n}\n",
        ),
        (
            "Core/FileScoped.cs",
            "using Lib;\n\nnamespace App.Core;\n\npublic class FileScoped\n{\n    Widget _widget;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, "Core/Block.cs"),
        vec!["App.Widget"]
    );
    assert_eq!(
        type_edge_targets_from(&g, "Core/FileScoped.cs"),
        vec!["App.Widget"]
    );
}

#[test]
fn an_enclosing_namespace_type_answers_before_a_global_using_from_another_file() {
    let files = fragments_for(&[
        ("GlobalUsings.cs", "global using Lib;\n"),
        ("Lib/Widget.cs", "namespace Lib { public class Widget { } }"),
        ("App/Widget.cs", "namespace App { public class Widget { } }"),
        (
            "Core/Block.cs",
            "namespace App.Core\n{\n    public class Block\n    {\n        Widget _widget;\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, "Core/Block.cs"),
        vec!["App.Widget"]
    );
}

#[test]
fn an_inner_level_using_answers_before_an_outer_level_using() {
    let files = fragments_for(&[
        ("X/T.cs", "namespace X { public class T { } }"),
        ("Y/T.cs", "namespace Y { public class T { } }"),
        (
            "A/B/Holder.cs",
            "namespace A\n{\n    using X;\n\n    namespace B\n    {\n        using Y;\n\n        public class Holder\n        {\n            T _t;\n        }\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(type_edge_targets_from(&g, "A/B/Holder.cs"), vec!["Y.T"]);
    assert!(ambiguous_edges_from(&g, "A/B/Holder.cs").is_empty());
}

#[test]
fn two_usings_at_one_level_importing_the_same_name_stay_ambiguous() {
    let files = fragments_for(&[
        ("X/T.cs", "namespace X { public class T { } }"),
        ("Y/T.cs", "namespace Y { public class T { } }"),
        (
            "A/Holder.cs",
            "namespace A\n{\n    using X;\n    using Y;\n\n    public class Holder\n    {\n        T _t;\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(type_edge_targets_from(&g, "A/Holder.cs").is_empty());
    let ambiguous = ambiguous_edges_from(&g, "A/Holder.cs");
    assert_eq!(ambiguous.len(), 1, "{ambiguous:?}");
    assert_eq!(ambiguous[0].2, vec!["X.T", "Y.T"]);
}

#[test]
fn a_using_in_a_sibling_namespace_block_does_not_apply_to_the_site() {
    let files = fragments_for(&[
        ("X/T.cs", "namespace X { public class T { } }"),
        ("Other/T.cs", "namespace Other { public class T { } }"),
        (
            "Pair.cs",
            "namespace A\n{\n    using X;\n}\n\nnamespace B\n{\n    public class Holder\n    {\n        T _t;\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        !type_edge_targets_from(&g, "Pair.cs").contains(&"X.T"),
        "{:?}",
        type_edge_targets_from(&g, "Pair.cs")
    );
}

#[test]
fn a_global_namespace_type_answers_before_a_compilation_unit_using() {
    let files = fragments_for(&[
        ("T.cs", "public class T { }"),
        ("X/T.cs", "namespace X { public class T { } }"),
        (
            "Holder.cs",
            "using X;\n\npublic class Holder\n{\n    T _t;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(type_edge_targets_from(&g, "Holder.cs"), vec!["T"]);
}

#[test]
fn a_cached_plain_using_without_a_namespace_reads_as_compilation_unit_level() {
    let cached: FragUsing =
        serde_json::from_str(r#"{"text":"X","global":false}"#).expect("plain using");
    assert_eq!(
        cached,
        FragUsing::Plain {
            text: "X".into(),
            global: false,
            namespace: String::new(),
        }
    );
    assert_eq!(
        serde_json::to_string(&cached).expect("serializes"),
        r#"{"text":"X","global":false}"#
    );
    let nested = FragUsing::Plain {
        text: "X".into(),
        global: false,
        namespace: "A.B".into(),
    };
    assert_eq!(
        serde_json::to_string(&nested).expect("serializes"),
        r#"{"text":"X","global":false,"namespace":"A.B"}"#
    );
}
