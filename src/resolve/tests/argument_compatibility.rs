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

#[test]
fn method_owned_type_parameter_does_not_take_the_receiver_binding() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Customer {} public class Cache<TValue> { public TValue Get<TKey>(TKey key) => default; public void Put<TKey>(TKey key) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Cache<Order> cache) { cache.Get(\"k\"); cache.Put(new Customer()); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Cache"; 2]);
}

#[test]
fn primitive_clr_names_keep_identity_and_implicit_numeric_conversions() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Envelope<T> {} public class Receiver { public void Delay(System.Int64 ms) {} public void Scale(Double factor) {} public void Code(Int32 value) {} public void Save(Envelope<int> value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver r, Int32 ms, Envelope<System.Int32> e) { r.Delay(100); r.Scale(2); r.Code('a'); r.Delay(ms); r.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"; 5]);
}

#[test]
fn receiver_and_method_type_parameters_stay_separate() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Customer {} public class Cache<TValue> { public void Pair<TKey>(TValue value, TKey key) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Cache<Order> cache) { cache.Pair(new Order(), new Customer()); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Cache"]);
}

#[test]
fn known_arguments_still_admit_dynamic_nullable_and_unknown_external_pairs() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Receiver { public void Flexible(dynamic value) {} public void Maybe(int? value) {} public void External(System.IO.Stream value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver r, System.Text.StringBuilder value) { r.Flexible(\"x\"); r.Maybe(5); r.External(value); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"; 3]);
}
