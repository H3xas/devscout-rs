use super::*;

#[test]
fn stage3_generic_concrete_this_args_must_match_the_receivers_concrete_args() {
    let files = fragments_for(&[
        (
            "Other/Types.cs",
            "namespace App.Other\n{\n  public class IDictionary<TKey, TValue> { }\n  public class IMessageDeserializer { }\n}\n",
        ),
        (
            "Ext/DictExtensions.cs",
            "namespace App.Ext { public static class DictExtensions { public static void TryGetValue(this IDictionary<string, object> d, int k) { } } }",
        ),
        (
            "Consumers/WrongArgs.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class WrongArgs\n{\n  public void Run(IDictionary<string, IMessageDeserializer> d) => d.TryGetValue(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/WrongArgs.cs").is_empty(),
        "the base name and arity both match -- only the type ARGUMENTS disagree, which is exactly the wrong edge the corpus audit found"
    );
}

#[test]
fn stage3_generic_exactly_matching_concrete_this_args_earn_the_edge() {
    let files = fragments_for(&[
        ("Other/Types.cs", "namespace App.Other { public class IDictionary<TKey, TValue> { } }"),
        (
            "Ext/DictExtensions.cs",
            "namespace App.Ext { public static class DictExtensions { public static void TryGetValue(this IDictionary<string, object> d, int k) { } } }",
        ),
        (
            "Consumers/RightArgs.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class RightArgs\n{\n  public void Run(IDictionary<string, object> d) => d.TryGetValue(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/RightArgs.cs"),
        vec![("App.Ext.DictExtensions", 9)]
    );
}

#[test]
fn stage3_generic_a_wildcard_this_arg_unifies_with_an_unbound_method_type_parameter() {
    let files = fragments_for(&[
        (
            "Other/Types.cs",
            "namespace App.Other\n{\n  public class EventPipelineBinder<TSaga, TData> { }\n  public class FutureState { }\n}\n",
        ),
        (
            "Ext/BinderExtensions.cs",
            "namespace App.Ext\n{\n  public static class BinderExtensions\n  {\n    public static void Then<TSaga, TData>(this EventPipelineBinder<TSaga, TData> b, int a) { }\n  }\n}\n",
        ),
        (
            "Consumers/Wildcards.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Wildcards\n{\n  public void Run<T>(EventPipelineBinder<FutureState, T> b) => b.Then(1);\n}\n",
        ),
    ]);
    let ext = &files
        .iter()
        .find(|(rel, _)| rel == "Ext/BinderExtensions.cs")
        .expect("ext fragment")
        .1;
    let d = ext
        .defs
        .iter()
        .find(|d| d.id == "App.Ext.BinderExtensions")
        .expect("BinderExtensions def present");
    assert_eq!(
        d.extension_methods[0].this_args,
        Some(vec!["*".to_string(), "*".to_string()]),
        "the extension's own type parameters are wildcards"
    );
    let consumer = &files
        .iter()
        .find(|(rel, _)| rel == "Consumers/Wildcards.cs")
        .expect("consumer fragment")
        .1;
    let r = consumer
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Then"))
        .expect("Then ref present");
    assert_eq!(
        r.receiver_args,
        Some(vec!["FutureState".to_string(), "*".to_string()]),
        "the enclosing method's own type parameter is a wildcard on the receiver side too"
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Wildcards.cs"),
        vec![("App.Ext.BinderExtensions", 9)]
    );
}

#[test]
fn stage3_generic_a_non_generic_receiver_never_binds_a_generic_this_parameter() {
    let files = fragments_for(&[
        ("Other/Types.cs", "namespace App.Other\n{\n  public class Box<T> { }\n  public class Widget { }\n}\n"),
        (
            "Ext/BoxExtensions.cs",
            "namespace App.Ext { public static class BoxExtensions { public static void Open(this Box<Widget> b) { } } }",
        ),
        (
            "Consumers/Bare.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Bare\n{\n  public void Run(Box b) => b.Open();\n}\n",
        ),
    ]);
    let consumer = &files
        .iter()
        .find(|(rel, _)| rel == "Consumers/Bare.cs")
        .expect("consumer fragment")
        .1;
    let r = consumer
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Open"))
        .expect("Open ref present");
    assert_eq!(r.receiver_type.as_deref(), Some("Box"));
    assert_eq!(
        r.receiver_args, None,
        "a non-generic declared type records no args at all"
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Bare.cs").is_empty(),
        "generic on one side and not the other is a mismatch, never a wildcard"
    );
}

// --- the scored heuristic tier ---
//
// Real fixtures run through this crate's own extractor, so a fact that never
// gets recorded fails here rather than passing vacuously.
