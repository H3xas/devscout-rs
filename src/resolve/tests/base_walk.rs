use super::*;

#[test]
fn stage7_base_member_that_is_protected_resolves_to_the_base_that_declares_it() {
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected void Touch() { }\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke() => base.Touch();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Domain/Order.cs"),
        vec![("App.Domain.Base", 6)],
        "Touch is protected -- absent from Base's public `methods` list, present only in \
         `nonPublicMethods` -- but a `base.` site is by construction inside the hierarchy it \
         is walking, so base_member_declared reads any-visibility and resolves precisely \
         anyway"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage7_base_lookup_skips_interface_bases() {
    let files = fragments_for(&[
        (
            "Domain/IGreeter.cs",
            "namespace App.Domain { public interface IGreeter { void Greet(); } }",
        ),
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base { public void Greet() { } } }",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : IGreeter, Base\n{\n    public void Greet() { }\n\n    public void Poke() => base.Greet();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Domain/Order.cs"),
        vec![("App.Domain.Base", 8)],
        "IGreeter is listed FIRST in Order's base list and also declares Greet, but base. \
         never names an interface member -- base_member_declared skips it (and never walks \
         its own closure) and continues to Base, the class, which is the right target. \
         Order's own override (also named Greet) is never even considered, matching the \
         existing non-interface base test."
    );
}

#[test]
fn stage7_extension_declared_on_an_interface_binds_through_the_receivers_base_closure() {
    let files = fragments_for(&[
        (
            "Domain/ISpecification.cs",
            "namespace App.Domain { public interface ISpecification { } }",
        ),
        (
            "Domain/BatchOptions.cs",
            "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class BatchOptions : ISpecification\n{\n    public void Validate() => this.Fail();\n}\n",
        ),
        (
            "Ext/SpecExtensions.cs",
            "namespace App.Ext { public static class SpecExtensions { public static void Fail(this ISpecification spec) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(BatchOptions opts) => opts.Fail();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    // The `this` receiver: BatchOptions itself declares nothing named
    // Fail, and typed_receiver_base_member's own base walk skips
    // ISpecification (an interface, per its own rule) and finds nothing
    // either -- so the exact-key lookup at tier (f) misses ("Fail
    // BatchOptions" names no bucket) and the closure fallback tries
    // BatchOptions's raw base string "ISpecification" next, which the
    // extension actually keys on.
    assert_eq!(
        heuristic_member_edges_from(&g, "Domain/BatchOptions.cs"),
        vec![("App.Ext.SpecExtensions", 8)],
        "this.Fail() binds through BatchOptions's OWN raw base string, tried as a fallback key \
         once the exact receiver-type key misses"
    );
    // The ordinary LOCAL receiver: `opts` is a ref with no this. shape at
    // all (its enclosing type is Runner, not BatchOptions), proving the
    // fallback is not `this.`-specific.
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Ext.SpecExtensions", 9)],
        "opts.Fail() -- an ordinary typed local, not this. -- binds through the exact same \
         closure fallback, which applies to every typed receiver, not just the this. shape"
    );
    assert_eq!(g.stats.heuristic_by_tier.ext, 2);
}

#[test]
fn stage7_typed_receiver_member_declared_on_an_in_graph_base_resolves_to_the_base() {
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base { public void Touch() { } } }",
        ),
        (
            "Domain/Order.cs",
            "namespace App.Domain { public class Order : Base { } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Order o) => o.Touch();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Base", 8)],
        "Order itself declares nothing named Touch -- the typed-receiver precise tier \
         (previously exact-def-only) now walks Order's in-graph base closure \
         and binds to Base, the first def that declares it. `o` is an ordinary parameter, not \
         `this.`, so only the public list is consulted -- proven sufficient here since Touch \
         is public."
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "a precise hit, not a guess"
    );
}

