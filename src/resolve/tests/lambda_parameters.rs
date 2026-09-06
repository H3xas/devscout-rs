use super::*;

#[test]
fn stage7_lambda_parameter_typed_from_an_action_parameter_of_an_in_graph_callee() {
    // Nothing in Host.cs types `x`: the delegate it fills is declared in
    // ANOTHER file, which is exactly the fact the extractor cannot see
    // and the slot records instead.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Register(Action<Options> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Register(x => x.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("App.Domain.Options", 13)],
        "Register's own Action<Options> parameter says what `x` is, so the lambda body binds \
         precisely"
    );
    assert!(
        heuristic_member_edges_from(&g, "App/Host.cs").is_empty(),
        "a precise hit, not a guess"
    );
}

#[test]
fn stage7_lambda_parameter_typed_from_a_func_and_an_expression_wrapped_func() {
    // Func drops its RETURN type before the lambda's own parameters are
    // read; Expression is a wrapper around the delegate, unwrapped once.
    // The two lambdas sit in SEPARATE methods on purpose: sibling
    // lambdas binding one name to two different callees conflict in the
    // extractor's own fact table, which is a different rule than this
    // one.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public bool Enabled { get; set; } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\nusing System.Linq.Expressions;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Pick(Func<Options, bool> f) { }\n    public void Select(Expression<Func<Options, object>> f) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Pick(o => o.Enabled);\n    }\n\n    public void Project()\n    {\n        _registrar.Select(o => o.Enabled);\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Enabled"),
        vec![("App.Domain.Options", 13), ("App.Domain.Options", 18)],
        "Func<Options,bool> types `o` as Options once the return type is dropped, and \
         Expression<Func<Options,object>> does the same one wrapper further out"
    );
}

#[test]
fn stage7_lambda_parameter_typed_from_an_in_graph_delegate_declaration() {
    // Neither Action nor Func: the parameter names a delegate this graph
    // declares, whose own parameter list is kept under "Invoke".
    let files = fragments_for(&[
        (
            "Domain/Channel.cs",
            "namespace App.Domain { public class Channel { public void Open() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nnamespace App.Domain;\n\npublic delegate void Wiring(Channel channel);\n\npublic class Registrar\n{\n    public void Wire(Wiring w) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Wire(c => c.Open());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Open"),
        vec![("App.Domain.Channel", 13)],
        "Wiring resolves in the file that declared Wire, and its Invoke parameter list types \
         `c` as Channel"
    );
}

#[test]
fn stage7_second_lambda_parameter_is_typed_positionally() {
    // The lambda is argument 1 of 2, and each of ITS OWN parameters
    // reads its own position out of the delegate's list.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Endpoint.cs",
            "namespace App.Domain { public class Endpoint { public void Bind() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Route(string name, Action<Options, Endpoint> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Route(\"main\", (o, e) => e.Bind());\n        _registrar.Route(\"alt\", (o, e) => o.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Bind"),
        vec![("App.Domain.Endpoint", 13)],
        "the SECOND lambda parameter reads the delegate's second argument"
    );
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("App.Domain.Options", 14)],
        "and the first reads the first, from the same slot"
    );
}

#[test]
fn stage7_overloads_disagreeing_on_the_delegate_type_leave_the_lambda_untyped() {
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Endpoint.cs",
            "namespace App.Domain { public class Endpoint { public void Configure() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Attach(Action<Options> a) { }\n    public void Attach(Action<Endpoint> a) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Attach(a => a.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_named(&g, "App/Host.cs", "Configure").is_empty(),
        "both overloads accept the one-argument call and they name different delegate \
         parameter types -- picking either would be a guess, so the site stays as untyped as \
         the extractor found it: {:?}",
        member_edges_named(&g, "App/Host.cs", "Configure")
    );
}

