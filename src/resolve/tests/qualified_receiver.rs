use super::*;

// Real C# through this crate's own extractor, so a fact that never gets
// recorded fails here rather than passing vacuously.

fn line_of(source: &str, marker: &str) -> usize {
    source
        .lines()
        .position(|line| line.contains(marker))
        .expect("fixture marker is present")
        + 1
}

/// One fixture source converted exactly as the fixture files are, kept
/// separately so a control can read the receiver facts the resolver reads.
fn fragment_of(source: &str) -> Fragment {
    crate::graph::fragment_from_extraction(&crate::extract::extract(source))
}

/// The DISTINCT written paths one receiver name records in a fragment -- a
/// qualifier survives extraction only if every ref of that name carries it.
fn written_paths(fragment: &Fragment, name: &str) -> Vec<Option<String>> {
    let mut paths: Vec<Option<String>> = fragment
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.name == name)
        .map(|r| r.receiver_type_qualified.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// One file's PRECISE member edges as (target id, target file, line), sorted --
/// the target file is what tells two defs sharing one id apart.
fn precise_edges_with_file<'a>(g: &'a Graph, file: &str) -> Vec<(&'a str, &'a str, usize)> {
    let mut edges: Vec<(&str, &str, usize)> = member_edges_with_file(g, file)
        .into_iter()
        .filter(|(_, _, _, tier)| tier.is_none())
        .map(|(to, line, to_file, _)| (to, to_file, line))
        .collect();
    edges.sort_unstable();
    edges
}

// --- A qualified receiver declaration -----------------------------------
//
// A receiver declared with a qualified type must resolve against that
// WRITTEN type. Reducing it to its last segment hands the lookup to the
// bare-name steps -- a `using` directive, then graph-wide uniqueness -- which
// answer with whatever same-named type those steps happen to reach, and a
// qualified receiver is precisely the case where that type is not the one
// the declaration named.

const QUALIFIED_PATTERN_FIXTURE: &[(&str, &str)] = &[
    (
        "Middleware/Outbox/OutboxSendEndpoint.cs",
        "\nnamespace App.Middleware.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
    ),
    (
        "Middleware/InMemoryOutbox/OutboxSendEndpoint.cs",
        "\nnamespace App.Middleware.InMemoryOutbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
    ),
];

#[test]
fn a_qualified_pattern_receiver_binds_its_written_type_not_the_using_sibling() {
    let mut files = fragments_for(QUALIFIED_PATTERN_FIXTURE);
    files.push((
        "Middleware/InternalOutboxExtensions.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(
            "\nnamespace App.Middleware\n{\n    using InMemoryOutbox;\n\n    public static class InternalOutboxExtensions\n    {\n        public static void Skip(IEndpoint endpoint)\n        {\n            if (endpoint is Outbox.OutboxSendEndpoint outboxEndpoint)\n            {\n                var found = outboxEndpoint.Endpoint;\n            }\n        }\n    }\n}\n",
        )),
    ));
    assert_eq!(
        member_edge_targets(&resolve_graph(&no_git_root(), &files)),
        vec!["App.Middleware.Outbox.OutboxSendEndpoint"],
        "the written `Outbox.` qualifier names the type the `using InMemoryOutbox;` does not"
    );

    // The control: the SAME site with the type written bare. Nothing but the
    // qualifier distinguishes the two declarations, so the bare name is what
    // the using-relative step answers -- correct for that source, and the
    // answer the qualified source must not inherit.
    let mut bare = fragments_for(QUALIFIED_PATTERN_FIXTURE);
    bare.push((
        "Middleware/InternalOutboxExtensions.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(
            "\nnamespace App.Middleware\n{\n    using InMemoryOutbox;\n\n    public static class InternalOutboxExtensions\n    {\n        public static void Skip(IEndpoint endpoint)\n        {\n            if (endpoint is OutboxSendEndpoint outboxEndpoint)\n            {\n                var found = outboxEndpoint.Endpoint;\n            }\n        }\n    }\n}\n",
        )),
    ));
    assert_eq!(
        member_edge_targets(&resolve_graph(&no_git_root(), &bare)),
        vec!["App.Middleware.InMemoryOutbox.OutboxSendEndpoint"]
    );
}

