use super::*;

const DELEGATES: (&str, &str) = (
    "Lists/Delegates.cs",
    "namespace App.Lists\n{\n  public delegate bool FilterDelegate<in TContext>(TContext context);\n  public delegate void V(int x);\n}\n",
);

// `S` declares a void-delegate overload and two generic overloads whose own
// type parameter only an explicit type-argument list can fix; its base
// declares the overload C# binds for an implicitly typed filter lambda.
const LISTS: (&str, &str) = (
    "Lists/Lists.cs",
    r#"namespace App.Lists
{
  public interface IX<T> { }
  public class E { }
  public class F { }
  public class A<TElement>
  {
    public Task<bool> Any(FilterDelegate<TElement> f, CancellationToken ct = default) => null;
  }
  public class S : A<E>
  {
    public Task<bool> Any(Action<F> apply = default, CancellationToken ct = default) => null;
    public Task<bool> Any<T>(CancellationToken ct = default) => null;
    public Task<bool> Any<T>(FilterDelegate<IX<T>> f, CancellationToken ct = default) => null;
    public void Self() => this.Any<E>(m => true);
  }
}
"#,
);

// The consumer's call sits on line 9.
fn consumer(params: &str, call: &str) -> String {
    format!(
        "\nusing App.Lists;\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class Runner\n{{\n  public void Run({params}) => {call};\n}}\n"
    )
}

fn graph_for(extra: &[(&str, &str)], params: &str, call: &str) -> Graph {
    let source = consumer(params, call);
    let mut files: Vec<(&str, &str)> = vec![DELEGATES, LISTS];
    files.extend_from_slice(extra);
    files.push(("Consumers/Runner.cs", source.as_str()));
    resolve_graph(&no_git_root(), &fragments_for(&files))
}

fn precise(g: &Graph) -> Vec<(&str, usize)> {
    member_edges_from(g, "Consumers/Runner.cs")
}

#[test]
fn a_value_only_filter_skips_the_void_and_uninferable_overloads_for_the_bases() {
    let g = graph_for(
        &[],
        "S s",
        "s.Any(m => m switch { E e => true, _ => false })",
    );
    assert_eq!(precise(&g), vec![("App.Lists.A", 9)]);
}

#[test]
fn a_statement_body_still_binds_the_void_delegate_overload() {
    let g = graph_for(&[], "S s", "s.Any(m => m.M())");
    assert_eq!(precise(&g), vec![("App.Lists.S", 9)]);
}

#[test]
fn a_written_type_argument_list_still_binds_the_generic_overload() {
    let g = graph_for(&[], "S s", "s.Any<E>(m => true)");
    assert_eq!(precise(&g), vec![("App.Lists.S", 9)]);
    let this_shaped: Vec<(&str, usize)> = member_edges_from(&g, "Lists/Lists.cs")
        .into_iter()
        .filter(|(_, line)| *line == 15)
        .collect();
    assert_eq!(this_shaped, vec![("App.Lists.S", 15)]);
}

#[test]
fn an_explicitly_typed_lambda_still_binds_the_generic_overload() {
    let g = graph_for(&[], "S s", "s.Any((IX<E> m) => true)");
    assert_eq!(precise(&g), vec![("App.Lists.S", 9)]);
}

#[test]
fn a_type_parameter_another_argument_or_the_return_can_fix_keeps_the_overload() {
    let other = (
        "Other/G.cs",
        "namespace App.Other\n{\n  public class G1 { public void M<T>(T seed, Func<T, bool> f) { } }\n  public class G2 { public void M<T>(Func<int, T> f) { } }\n}\n",
    );
    let seeded = graph_for(&[other], "G1 g", "g.M(1, x => true)");
    assert_eq!(precise(&seeded), vec![("App.Other.G1", 9)]);
    let returned = graph_for(&[other], "G2 g", "g.M(x => 1)");
    assert_eq!(precise(&returned), vec![("App.Other.G2", 9)]);
}