#[test]
fn stage7_overloads_agreeing_on_the_delegate_type_bind() {
    // Three overloads share the name; two of them take the lambda and
    // agree, and the string one cannot take a lambda at all, so it is
    // dropped rather than counted as disagreement.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Tune() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Same(Action<Options> a) { }\n    public void Same(Action<Options> a, bool eager) { }\n    public void Same(string tag) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Same(s => s.Tune());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Tune"),
        vec![("App.Domain.Options", 13)],
        "every overload that could take the lambda names Action<Options>, so there is nothing \
         left to guess at"
    );
}

#[test]
fn stage7_generic_delegate_parameter_yields_no_edge() {
    // Action<T> records its argument as a wildcard: nothing at this call
    // site knows what T is bound to, the same refusal every other
    // wildcard generic-arg fact in this file makes.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Generic<T>(Action<T> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Generic(g => g.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_named(&g, "App/Host.cs", "Configure").is_empty(),
        "the delegate types its parameter with the method's own type parameter, which names \
         nothing: {:?}",
        member_edges_named(&g, "App/Host.cs", "Configure")
    );
    // The wildcard yields NO receiver type rather than a `*` one: a named
    // receiver that resolves to nothing would silence the scored tier,
    // and the site is still an ordinary untyped `g.Configure()` to it.
    assert!(
        heuristic_member_edges_from(&g, "App/Host.cs")
            .iter()
            .any(|(_, line)| *line == 13),
        "the untyped site still reaches the scored tier: {:?}",
        heuristic_member_edges_from(&g, "App/Host.cs")
    );
}