#[test]
fn a_qualified_field_receiver_binds_the_nested_declared_type_over_a_top_level_decoy() {
    let files = fragments_for(&[
        (
            "Rabbit/ConfigurationHostSettings.cs",
            "\nnamespace App.Rabbit.Configuration\n{\n    public class ConfigurationHostSettings\n    {\n        public class ConfigurationBatchSettings\n        {\n            public bool Enabled { get; set; }\n        }\n    }\n}\n",
        ),
        (
            "Bench/ConfigurationBatchSettings.cs",
            "\nnamespace App.Bench;\n\npublic class ConfigurationBatchSettings\n{\n    public bool Enabled { get; set; }\n}\n",
        ),
        (
            "Rabbit/RabbitMqBatchPublishConfigurator.cs",
            "\nnamespace App.Rabbit.Configuration\n{\n    internal class RabbitMqBatchPublishConfigurator\n    {\n        readonly ConfigurationHostSettings.ConfigurationBatchSettings _settings;\n\n        public bool Enabled\n        {\n            set => _settings.Enabled = value;\n        }\n    }\n}\n",
        ),
    ]);
    assert_eq!(
        member_edge_targets(&resolve_graph(&no_git_root(), &files)),
        vec!["App.Rabbit.Configuration.ConfigurationHostSettings+ConfigurationBatchSettings"],
        "the field's written `ConfigurationHostSettings.` names the nested type, not the \
         top-level decoy sharing its last segment"
    );
}

#[test]
fn a_qualified_receiver_whose_written_type_is_absent_never_falls_back_to_a_same_tail_decoy() {
    // Only the sibling exists, so the written path resolves to nothing at
    // all. Finishing empty is the answer: retrying the bare tail here is
    // what produces a precise edge to a type the declaration never named.
    let mut files = fragments_for(&[QUALIFIED_PATTERN_FIXTURE[1]]);
    files.push((
        "Middleware/InternalOutboxExtensions.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(
            "\nnamespace App.Middleware\n{\n    public static class InternalOutboxExtensions\n    {\n        public static void Skip(IEndpoint endpoint)\n        {\n            if (endpoint is Outbox.OutboxSendEndpoint outboxEndpoint)\n            {\n                var found = outboxEndpoint.Endpoint;\n            }\n        }\n    }\n}\n",
        )),
    ));
    assert_eq!(
        member_edge_targets(&resolve_graph(&no_git_root(), &files)),
        Vec::<&str>::new()
    );
}

#[test]
fn a_qualified_receiver_matching_two_same_tail_types_stays_ambiguous() {
    let files = fragments_for(&[
        (
            "One/OutboxSendEndpoint.cs",
            "\nnamespace App.One.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            "Two/OutboxSendEndpoint.cs",
            "\nnamespace App.Two.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            "Three/Skip.cs",
            "\nnamespace App.Three;\n\npublic static class SkipExtensions\n{\n    public static void Skip(IEndpoint endpoint)\n    {\n        if (endpoint is Outbox.OutboxSendEndpoint outboxEndpoint)\n        {\n            var found = outboxEndpoint.Endpoint;\n        }\n    }\n}\n",
        ),
    ]);
    assert_eq!(
        member_edge_targets(&resolve_graph(&no_git_root(), &files)),
        Vec::<&str>::new(),
        "two defs end with the written path, so neither may earn a precise edge"
    );
}

