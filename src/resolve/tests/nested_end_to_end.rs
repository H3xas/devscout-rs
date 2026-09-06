use super::*;

#[test]
fn stage2_end_to_end_static_property_access_emits_and_an_undeclared_member_does_not() {
    let files = fragments_for(&[
        (
            "Other/MessageUrn.cs",
            "namespace App.Consumers { public static class MessageUrn { public static string Prefix { get; } } }",
        ),
        (
            "Consumers/UsesProperty.cs",
            "\nnamespace App.Consumers;\n\npublic class UsesProperty\n{\n  public object Get() => MessageUrn.Prefix;\n  public object Miss() => MessageUrn.NotDeclared;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesProperty.cs"),
        vec![("App.Consumers.MessageUrn", 6)],
        "the declared property emits at its own line; the undeclared member does not"
    );
}

#[test]
fn stage2_end_to_end_a_declared_local_receiver_earns_an_edge_at_the_access_line() {
    let files = fragments_for(&[
        ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
        (
            "Consumers/LocalReceiver.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class LocalReceiver\n{\n  public void Run()\n  {\n    Widget w = new Widget();\n    w.Render();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/LocalReceiver.cs"),
        vec![("App.Other.Widget", 11)]
    );
}

#[test]
fn stage2_end_to_end_a_class_field_receiver_earns_an_edge_the_ctor_injection_shape() {
    let files = fragments_for(&[
        ("Other/IRepo.cs", "namespace App.Other { public interface IRepo { void Save(); } }"),
        (
            "Consumers/FieldReceiver.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class FieldReceiver\n{\n  private readonly IRepo _repo;\n\n  public FieldReceiver(IRepo repo) { _repo = repo; }\n\n  public void Run() { _repo.Save(); }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/FieldReceiver.cs"),
        vec![("App.Other.IRepo", 12)],
        "the field access in Run, not the constructor assignment"
    );
}

#[test]
fn stage2_end_to_end_a_chain_tail_earns_no_edge_from_a_fact_it_did_not_inherit() {
    // Widget declares BOTH Inner and Tail, so if the flattened tail
    // ("w.Inner") had inherited the head's receiverType it would have
    // produced a second, wrong edge. Stage-1 chain-tail regression class.
    let files = fragments_for(&[
        (
            "Other/Widget.cs",
            "namespace App.Other { public class Widget { public object Inner { get; } public void Tail() { } } }",
        ),
        (
            "Consumers/Chain.cs",
            "using App.Other;\n\nnamespace App.Consumers;\n\npublic class Chain\n{\n  public void Run()\n  {\n    Widget w = new Widget();\n    w.Inner.Tail();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Chain.cs"),
        vec![("App.Other.Widget", 10)],
        "only the head access (\"w.Inner\") earns a PRECISE edge, and it is the head's line, not the tail's"
    );
    // End-to-end half of the same split: the tail is guessed at by
    // member-name uniqueness, tagged, on the same line.
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Chain.cs"),
        vec![("App.Other.Widget", 10)],
        "exactly one guess -- the tail; the head already has its fact and is never second-guessed"
    );
}

// --- nested-qualifier chain: the qualifier ladder walking through
// nested types ---
//
// A qualified expression that crosses one or more nesting boundaries
// (`Outer.Inner.Value`, `Outer.Middle.Leaf.Value`) must bind to the LEAF
// def the compiler actually binds -- using the "+"-joined id `type_id`
// (extract.rs) gives every nested type -- never to an outer container
// with the next dotted segment misread as one of ITS members. Real
// fixtures run through this crate's own extractor, matching the tier
// (e) end-to-end tests above.

#[test]
fn end_to_end_two_level_nested_qualifier_binds_the_inner_type_and_its_const() {
    let files = fragments_for(&[
        (
            "Other/Outer.cs",
            "namespace App.Other { public static class Outer { public static class Inner { public const string Value = \"v\"; } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => Outer.Inner.Value;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Outer+Inner", 8)],
        "exactly one precise edge, targeting the nested type -- not Outer"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("Value"))
        }
        _ => unreachable!(),
    }
}

#[test]
fn end_to_end_three_level_nested_qualifier_binds_the_leaf_type_and_its_const() {
    let files = fragments_for(&[
        (
            "Other/Outer.cs",
            "namespace App.Other { public struct Outer { public struct Middle { public struct Leaf { public const string Value = \"v\"; } } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => Outer.Middle.Leaf.Value;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Outer+Middle+Leaf", 8)],
        "the whole chain binds to the LEAF struct, not the outermost container"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("Value"))
        }
        _ => unreachable!(),
    }
}

