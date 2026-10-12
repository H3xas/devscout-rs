use super::*;

const TOPOLOGY_BASE: &str = "namespace App.Config\n{\n    public interface ITopology { }\n    public interface IInMemoryTopology : ITopology { }\n    public interface IEndpointConfiguration { ITopology Topology { get; } }\n    public interface IBusConfiguration : IEndpointConfiguration { }\n}\n";

#[test]
fn property_read_through_an_interface_binds_the_redeclaration_that_hides_the_base_property() {
    let files = fragments_for(&[
        ("Config/Base.cs", TOPOLOGY_BASE),
        (
            "Config/InMemory.cs",
            "namespace App.Config\n{\n    public interface IInMemoryEndpointConfiguration : IEndpointConfiguration\n    {\n        new IInMemoryTopology Topology { get; }\n    }\n    public interface IInMemoryBusConfiguration : IBusConfiguration, IInMemoryEndpointConfiguration { }\n}\n",
        ),
        (
            "Consumers/Factory.cs",
            "\nusing App.Config;\n\nnamespace App.Consumers;\n\npublic class Factory\n{\n    readonly IInMemoryBusConfiguration _cfg;\n\n    public ITopology Run() => _cfg.Topology;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Factory.cs"),
        vec![("App.Config.IInMemoryEndpointConfiguration", 10)],
        "IEndpointConfiguration.Topology is reached first through IBusConfiguration, but it is \
         declared in a base of IInMemoryEndpointConfiguration, whose own Topology property hides \
         it, so member lookup binds the redeclaration"
    );
}

#[test]
fn a_redeclared_property_hides_the_base_property_without_the_new_modifier() {
    let files = fragments_for(&[
        ("Config/Base.cs", TOPOLOGY_BASE),
        (
            "Config/InMemory.cs",
            "namespace App.Config\n{\n    public interface IInMemoryEndpointConfiguration : IEndpointConfiguration\n    {\n        IInMemoryTopology Topology { get; }\n    }\n    public interface IInMemoryBusConfiguration : IBusConfiguration, IInMemoryEndpointConfiguration { }\n}\n",
        ),
        (
            "Consumers/Factory.cs",
            "\nusing App.Config;\n\nnamespace App.Consumers;\n\npublic class Factory\n{\n    public ITopology Run(IInMemoryBusConfiguration cfg) => cfg.Topology;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Factory.cs"),
        vec![("App.Config.IInMemoryEndpointConfiguration", 8)],
        "hiding follows from the redeclaration alone; `new` only silences the compiler warning"
    );
}

#[test]
fn a_hider_deeper_than_the_shortest_path_to_the_hidden_property_still_wins() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IRoot { int P { get; } }\n    public interface IMid : IRoot { new int P { get; } }\n    public interface IStep : IMid { }\n    public interface IA : IRoot { }\n    public interface IB : IStep { }\n    public interface IX : IA, IB { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public int Read(IX x) => x.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.IMid", 8)],
        "IRoot is two levels up and IMid three, but IRoot is a base of IMid, which redeclares P: \
         hiding is decided by the base relation, never by depth"
    );
}

#[test]
fn a_generic_interface_redeclaring_a_property_hides_the_base_property() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IBase { object P { get; } }\n    public interface IDerived<T> : IBase { new T P { get; } }\n    public interface IOther : IBase { }\n    public interface IRecv : IOther, IDerived<int> { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public int Read(IRecv r) => r.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.IDerived", 8)],
        "a constructed generic base resolves to its declaration, and that declaration's P hides \
         IBase.P"
    );
}

#[test]
fn unrelated_interfaces_declaring_the_same_property_keep_the_first_in_base_order() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IA { int P { get; } }\n    public interface IB { int P { get; } }\n    public interface IX : IA, IB { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public int Read(IX x) => x.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.IA", 8)],
        "neither declaration hides the other, so the first base in declaration order still answers"
    );
}

#[test]
fn a_call_through_a_diamond_ignores_a_more_derived_overload() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IRoot { void M(int value); }\n    public interface IA : IRoot { }\n    public interface IB : IRoot { void M(string value); }\n    public interface IX : IA, IB { }\n}\n",
        ),
        (
            "Consumers/Caller.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Caller\n{\n    public void Run(IX x) => x.M(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Caller.cs"),
        vec![("App.Model.IRoot", 8)],
        "a call keeps the first declaring base: dropping IRoot.M(int) would need the argument \
         types to show IB.M(string) applies"
    );
}

#[test]
fn a_method_redeclaration_does_not_hide_a_base_property_on_a_read() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IRoot { int P { get; } }\n    public interface IA : IRoot { }\n    public interface IB : IRoot { new int P(); }\n    public interface IX : IA, IB { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public object Read(IX x) => x.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.IRoot", 8)],
        "only a property or field redeclaration hides a read; a method named P leaves IRoot.P in \
         place"
    );
}

#[test]
fn a_class_receiver_binds_the_nearest_class_that_redeclares_the_property() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public class Root { public int P { get; } }\n    public class Mid : Root { public new int P { get; } }\n    public class Leaf : Mid { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public int Read(Leaf leaf) => leaf.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.Mid", 8)],
        "a class chain has a single path, so its first declaring class is already the nearest"
    );
}

#[test]
fn a_cyclic_interface_hierarchy_keeps_the_first_declaring_base() {
    let files = fragments_for(&[
        (
            "Model/Shapes.cs",
            "namespace App.Model\n{\n    public interface IA : IB { int P { get; } }\n    public interface IB : IA { int P { get; } }\n    public interface IX : IA { }\n}\n",
        ),
        (
            "Consumers/Reader.cs",
            "\nusing App.Model;\n\nnamespace App.Consumers;\n\npublic class Reader\n{\n    public int Read(IX x) => x.P;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Reader.cs"),
        vec![("App.Model.IA", 8)],
        "when a cycle makes every declaration hide every other, the lookup falls back to the \
         first declaring base instead of dropping them all"
    );
}
