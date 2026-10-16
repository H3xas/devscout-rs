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

// --- spelling-equivalent generic type arguments ---------------------------

#[test]
fn generic_argument_spelling_ac_1_a_parameter_alias_admits_the_written_argument() {
    let files = [
        ("Domain/Types.cs", "using O = App.Order; namespace App { public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<O> e) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Svc s, Envelope<Order> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_1_an_alias_on_the_argument_side_admits() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<Order> e) {} } }"),
        ("Use/Run.cs", "using O = App.Order; namespace App { public class Runner { public void Run(Svc s, Envelope<O> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_1_two_alias_names_for_one_type_admit() {
    let files = [
        ("Domain/Types.cs", "using A = App.Order; namespace App { public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<A> e) {} } }"),
        ("Use/Run.cs", "using B = App.Order; namespace App { public class Runner { public void Run(Svc s, Envelope<B> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_1_a_namespace_level_alias_admits() {
    let files = [
        ("Domain/Types.cs", "namespace App { using O = App.Order; public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<O> e) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Svc s, Envelope<Order> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_1_a_global_alias_from_another_file_admits() {
    let files = [
        ("Domain/Global.cs", "global using O = App.Order;"),
        ("Domain/Types.cs", "namespace App { public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<O> e) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Svc s, Envelope<Order> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_1_aliases_of_primitive_and_bcl_types_admit() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Envelope<T> {} public class Svc { public void SaveLong(Envelope<long> e) {} public void SaveString(Envelope<string> e) {} } }"),
        ("Use/Run.cs", "using Millis = System.Int64; using Text = System.String; namespace App { public class Runner { public void Run(Svc s, Envelope<Millis> ms, Envelope<Text> text) { s.SaveLong(ms); s.SaveString(text); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"; 2]);
}

#[test]
fn generic_argument_spelling_ac_1_an_alias_of_a_closed_generic_admits() {
    let files = [
        ("Domain/Types.cs", "using System.Collections.Generic; namespace App { public class Order {} public class Envelope<T> {} public class Svc { public void Save(Envelope<List<Order>> e) {} } }"),
        ("Use/Run.cs", "using Orders = System.Collections.Generic.List<App.Order>; namespace App { public class Runner { public void Run(Svc s, Envelope<Orders> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

#[test]
fn generic_argument_spelling_ac_2_nullable_spellings_admit() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Envelope<T> {} public class Svc { public void SaveNullable(Envelope<System.Nullable<int>> e) {} public void SaveQuestion(Envelope<int?> e) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Svc s, Envelope<int?> q, Envelope<System.Nullable<int>> n) { s.SaveNullable(q); s.SaveQuestion(n); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"; 2]);
}

#[test]
fn generic_argument_spelling_ac_2_object_and_dynamic_spellings_admit() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Envelope<T> {} public class Svc { public void SaveObject(Envelope<object> e) {} public void SaveDynamic(Envelope<dynamic> e) {} public void SaveQualified(Envelope<System.Object> e) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Svc s, Envelope<dynamic> d, Envelope<object> o, Envelope<dynamic> q) { s.SaveObject(d); s.SaveDynamic(o); s.SaveQualified(q); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"; 3]);
}

#[test]
fn generic_argument_spelling_ac_3_positions_depths_arrays_and_placeholder_admit() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Bag<T> {} public class Envelope<T> {} public class Pair<TFirst, TSecond> {} public class Svc { public void SaveBag(Envelope<Bag<Order>> e) {} public void SaveDynamicBag(Envelope<Bag<object>> e) {} public void SaveArray(Envelope<Order[]> e) {} public void SaveTuple(Envelope<System.ValueTuple<int, int>> e) {} public void SavePair(Pair<Order, object> p) {} } }"),
        ("Use/Run.cs", "using O = App.Order; namespace App { public class Runner { public void Run(Svc s, Envelope<Bag<O>> bag, Envelope<Bag<dynamic>> dynamicBag, Envelope<O[]> array, Envelope<(int, int)> tuple, Pair<O, dynamic> pair) { s.SaveBag(bag); s.SaveDynamicBag(dynamicBag); s.SaveArray(array); s.SaveTuple(tuple); s.SavePair(pair); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"; 5]);
}

#[test]
fn generic_argument_spelling_ac_4_distinct_spellings_bind_the_other_overload() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Customer {} public class Envelope<T> {} public class Svc { public void Save(Envelope<Customer> e) {} } }"),
        ("Ext/Save.cs", "namespace App { public static class SvcExtensions { public static void Save(this Svc s, Envelope<Order> e) {} } }"),
        ("Use/Run.cs", "using O = App.Order; namespace App { public class Runner { public void Run(Svc s, Envelope<O> e) { s.Save(e); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.SvcExtensions"]);
}

#[test]
fn generic_argument_spelling_ac_4_distinct_type_arguments_still_decline() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Order {} public class Customer {} public class Bag<T> {} public class Envelope<T> {} public class Pair<TFirst, TSecond> {} public class Svc { public void SaveNullable(Envelope<System.Nullable<int>> e) {} public void SaveObject(Envelope<object> e) {} public void SaveBag(Envelope<Bag<Order>> e) {} public void SavePair(Pair<long, Order> p) {} public void SaveOrder(Envelope<Customer> e) {} } }"),
        ("Use/Run.cs", "using O = App.Order; namespace App { public class Runner { public void Run(Svc s, Envelope<int?> ok, Envelope<long> wide, Envelope<string> text, Envelope<Bag<Customer>> bag, Pair<int, Order> pair, Envelope<O> aliased) { s.SaveNullable(wide); s.SaveObject(text); s.SaveBag(bag); s.SavePair(pair); s.SaveOrder(aliased); s.SaveNullable(ok); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Svc"]);
}