#[test]
fn end_to_end_nested_enum_member_binds_the_enum_and_not_the_enclosing_type() {
    let files = fragments_for(&[
        (
            "Other/Outer.cs",
            "namespace App.Other { public class Outer { public enum Kind { First, Second } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public object Get() => Outer.Kind.First;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember {
            to,
            to_file,
            member,
            heuristic,
            ..
        } => {
            assert_eq!(to, "App.Other.Outer+Kind.First");
            assert_eq!(to_file, "Other/Outer.cs");
            assert_eq!(member.as_deref(), Some("First"));
            assert!(!heuristic, "the enum member itself is the precise target");
        }
        _ => unreachable!(),
    }
}

#[test]
fn end_to_end_collision_a_namespace_type_must_not_shadow_a_same_named_nested_type() {
    // "Config" names two unrelated defs: a top-level class in namespace
    // Shared.Config, and a class nested in Outer. `Outer.Config.Value`
    // can only ever mean the NESTED one -- "Outer" already names a
    // specific type, so C# never even considers the unrelated
    // namespace's same-named class. The smallest fixture that puts both
    // candidates in the same simple-name pool: this only breaks once
    // the global-uniqueness fallback sees a same-named type ANYWHERE in
    // the corpus and cannot tell the two apart.
    let files = fragments_for(&[
        (
            "Shared/Config.cs",
            "namespace Shared.Config { public class Config { public const string Value = \"ns\"; } }",
        ),
        (
            "Other/Outer.cs",
            "namespace App.Other { public class Outer { public class Config { public const string Value = \"nested\"; } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => Outer.Config.Value;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Outer+Config", 8)],
        "Outer.Config.Value must bind precisely to the NESTED Config -- never guess at the unrelated namespace-level Config"
    );
    assert!(
        !any_member_edge_targets(&g, "Shared.Config.Config"),
        "no edge of any tier may reach the namespace-level Config"
    );
}

#[test]
fn end_to_end_plain_const_on_the_outer_type_still_resolves_precisely() {
    // Control: no nesting at all. Guards the ladder change against
    // regressing the ordinary case.
    let files = fragments_for(&[
        (
            "Other/Outer.cs",
            "namespace App.Other { public static class Outer { public const string Value = \"v\"; } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => Outer.Value;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Outer", 8)]
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("Value"))
        }
        _ => unreachable!(),
    }
}

#[test]
fn end_to_end_namespace_qualified_head_walks_nested_types_to_the_leaf() {
    let files = fragments_for(&[
        (
            "Other/Outer.cs",
            "namespace App.Other { public static class Outer { public static class Middle { public static class Leaf { public const string Value = \"v\"; } } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => App.Other.Outer.Middle.Leaf.Value;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Outer+Middle+Leaf", 6)],
        "a namespace-qualified head walks every nested level to the leaf, with no using at all"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("Value"))
        }
        _ => unreachable!(),
    }
    assert!(
        !any_member_edge_targets(&g, "App.Other.Outer"),
        "no uses-member edge, precise or heuristic, may target the outer container"
    );
}

