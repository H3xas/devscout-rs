use super::*;

const CONTRACT: (&str, &str) = (
    "Contracts/I.cs",
    "namespace App.Contracts { public interface I { void M<T>(); } }",
);

const EXTENSIONS: (&str, &str) = (
    "Ext/Extensions.cs",
    "namespace App.Ext\n{\n  public static class E1 { public static void M<T>(this I x) { } }\n  public static class E2 { public static void M<T, U>(this I x) { } }\n}\n",
);

// The consumer's call sits on line 9.
fn consumer(call: &str) -> String {
    format!(
        "\nusing App.Contracts;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{{\n  public void Run(I x) => {call};\n}}\n"
    )
}

fn graph_for(extra: &[(&str, &str)], call: &str) -> Graph {
    let source = consumer(call);
    let mut files: Vec<(&str, &str)> = extra.to_vec();
    files.push(("Consumers/Runner.cs", source.as_str()));
    resolve_graph(&no_git_root(), &fragments_for(&files))
}

#[test]
fn two_written_type_arguments_skip_the_one_parameter_interface_overload_for_the_matching_extension()
{
    let g = graph_for(&[CONTRACT, EXTENSIONS], "x.M<A, B>()");
    assert!(
        member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "the interface declares M with one type parameter only"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Ext.E2", 9)]
    );
    assert_eq!(
        heuristic_member_tiers_from(&g, "Consumers/Runner.cs"),
        vec![Some(HeuristicTier::Ext)]
    );
}

#[test]
fn one_written_type_argument_still_binds_the_interface_overload() {
    let g = graph_for(&[CONTRACT, EXTENSIONS], "x.M<A>()");
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)]
    );
    assert!(heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty());
}

#[test]
fn a_call_without_a_type_argument_list_keeps_the_value_only_rule() {
    let g = graph_for(&[CONTRACT, EXTENSIONS], "x.M()");
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)]
    );
    assert!(heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty());
}

#[test]
fn one_overload_must_admit_both_the_type_and_the_value_argument_counts() {
    let owner = (
        "Contracts/I.cs",
        "namespace App.Contracts { public class I { public void M<T>(int a) { } public void M<T, U>() { } } }",
    );
    let refused = graph_for(&[owner], "x.M<A, B>(1)");
    assert!(
        member_edges_from(&refused, "Consumers/Runner.cs").is_empty(),
        "M<T>(a) admits the value count and M<T, U>() the type count, but neither admits both"
    );
    assert!(heuristic_member_edges_from(&refused, "Consumers/Runner.cs").is_empty());
    let admitted = graph_for(&[owner], "x.M<A, B>()");
    assert_eq!(
        member_edges_from(&admitted, "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)]
    );
}

#[test]
fn a_this_call_checks_the_count_against_a_private_overload() {
    let files = fragments_for(&[(
        "Registry/Registrar.cs",
        "namespace App.Registry\n{\n  public class Registrar\n  {\n    private void M<T>() { }\n    public void Two() => this.M<A, B>();\n    public void One() => this.M<A>();\n  }\n}\n",
    )]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Registry/Registrar.cs"),
        vec![("App.Registry.Registrar", 7)],
        "only the one-type-argument call binds the private M<T>"
    );
}

#[test]
fn a_base_class_overload_of_another_type_parameter_count_is_skipped_by_the_base_walk() {
    let owner = (
        "Contracts/I.cs",
        "namespace App.Contracts\n{\n  public class Root { public void M<T>() { } }\n  public class I : Root { }\n}\n",
    );
    let refused = graph_for(&[owner], "x.M<A, B>()");
    assert!(member_edges_from(&refused, "Consumers/Runner.cs").is_empty());
    let admitted = graph_for(&[owner], "x.M<A>()");
    assert_eq!(
        member_edges_from(&admitted, "Consumers/Runner.cs"),
        vec![("App.Contracts.Root", 9)]
    );
}

#[test]
fn missing_per_overload_counts_fall_back_to_the_value_only_rule() {
    let source = consumer("x.M<A, B>()");
    let mut files = fragments_for(&[CONTRACT, EXTENSIONS, ("Consumers/Runner.cs", &source)]);
    for (_, fragment) in &mut files {
        for d in &mut fragment.defs {
            d.method_type_arities = crate::graph::OrderedMap::new();
            for e in &mut d.extension_methods {
                e.type_param_count = None;
            }
        }
    }
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)],
        "with no counts recorded the interface binds exactly as before the counts existed"
    );
}

#[test]
fn counts_not_aligned_with_the_value_ranges_fall_back_to_the_value_only_rule() {
    let source = consumer("x.M<A, B>()");
    let mut files = fragments_for(&[CONTRACT, ("Consumers/Runner.cs", &source)]);
    for (_, fragment) in &mut files {
        for d in &mut fragment.defs {
            if d.id == "App.Contracts.I" {
                d.method_type_arities.insert("M".to_string(), vec![1, 2]);
            }
        }
    }
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)]
    );
}

#[test]
fn a_partial_types_parts_keep_each_overloads_counts_paired() {
    let g = |call: &str| {
        graph_for(
            &[
                (
                    "Contracts/IPartOne.cs",
                    "namespace App.Contracts { public partial class I { public void M<T>(int a) { } } }",
                ),
                (
                    "Contracts/IPartTwo.cs",
                    "namespace App.Contracts { public partial class I { public void M<T, U>(int a) { } } }",
                ),
            ],
            call,
        )
    };
    assert_eq!(
        member_edges_from(&g("x.M<A, B>(1)"), "Consumers/Runner.cs"),
        vec![("App.Contracts.I", 9)],
        "the second part's overload shares the first part's value range and must survive the merge"
    );
    assert!(member_edges_from(&g("x.M<A, B, C>(1)"), "Consumers/Runner.cs").is_empty());
}

#[test]
fn one_extension_class_with_overloads_of_both_counts_binds_either_call() {
    let ext = (
        "Ext/Extensions.cs",
        "namespace App.Ext\n{\n  public static class E\n  {\n    public static void M<T>(this I x) { }\n    public static void M<T, U>(this I x) { }\n  }\n}\n",
    );
    let contract = (
        "Contracts/I.cs",
        "namespace App.Contracts { public interface I { } }",
    );
    for call in ["x.M<A>()", "x.M<A, B>()"] {
        let g = graph_for(&[contract, ext], call);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Ext.E", 9)],
            "{call}"
        );
    }
    let g = graph_for(&[contract, ext], "x.M<A, B, C>()");
    assert!(heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty());
}

#[test]
fn receiver_visibility_admits_only_the_extension_of_the_written_count() {
    let files = fragments_for(&[
        (
            "Hidden/Extensions.cs",
            "namespace App.Hidden\n{\n  public static class E1 { public static void M<T>(this IExternal x) { } }\n  public static class E2 { public static void M<T, U>(this IExternal x) { } }\n}\n",
        ),
        (
            "Consumers/Runner.cs",
            "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  public void Run(IExternal x) => x.M<A, B>();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Hidden.E2", 6)],
        "the namespace is not imported, so only the scored tier's receiver rule can name a class"
    );
}