#[test]
fn a_qualified_field_receiver_binds_the_nested_type_under_a_project_model_too() {
    let root = no_git_root();
    let files = fragments_for(&[
        (
            "src/Rabbit/ConfigurationHostSettings.cs",
            "\nnamespace App.Rabbit.Configuration\n{\n    public class ConfigurationHostSettings\n    {\n        public class ConfigurationBatchSettings\n        {\n            public bool Enabled { get; set; }\n        }\n    }\n}\n",
        ),
        (
            "src/Bench/ConfigurationBatchSettings.cs",
            "\nnamespace App.Bench;\n\npublic class ConfigurationBatchSettings\n{\n    public bool Enabled { get; set; }\n}\n",
        ),
        (
            "src/App/RabbitMqBatchPublishConfigurator.cs",
            "\nnamespace App.Rabbit.Configuration\n{\n    internal class RabbitMqBatchPublishConfigurator\n    {\n        readonly ConfigurationHostSettings.ConfigurationBatchSettings _settings;\n\n        public bool Enabled\n        {\n            set => _settings.Enabled = value;\n        }\n    }\n}\n",
        ),
    ]);
    let model = model_of(vec![
        unit("src/App/App.csproj", &["src/Rabbit/Rabbit.csproj"], false),
        unit("src/Rabbit/Rabbit.csproj", &[], false),
        unit("src/Bench/Bench.csproj", &[], false),
    ]);
    assert_eq!(
        member_edge_targets(&resolve_graph_with_model(
            &root,
            &files,
            &[],
            Some(&model),
            None
        )),
        vec!["App.Rabbit.Configuration.ConfigurationHostSettings+ConfigurationBatchSettings"],
        "the written path answers before any reachability narrowing, exactly as it does with no \
         model at all"
    );
}

#[test]
fn a_qualified_receiver_ambiguous_answer_is_still_narrowed_by_project_reachability() {
    // Two defs end with the written path, so the qualifier alone cannot
    // choose. The reachability filter that already narrows an ambiguous
    // bare-name answer must keep working on a qualified one, or the
    // qualifier would turn a filterable ambiguity into a silent refusal.
    let root = no_git_root();
    let files = fragments_for(&[
        (
            "src/One/OutboxSendEndpoint.cs",
            "\nnamespace App.One.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            "src/Two/OutboxSendEndpoint.cs",
            "\nnamespace App.Two.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            "src/App/Skip.cs",
            "\nnamespace App.Skip;\n\npublic static class SkipExtensions\n{\n    public static void Skip(IEndpoint endpoint)\n    {\n        if (endpoint is Outbox.OutboxSendEndpoint outboxEndpoint)\n        {\n            var found = outboxEndpoint.Endpoint;\n        }\n    }\n}\n",
        ),
    ]);
    let reaches_one = model_of(vec![
        unit("src/App/App.csproj", &["src/One/One.csproj"], false),
        unit("src/One/One.csproj", &[], false),
        unit("src/Two/Two.csproj", &[], false),
    ]);
    assert_eq!(
        member_edge_targets(&resolve_graph_with_model(
            &root,
            &files,
            &[],
            Some(&reaches_one),
            None
        )),
        vec!["App.One.Outbox.OutboxSendEndpoint"]
    );
}

// --- The three receiver controls ---------------------------------------
//
// A qualified receiver and a bare one, a written path whose containing
// segment carries a type-argument list, and an extension-only member: each
// shape answers through a different step, so each needs its own control
// rather than the coverage another test happens to give the neighbouring
// shape.

const GENERIC_CONTROL_DEFS: &[(&str, &str)] = &[
    (
        "App/Widgets/BoxPlain.cs",
        "namespace App.Widgets { public class Box { public void Render() { } public void Empty() { } } }",
    ),
    (
        "App/Widgets/BoxGeneric.cs",
        "namespace App.Widgets { public class Box<T> { public void Render() { } public void Fill() { } } }",
    ),
];

const GENERIC_CONTROL_SITE: &str = r#"
using App.Widgets;

namespace App
{
    public class Site
    {
        public void Run()
        {
            Widgets.Box<int> qualifiedGeneric = null;
            Widgets.Box qualifiedPlain = null;
            Box<int> bareGeneric = null;
            Box barePlain = null;
            qualifiedGeneric.Render();
            qualifiedPlain.Render();
            bareGeneric.Render();
            barePlain.Render();
            qualifiedGeneric.Fill();
            bareGeneric.Fill();
            qualifiedPlain.Empty();
            barePlain.Empty();
            qualifiedGeneric.Empty();
            bareGeneric.Empty();
            qualifiedPlain.Fill();
            barePlain.Fill();
        }
    }
}
"#;

fn generic_control_graph(plain_first: bool) -> Graph {
    let mut files = fragments_for(GENERIC_CONTROL_DEFS);
    if !plain_first {
        files.swap(0, 1);
    }
    files.push(("App/Site.cs".to_string(), fragment_of(GENERIC_CONTROL_SITE)));
    resolve_graph(&no_git_root(), &files)
}