#[test]
fn end_to_end_repeated_nested_leaf_name_binds_the_named_container() {
    let files = fragments_for(&[
        (
            "Other/Constants.cs",
            "namespace App.Other { public static class Constants { public static class SalesInvoice { public static class FormField { public const string RecId = \"a\"; } } public static class PurchaseOrder { public static class FormField { public const string RecId = \"b\"; } } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => App.Other.Constants.SalesInvoice.FormField.RecId;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Constants+SalesInvoice+FormField", 6)],
        "two same-named FormField leaves under different containers -- the qualifier's own container segment picks the right one"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("RecId"))
        }
        _ => unreachable!(),
    }
    assert!(
        !any_member_edge_targets(&g, "App.Other.Constants"),
        "no uses-member edge, precise or heuristic, may target the outer container"
    );
}

#[test]
fn end_to_end_repeated_nested_leaf_name_binds_through_a_using_head() {
    let files = fragments_for(&[
        (
            "Other/Constants.cs",
            "namespace App.Other { public static class Constants { public static class SalesInvoice { public static class FormField { public const string RecId = \"a\"; } } public static class PurchaseOrder { public static class FormField { public const string RecId = \"b\"; } } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public string Get() => Constants.SalesInvoice.FormField.RecId;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Constants+SalesInvoice+FormField", 8)],
        "the same walk through a using-resolved head instead of a namespace-qualified one"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("RecId"))
        }
        _ => unreachable!(),
    }
    assert!(
        !any_member_edge_targets(&g, "App.Other.Constants"),
        "no uses-member edge, precise or heuristic, may target the outer container"
    );
}

#[test]
fn end_to_end_nested_enum_two_levels_deep_behind_a_namespace_qualified_head() {
    let files = fragments_for(&[
        (
            "Other/Box.cs",
            "namespace App.Other { public static class Box { public static class Inner { public enum Kind { First, Second } } } }",
        ),
        (
            "Consumers/UsesNested.cs",
            "\nnamespace App.Consumers;\n\npublic class UsesNested\n{\n  public object Get() => App.Other.Box.Inner.Kind.First;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesNested.cs"),
        vec![("App.Other.Box+Inner+Kind.First", 6)],
        "the enum sits two nesting levels deep behind a namespace-qualified head"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { member, .. } => {
            assert_eq!(member.as_deref(), Some("First"))
        }
        _ => unreachable!(),
    }
    assert!(
        !any_member_edge_targets(&g, "App.Other.Box"),
        "no uses-member edge, precise or heuristic, may target the outer container"
    );
}

#[test]
fn end_to_end_instance_receiver_named_like_a_type_keeps_its_receiver_typed_edge() {
    // `Settings` is BOTH a field of type JobSettings in the reference
    // site's class and a globally unique type name with a nested `Retry`.
    // The extractor types the field, so the chain is an instance access:
    // the nested walk and the nested-segment suppression must both stand
    // aside and leave the receiver tiers to bind JobSettings.Retry.
    let files = fragments_for(&[
        (
            "Domain/Settings.cs",
            "namespace App.Domain { public class Settings { public class Retry { public const int Max = 3; } } }",
        ),
        (
            "Web/Job.cs",
            "\nnamespace App.Web;\n\npublic class RetryPolicy { public int Max { get; set; } }\npublic class JobSettings { public RetryPolicy Retry { get; set; } }\npublic class Job\n{\n  private readonly JobSettings Settings;\n  public int M() => Settings.Retry.Max;\n}\n",
        ),
    ]);
    // The second window (`Settings.Retry` with member `Max`) is a
    // separate matter: the tail-name fallback in step 4 still answers it
    // by the globally unique `Retry`, which is the qualified-name
    // fallback's own problem and not the nested walk's -- this test pins
    // only the first window and the walk's abstention.
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        !any_member_edge_targets(&g, "App.Domain.Settings"),
        "the field access `Settings.Retry` never binds the same-named type: {:?}",
        member_edges_from(&g, "Web/Job.cs")
    );
    assert!(
        member_edges_from(&g, "Web/Job.cs").contains(&("App.Web.JobSettings", 9)),
        "the receiver-typed edge to JobSettings.Retry survives: {:?}",
        member_edges_from(&g, "Web/Job.cs")
    );
}

// --- tier (f): extension methods ---
//
// Real fixtures run through this crate's own extractor, so an extension fact
// that never gets recorded fails here rather than passing vacuously.