#[test]
fn an_in_graph_void_delegate_refuses_a_value_only_body() {
    let other = (
        "Other/H.cs",
        "namespace App.Other\n{\n  public class H { public void Run(V v) { } }\n}\n",
    );
    let refused = graph_for(&[other], "H h", "h.Run(x => 1)");
    assert!(precise(&refused).is_empty());
    let statement = graph_for(&[other], "H h", "h.Run(x => x.M())");
    assert_eq!(precise(&statement), vec![("App.Other.H", 9)]);
}

#[test]
fn an_in_graph_delegate_without_a_signature_fact_admits() {
    let other = (
        "Other/H.cs",
        "namespace App.Other\n{\n  public class H { public void Run(V v) { } }\n}\n",
    );
    let source = consumer("H h", "h.Run(x => 1)");
    let mut fragments = fragments_for(&[DELEGATES, LISTS, other, ("Consumers/Runner.cs", &source)]);
    for def in &mut fragments[0].1.defs {
        def.delegate_signature = None;
    }
    let g = resolve_graph(&no_git_root(), &fragments);
    assert_eq!(precise(&g), vec![("App.Other.H", 9)]);
}

#[test]
fn every_lambda_position_must_pass_on_the_same_overload() {
    let other = (
        "Other/K.cs",
        r#"namespace App.Other
{
  public class K
  {
    public void Two(Action<int> a, Func<int, bool> f) { }
    public void Two(Func<int, bool> f, Action<int> a) { }
    public void Arity(Action<int> a, Action<int, int> b) { }
    public void Arity(Action<int, int> a, Action<int> b) { }
  }
}
"#,
    );
    let shapes = graph_for(&[other], "K k", "k.Two(x => true, y => true)");
    assert!(precise(&shapes).is_empty());
    let arities = graph_for(&[other], "K k", "k.Arity(x => x.M(), y => y.M())");
    assert!(precise(&arities).is_empty());
    let admitted = graph_for(&[other], "K k", "k.Two(x => x.M(), y => true)");
    assert_eq!(precise(&admitted), vec![("App.Other.K", 9)]);
}

#[test]
fn a_call_without_a_lambda_argument_keeps_its_binding() {
    let g = graph_for(&[], "S s, FilterDelegate<IX<E>> f", "s.Any(f)");
    assert_eq!(precise(&g), vec![("App.Lists.S", 9)]);
}

// Precise `Sel` edges only: the hop's own `h.P` read earns an edge of its own.
fn precise_sel(g: &Graph) -> Vec<&str> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic: false,
                ..
            } if from_file == "Consumers/Runner.cs" && member.as_deref() == Some("Sel") => {
                Some(to.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_property_hop_refuses_an_overload_the_lambda_cannot_convert_to() {
    let hop = |sel: &str| {
        (
            "Other/Holder.cs",
            format!(
                "using App.Lists;\n\nnamespace App.Other\n{{\n  public class Holder {{ public Pt P {{ get; }} }}\n  public class Pt\n  {{\n{sel}\n  }}\n}}\n"
            ),
        )
    };
    let refusing = hop(
        "    public void Sel(Action<F> a) { }\n    public void Sel<T>(FilterDelegate<IX<T>> f) { }",
    );
    let files = [(refusing.0, refusing.1.as_str())];
    let g = graph_for(&files, "Holder h", "h.P.Sel(_ => true)");
    assert!(precise_sel(&g).is_empty());
    let written = graph_for(&files, "Holder h", "h.P.Sel<E>(_ => true)");
    assert_eq!(precise_sel(&written), vec!["App.Other.Pt"]);
    let admitting = hop("    public void Sel(Func<IX<E>, bool> f) { }");
    let g = graph_for(
        &[(admitting.0, admitting.1.as_str())],
        "Holder h",
        "h.P.Sel(_ => true)",
    );
    assert_eq!(precise_sel(&g), vec!["App.Other.Pt"]);
}