// --- interpolated string handler parameters -------------------------------

#[test]
fn interpolated_handler_ac_1_the_mark_merges_across_partial_declarations() {
    let files = [
        ("Domain/Handlers.cs", "namespace App { public partial struct LogHandler { public LogHandler(int literalLength, int formattedCount) {} } }"),
        ("Domain/Handlers.Marked.cs", "namespace App { [InterpolatedStringHandler] public partial struct LogHandler {} }"),
        ("Domain/Logger.cs", "namespace App { public class Logger { public void Log(LogHandler handler) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger, int n) { logger.Log($\"count {n}\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Logger"]);
}

#[test]
fn interpolated_handler_ac_3_a_marked_handler_parameter_takes_an_interpolated_argument() {
    let files = [
        ("Domain/Logging.cs", "namespace App { [System.Runtime.CompilerServices.InterpolatedStringHandler] public ref struct LogHandler { public LogHandler(int literalLength, int formattedCount) {} } public class Logger { public void Log(LogHandler handler) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger, int n) { logger.Log($\"count {n}\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Logger"]);
}

#[test]
fn interpolated_handler_ac_3_a_modified_handler_after_a_leading_parameter_binds() {
    let files = [
        ("Domain/Check.cs", "namespace App { [InterpolatedStringHandler] public struct CheckHandler {} public class Checker { public void Check(bool strict, ref CheckHandler handler) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Checker checker, bool strict) { checker.Check(strict, $\"v{strict}\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Checker"]);
}

#[test]
fn interpolated_handler_ac_3_a_nested_handler_type_binds_its_declaring_type() {
    let files = [
        ("Domain/Outer.cs", "namespace App { public class Outer { [InterpolatedStringHandler] public struct Nested { } public void Send(Nested handler) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Outer outer) { outer.Send($\"n\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Outer"]);
}

#[test]
fn interpolated_handler_ac_3_a_base_declared_handler_overload_binds_on_a_derived_receiver() {
    let files = [
        ("Domain/Base.cs", "namespace App { [InterpolatedStringHandler] public struct LogHandler {} public class BaseLogger { public void Log(LogHandler handler) {} } }"),
        ("Domain/Derived.cs", "namespace App { public class Logger : BaseLogger {} }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger) { logger.Log($\"d\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.BaseLogger"]);
}

#[test]
fn interpolated_handler_ac_3_an_extension_method_taking_a_handler_binds_its_class() {
    let files = [
        ("Domain/Log.cs", "namespace App { [InterpolatedStringHandler] public struct LogHandler {} public class Logger { } public static class LogExtensions { public static void Log(this Logger logger, LogHandler handler) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger) { logger.Log($\"x\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.LogExtensions"]);
}

#[test]
fn interpolated_handler_ac_4_plain_strings_against_a_marked_handler_keep_the_extension() {
    let files = [
        ("Domain/Log.cs", "namespace App { [InterpolatedStringHandler] public struct LogHandler {} public class Logger { public void Log(LogHandler handler) {} } public static class LogExtensions { public static void Log(this Logger logger, string message) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger, string message) {\n logger.Log(\"plain\");\n logger.Log(message);\n } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.LogExtensions"; 2]);
}

#[test]
fn interpolated_handler_ac_4_interpolated_arguments_against_unmarked_and_fixed_types_keep_the_extension(
) {
    let files = [
        ("Domain/Types.cs", "namespace App { public class StopContext {} public class Receiver { public void Stop(StopContext context) {} public void Count(int value) {} public void Delay(TimeSpan due) {} } public static class ReceiverExtensions { public static void Stop(this Receiver receiver, string reason) {} public static void Count(this Receiver receiver, string value) {} public static void Delay(this Receiver receiver, string value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver) { receiver.Stop($\"a\"); receiver.Count($\"b\"); receiver.Delay($\"c\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.ReceiverExtensions"; 3]);
}

#[test]
fn interpolated_handler_ac_4_string_object_formattable_and_external_parameters_keep_the_declaring_type(
) {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Receiver { public void Text(string value) {} public void Any(object value) {} public void Format(IFormattable value) {} public void Formattable(FormattableString value) {} public void External(System.Text.StringBuilder value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver) { receiver.Text($\"a\"); receiver.Any($\"b\"); receiver.Format($\"c\"); receiver.Formattable($\"d\"); receiver.External($\"e\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Receiver"; 5]);
}

#[test]
fn interpolated_handler_ac_4_a_declared_string_conversion_keeps_the_handler_overload() {
    let files = [
        ("Domain/Log.cs", "namespace App { [InterpolatedStringHandler] public struct LogHandler { public static implicit operator LogHandler(string value) => default; } public class Logger { public void Log(LogHandler handler) {} } public static class LogExtensions { public static void Log(this Logger logger, string message) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Logger logger) { logger.Log(\"done\"); } } }"),
    ];
    assert_eq!(targets(&files), vec!["App.Logger"]);
}
