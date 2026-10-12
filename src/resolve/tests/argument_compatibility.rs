use super::*;

fn targets(source: &[(&str, &str)]) -> Vec<String> {
    let graph = resolve_graph(&no_git_root(), &fragments_for(source));
    graph
        .edges
        .iter()
        .filter_map(|edge| match edge {
            Edge::UsesMember { from_file, to, .. } if from_file == "Use/Run.cs" => Some(to.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn derived_numeric_nullable_and_null_arguments_stay_accepted() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Base {} public class Child : Base {} public class Receiver { public void Use(Base value) {} public void Count(long value) {} public void Maybe(int? value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver, Child child) { receiver.Use(child); receiver.Count(1); receiver.Maybe(null); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"; 3]);
}

#[test]
fn object_dynamic_unknown_and_params_shapes_do_not_veto_a_method() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Receiver { public void Any(object value) {} public void Flexible(dynamic value) {} public void Many(params string[] values) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver) { receiver.Any(new object()); receiver.Flexible(missing); receiver.Many(\"a\", \"b\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"; 3]);
}

#[test]
fn covariant_generic_parameter_keeps_a_possible_conversion() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Base {} public class Child : Base {} public interface IBox<out T> {} public class Receiver { public void Use(IBox<Base> value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver, IBox<Child> box) { receiver.Use(box); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"]);
}
