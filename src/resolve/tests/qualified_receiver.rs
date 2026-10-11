use super::*;

// Real C# through this crate's own extractor, so a fact that never gets
// recorded fails here rather than passing vacuously.

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
        member_edge_targets(&resolve_graph_with_model(&root, &files, &[], Some(&model), None)),
        vec!["App.Rabbit.Configuration.ConfigurationHostSettings+ConfigurationBatchSettings"],
        "the written path answers before any reachability narrowing, exactly as it does with no \n         model at all"
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
