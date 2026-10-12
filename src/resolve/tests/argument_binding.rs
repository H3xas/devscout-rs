use super::*;

fn target(source: &[(&str, &str)], file: &str) -> Vec<String> {
    let graph = resolve_graph(&no_git_root(), &fragments_for(source));
    graph
        .edges
        .iter()
        .filter_map(|edge| match edge {
            Edge::UsesMember { from_file, to, .. } if from_file == file => Some(to.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn string_argument_passes_a_same_count_class_overload_for_an_extension() {
    let files = [
        ("Domain/Types.cs", "namespace App { public interface IAgent {} public class StopContext {} public class Agent : IAgent { public void Stop(StopContext context) {} } }"),
        ("Ext/Stop.cs", "namespace App { public static class StopExtensions { public static void Stop(this IAgent agent, string reason) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Agent agent) { agent.Stop(\"done\"); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.StopExtensions"]);
}

#[test]
fn a_declared_implicit_conversion_keeps_the_class_overload() {
    let files = [
        ("Domain/Types.cs", "namespace App { public interface IAgent {} public class StopContext { public static implicit operator StopContext(string value) => new StopContext(); } public class Agent : IAgent { public void Stop(StopContext context) {} } }"),
        ("Ext/Stop.cs", "namespace App { public static class StopExtensions { public static void Stop(this IAgent agent, string reason) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Agent agent) { agent.Stop(\"done\"); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.Agent"]);
}

#[test]
fn the_base_walk_reaches_a_sibling_interface_with_the_accepted_argument() {
    let files = [
        ("Domain/Types.cs", "namespace App { public interface IAlphaBuilder {} public interface IBetaBuilder {} public interface IAlpha { void Apply(IAlphaBuilder builder); } public interface IBeta { void Apply(IBetaBuilder builder); } public interface ICombined : IAlpha, IBeta {} }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(ICombined receiver, IBetaBuilder builder) { receiver.Apply(builder); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.IBeta"]);
}

#[test]
fn a_known_member_return_reaches_the_base_overload() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Base { public void ScheduleSend(TimeSpan delay) {} } public class Derived : Base { public void ScheduleSend(DateTime due) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Derived d) { d.ScheduleSend(TimeSpan.FromSeconds(1)); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.Base"]);
}

#[test]
fn instantiated_invariant_parameter_declines_the_different_generic_shape() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class ConsumeContext {} public class ConsumeContext<T> {} public interface IPipeSpecification<T> {} public class BaseSender { public void AddPipeSpecification(IPipeSpecification<ConsumeContext> pipe) {} } public class Sender<T> : BaseSender { public void AddPipeSpecification(IPipeSpecification<ConsumeContext<T>> pipe) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Sender<string> sender, IPipeSpecification<ConsumeContext> pipe) { sender.AddPipeSpecification(pipe); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.BaseSender"]);
}

#[test]
fn nested_receiver_argument_with_lost_inner_binding_stays_admissible() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class Inner<T> {} public interface IPipeBuilder<T> {} public interface IBox<T> { void Apply(IPipeBuilder<T> builder); } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(IBox<Inner<string>> box, IPipeBuilder<Inner<string>> builder) { box.Apply(builder); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.IBox"]);
}

#[test]
fn known_timespan_cannot_satisfy_uri_parameter_and_reaches_extension() {
    let files = [
        ("Domain/Types.cs", "namespace App { public interface IScheduler { void ScheduleSend(Uri destination); } public static class ScheduleExtensions { public static void ScheduleSend(this IScheduler scheduler, TimeSpan delay) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(IScheduler scheduler) { scheduler.ScheduleSend(TimeSpan.FromSeconds(1)); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.ScheduleExtensions"]);
}

#[test]
fn an_in_graph_type_named_timespan_wins_over_the_fixed_library_member() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class TimeSpan { public static string FromSeconds(int value) => \"done\"; } public class Receiver { public void Stop(TimeSpan value) {} } public static class StopExtensions { public static void Stop(this Receiver receiver, string value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver) { receiver.Stop(TimeSpan.FromSeconds(1)); } } }"),
    ];
    assert_eq!(
        target(&files, "Use/Run.cs"),
        vec!["App.StopExtensions", "App.TimeSpan"]
    );
}

#[test]
fn zero_numeric_literal_keeps_a_possible_enum_conversion() {
    let files = [
        ("Domain/Types.cs", "namespace App { public enum State { Zero } public class Receiver { public void Set(State state) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver) { receiver.Set(0); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.Receiver"]);
}

#[test]
fn property_field_and_method_result_types_are_materialized_for_calls() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class StopContext {} public class Box<T> { public T Value { get; } public string Field; public string Get() => Field; } public class Receiver { public void Stop(StopContext value) {} } public static class StopExtensions { public static void Stop(this Receiver receiver, string value) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Receiver receiver, Box<string> box) {\n receiver.Stop(box.Value);\n receiver.Stop(box.Field);\n receiver.Stop(box.Get());\n } } }"),
    ];
    let graph = resolve_graph(&no_git_root(), &fragments_for(&files));
    let stop_targets: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|edge| match edge {
            Edge::UsesMember {
                from_file,
                member,
                to,
                ..
            } if from_file == "Use/Run.cs" && member.as_deref() == Some("Stop") => {
                Some(to.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(stop_targets, vec!["App.StopExtensions"; 3]);
}

#[test]
fn unknown_and_null_arguments_keep_the_declaring_type() {
    let files = [
        ("Domain/Types.cs", "namespace App { public class StopContext {} public class Agent { public void Stop(StopContext context) {} } }"),
        ("Use/Run.cs", "namespace App { public class Runner { public void Run(Agent agent) { agent.Stop(missing); agent.Stop(null); } } }"),
    ];
    assert_eq!(target(&files, "Use/Run.cs"), vec!["App.Agent", "App.Agent"]);
}
