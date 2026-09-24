use super::*;

#[test]
fn stage7_this_member_resolves_to_the_declaring_def_across_partial_files() {
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public string Name;\n}\n",
        ),
        (
            "Domain/Order.Validation.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public int Describe() => this.Name.Length;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let name_edges: Vec<(&str, &str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "Domain/Order.Validation.cs" && member.as_deref() == Some("Name") => {
                Some((to.as_str(), member.as_deref().unwrap(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        name_edges,
        vec![("App.Domain.Order", "Name", false)],
        "this.Name resolves through the ordinary typed-receiver path -- Name is declared in the \
         OTHER partial-class file, which the merged member lists already cover; no self-edge \
         rule was needed"
    );
}

#[test]
fn stage7_base_member_resolves_to_the_first_base_that_declares_it() {
    let files = fragments_for(&[
        (
            "Domain/GrandBase.cs",
            "\nnamespace App.Domain;\n\npublic class GrandBase\n{\n    public void Touch() { }\n}\n",
        ),
        (
            "Domain/Base.cs",
            "\nnamespace App.Domain;\n\npublic class Base : GrandBase\n{\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Touch() { }\n\n    public void Poke()\n    {\n        base.Touch();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let touch_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Touch") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        touch_edges,
        vec![("App.Domain.GrandBase", false)],
        "base.Touch() starts at Order's OWN bases -- Base does not declare Touch, so the walk \
         continues to Base's own base GrandBase, which does; Order's OWN override (also named \
         Touch) is never even considered"
    );
}

#[test]
fn stage7_base_member_declared_nowhere_in_graph_resolves_external() {
    let files = fragments_for(&[
        (
            "Domain/Base.cs",
            "\nnamespace App.Domain;\n\npublic class Base\n{\n    public void Other() { }\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Touch() { }\n\n    public void Poke()\n    {\n        base.Touch();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let touch_edges: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| {
            matches!(e, Edge::UsesMember { from_file, member, .. }
                if from_file == "Domain/Order.cs" && member.as_deref() == Some("Touch"))
        })
        .collect();
    assert!(
        touch_edges.is_empty(),
        "no in-graph base declares Touch -- base.Touch() is external like any other unresolved \
         receiver, never a scored guess, even though Order itself declares Touch: {touch_edges:?}"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage7_base_member_lookup_on_a_cyclic_hierarchy_never_binds_the_enclosing_type() {
    // `class A : B` / `class B : A` is not valid C#, but it parses, and a
    // graph built from half-written source can hold it. Walking B's own
    // bases leads straight back to A, and A declares Only -- so without a
    // guard the `base.` lookup answers with the very type the call was
    // written in, the self-edge a `base.` qualifier can never mean.
    let files = fragments_for(&[
        (
            "Domain/A.cs",
            "\nnamespace App.Domain;\n\npublic class A : B\n{\n    public void Only() { }\n\n    public void Go() { base.Only(); }\n}\n",
        ),
        (
            "Domain/B.cs",
            "\nnamespace App.Domain;\n\npublic class B : A\n{\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Domain/A.cs").is_empty(),
        "no in-graph BASE of A declares Only -- reaching A again through the cycle is not an \
         answer, so base.Only() is external exactly like a member no base declares at all: \
         {:?}",
        member_edges_from(&g, "Domain/A.cs")
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "and a `base.` ref never falls through to a guess either"
    );
}

#[test]
fn stage7_awaited_static_call_local_unwraps_task_once() {
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "Infra/Repo.cs",
            "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static Task<Order> LoadAsync() => null;\n    public static Task<Task<Order>> LoadNestedAsync() => null;\n}\n",
        ),
        (
            "App/Worker.cs",
            "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public async Task Run()\n    {\n        var order = await Repo.LoadAsync();\n        order.Validate();\n\n        var nested = await Repo.LoadNestedAsync();\n        nested.Validate();\n\n        var plain = Repo.LoadAsync();\n        plain.Validate();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let validate_edges: Vec<(&str, usize, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                member,
                heuristic,
                ..
            } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                Some((to.as_str(), *from_line, *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        validate_edges,
        vec![("App.Domain.Order", 9, false)],
        "the SINGLY-wrapped AWAITED call (`order`) unwraps Task<Order> to Order precisely; the \
         DOUBLY-wrapped awaited call (`nested`) unwraps only once, landing on the bare name \
         \"Task\" (never Order), and the UNAWAITED call (`plain`) is never unwrapped at all -- \
         both of the latter two stay typed \"Task\", resolve to nothing in-graph, and earn no \
         edge at all, guessed or otherwise"
    );
}

// --- Unit C: chain-tail receivers -----------------------------------

#[test]
fn stage7_chain_tail_resolves_through_one_method_return_hop() {
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
        ),
        (
            "Infra/Repo.cs",
            "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static Order Load() => null;\n}\n",
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
        vec![("App.Domain.Order", false)],
        "the chain tail `.Validate()` resolves through the ONE method_returns hop off \
         `Repo.Load()`, precisely and non-heuristically -- exactly like a `var x = \
         Repo.Load(); x.Validate();` local already would"
    );
}

// --- Unit B: cross-file field facts, the bare-identifier fallback -------
//
// All three run real C# through the extractor (`fragments_for`), the same
// choice the four stage-7 tests above make: a field's declared type is an
// extractor fact (`FragDef.fieldTypes`), so a test that hand-built the
// fragments would take the extractor's word for it rather than proving
// it end to end.

#[test]
fn stage7_partial_class_field_declared_in_a_sibling_file_types_the_receiver() {
    let files = fragments_for(&[
        (
            "Infra/Widget.cs",
            "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    private Widget _widget;\n}\n",
        ),
        (
            "Domain/Order.Extra.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        _widget.Spin();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let spin_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "Domain/Order.Extra.cs" && member.as_deref() == Some("Spin") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        spin_edges,
        vec![("App.Infra.Widget", false)],
        "_widget.Spin() carries no in-file fact at all in Order.Extra.cs -- _widget is declared \
         as a field only in the OTHER partial-class file -- so it is typed from the merged \
         field_types table the resolver builds across both files instead"
    );
}

#[test]
fn stage7_protected_field_declared_on_a_base_types_the_receiver() {
    let files = fragments_for(&[
        (
            "Infra/Logger.cs",
            "\nnamespace App.Infra;\n\npublic class Logger\n{\n    public void Log() { }\n}\n",
        ),
        (
            "Domain/Base.cs",
            "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected Logger _logger;\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke()\n    {\n        _logger.Log();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let log_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Log") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        log_edges,
        vec![("App.Infra.Logger", false)],
        "_logger.Log() has no in-file fact anywhere in Order.cs -- Order itself declares no \
         _logger field at all -- so the fallback walks Order's OWN bases: Base declares it, \
         typed Logger, which declares Log"
    );
}

#[test]
fn stage7_a_cross_file_field_type_resolves_in_its_declaring_files_context() {
    // Two types named Alpha, in two namespaces. The base declares the
    // field under `using N1`; the derived file that reads it imports N2
    // instead and has never heard of N1.Alpha. The field's declared type
    // is a bare name that only means N1.Alpha, so the reading file's own
    // imports must not be what decides which Alpha it names.
    let files = fragments_for(&[
        (
            "N1/Alpha.cs",
            "\nnamespace N1;\n\npublic class Alpha\n{\n    public void Ship() { }\n}\n",
        ),
        (
            "N2/Alpha.cs",
            "\nnamespace N2;\n\npublic class Alpha\n{\n    public void Ship() { }\n}\n",
        ),
        (
            "App/BaseT.cs",
            "\nusing N1;\n\nnamespace App;\n\npublic class BaseT\n{\n    protected Alpha _thing;\n}\n",
        ),
        (
            "App/Derived.cs",
            "\nusing N2;\n\nnamespace App;\n\npublic class Derived : BaseT\n{\n    public void Go() { _thing.Ship(); }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "App/Derived.cs"),
        vec![("N1.Alpha", 8)],
        "the field fact came from BaseT.cs, so its type name is resolved under BaseT.cs's own \
         usings, namespace and nesting -- Derived.cs's `using N2` is not evidence about a \
         declaration written in another file"
    );
}

#[test]
fn stage7_a_catch_variable_shadows_a_same_named_base_field_fact() {
    // The handler declares `e` as a WidgetException, a type this graph
    // does not hold. A protected field on the base happens to share the
    // name and IS in-graph -- and before the catch designation earned a
    // fact of its own, the bare-identifier fallback typed the caught
    // exception from that field and bound the call to it.
    let files = fragments_for(&[
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Ship() { }\n}\n",
        ),
        (
            "Domain/BaseT.cs",
            "\nnamespace App.Domain;\n\npublic class BaseT\n{\n    protected Order e;\n}\n",
        ),
        (
            "Domain/Derived.cs",
            "\nnamespace App.Domain;\n\npublic class Derived : BaseT\n{\n    public void Go()\n    {\n        try { Work(); }\n        catch (WidgetException e) { e.Ship(); }\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Domain/Derived.cs").is_empty(),
        "the caught exception shadows the base field, and its own type is out of graph -- so \
         the site is an ordinary external miss, never a precise edge to Order.Ship: {:?}",
        member_edges_from(&g, "Domain/Derived.cs")
    );
}

#[test]
fn stage7_an_in_file_local_shadows_a_same_named_field_fact() {
    let files = fragments_for(&[
        (
            "Infra/Widget.cs",
            "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
        ),
        (
            "Infra/Gadget.cs",
            "\nnamespace App.Infra;\n\npublic class Gadget\n{\n    public void Zap() { }\n}\n",
        ),
        (
            "Domain/Base.cs",
            "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected Widget _item;\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke()\n    {\n        var _item = new Gadget();\n        _item.Zap();\n    }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let zap_edges: Vec<(&str, bool)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                to,
                member,
                heuristic,
                ..
            } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Zap") => {
                Some((to.as_str(), *heuristic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        zap_edges,
        vec![("App.Infra.Gadget", false)],
        "Poke's own local `_item` (typed Gadget, which declares Zap) is an in-file fact for the \
         name, so the base's same-named field (typed Widget, which does NOT declare Zap) is \
         never even consulted -- a local always shadows a same-named field fact, precisely \
         because the fallback only ever runs when receiver_type is still unset"
    );
}

#[test]
fn stage7_an_untyped_in_file_local_still_shadows_a_same_named_field_fact() {
    // `var order = Unknown();` is a BARE (undotted) call -- a shape
    // `invocation_call` never matches (it requires a dotted qualifier,
    // "Q.M()") -- so `order` settles as an ordinary taken-but-unknown
    // member-table entry: no `Fact` vouches for its type, but the name
    // IS in scope, exactly like a real (typed) local. `Order.Fields.cs`
    // declares a field of the SAME name in a SIBLING partial-class
    // file, which is precisely the shape the field/property fallback
    // exists to answer for a name with no in-file fact -- this proves
    // it does NOT answer here, because `order` is one.
    let with_field = fragments_for(&[
        (
            "Infra/Widget.cs",
            "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
        ),
        (
            "Domain/Order.Fields.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    private Widget order;\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        var order = Unknown();\n        order.Spin();\n    }\n}\n",
        ),
    ]);
    let without_field = fragments_for(&[
        (
            "Infra/Widget.cs",
            "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
        ),
        (
            "Domain/Order.Fields.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n}\n",
        ),
        (
            "Domain/Order.cs",
            "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        var order = Unknown();\n        order.Spin();\n    }\n}\n",
        ),
    ]);
    let g_with = resolve_graph(&no_git_root(), &with_field);
    let g_without = resolve_graph(&no_git_root(), &without_field);
    // Scoped to `uses-member` edges: the field's OWN declared type earns
    // an ordinary `uses-type` ref (see the walk's `field_declaration`
    // arm) whether or not this test's concern holds, so comparing the
    // WHOLE graph would differ by that one incidental edge every time --
    // it is not what this test is about. What this test is about is
    // whether the field ever gets to answer a `uses-member` ref it has
    // no business answering.
    fn uses_member_edges(g: &Graph) -> Vec<Edge> {
        g.edges
            .iter()
            .filter(|e| matches!(e, Edge::UsesMember { .. }))
            .cloned()
            .collect()
    }
    assert_eq!(
        uses_member_edges(&g_with),
        uses_member_edges(&g_without),
        "the untyped local `order` is a member-table entry for the name (taken, unknown) -- \
         `receiver_local` -- so it shadows the sibling file's same-named field exactly like a \
         typed local already does; the uses-member edge set must be identical whether or not \
         that field exists at all"
    );
    // Both variants DO carry one `uses-member` edge for `order.Spin()` --
    // `Spin` is declared by exactly one def anywhere in this fixture
    // (Widget), so the SCORED tier's own uniqueness fallback (a
    // wholly separate mechanism from the field/property fallback this
    // test guards, reached only when a ref carries NO receiver fact at
    // all) claims it as a heuristic guess in BOTH variants alike --
    // proof by itself that the field played no part, since it fires
    // identically whether or not the field exists. What distinguishes
    // "the field answered" from "an unrelated tier guessed" is
    // `heuristic`: the field/property fallback feeds the ordinary
    // typed-receiver path, which only ever emits a PRECISE
    // (non-heuristic) edge.
    let edges = uses_member_edges(&g_with);
    assert_eq!(
        edges,
        vec![Edge::UsesMember {
            from_file: "Domain/Order.cs".to_string(),
            from_line: 9,
            to: "App.Infra.Widget".to_string(),
            to_file: "Infra/Widget.cs".to_string(),
            member: Some("Spin".to_string()),
            heuristic: true,
            tier: Some(HeuristicTier::Guess),
            source: None,
            overload_signature: None,
        }],
        "the one edge present is the SCORED tier's own heuristic guess, never a precise edge \
         from the field/property fallback (which the shadowing rule keeps from ever running \
         here): {edges:?}"
    );
}

// --- Non-public hierarchy-internal members, interface-skipped
// base lookup, tier (f)'s closure fallback, and the typed-receiver
// precise tier's own base walk -----------------------------------------
