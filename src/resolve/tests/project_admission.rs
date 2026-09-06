use super::*;

#[test]
fn stage6_admission_a_scored_guess_never_names_a_def_in_a_project_the_site_cannot_reach() {
    // `Build()` resolves to nothing, so `q` has no recorded type: the ref
    // carries no receiver fact at all and lands in the scored tier's
    // uniqueness pool, where the only evidence is the member NAME. Two
    // projects declare `Enqueue`; only one of them is on the site's
    // reference closure.
    let files = fragments_for(&[
        (
            "src/Domain/Order.cs",
            "namespace Fixture.Domain { public class Order { public void Enqueue(string m) { } } }",
        ),
        (
            "src/Unreachable/Mailer.cs",
            "namespace Fixture.Unreachable { public class Mailer { public void Enqueue(string m) { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var q = Build();\n    q.Enqueue(\"x\");\n  }\n}\n",
        ),
    ]);
    let root = no_git_root();

    assert_eq!(
        heuristic_member_edge_targets(&resolve_graph(&root, &files)),
        vec!["Fixture.Domain.Order", "Fixture.Unreachable.Mailer"],
        "without a model the tier has only the member name to go on, and both declarers are equally plausible"
    );

    let model = model_of(vec![
        unit("src/App/App.csproj", &["src/Domain/Domain.csproj"], false),
        unit("src/Domain/Domain.csproj", &[], false),
        unit("src/Unreachable/Unreachable.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["Fixture.Domain.Order"],
        "App references Domain and nothing references Unreachable -- Mailer.Enqueue is not a call App could ever have made"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 1,
        "the refused guess is dropped, not retagged"
    );
}

#[test]
fn stage6_admission_a_non_test_site_never_names_a_def_in_a_test_project_even_when_it_has_no_test_methods(
) {
    // The fixture-class shape: a helper in a test project carrying no
    // `[Fact]`/`[Test]` attribute at all, so `test_def_count` cannot see
    // it and no attribute-based rule would refuse it. Reachability cannot
    // refuse it either -- this model deliberately lets the production
    // project reference the test one, so the ONLY thing standing between
    // the guess and the edge is the test-project half of the gate.
    let files = fragments_for(&[
        (
            "tests/App.Tests/AdapterFixture.cs",
            "namespace Fixture.App.Tests { public class AdapterFixture { public void Reset() { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Reset();\n  }\n}\n",
        ),
    ]);
    let root = no_git_root();

    assert_eq!(
        heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
        vec![("Fixture.App.Tests.AdapterFixture", 9)],
        "without a model the guess stands -- nothing in the sources says AdapterFixture is test-only"
    );

    let model = model_of(vec![
        unit(
            "src/App/App.csproj",
            &["tests/App.Tests/App.Tests.csproj"],
            false,
        ),
        unit("tests/App.Tests/App.Tests.csproj", &[], true),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
    assert_eq!(
        g.stats.test_def_count, 0,
        "AdapterFixture declares no test method, so def-level test detection never marked it -- the UNIT is what makes it test-only"
    );
    assert!(
        heuristic_member_edges_from(&g, "src/App/Runner.cs").is_empty(),
        "production code calling into a test assembly is not a thing the build allows, whatever the name says"
    );
}

#[test]
fn stage6_admission_a_test_site_may_name_a_def_in_a_referenced_test_utility_project() {
    // The other side of the same rule: test -> test is an ordinary
    // reference, so the gate must not turn "is a test project" into a
    // blanket refusal.
    let files = fragments_for(&[
        (
            "tests/Test.Utilities/FakeServer.cs",
            "namespace Fixture.Test.Utilities { public class FakeServer { public void Reset() { } } }",
        ),
        (
            "tests/App.Tests/WorkerTests.cs",
            "\nnamespace Fixture.App.Tests;\n\npublic class WorkerTests\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Reset();\n  }\n}\n",
        ),
    ]);
    let model = model_of(vec![
        unit(
            "tests/App.Tests/App.Tests.csproj",
            &["tests/Test.Utilities/Test.Utilities.csproj"],
            true,
        ),
        unit("tests/Test.Utilities/Test.Utilities.csproj", &[], true),
    ]);
    let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));
    assert_eq!(
        heuristic_member_edges_from(&g, "tests/App.Tests/WorkerTests.cs"),
        vec![("Fixture.Test.Utilities.FakeServer", 9)],
        "a test site reaching a referenced test-utility project is exactly what that project is for"
    );
}

#[test]
fn stage6_admission_tier_f_ignores_an_unreachable_duplicate_and_emits_the_reachable_one() {
    // Tier (f) emits on exactly ONE distinct declaring class, so a second
    // same-named extension method in the same namespace silences it
    // entirely and the ref falls through to the scored tier, which names
    // both. The gate runs BEFORE that count, which is why an unreachable
    // duplicate stops being an ambiguity at all rather than merely losing
    // a race -- and the edge that comes back is the EXT one, not the pair
    // of guesses the fallthrough produced.
    let files = fragments_for(&[
        (
            "src/Ext.Adapters/ServiceCollectionExtensions.cs",
            "namespace Fixture.Registration { public static class ServiceCollectionExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
        ),
        (
            "src/Unreachable/UnreachableExtensions.cs",
            "namespace Fixture.Registration { public static class UnreachableExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
        ),
        (
            "src/App/Startup.cs",
            "\nusing Fixture.Registration;\n\nnamespace Fixture.App;\n\npublic class Startup\n{\n  public void Run(IServiceCollection s) => s.AddWidgets();\n}\n",
        ),
    ]);
    let root = no_git_root();

    let bare = resolve_graph(&root, &files);
    assert_eq!(
        heuristic_member_edges_from(&bare, "src/App/Startup.cs"),
        vec![
            ("Fixture.Registration.ServiceCollectionExtensions", 8),
            ("Fixture.Registration.UnreachableExtensions", 8)
        ],
        "without a model both static classes clear every tier-(f) filter, two distinct classes is an ambiguity, and the tier stays silent"
    );
    assert_eq!(
        heuristic_member_tiers_from(&bare, "src/App/Startup.cs"),
        vec![Some(HeuristicTier::Guess), Some(HeuristicTier::Guess)],
        "the two edges are the scored tier's, re-admitted by the receiver rule because each `this` parameter names the receiver type exactly"
    );

    let model = model_of(vec![
        unit(
            "src/App/App.csproj",
            &["src/Ext.Adapters/Ext.Adapters.csproj"],
            false,
        ),
        unit("src/Ext.Adapters/Ext.Adapters.csproj", &[], false),
        unit("src/Unreachable/Unreachable.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
    assert_eq!(
        heuristic_member_edges_from(&g, "src/App/Startup.cs"),
        vec![("Fixture.Registration.ServiceCollectionExtensions", 8)],
        "one admitted candidate is one distinct class, and tier (f) emits"
    );
    assert_eq!(
        heuristic_member_tiers_from(&g, "src/App/Startup.cs"),
        vec![Some(HeuristicTier::Ext)],
        "the edge is tier (f)'s, not the scored tier's second-guess"
    );
}

#[test]
fn stage6_admission_a_file_outside_every_project_fails_open() {
    // Both directions of "unknown": a candidate whose file no project
    // owns, and a SITE whose file no project owns. Neither may lose an
    // edge -- the gate refuses only on a positive answer.
    let files = fragments_for(&[
        (
            "src/App/Widget.cs",
            "namespace Fixture.App { public class Widget { public void Ping() { } } }",
        ),
        (
            "tools/Helper.cs",
            "namespace Fixture.Tools { public class Helper { public void Pong() { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Pong();\n  }\n}\n",
        ),
        (
            "tools/Script.cs",
            "\nnamespace Fixture.Tools;\n\npublic class Script\n{\n  public void Run()\n  {\n    var w = Build();\n    w.Ping();\n  }\n}\n",
        ),
    ]);
    let model = model_of(vec![unit("src/App/App.csproj", &[], false)]);
    let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

    assert_eq!(
        heuristic_member_edges_from(&g, "src/App/Runner.cs"),
        vec![("Fixture.Tools.Helper", 9)],
        "the candidate sits outside every project, so nothing can be proven about reaching it"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "tools/Script.cs"),
        vec![("Fixture.App.Widget", 9)],
        "the SITE sits outside every project -- same fail-open answer from the other side"
    );
}

// --- stage 6: `global using` is a per-PROJECT fact ---------------------
//
// A `global using` is scoped to the compilation that declares it and does
// NOT flow across a ProjectReference. Without a model the resolver cannot
// see project boundaries and pools every global using repo-wide (the
// documented over-approximation); with one, each file is seeded from its
// OWN project's globals only.

#[test]
fn stage6_global_usings_are_scoped_to_the_declaring_unit_when_a_model_exists() {
    let files = fragments_for(SCOPED_GLOBAL_USING_FIXTURE);
    // Both consumers reference both Alpha and Beta, so admission has
    // nothing to say here: the only difference between the two files is
    // which project declared the `global using`.
    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/Beta/Beta.csproj", &[], false),
        unit(
            "src/App/App.csproj",
            &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
            false,
        ),
        unit(
            "src/Other/Other.csproj",
            &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
            false,
        ),
    ]);
    let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

    assert_eq!(
        member_edges_from(&g, "src/App/AppConsumer.cs"),
        vec![("Fixture.Alpha.Config", 6)],
        "the declaring project's own files still see its global using"
    );
    assert!(
        member_edges_from(&g, "src/Other/OtherConsumer.cs").is_empty(),
        "the other project never wrote that global using, so `Config` names nothing there"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "src/Other/OtherConsumer.cs"),
        vec![("Fixture.Alpha.Config", 6), ("Fixture.Beta.Config", 6)],
        "it degrades to the ordinary two-way ambiguity an unimported `Config` always is -- not to a precise edge borrowed from another project"
    );
}

#[test]
fn stage6_global_usings_are_repo_wide_without_one() {
    let files = fragments_for(SCOPED_GLOBAL_USING_FIXTURE);
    let g = resolve_graph(&no_git_root(), &files);

    assert_eq!(
        member_edges_from(&g, "src/App/AppConsumer.cs"),
        vec![("Fixture.Alpha.Config", 6)]
    );
    assert_eq!(
        member_edges_from(&g, "src/Other/OtherConsumer.cs"),
        vec![("Fixture.Alpha.Config", 6)],
        "with no project boundaries to read, every global using is in scope everywhere -- the pre-stage-6 behaviour, unchanged"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "both qualifiers resolved precisely, so no ref ever reached a heuristic tier"
    );
}

#[test]
fn stage6_global_usings_fall_open_to_the_repo_wide_pool_for_a_file_no_project_owns() {
    // A file under no project directory has no compilation whose global
    // usings could be read, so it is NOT an owned unit that happened to
    // declare none -- it is the no-model case in miniature, and it falls
    // open to the repo-wide pool. Seeding it from nothing instead would
    // strip a loose file of every global using in the tree and silently
    // demote a resolvable name to a guess.
    let mut files: Vec<(&str, &str)> = SCOPED_GLOBAL_USING_FIXTURE.to_vec();
    files.push((
        "Loose/LooseConsumer.cs",
        "\nnamespace Fixture.Loose;\n\npublic class LooseConsumer\n{\n  public void Run() => Config.Load();\n}\n",
    ));
    let files = fragments_for(&files);
    // Every unit lives under `src/`; `Loose/` is under none of them, so
    // `unit_of_file` answers `None` for the consumer and admission -- which
    // needs a site unit to filter anything -- has nothing to say either.
    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/Beta/Beta.csproj", &[], false),
        unit(
            "src/App/App.csproj",
            &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
            false,
        ),
        unit(
            "src/Other/Other.csproj",
            &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
            false,
        ),
    ]);
    let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

    assert_eq!(
        model.unit_of_file("Loose/LooseConsumer.cs"),
        None,
        "the fixture only means anything while this file is owned by no unit"
    );
    assert_eq!(
        member_edges_from(&g, "Loose/LooseConsumer.cs"),
        vec![("Fixture.Alpha.Config", 6)],
        "the App project's `global using Fixture.Alpha;` is in the repo-wide pool, and an unowned file draws from that pool"
    );
    assert!(
        heuristic_member_edges_from(&g, "Loose/LooseConsumer.cs").is_empty(),
        "the name resolved precisely, so no tier ever had a guess to make"
    );
    assert_eq!(
        member_edges_from(&g, "src/Other/OtherConsumer.cs"),
        Vec::new(),
        "a file an OWNED project holds still sees only its own unit's globals -- the fall-open is for unowned files alone"
    );
}