#[test]
fn stage7_overloads_spelling_one_name_for_different_types_leave_the_lambda_untyped() {
    // Both parts of a partial class write `Action<Options>`, but each
    // file imports a different `Options`. The heads agree; what they name
    // does not, so binding to either file's meaning would be a guess.
    let files = fragments_for(&[
        (
            "Alpha/Options.cs",
            "namespace App.Alpha { public class Options { public void Configure() { } } }",
        ),
        (
            "Beta/Options.cs",
            "namespace App.Beta { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Registrar.Alpha.cs",
            "\nusing System;\nusing App.Alpha;\n\nnamespace App.Domain;\n\npublic partial class Registrar\n{\n    public void Attach(Action<Options> a) { }\n}\n",
        ),
        (
            "Domain/Registrar.Beta.cs",
            "\nusing System;\nusing App.Beta;\n\nnamespace App.Domain;\n\npublic partial class Registrar\n{\n    public void Attach(Action<Options> a, bool eager) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Attach(a => a.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_named(&g, "App/Host.cs", "Configure").is_empty(),
        "one spelling, two meanings: {:?}",
        member_edges_named(&g, "App/Host.cs", "Configure")
    );
}

#[test]
fn stage7_extension_callee_with_one_argument_too_many_leaves_the_lambda_untyped() {
    // The extension's list starts with its `this` parameter, so a call
    // that passes more arguments than the overload has left after it
    // cannot be the one the lambda binds to.
    let files = fragments_for(&[
        (
            "Domain/Endpoint.cs",
            "namespace App.Domain { public class Endpoint { public void Bind() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic class Registrar { }\n\npublic static class RegistrarExtensions\n{\n    public static void Extend(this Registrar r, Action<Endpoint> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Extend(p => p.Bind(), true);\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_named(&g, "App/Host.cs", "Bind").is_empty(),
        "two arguments against one non-receiver parameter: {:?}",
        member_edges_named(&g, "App/Host.cs", "Bind")
    );
}

#[test]
fn stage7_external_callee_leaves_the_lambda_untyped() {
    // The owner resolves to nothing in-graph and no extension declares
    // the member either, so there is no parameter list to read at all.
    let files = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    public void Run(IServiceCollection services)\n    {\n        services.AddThing(x => x.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_named(&g, "App/Host.cs", "Configure").is_empty(),
        "an external callee's parameter list is not this graph's to read: {:?}",
        member_edges_named(&g, "App/Host.cs", "Configure")
    );
}

#[test]
fn stage7_bare_call_lambda_is_typed_through_the_enclosing_type_and_its_base() {
    // A bare `M(...)` records the ENCLOSING type as the slot's owner, so
    // the parameter list is looked up there first and then, exactly like
    // any other member lookup, across its in-graph bases.
    let options = (
        "Domain/Options.cs",
        "namespace App.Domain { public class Options { public void Configure() { } } }",
    );
    let inherited = fragments_for(&[
        options,
        (
            "App/HostBase.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class HostBase\n{\n    protected void Register(Action<Options> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host : HostBase\n{\n    public void Run()\n    {\n        Register(y => y.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &inherited);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("App.Domain.Options", 11)],
        "the enclosing type declares no Register of its own, so the base that does supplies \
         the delegate parameter"
    );

    let own = fragments_for(&[
        options,
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    public void Run()\n    {\n        Register(y => y.Configure());\n    }\n\n    private void Register(Action<Options> configure) { }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &own);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("App.Domain.Options", 11)],
        "and a PRIVATE overload the enclosing type declares itself answers just as well -- the \
         parameter table records every method, whatever its visibility"
    );
}

#[test]
fn stage7_extension_callee_types_the_lambda_after_the_this_parameter() {
    // An extension method's own parameter list carries the receiver in
    // position 0, so every argument the SITE wrote sits one place
    // further right.
    let in_graph = fragments_for(&[
        (
            "Domain/Endpoint.cs",
            "namespace App.Domain { public class Endpoint { public void Bind() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "namespace App.Domain { public class Registrar { } }",
        ),
        (
            "Domain/RegistrarExtensions.cs",
            "\nusing System;\n\nnamespace App.Domain;\n\npublic static class RegistrarExtensions\n{\n    public static void Extend(this Registrar r, Action<Endpoint> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Extend(p => p.Bind());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &in_graph);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Bind"),
        vec![("App.Domain.Endpoint", 13)],
        "Registrar declares no Extend of its own, so the extension bucket answers -- and its \
         second parameter, not its first, is the lambda's slot"
    );

    let external = fragments_for(&[
        (
            "Domain/Options.cs",
            "namespace App.Domain { public class Options { public void Configure() { } } }",
        ),
        (
            "Ext/ServiceExtensions.cs",
            "\nusing System;\nusing App.Domain;\n\nnamespace App.Ext;\n\npublic static class ServiceExtensions\n{\n    public static void AddThing(this IServiceCollection s, Action<Options> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing System;\nusing App.Ext;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    public void Run(IServiceCollection services)\n    {\n        services.AddThing(x => x.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &external);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("App.Domain.Options", 11)],
        "an EXTERNAL receiver still names its own bucket key, and the extension declared \
         against it types the lambda from its own file's context"
    );
}

#[test]
fn stage7_lambda_receiver_type_resolves_in_the_declaring_file_context() {
    // Two types share the simple name Options. The callee's file imports
    // one, the site's file imports the other -- and the descriptor is a
    // bare identifier that only means what the file that WROTE it meant.
    let files = fragments_for(&[
        (
            "Alpha/Options.cs",
            "namespace Domain.Alpha { public class Options { public void Configure() { } } }",
        ),
        (
            "Beta/Options.cs",
            "namespace Domain.Beta { public class Options { public void Configure() { } } }",
        ),
        (
            "Domain/Registrar.cs",
            "\nusing System;\nusing Domain.Alpha;\n\nnamespace App.Domain;\n\npublic class Registrar\n{\n    public void Register(Action<Options> configure) { }\n}\n",
        ),
        (
            "App/Host.cs",
            "\nusing App.Domain;\nusing Domain.Beta;\n\nnamespace App.Hosting;\n\npublic class Host\n{\n    private readonly Registrar _registrar;\n\n    public void Run()\n    {\n        _registrar.Register(x => x.Configure());\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_named(&g, "App/Host.cs", "Configure"),
        vec![("Domain.Alpha.Options", 13)],
        "the declaring file imports Domain.Alpha, so that is the Options its parameter names \
         -- resolving the descriptor under the SITE's own usings would answer Domain.Beta"
    );
}