#[test]
fn stage7_a_scored_guess_never_vouches_through_a_non_public_member() {
    // Two same-named, same-shaped classes in different namespaces (no
    // using imports either), so `_widget`'s declared type "Widget"
    // resolves AMBIGUOUS -- the scored tier's ambiguous pool, filtered by
    // `member_vouched`. Alpha.Widget declares Ping publicly; Beta.Widget
    // declares the SAME name but only privately.
    let files = fragments_for(&[
        (
            "Alpha/Widget.cs",
            "namespace Fixture.Alpha { public class Widget { public void Ping() { } } }",
        ),
        (
            "Beta/Widget.cs",
            "namespace Fixture.Beta { public class Widget { private void Ping() { } } }",
        ),
        (
            "App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Widget _widget;\n\n  public void Run() => _widget.Ping();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "App/Runner.cs"),
        vec![("Fixture.Alpha.Widget", 8)],
        "Beta.Widget also declares Ping, but only NON-publicly -- member_vouched (and the \
         Call-shape `declares_member` it reads) is untouched by the non-public member tables, \
         so Beta.Widget never enters the scored guess at all, even though it sits right in the \
         ambiguous pool this ref's receiver resolved to; only Alpha.Widget, which declares \
         Ping publicly, vouches"
    );
    assert_eq!(g.stats.heuristic_by_tier.guess, 1);
}

// --- Base-walk declaration order (+ interface skip at any
// depth) and arity-aware call vouching -----------------------------

#[test]
fn stage7_base_walk_visits_class_bases_in_declaration_order_before_any_interface() {
    // Endpoint : BaseEndpoint (a single class base). BaseEndpoint's OWN
    // base list names its class base FIRST, an interface SECOND --
    // BasePipe declares Go directly; IEndpoint reaches Go only through
    // ITS OWN base, IPipe, two levels down. A walk that visits siblings
    // in REVERSE declaration order (a LIFO stack popping the
    // last-pushed base first) would explore IEndpoint's entire closure
    // -- and find IPipe's Go -- before ever touching BasePipe, which is
    // the correct C# answer.
    let files = fragments_for(&[
        (
            "Domain/IPipe.cs",
            "namespace App.Domain { public interface IPipe { void Go(); } }",
        ),
        (
            "Domain/IEndpoint.cs",
            "namespace App.Domain { public interface IEndpoint : IPipe { } }",
        ),
        (
            "Domain/BasePipe.cs",
            "namespace App.Domain { public class BasePipe { public void Go() { } } }",
        ),
        (
            "Domain/BaseEndpoint.cs",
            "namespace App.Domain { public class BaseEndpoint : BasePipe, IEndpoint { } }",
        ),
        (
            "Domain/Endpoint.cs",
            "namespace App.Domain { public class Endpoint : BaseEndpoint { } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Endpoint ep) => ep.Go();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.BasePipe", 8)],
        "BasePipe, BaseEndpoint's FIRST base, wins over IEndpoint's (SECOND base) own \
         interface closure -- declaration order, not stack order"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "a precise hit, not a guess"
    );
}

#[test]
fn stage7_class_typed_receiver_never_binds_to_an_interface_declaration_at_any_depth() {
    // Touch is declared ONLY on IHasTouch, an interface reached
    // TRANSITIVELY through Base's own base list -- not a direct base of
    // the class-typed receiver Derived at all (Derived -> Base ->
    // IHasTouch, two levels down). Base implements IHasTouch but
    // declares no override of its own, and Derived adds nothing either.
    let files = fragments_for(&[
        (
            "Domain/IHasTouch.cs",
            "namespace App.Domain { public interface IHasTouch { void Touch(); } }",
        ),
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base : IHasTouch { } }",
        ),
        (
            "Domain/Derived.cs",
            "namespace App.Domain { public class Derived : Base { } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Derived d) => d.Touch();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "the base walk must skip IHasTouch at EVERY depth it is reached, not only when it is \
         Derived's own direct base -- an interface's member declaration is a contract, never a \
         precise bind target, for a class-typed receiver"
    );
    assert!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "no extension method named Touch exists anywhere in this fixture either, so this is a \
         silent miss, not a guess"
    );
}

#[test]
fn stage7_a_call_whose_arity_matches_no_instance_overload_falls_through_to_the_extension_tier() {
    // Widget.Stop takes exactly one argument; the call passes two. No
    // overload admits it, so the precise tier must decline -- and tier
    // (f)'s own veto, reading the SAME arity-aware `declares_member`,
    // must decline too, letting the two-argument extension bind.
    let files = fragments_for(&[
        (
            "Domain/Widget.cs",
            "namespace App.Domain { public class Widget { public void Stop(int a) { } } }",
        ),
        (
            "Ext/WidgetExt.cs",
            "using App.Domain;\n\nnamespace App.Ext { public static class WidgetExt { public static void Stop(this Widget w, int a, int b) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Widget w) => w.Stop(1, 2);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "Widget declares Stop, but only a ONE-argument overload -- the call passes two \
         arguments, which no overload admits, so the precise tier must not claim the ref"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Ext.WidgetExt", 9)],
        "the arity mismatch on the instance side also un-vetoes tier (f): Stop(this Widget w, \
         int a, int b) admits two arguments and is the only candidate"
    );
}

#[test]
fn stage7_a_call_admitted_by_a_params_or_optional_overload_binds_to_the_instance_member() {
    let files = fragments_for(&[
        (
            "Domain/Widget.cs",
            "namespace App.Domain { public class Widget { public void Send(int a, int b = 0) { } public void Spray(params int[] xs) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void RunOptional(Widget w) => w.Send(1);\n    public void RunParams(Widget w) => w.Spray(1, 2, 3, 4, 5);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Widget", 8), ("App.Domain.Widget", 9)],
        "Send(1) falls inside the OPTIONAL-parameter overload's (1, 2) range, and Spray(1, 2, \
         3, 4, 5) falls inside the `params` overload's unbounded (0, -1) range -- both admit \
         the call, so both bind precisely to the instance member"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "two precise hits, no guess"
    );
}

#[test]
fn stage7_partial_class_overloads_declared_in_sibling_files_both_admit_their_calls() {
    // One partial class, one overload set, split across two files. The
    // merged arity table has to hold BOTH overloads: either call is a
    // call the type accepts, and the file that cannot see the other
    // part's declaration is exactly the file that needs the merge.
    let files = fragments_for(&[
        (
            "Domain/Svc.cs",
            "\nnamespace App.Domain;\n\npublic partial class Svc\n{\n    public void Run() { }\n\n    public void First() { this.Run(1); }\n}\n",
        ),
        (
            "Domain/Svc.More.cs",
            "\nnamespace App.Domain;\n\npublic partial class Svc\n{\n    public void Run(int n) { }\n\n    public void Second() { this.Run(); }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Domain/Svc.cs"),
        vec![("App.Domain.Svc", 8)],
        "this.Run(1) is admitted by the ONE-argument overload the SIBLING file declares --              merging the arity ranges per name is what lets the first-declaring part's              zero-argument range stop hiding it"
    );
    assert_eq!(
        member_edges_from(&g, "Domain/Svc.More.cs"),
        vec![("App.Domain.Svc", 8)],
        "and the zero-argument call keeps binding from the other direction -- the merge is a              union, so neither part's overload set is lost"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "two precise hits, no guess"
    );
}

#[test]
fn stage7_a_read_of_a_property_is_still_name_only() {
    let files = fragments_for(&[
        (
            "Domain/Sensor.cs",
            "namespace App.Domain { public class Sensor { public string Label { get; } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public string Run(Sensor s) => s.Label;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Sensor", 8)],
        "s.Label is a READ (no argCount at all) -- declares_member's arg_count == None branch \
         is untouched by the arity gate, so a property still resolves precisely on \
         name alone, exactly as before"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

// --- Chain-tail hop failures, and closure-key generic
// unification against the MATCHED base's own arguments ------------------

#[test]
fn stage7_base_qualified_chain_tail_hops_through_the_base_method_return() {
    // Use hides BaseC.Make with a `new` declaration returning a
    // different type. `base.Make()` calls the BASE's Make, so the tail
    // is an Order; `this.Make()` calls Use's own, so the tail is a
    // Widget. Both heads type as the enclosing type -- only the ref's
    // base marker separates them.
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "Domain/Widget.cs",
            "\nnamespace App.Domain;\n\npublic class Widget\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "Domain/BaseC.cs",
            "\nnamespace App.Domain;\n\npublic class BaseC\n{\n    public virtual Order Make() { return null; }\n}\n",
        ),
        (
            "Domain/Use.cs",
            "\nnamespace App.Domain;\n\npublic class Use : BaseC\n{\n    public new Widget Make() { return null; }\n\n    public void Run()\n    {\n        base.Make().Validate();\n    }\n\n    public void RunThis()\n    {\n        this.Make().Validate();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let validate: Vec<(&str, usize)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                member,
                heuristic: false,
                ..
            } if from_file == "Domain/Use.cs" && member.as_deref() == Some("Validate") => {
                Some((to.as_str(), *from_line))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        validate,
        vec![("App.Domain.Order", 10), ("App.Domain.Widget", 15)],
        "base.Make() reads its return type off the first in-graph base that declares Make, \
         never off the enclosing type that hides it -- and this.Make() still reads the \
         enclosing type's own"
    );
}

#[test]
fn stage7_a_chain_tail_whose_hop_fails_emits_no_guess() {
    // `Unknown` names no in-graph def at all, so the chain tail's own
    // method-return hop cannot even resolve an OWNER, let alone a
    // return type: `receiver_type_name` stays `None`. `Order.Validate`
    // is the only in-graph def vouching for the member name "Validate"
    // -- exactly the sole candidate a scored guess drawn from the raw,
    // receiver-blind name-uniqueness pool would land on, since nothing
    // can filter that pool by receiver when there IS no receiver type
    // at all.
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "App/Worker.cs",
            "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public void Run()\n    {\n        Unknown.Load().Validate();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let validate_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        validate_edges,
        Vec::<(&str, bool)>::new(),
        "Unknown.Load() never resolves an owner in-graph at all -- the hop yields NO receiver \
         type, in-graph or otherwise -- so the chain tail `.Validate()` is finished as external \
         right there: without this guard it would fall into the scored tier's unfiltered \
         name-uniqueness pool and guess App.Domain.Order, the sole in-graph Validate"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage7_a_chain_tail_whose_hop_lands_on_an_external_type_is_silent() {
    // `Repo.Load()` DOES resolve an in-graph owner and DOES have a
    // recorded return type -- "ExternalWidget" -- so the hop is not the
    // empty case the guard above catches: `receiver_type_name` is
    // `Some("ExternalWidget")`, exactly like a `Q.M()` local's own call
    // hop, and this ref keeps walking the ordinary typed-receiver path
    // rather than being force-silenced. "ExternalWidget" is declared
    // NOWHERE in this fixture, so that path itself comes up empty on
    // its own: tier (f) finds no "Validate ExternalWidget" extension
    // bucket, and the scored tier's own receiver rule
    // (`receiver_admits_candidate`) correctly refuses the one same-named
    // candidate (App.Domain.Order, which declares Validate) because
    // Order is nominally assignable to nothing named "ExternalWidget" --
    // no base, no name match. The observable result is the same silence
    // a resolved-but-external chain-tail owner requires, produced by
    // the EXISTING filters rather than a new one: a chain tail with a
    // real but external target type is still an answerable receiver,
    // just one this corpus proves nothing about here.
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "Infra/Repo.cs",
            "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static ExternalWidget Load() => null;\n}\n",
        ),
        (
            "App/Worker.cs",
            "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public void Run()\n    {\n        Repo.Load().Validate();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let validate_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        validate_edges,
        Vec::<(&str, bool)>::new(),
        "the hop lands the receiver on \"ExternalWidget\", a real but external type name -- no \
         extension binds it and App.Domain.Order (the only in-graph Validate) is not \
         assignable to it, so the ref is silent"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage7_extension_on_an_implemented_interface_binds_for_a_generic_enclosing_type() {
    // BatchOptions<T> is GENERIC (unlike the earlier non-generic
    // BatchOptions fixture), so `this.Fail()`'s receiver_args is
    // `Some(["*"])` -- BatchOptions's own type parameter, wildcarded.
    // ISpecification is written into BatchOptions's base list with NO
    // type-argument list at all (it is not generic), so
    // `base_generic_args` records no entry for it at all. Before this
    // fix, filter 3 compared SpecExtensions's `this_args` (`None`
    // -- Fail's `this ISpecification` is non-generic) against the
    // RECEIVER's own `Some(["*"])`, a hard (None, Some) mismatch that
    // dropped the edge; the fix compares against the matched base's own
    // arguments (`None`, since ISpecification carries none), which
    // unify with a non-generic `this` regardless of BatchOptions's own
    // arity.
    let files = fragments_for(&[
        (
            "Domain/ISpecification.cs",
            "namespace App.Domain { public interface ISpecification { } }",
        ),
        (
            "Domain/BatchOptions.cs",
            "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class BatchOptions<T> : ISpecification\n{\n    public void Validate() => this.Fail();\n}\n",
        ),
        (
            "Ext/SpecExtensions.cs",
            "namespace App.Ext { public static class SpecExtensions { public static void Fail(this ISpecification spec) { } } }",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Domain/BatchOptions.cs"),
        vec![("App.Ext.SpecExtensions", 8)],
        "this.Fail() binds through BatchOptions's OWN raw base string \"ISpecification\", with \
         the non-generic base's OWN (absent) arguments unifying against Fail's non-generic \
         this-parameter -- BatchOptions's own generic arity never enters the comparison"
    );
    assert_eq!(g.stats.heuristic_by_tier.ext, 1);
}

#[test]
fn stage7_extension_unification_uses_the_matched_base_arguments() {
    // Repository<TKey, TValue> implements IRepository<TValue> -- ONE of
    // its own two type parameters, not both -- so `base_generic_args`
    // records IRepository's own arity as a SINGLE wildcard
    // (`Some(["*"])`), one element shorter than the receiver's own
    // `receiver_args` (`Some(["*", "*"])`, both of Repository's own type
    // parameters). RepoExtensions.Validate<T>(this IRepository<T> repo)
    // is generic too, so `this_args` is also a single wildcard
    // (`Some(["*"])`). Unifying against the RECEIVER's own two-element
    // arguments (the old behaviour) is a length mismatch that
    // drops the edge; unifying against the matched base's own
    // one-element arguments -- what the fix wires -- matches.
    let files = fragments_for(&[
        (
            "Domain/IRepository.cs",
            "namespace App.Domain { public interface IRepository<T> { } }",
        ),
        (
            "Domain/Repository.cs",
            "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class Repository<TKey, TValue> : IRepository<TValue>\n{\n    public void Poke() => this.Validate();\n}\n",
        ),
        (
            "Ext/RepoExtensions.cs",
            "namespace App.Ext { public static class RepoExtensions { public static void Validate<T>(this IRepository<T> repo) { } } }",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Domain/Repository.cs"),
        vec![("App.Ext.RepoExtensions", 8)],
        "this.Validate() binds through IRepository, unifying Validate's own single wildcard \
         this-argument against IRepository's own single wildcard argument AS Repository \
         DECLARED IT (\"IRepository<TValue>\") -- not against Repository's own two-argument \
         receiver_args, which would fail the length check"
    );
    assert_eq!(g.stats.heuristic_by_tier.ext, 1);
}

// --- Stage 8: declaring type along the base and interface direction ----
//
// The compiler binds a member to the type that DECLARES it in the
// receiver's static chain. Three shapes that used to fall short of that
// rule, each through real C# (`fragments_for`), plus the class-receiver
// control that keeps the interface half of the rule from widening.

#[test]
fn stage8_qualified_static_qualifier_binds_the_base_that_declares_the_member() {
    // `App.Domain.Derived.Create()` names the derived type through an
    // exact qualified name; Create is declared only on Base. The
    // exact-qualified certainty hatch used to bind Derived itself; the
    // declaring base wins now, the same answer a bare `Derived.Create()`
    // already gives.
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base { public static Base Create() => new Base(); } }",
        ),
        (
            "Domain/Derived.cs",
            "namespace App.Domain { public class Derived : Base { } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run()\n    {\n        var a = App.Domain.Derived.Create();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Base", 8)],
        "a qualified static qualifier binds the base that declares the member, not the \
         derived type the source names"
    );
}

#[test]
fn stage8_generic_static_qualifier_binds_the_base_that_declares_the_member() {
    // `Derived<int>.Create()`: the type-argument list marks the
    // qualifier as a type with certainty, and Create is declared only on
    // the non-generic Base. The certainty hatch used to bind Derived;
    // the declaring base wins now.
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base { public static Base Create() => new Base(); } }",
        ),
        (
            "Domain/Derived.cs",
            "namespace App.Domain { public class Derived<T> : Base { public T Item = default!; } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run()\n    {\n        var a = Derived<int>.Create();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Base", 10)],
        "a generic static qualifier binds the base that declares the member"
    );
}

#[test]
fn stage8_certainty_hatches_still_bind_the_named_type_when_no_base_declares_the_member() {
    // The control for the two tests above: Derived has an in-graph base
    // that does NOT declare Helper (it lives on an external base, or on
    // nothing this graph can see), so both hatches keep today's answer
    // -- the named type on type certainty alone.
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "namespace App.Domain { public class Base { } }",
        ),
        (
            "Domain/Derived.cs",
            "namespace App.Domain { public class Derived<T> : Base { public T Item = default!; } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run()\n    {\n        var a = Derived<int>.Helper();\n        var b = App.Domain.Derived<int>.Helper();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Domain.Derived", 10), ("App.Domain.Derived", 11)],
        "with no in-graph declaration anywhere in the closure, a type-certain qualifier still \
         binds the type it names"
    );
}