#[test]
fn qualified_receiver_control_ac_1_each_spelling_binds_its_aritys_sibling() {
    for plain_first in [true, false] {
        let g = generic_control_graph(plain_first);
        assert_eq!(
            precise_edges_with_file(&g, "App/Site.cs"),
            vec![
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxGeneric.cs",
                    line_of(GENERIC_CONTROL_SITE, "qualifiedGeneric.Render()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxGeneric.cs",
                    line_of(GENERIC_CONTROL_SITE, "bareGeneric.Render()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxGeneric.cs",
                    line_of(GENERIC_CONTROL_SITE, "qualifiedGeneric.Fill()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxGeneric.cs",
                    line_of(GENERIC_CONTROL_SITE, "bareGeneric.Fill()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxPlain.cs",
                    line_of(GENERIC_CONTROL_SITE, "qualifiedPlain.Render()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxPlain.cs",
                    line_of(GENERIC_CONTROL_SITE, "barePlain.Render()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxPlain.cs",
                    line_of(GENERIC_CONTROL_SITE, "qualifiedPlain.Empty()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxPlain.cs",
                    line_of(GENERIC_CONTROL_SITE, "barePlain.Empty()")
                ),
            ],
            "`Render` and `Fill` take the one-parameter def and `Render` and `Empty` the \
             arity-zero one, for the qualified and the bare spelling alike, in either \
             declaration order"
        );
        for marker in [
            "qualifiedGeneric.Empty()",
            "bareGeneric.Empty()",
            "qualifiedPlain.Fill()",
            "barePlain.Fill()",
        ] {
            assert!(
                !precise_edges_with_file(&g, "App/Site.cs")
                    .iter()
                    .any(|(_, _, line)| *line == line_of(GENERIC_CONTROL_SITE, marker)),
                "`{marker}` names a member its own def does not declare, and the sibling's \
                 same-named member is not the answer"
            );
        }
    }
    let site = fragment_of(GENERIC_CONTROL_SITE);
    assert_eq!(
        written_paths(&site, "qualifiedGeneric"),
        vec![Some("Widgets.Box<int>".to_string())],
        "the written path keeps the argument list the declaration wrote"
    );
    assert_eq!(
        written_paths(&site, "qualifiedPlain"),
        vec![Some("Widgets.Box".to_string())]
    );
    assert_eq!(
        written_paths(&site, "bareGeneric"),
        vec![None],
        "the bare spelling writes no path, so the arity step alone answers it"
    );
    assert_eq!(written_paths(&site, "barePlain"), vec![None]);
}

const CONTAINER_CONTROL_FILES: &[(&str, &str)] = &[
    (
        "App/OuterContainer.cs",
        "namespace App { public class Outer<T> { public class Inner { public void Go() { } } } }",
    ),
    (
        "App/InnerDecoy.cs",
        "namespace App { public class Inner { public void Go() { } } }",
    ),
];

const CONTAINER_CONTROL_SITE: &str = r#"
namespace App
{
    public class Site
    {
        private Outer<int>.Inner _field;

        public void Run(Outer<int>.Inner parameter)
        {
            Outer<int>.Inner local = null;
            Inner bare = null;
            parameter.Go();
            local.Go();
            _field.Go();
            bare.Go();
            if (parameter is Outer<int>.Inner pattern)
            {
                pattern.Go();
            }
        }
    }
}
"#;

fn container_control_graph() -> Graph {
    let mut files = fragments_for(CONTAINER_CONTROL_FILES);
    files.push((
        "App/Site.cs".to_string(),
        fragment_of(CONTAINER_CONTROL_SITE),
    ));
    resolve_graph(&no_git_root(), &files)
}

#[test]
fn qualified_receiver_control_ac_3_a_generic_containers_nested_type_binds_over_the_decoy() {
    let g = container_control_graph();
    assert_eq!(
        precise_edges_with_file(&g, "App/Site.cs"),
        vec![
            (
                "App.Inner",
                "App/InnerDecoy.cs",
                line_of(CONTAINER_CONTROL_SITE, "bare.Go()")
            ),
            (
                "App.Outer+Inner",
                "App/OuterContainer.cs",
                line_of(CONTAINER_CONTROL_SITE, "parameter.Go()")
            ),
            (
                "App.Outer+Inner",
                "App/OuterContainer.cs",
                line_of(CONTAINER_CONTROL_SITE, "local.Go()")
            ),
            (
                "App.Outer+Inner",
                "App/OuterContainer.cs",
                line_of(CONTAINER_CONTROL_SITE, "_field.Go()")
            ),
            (
                "App.Outer+Inner",
                "App/OuterContainer.cs",
                line_of(CONTAINER_CONTROL_SITE, "pattern.Go()")
            ),
        ],
        "every `Outer<int>.Inner` receiver takes the nested type and only the bare `Inner` \
         takes the decoy, so a dropped list on the containing segment would show up as a \
         decoy edge"
    );
    let site = fragment_of(CONTAINER_CONTROL_SITE);
    for name in ["parameter", "local", "_field", "pattern"] {
        assert_eq!(
            written_paths(&site, name),
            vec![Some("Outer<int>.Inner".to_string())],
            "`{name}` declares the type with its argument list"
        );
    }
    assert_eq!(written_paths(&site, "bare"), vec![None]);
}

const EXTENSION_CONTROL_FILES: &[(&str, &str)] = &[
    (
        "App/Widgets/Widget.cs",
        "namespace App.Widgets { public class Widget { public void Go() { } } }",
    ),
    (
        "App/Widgets/WidgetExtensions.cs",
        "namespace App.Widgets { public static class WidgetExtensions { public static void Polish(this Widget widget) { } } }",
    ),
];

const EXTENSION_CONTROL_SITE: &str = r#"
using App.Widgets;

namespace App
{
    public class Site
    {
        public void Run(Widgets.Widget qualified, Widget bare)
        {
            qualified.Polish();
            bare.Polish();
            qualified.Go();
            bare.Go();
        }
    }
}
"#;

fn extension_control_graph() -> Graph {
    let mut files = fragments_for(EXTENSION_CONTROL_FILES);
    files.push((
        "App/Site.cs".to_string(),
        fragment_of(EXTENSION_CONTROL_SITE),
    ));
    resolve_graph(&no_git_root(), &files)
}

#[test]
fn qualified_receiver_control_ac_4_an_extension_only_member_keeps_its_extension_edge() {
    let g = extension_control_graph();
    assert_eq!(
        precise_edges_with_file(&g, "App/Site.cs"),
        vec![
            (
                "App.Widgets.Widget",
                "App/Widgets/Widget.cs",
                line_of(EXTENSION_CONTROL_SITE, "qualified.Go()")
            ),
            (
                "App.Widgets.Widget",
                "App/Widgets/Widget.cs",
                line_of(EXTENSION_CONTROL_SITE, "bare.Go()")
            ),
        ],
        "the instance member answers both spellings, and no extension competes with it"
    );
    let mut guessed: Vec<(&str, usize)> = member_edges_with_file(&g, "App/Site.cs")
        .into_iter()
        .filter(|(_, _, _, tier)| tier.is_some())
        .map(|(to, line, _, _)| (to, line))
        .collect();
    guessed.sort_unstable();
    assert_eq!(
        guessed,
        vec![
            (
                "App.Widgets.WidgetExtensions",
                line_of(EXTENSION_CONTROL_SITE, "qualified.Polish()")
            ),
            (
                "App.Widgets.WidgetExtensions",
                line_of(EXTENSION_CONTROL_SITE, "bare.Polish()")
            ),
        ],
        "an extension-only member is reachable from a qualified receiver exactly as from a \
         bare one"
    );
    assert!(
        member_edges_with_file(&g, "App/Site.cs")
            .iter()
            .filter(|(_, _, _, tier)| tier.is_some())
            .all(|(_, _, _, tier)| *tier == Some(HeuristicTier::Ext)),
        "each guessed edge is the extension tier"
    );
    let site = fragment_of(EXTENSION_CONTROL_SITE);
    assert_eq!(
        written_paths(&site, "qualified"),
        vec![Some("Widgets.Widget".to_string())]
    );
    assert_eq!(written_paths(&site, "bare"), vec![None]);
}
