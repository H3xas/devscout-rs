use super::*;

#[test]
fn stage5_receiver_rule_an_external_receiver_refuses_a_candidate_not_assignable_to_it() {
    let files = fragments_for(&[
        (
            "Logging/DbUpLogAdapter.cs",
            "namespace App.Logging { public class DbUpLogAdapter : IUpgradeLog { public void LogInformation(string m) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
        "DbUpLogAdapter implements IUpgradeLog and nothing in its closure names ILogger -- the receiver's own type disproves the guess"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage5_receiver_rule_a_candidate_whose_base_closure_names_the_receiver_type_still_emits() {
    let files = fragments_for(&[
        (
            "Logging/FileLogger.cs",
            "namespace App.Logging { public class FileLogger : ILogger { public void LogInformation(string m) { } } }",
        ),
        (
            "Logging/Base.cs",
            "namespace App.Logging { public class Base : ILogger { } }",
        ),
        (
            "Logging/Derived.cs",
            "namespace App.Logging { public class Derived : Base { public void LogInformation(string m) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Logging.Derived", 7), ("App.Logging.FileLogger", 7)],
        "FileLogger names ILogger directly; Derived reaches it one in-graph hop up, through Base"
    );
}

#[test]
fn stage5_receiver_rule_generic_arguments_must_unify_on_the_matched_base() {
    let files = fragments_for(&[
        (
            "Logging/Adapter.cs",
            "namespace App.Logging { public class Adapter : ILogger { public void LogInformation(string m) { } } }",
        ),
        (
            "Logging/Typed.cs",
            "namespace App.Logging { public class Typed : ILogger<Worker> { public void LogInformation(string m) { } } }",
        ),
        (
            "Logging/Open.cs",
            "namespace App.Logging { public class Open<T> : ILogger<T> { public void LogInformation(string m) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger<Worker> _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Logging.Open", 7), ("App.Logging.Typed", 7)],
        "the base NAME matching is not enough: the non-generic `: ILogger` never binds an ILogger<Worker> receiver, while a closed and an open implementation both do"
    );
}

#[test]
fn stage5_receiver_rule_a_call_hop_receiver_with_unknown_args_compares_by_name_only() {
    let files = fragments_for(&[
        (
            "Logging/LoggerFactory.cs",
            "namespace App.Logging { public class LoggerFactory { public static ILogger Make() { return null; } } }",
        ),
        (
            "Logging/Typed.cs",
            "namespace App.Logging { public class Typed : ILogger<Worker> { public void LogInformation(string m) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "\nusing App.Logging;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var l = LoggerFactory.Make();\n    l.LogInformation(\"x\");\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
        vec![("App.Logging.Typed", 11)],
        "a method's recorded RETURN type carries a name and no type arguments, so the rule compares names only rather than refusing every generic implementation"
    );
}

#[test]
fn stage5_receiver_rule_readmits_an_extension_of_the_receiver_type_declined_on_namespace() {
    let files = fragments_for(&[
        (
            "Ext/LogExt.cs",
            "namespace App.Ext { public static class LogExt { public static void LogInformation(this IOtherLogger l, string m) { } } }",
        ),
        (
            "Registration/WidgetServiceExtensions.cs",
            "namespace App.Registration { public static class WidgetServiceExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
        ),
        (
            "Consumers/Startup.cs",
            "\nnamespace App.Consumers;\n\npublic class Startup\n{\n  public void Run(ILogger logger, IServiceCollection services)\n  {\n    logger.LogInformation(\"x\");\n    services.AddWidgets();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Startup.cs"),
        vec![("App.Registration.WidgetServiceExtensions", 9)],
        "LogExt extends IOtherLogger, not ILogger, so no this-type of its own answers the receiver and nothing else connects it -- while AddWidgets extends the receiver type exactly and only tier (f)'s namespace test (App.Registration is not imported here) kept it out"
    );
}

#[test]
fn stage5_receiver_rule_an_extension_tier_f_declined_on_arity_is_not_readmitted_as_a_guess() {
    // `App.Ext` IS imported, so tier (f) reached its arity filter and
    // declined there: `Trace(this IThing, string, string)` cannot take zero
    // arguments under any import. The scored tier must not turn that into
    // a guess -- the call has no binding at all.
    let files = fragments_for(&[
        (
            "Ext/LogExt.cs",
            "namespace App.Ext { public static class LogExt { public static void Trace(this IThing t, string a, string b) { } } }",
        ),
        (
            "Consumers/Runner.cs",
            "using App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n  public void Run(IThing thing)\n  {\n    thing.Trace();\n    thing.Trace(\"a\", \"b\");\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edges: Vec<(&str, usize, Option<HeuristicTier>)> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                tier,
                ..
            } if from_file == "Consumers/Runner.cs" => Some((to.as_str(), *from_line, *tier)),
            _ => None,
        })
        .collect();
    assert_eq!(
        edges,
        vec![("App.Ext.LogExt", 10, Some(HeuristicTier::Ext))],
        "line 9 has no binding and no edge; line 10 binds through tier (f) as before"
    );
}

#[test]
fn stage5_receiver_rule_a_generic_receiver_binds_the_generic_sibling_not_the_first_indexed() {
    // `Context` (non-generic) is indexed FIRST; `Context<T>` (its own
    // generic sibling) SECOND. Both share the id `App.Contexts.Context`,
    // so the arity-blind map alone would answer every bare `Context`
    // lookup with the FIRST-indexed, non-generic def -- wrong for a
    // receiver written `Context<Order>`, whose type-argument count names
    // the generic sibling instead.
    //
    // Classes rather than interfaces here: the base walk that reaches
    // Publish from the generic sibling (`typed_receiver_base_member`)
    // deliberately never crosses an INTERFACE base (the
    // `skip_interfaces` rule, unrelated to this defect), so an
    // interface-extends-interface pair would mask the very base-walk
    // path this test means to exercise.
    let files = fragments_for(&[
        (
            "Contexts/Context.cs",
            "namespace App.Contexts { public class Context { public void Publish() { } } }",
        ),
        (
            "Contexts/ContextOfT.cs",
            "namespace App.Contexts { public class Context<T> : Context { public T Message { get; } } }",
        ),
        (
            "Consumers/Handler.cs",
            "\nusing App.Contexts;\n\nnamespace App.Consumers;\n\npublic class Handler\n{\n  public void Handle(Context<Order> ctx)\n  {\n    var m = ctx.Message;\n    ctx.Publish();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edges = member_edges_with_file(&g, "Consumers/Handler.cs");
    assert_eq!(
        edges,
        vec![
            ("App.Contexts.Context", 10, "Contexts/ContextOfT.cs", None),
            ("App.Contexts.Context", 11, "Contexts/Context.cs", None),
        ],
        "ctx.Message binds to the generic sibling (the only one declaring Message); \
         ctx.Publish binds through the generic sibling's own base to the non-generic \
         sibling -- neither answer is the first-indexed def by accident"
    );
}

#[test]
fn stage5_receiver_rule_a_non_generic_receiver_binds_the_non_generic_sibling_when_the_generic_was_indexed_first(
) {
    // Same pair of siblings, but `Context<T>` is indexed FIRST this time:
    // a bare `Context ctx` receiver must still bind Publish to the
    // NON-generic sibling and must never answer Message precisely --
    // Message is declared only on the generic sibling, which a bare,
    // arity-0 receiver is not assignable to.
    let files = fragments_for(&[
        (
            "Contexts/ContextOfT.cs",
            "namespace App.Contexts { public interface Context<T> : Context { T Message { get; } } }",
        ),
        (
            "Contexts/Context.cs",
            "namespace App.Contexts { public interface Context { void Publish(); } }",
        ),
        (
            "Consumers/Handler.cs",
            "\nusing App.Contexts;\n\nnamespace App.Consumers;\n\npublic class Handler\n{\n  public void Handle(Context ctx)\n  {\n    ctx.Publish();\n    var m = ctx.Message;\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edges = member_edges_with_file(&g, "Consumers/Handler.cs");
    assert_eq!(
        edges
            .iter()
            .filter(|(_, _, _, tier)| tier.is_none())
            .cloned()
            .collect::<Vec<_>>(),
        vec![("App.Contexts.Context", 10, "Contexts/Context.cs", None)],
        "a bare Context receiver binds Publish to the non-generic sibling, and line 11 \
         (ctx.Message) has no precise edge -- Message is declared only on the generic \
         sibling, which a bare, arity-0 receiver is not assignable to; a guess-tier edge \
         there, if the graph produces one, is not asserted against here"
    );
}

#[test]
fn stage5_receiver_rule_a_base_written_without_arguments_walks_to_the_non_generic_sibling() {
    // `Context<T>` indexed FIRST again. The receiver here is the GENERIC
    // sibling (`Context<Order>`), and Publish is reached only by walking
    // its base `Context` -- written bare, with no argument list -- which
    // must resolve to the non-generic sibling (arity 0), not back to
    // whichever sibling the blind map happened to index first.
    //
    // Classes rather than interfaces here for the same reason as the
    // test above: the base walk must actually cross the `Context` base,
    // which `skip_interfaces` would otherwise prune before it is ever
    // tried.
    let files = fragments_for(&[
        (
            "Contexts/ContextOfT.cs",
            "namespace App.Contexts { public class Context<T> : Context { public T Message { get; } } }",
        ),
        (
            "Contexts/Context.cs",
            "namespace App.Contexts { public class Context { public void Publish() { } } }",
        ),
        (
            "Consumers/Handler.cs",
            "\nusing App.Contexts;\n\nnamespace App.Consumers;\n\npublic class Handler\n{\n  public void Handle(Context<Order> ctx)\n  {\n    ctx.Publish();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edges = member_edges_with_file(&g, "Consumers/Handler.cs");
    assert_eq!(
        edges,
        vec![("App.Contexts.Context", 10, "Contexts/Context.cs", None)],
        "the bare base name Context, carrying no argument list, walks to the \
         non-generic sibling regardless of index order"
    );
}

#[test]
fn stage5_receiver_rule_a_receiver_arity_with_no_in_graph_def_keeps_the_arity_blind_answer() {
    // Only the non-generic `Repository` is in the graph; the receiver is
    // written `Repository<Order>`. The exact-arity pass finds no def and
    // the arity-blind ladder answers as it always has: the site keeps
    // its precise edge rather than turning external on an arity the
    // graph cannot confirm or refute.
    let files = fragments_for(&[
        (
            "Data/Repository.cs",
            "namespace App.Data { public class Repository { public void Save() { } } }",
        ),
        (
            "Consumers/Handler.cs",
            "\nusing App.Data;\n\nnamespace App.Consumers;\n\npublic class Handler\n{\n  public void Handle(Repository<Order> repo)\n  {\n    repo.Save();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_with_file(&g, "Consumers/Handler.cs"),
        vec![("App.Data.Repository", 10, "Data/Repository.cs", None)],
        "no def of arity 1 exists, so the arity-blind answer stands"
    );
}

#[test]
fn stage5_receiver_rule_an_extension_stops_binding_when_the_generic_sibling_declares_the_member() {
    // `Context` (non-generic) indexed FIRST, `Context<T>` (declaring
    // Respond) SECOND, plus an extension method of the same name and
    // matching this-type. Before the fix, a `Context<Order>` receiver
    // resolved arity-blind to the non-generic sibling, which does not
    // declare Respond, so the call fell through to the extension tier.
    // Arity-aware resolution must bind the receiver to the generic
    // sibling directly, which declares Respond itself -- so the
    // extension never gets a chance to answer.
    let files = fragments_for(&[
        (
            "Contexts/Context.cs",
            "namespace App.Contexts { public interface Context { void Publish(); } }",
        ),
        (
            "Contexts/ContextOfT.cs",
            "namespace App.Contexts { public interface Context<T> : Context { void Respond(string s); } }",
        ),
        (
            "Ext/ContextExt.cs",
            "namespace App.Ext { public static class ContextExt { public static void Respond<T>(this App.Contexts.Context<T> c, string s) { } } }",
        ),
        (
            "Consumers/Handler.cs",
            "\nusing App.Contexts;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Handler\n{\n  public void Handle(Context<Order> ctx)\n  {\n    ctx.Respond(\"x\");\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let edges = member_edges_with_file(&g, "Consumers/Handler.cs");
    assert_eq!(
        edges,
        vec![("App.Contexts.Context", 11, "Contexts/ContextOfT.cs", None)],
        "the generic sibling declares Respond itself, so the receiver binds precisely \
         to it rather than falling through to the extension"
    );
    assert!(
        !g.edges.iter().any(|e| matches!(
            e,
            Edge::UsesMember { from_file, tier, .. }
                if from_file == "Consumers/Handler.cs" && *tier == Some(HeuristicTier::Ext)
        )),
        "no extension-tier edge from the consumer file -- tier (e) already claimed the call"
    );
}

#[test]
fn stage5_receiver_rule_an_in_graph_receiver_still_resolves_precisely() {
    let files = fragments_for(&[
        (
            "Widgets/Widget.cs",
            "namespace App.Widgets { public class Widget { public void Render() { } } }",
        ),
        (
            "Consumers/UsesWidget.cs",
            "\nusing App.Widgets;\n\nnamespace App.Consumers;\n\npublic class UsesWidget\n{\n  private Widget _widget;\n  public void Run() => _widget.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/UsesWidget.cs"),
        vec![("App.Widgets.Widget", 9)],
        "an in-graph receiver never reaches the scored tier at all -- tier (e) answers it precisely"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

// The probe the design asked for: a base written with its namespace
// (`class Handle : System.IDisposable`) is recorded as the bare identifier
// `IDisposable`, and a receiver declared the same dotted way is recorded
// bare too, so the two raw strings meet and the rule admits the candidate.
// Both halves of that are extractor behaviour, which is why this runs real
// sources rather than hand-built facts.
#[test]
fn stage5_receiver_rule_a_dotted_base_name_meets_a_dotted_receiver_type_by_bare_identifier() {
    let files = fragments_for(&[
        (
            "Io/Handle.cs",
            "namespace App.Io { public class Handle : System.IDisposable { public void Dispose() { } } }",
        ),
        (
            "Consumers/Closer.cs",
            "\nnamespace App.Consumers;\n\npublic class Closer\n{\n  public void Run(System.IDisposable d) => d.Dispose();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Closer.cs"),
        vec![("App.Io.Handle", 6)],
        "both sides reduce to the bare identifier IDisposable, so the raw base string answers the receiver type"
    );
}