// --- stage 6: narrowing an AMBIGUOUS resolution by reachability -------
//
// The ladder pools same-named defs and refuses to pick; the project model
// can settle some of those refusals with the language's own rule rather
// than a guess -- a type in a project this one does not reference cannot
// be named here at all, so it was never a candidate. The narrowing runs
// OUTSIDE the ladder, at the three places that consume an `Ambiguous`,
// which is why it can turn one into a PRECISE edge without any tier
// learning about projects.

#[test]
fn stage6_narrowing_turns_a_two_project_ambiguity_into_a_precise_edge_when_only_one_is_reachable() {
    let files = fragments_for(CROSS_PROJECT_AMBIGUITY_FIXTURE);
    let root = no_git_root();

    let bare = resolve_graph(&root, &files);
    assert_eq!(
        ambiguous_edges_from(&bare, "src/App/Runner.cs"),
        vec![(
            "uses-type",
            "Config",
            vec!["Fixture.Alpha.Config", "Fixture.Beta.Config"],
            2
        )],
        "without a model the two Configs are indistinguishable and the type ref stays an ambiguity"
    );
    assert_eq!(
        heuristic_member_edges_from(&bare, "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
        "and the qualifier's ambiguity is what feeds the scored tier's strong pool"
    );

    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/App/App.csproj", &["src/Alpha/Alpha.csproj"], false),
        unit("src/Beta/Beta.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

    assert_eq!(
        type_edge_targets_from(&g, "src/App/Runner.cs"),
        vec!["Fixture.Alpha.Config"],
        "App cannot reference Beta, so `Config` in this file has exactly one meaning and the type ref is a FACT"
    );
    assert_eq!(
        member_edges_from(&g, "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8)],
        "the same narrowing at the uses-member qualifier promotes the call out of the scored tier entirely"
    );
    assert!(
        ambiguous_edges_from(&g, "src/App/Runner.cs").is_empty() && g.stats.ambiguous_count == 0,
        "a settled ambiguity is not an ambiguity: the edge and the count both go"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "nothing is guessed when the language's own reference rule already answers"
    );
    assert_eq!(
        g.stats.unresolved_external_count, 0,
        "narrowed to ONE, not to zero -- the external counter must not move"
    );
}

#[test]
fn stage6_narrowing_settles_the_receiver_probe_so_a_field_hop_lands_on_a_precise_edge() {
    // The third consumer: tier (e) resolves the RECEIVER's recorded type
    // through the same ladder, and an ambiguous answer there stops the hop
    // dead -- the tier emits only on exactly one def. Narrowing the probe
    // is what turns `_config.Load()` from two scored guesses into the one
    // edge the compiler would bind.
    let files = fragments_for(&[
        (
            "src/Alpha/Config.cs",
            "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
        ),
        (
            "src/Beta/Config.cs",
            "namespace Fixture.Beta { public class Config { public void Load() { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Config _config;\n\n  public void Run() => _config.Load();\n}\n",
        ),
    ]);
    let root = no_git_root();

    assert_eq!(
        heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
        "without a model the receiver type is ambiguous, tier (e) declines and the scored tier names both"
    );

    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/App/App.csproj", &["src/Alpha/Alpha.csproj"], false),
        unit("src/Beta/Beta.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
    assert_eq!(
        member_edges_from(&g, "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8)],
        "one reachable receiver type is one receiver type, and the field hop is precise again"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "the guesses are replaced, not joined"
    );
}

#[test]
fn stage6_narrowing_keeps_an_ambiguity_between_two_reachable_projects() {
    // The gate is subtractive and nothing more: when the site can
    // reference both projects the model has nothing to say, and the
    // resolver must go on refusing to pick rather than inventing a
    // tie-break.
    let files = fragments_for(CROSS_PROJECT_AMBIGUITY_FIXTURE);
    let root = no_git_root();
    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit(
            "src/App/App.csproj",
            &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
            false,
        ),
        unit("src/Beta/Beta.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

    assert_eq!(
        ambiguous_edges_from(&g, "src/App/Runner.cs"),
        ambiguous_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
        "both candidates survive the filter, so the edge is the one the model-less resolve emits"
    );
    assert_eq!(g.stats.ambiguous_count, 1);
    assert_eq!(
        heuristic_member_edges_from(&g, "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
        "and the qualifier still reaches the scored tier with both candidates in its pool"
    );
}

#[test]
fn stage6_narrowing_never_touches_ctor_di_implementor_choice() {
    // The ctor-DI resolver picks an IMPLEMENTATION of an interface the
    // site names -- a different question from "which same-named type did
    // this reference mean", and one the model is not entitled to answer:
    // an unreachable implementor is still evidence that the interface has
    // more than one, and silently promoting the reachable one would turn a
    // reported ambiguity into a confident wrong answer whenever the
    // path-based ownership guess is off.
    let files = fragments_for(&[
        (
            "src/Contracts/IRepo.cs",
            "namespace Fixture.Contracts { public interface IRepo { void Save(); } }",
        ),
        (
            "src/Files/FileRepo.cs",
            "using Fixture.Contracts;\n\nnamespace Fixture.Files { public class FileRepo : IRepo { public void Save() { } } }",
        ),
        (
            "src/Sql/SqlRepo.cs",
            "using Fixture.Contracts;\n\nnamespace Fixture.Sql { public class SqlRepo : IRepo { public void Save() { } } }",
        ),
        (
            "src/App/Service.cs",
            "using Fixture.Contracts;\n\nnamespace Fixture.App;\n\npublic class Service\n{\n  public Service(IRepo repo) { }\n}\n",
        ),
    ]);
    let root = no_git_root();
    // App can reach Sql and not Files -- exactly the shape that settles a
    // ladder ambiguity, applied to a question the ladder never asked.
    let model = model_of(vec![
        unit(
            "src/App/App.csproj",
            &["src/Contracts/Contracts.csproj", "src/Sql/Sql.csproj"],
            false,
        ),
        unit("src/Contracts/Contracts.csproj", &[], false),
        unit(
            "src/Files/Files.csproj",
            &["src/Contracts/Contracts.csproj"],
            false,
        ),
        unit(
            "src/Sql/Sql.csproj",
            &["src/Contracts/Contracts.csproj"],
            false,
        ),
    ]);

    let ctor_di = |g: &Graph| -> (String, Vec<String>) {
        match find_edge(g, |e| matches!(e, Edge::CtorDi { .. })).expect("ctor-di edge present") {
            Edge::CtorDi {
                resolution,
                candidates,
                ..
            } => (
                resolution.clone(),
                candidates.iter().map(|c| c.id.clone()).collect(),
            ),
            _ => unreachable!(),
        }
    };
    assert_eq!(
        ctor_di(&resolve_graph_with_model(&root, &files, &[], Some(&model))),
        ctor_di(&resolve_graph(&root, &files)),
        "two implementors is two implementors, model or no model"
    );
    assert_eq!(
        ctor_di(&resolve_graph(&root, &files)),
        (
            "ambiguous".to_string(),
            vec![
                "Fixture.Files.FileRepo".to_string(),
                "Fixture.Sql.SqlRepo".to_string()
            ]
        ),
        "pinned so the assertion above cannot pass on two identically-broken answers"
    );
}

#[test]
fn stage6_narrowing_to_zero_gives_the_scored_tier_an_empty_pool_not_a_graph_wide_guess() {
    // Narrowing can also empty the candidate list, and the result is an
    // ordinary External -- not a silently-kept ambiguity and not an
    // invented pick. For the type ref that means the unresolved counter
    // rather than the ambiguous one.
    //
    // For the QUALIFIER it means no heuristic edge at all. The ladder did
    // find candidates here; the project model answered that none of them
    // is nameable at this site. That is an answer, so the scored tier gets
    // an empty pool rather than the graph-wide member-name uniqueness pool
    // an unfound name would get. `Ledger` is the proof the tier really
    // declines: it is not a `Config` at all, it is reachable from `App`,
    // and it declares `Load` -- so it is exactly the stranger the
    // uniqueness pool would have handed over.
    let mut files: Vec<(&str, &str)> = CROSS_PROJECT_AMBIGUITY_FIXTURE.to_vec();
    files.push((
        "src/Shared/Ledger.cs",
        "namespace Fixture.Shared { public class Ledger { public void Load() { } } }",
    ));
    let files = fragments_for(&files);
    let root = no_git_root();

    assert_eq!(
        heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
        vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
        "without a model the ladder's ambiguous pool wins and Ledger is never in the running"
    );

    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/App/App.csproj", &["src/Shared/Shared.csproj"], false),
        unit("src/Beta/Beta.csproj", &[], false),
        unit("src/Shared/Shared.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

    assert!(
        ambiguous_edges_from(&g, "src/App/Runner.cs").is_empty(),
        "neither Config is nameable here, so there is nothing left to be ambiguous between"
    );
    assert_eq!(
        (g.stats.ambiguous_count, g.stats.unresolved_external_count),
        (0, 1),
        "the type ref moves from the ambiguous count to the external one, which is what an emptied pool MEANS"
    );
    assert!(
        member_edges_from(&g, "src/App/Runner.cs").is_empty(),
        "no precise edge is invented out of an empty candidate list"
    );
    assert!(
        heuristic_member_edges_from(&g, "src/App/Runner.cs").is_empty(),
        "every real `Config` candidate was ruled unreachable, which is an ANSWER -- the tier must not answer it again with a reachable stranger that merely declares `Load`"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "and nothing counted either: a declined guess is not a guess"
    );
}

#[test]
fn stage6_a_bare_qualifier_narrowed_to_zero_declines_while_an_unfound_one_still_guesses() {
    // The two `External`s the scored tier must tell apart, in one resolve
    // and one file:
    //   `Foo.Bar()`     -- two real `Foo` candidates, neither reachable
    //                      from `App`. Narrowed to zero, so the tier
    //                      declines even though reachable `Ledger`
    //                      declares `Bar`.
    //   `Missing.Bar()` -- a name the ladder never found at all. Nothing
    //                      was ever narrowed, so the member-name
    //                      uniqueness pool applies as it always has and
    //                      `Ledger` IS the guess.
    // Without the split, both lines would guess `Ledger`.
    let files = fragments_for(&[
        (
            "src/Alpha/Foo.cs",
            "namespace Fixture.Alpha { public class Foo { public void Bar() { } } }",
        ),
        (
            "src/Beta/Foo.cs",
            "namespace Fixture.Beta { public class Foo { public void Bar() { } } }",
        ),
        (
            "src/Shared/Ledger.cs",
            "namespace Fixture.Shared { public class Ledger { public void Bar() { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    Foo.Bar();\n    Missing.Bar();\n  }\n}\n",
        ),
    ]);
    let root = no_git_root();

    let model = model_of(vec![
        unit("src/Alpha/Alpha.csproj", &[], false),
        unit("src/App/App.csproj", &["src/Shared/Shared.csproj"], false),
        unit("src/Beta/Beta.csproj", &[], false),
        unit("src/Shared/Shared.csproj", &[], false),
    ]);
    let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

    assert_eq!(
        heuristic_member_edges_from(&g, "src/App/Runner.cs"),
        vec![("Fixture.Shared.Ledger", 9)],
        "line 8's `Foo` was narrowed to zero and declines; line 9's `Missing` was never found and still reaches the uniqueness pool"
    );
    assert!(
        member_edges_from(&g, "src/App/Runner.cs").is_empty(),
        "no precise edge on either line"
    );
}
