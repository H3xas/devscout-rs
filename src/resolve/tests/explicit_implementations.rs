use super::*;

// An explicit interface implementation is reachable only through a receiver
// typed as the interface. Inside the implementing type, `this.M()`, a
// property hop and a bare name all bind what C# member lookup finds, which
// never includes the explicit declaration.

#[test]
fn this_call_to_an_explicit_only_method_earns_no_precise_edge_to_the_class() {
    let files = fragments_for(&[
        (
            "Kafka/IFactory.cs",
            "namespace App.Kafka { public interface IFactory { void Produce(string topic, Config config, int retries); } }",
        ),
        ("Kafka/Config.cs", "namespace App.Kafka { public class Config { } }"),
        (
            "Kafka/FactoryExtensions.cs",
            "namespace App.Kafka { public static class FactoryExtensions { public static void Produce(this IFactory factory, string topic, Config config, int retries) { } } }",
        ),
        (
            "Kafka/Factory.cs",
            "namespace App.Kafka;\n\npublic class Factory : IFactory\n{\n    void IFactory.Produce(string topic, Config config, int retries) { }\n\n    public void Build()\n    {\n        this.Produce(\"orders\", new Config(), 3);\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Kafka/Factory.cs"),
        vec![],
        "the class's own member lookup never finds its explicit implementation"
    );
}

#[test]
fn this_call_binds_the_base_classs_public_method_beside_an_explicit_one() {
    let files = fragments_for(&[
        (
            "Shapes/IStamp.cs",
            "namespace App.Shapes { public interface IStamp { void Stamp(); } }",
        ),
        (
            "Shapes/Stamper.cs",
            "namespace App.Shapes { public class Stamper { public void Stamp() { } } }",
        ),
        (
            "Shapes/Mixed.cs",
            "namespace App.Shapes;\n\npublic class Mixed : Stamper, IStamp\n{\n    void IStamp.Stamp() { }\n\n    public void Run()\n    {\n        this.Stamp();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Shapes/Mixed.cs"),
        vec![("App.Shapes.Stamper", 9)],
        "member lookup on the class reaches the base's public method"
    );
}

#[test]
fn property_hop_reads_the_public_property_declared_after_an_explicit_one() {
    let files = fragments_for(&[
        (
            "Hosting/IHost.cs",
            "namespace App.Hosting { public interface IHost { void Open(); } }",
        ),
        (
            "Hosting/IMemoryHost.cs",
            "namespace App.Hosting { public interface IMemoryHost : IHost { void Open(int port); } }",
        ),
        (
            "Hosting/IBus.cs",
            "namespace App.Hosting { public interface IBus { IHost Host { get; } } }",
        ),
        (
            "Hosting/MemoryBus.cs",
            "namespace App.Hosting;\n\npublic class MemoryBus : IBus\n{\n    IHost IBus.Host => Host;\n\n    public IMemoryHost Host { get; }\n}\n",
        ),
        (
            "Consumers/Starter.cs",
            "\nusing App.Hosting;\n\nnamespace App.Consumers;\n\npublic class Starter\n{\n    public void Run(MemoryBus bus) => bus.Host.Open(5);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Starter.cs"),
        vec![("App.Hosting.IMemoryHost", 8), ("App.Hosting.MemoryBus", 8)],
        "the hop types `Host` by the public property, the one member lookup finds"
    );
}

#[test]
fn bare_name_of_an_explicit_only_property_names_the_static_class() {
    let files = fragments_for(&[
        (
            "Logging/ILogContext.cs",
            "namespace App.Logging { public interface ILogContext { ILogger Info { get; } } }",
        ),
        (
            "Logging/ILogger.cs",
            "namespace App.Logging { public interface ILogger { void Log(string text); } }",
        ),
        (
            "Logging/LogContext.cs",
            "namespace App.Logging { public static class LogContext { public static ILogger Info { get; } } }",
        ),
        (
            "Logging/ISwitch.cs",
            "namespace App.Logging { public interface ISwitch { ILogContext LogContext { get; } } }",
        ),
        (
            "Logging/Switch.cs",
            "namespace App.Logging;\n\npublic class Switch : ISwitch\n{\n    ILogContext ISwitch.LogContext => null;\n\n    public void Trip()\n    {\n        var logger = LogContext.Info;\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Logging/Switch.cs"),
        vec![("App.Logging.LogContext", 9)],
        "inside the type the bare name is not the explicit property"
    );
}

#[test]
fn an_interface_typed_receiver_still_binds_the_explicitly_implemented_member() {
    let files = fragments_for(&[
        (
            "Shapes/IStamp.cs",
            "namespace App.Shapes { public interface IStamp { void Stamp(); } }",
        ),
        (
            "Shapes/Sealer.cs",
            "namespace App.Shapes { public class Sealer : IStamp { void IStamp.Stamp() { } } }",
        ),
        (
            "Consumers/Press.cs",
            "\nusing App.Shapes;\n\nnamespace App.Consumers;\n\npublic class Press\n{\n    public void Run(IStamp stamp) => stamp.Stamp();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Press.cs"),
        vec![("App.Shapes.IStamp", 8)]
    );
}

#[test]
fn an_ordinary_member_beside_a_same_named_explicit_one_binds_its_own_type() {
    let files = fragments_for(&[
        (
            "Storage/IStore.cs",
            "namespace App.Storage { public interface IStore { void Save(); Widget Current { get; } } }",
        ),
        (
            "Storage/Widget.cs",
            "namespace App.Storage { public class Widget { public void Render() { } } }",
        ),
        (
            "Storage/Store.cs",
            "namespace App.Storage;\n\npublic class Store : IStore\n{\n    void IStore.Save() { }\n    public void Save() { }\n    Widget IStore.Current => Current;\n    public Widget Current { get; }\n\n    public void Flush()\n    {\n        this.Save();\n        Current.Render();\n    }\n}\n",
        ),
        (
            "Consumers/Saver.cs",
            "\nusing App.Storage;\n\nnamespace App.Consumers;\n\npublic class Saver\n{\n    public void Run(Store store) => store.Save();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Storage/Store.cs"),
        vec![("App.Storage.Store", 12), ("App.Storage.Widget", 13)]
    );
    assert_eq!(
        member_edges_from(&g, "Consumers/Saver.cs"),
        vec![("App.Storage.Store", 8)]
    );
}

#[test]
fn a_registered_implementation_pairs_its_explicit_method_with_the_service() {
    let files = fragments_for(&[
        (
            "Jobs/IService.cs",
            "namespace App.Jobs { public interface IService { void Run(); } }",
        ),
        (
            "Jobs/Impl.cs",
            "namespace App.Jobs;\n\npublic class Impl : IService\n{\n    void IService.Run() { }\n}\n",
        ),
        (
            "Jobs/Startup.cs",
            "namespace App.Jobs;\n\npublic class Startup\n{\n    public void Configure()\n    {\n        services.AddScoped<IService, Impl>();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let implements: Vec<(&str, usize, &str, &str, Option<&str>)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::Implements {
                from_file,
                from_line,
                to,
                to_file,
                member,
            } => Some((
                from_file.as_str(),
                *from_line,
                to.as_str(),
                to_file.as_str(),
                member.as_deref(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        implements,
        vec![
            (
                "Jobs/Impl.cs",
                3,
                "App.Jobs.IService",
                "Jobs/IService.cs",
                None
            ),
            (
                "Jobs/Impl.cs",
                3,
                "App.Jobs.IService",
                "Jobs/IService.cs",
                Some("Run")
            ),
        ],
        "the explicit method still satisfies the service member it implements"
    );
}