#[test]
fn stage8_interface_receiver_binds_the_base_interface_that_declares_the_member() {
    // `IExtended : IContract`; Fulfil is declared on IContract only, and
    // the receiver is typed IExtended. An interface's closure holds
    // nothing but interfaces, so the walk keeps them for an interface
    // receiver and binds IContract -- the compiler's own containing type.
    // A class in the graph implements Fulfil too, and must not be named.
    let files = fragments_for(&[
        (
            "Domain/IContract.cs",
            "namespace App.Domain { public interface IContract { void Fulfil(); int Size { get; } } }",
        ),
        (
            "Domain/IExtended.cs",
            "namespace App.Domain { public interface IExtended : IContract { void Extra(); } }",
        ),
        (
            "Domain/Both.cs",
            "namespace App.Domain { public class Both : IExtended { public void Fulfil() { } public int Size => 0; public void Extra() { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(IExtended ext)\n    {\n        ext.Fulfil();\n        var n = ext.Size;\n        ext.Extra();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Runner.cs"),
        vec![
            ("App.Domain.IContract", 10),
            ("App.Domain.IContract", 11),
            ("App.Domain.IExtended", 12),
        ],
        "an interface-typed receiver binds the base interface that declares the member (a \
         method and a property alike) and its own declaration for its own member; the \
         implementing class is never named"
    );
    assert!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "every site is precise; nothing is left for the heuristic tiers"
    );
}

#[test]
fn stage8_class_receiver_still_never_binds_an_interface_ancestor() {
    // The other half of the interface rule, unchanged: a CLASS receiver
    // whose closure reaches Fulfil only through an interface (the class
    // itself implements it explicitly, which the def's public member
    // list does not record) earns no precise edge to the interface.
    let files = fragments_for(&[
        (
            "Domain/IContract.cs",
            "namespace App.Domain { public interface IContract { void Fulfil(); } }",
        ),
        (
            "Domain/IExtended.cs",
            "namespace App.Domain { public interface IExtended : IContract { } }",
        ),
        (
            "Domain/Explicit.cs",
            "namespace App.Domain { public class Explicit : IExtended { void IContract.Fulfil() { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Explicit e) => e.Fulfil();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "a class-typed receiver skips every interface in its closure, at any depth, even when \
         the class itself only implements the member explicitly"
    );
}

// --- Unit D: untyped lambda parameters typed from the callee's slot ----
