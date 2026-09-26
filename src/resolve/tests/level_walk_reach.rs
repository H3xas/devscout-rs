use super::*;

// C# cannot name a type declared in an assembly the site's project does not
// reference, so with a project model the namespace-level walk passes over a
// level's own type, or a using-imported candidate, declared only in a project
// outside the site project's reference closure. Every fixture except the
// graph-wide-unique one declares two same-named types, so the unique-name
// fallback can never be the step that answers.

fn member_targets<'a>(g: &'a Graph, from: &str, member: &str) -> Vec<&'a str> {
    member_edges_named(g, from, member)
        .into_iter()
        .map(|(to, _)| to)
        .collect()
}

fn with_model(files: &[(String, Fragment)], units: Vec<crate::project::Unit>) -> Graph {
    let model = model_of(units);
    resolve_graph_with_model(&no_git_root(), files, &[], Some(&model), None)
}

const COMPOSITE_FILTER_FIXTURE: &[(&str, &str)] = &[
    (
        "src/Abs/Filters/CompositeFilter.cs",
        "namespace App.Filters { public class CompositeFilter<T> { public void Includes(T x) { } } }",
    ),
    (
        "src/Core/Configuration/CompositeFilter.cs",
        "namespace App.Configuration { public class CompositeFilter<T> { public void Includes(T x) { } } }",
    ),
    (
        "src/Abs/Configuration/ExceptionSpecification.cs",
        "namespace App.Configuration\n{\n    using Filters;\n\n    public class ExceptionSpecification\n    {\n        readonly CompositeFilter<int> _f;\n\n        public void Include(int x)\n        {\n            _f.Includes(x);\n        }\n    }\n}\n",
    ),
];

fn abs_and_core() -> Vec<crate::project::Unit> {
    vec![
        unit("src/Abs/Abs.csproj", &[], false),
        unit("src/Core/Core.csproj", &["src/Abs/Abs.csproj"], false),
    ]
}

#[test]
fn a_namespace_level_type_in_an_unreferenced_project_yields_to_the_same_level_using() {
    let files = fragments_for(COMPOSITE_FILTER_FIXTURE);
    let site = "src/Abs/Configuration/ExceptionSpecification.cs";

    let g = with_model(&files, abs_and_core());
    assert_eq!(
        type_edge_targets_from(&g, site),
        vec!["App.Filters.CompositeFilter"]
    );
    assert_eq!(
        member_targets(&g, site, "Includes"),
        vec!["App.Filters.CompositeFilter"]
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, site),
        vec!["App.Configuration.CompositeFilter"]
    );
    assert_eq!(
        member_targets(&g, site, "Includes"),
        vec!["App.Configuration.CompositeFilter"]
    );
}

#[test]
fn a_namespace_level_enum_in_an_unreferenced_project_yields_to_a_using_read_from_that_level() {
    let files = fragments_for(&[
        (
            "src/Rmq/Topology/Options.cs",
            "namespace App { public enum Options { None, MaintainHierarchy } }",
        ),
        (
            "src/Amq/Topology/Options.cs",
            "namespace App.Amq.Topology { public enum Options { None, MaintainHierarchy } }",
        ),
        (
            "src/Amq/Topology/IPublishTopology.cs",
            "namespace App\n{\n    using Amq.Topology;\n\n    public class PublishTopology\n    {\n        Options _options;\n\n        public void Set()\n        {\n            var o = Options.MaintainHierarchy;\n        }\n    }\n}\n",
        ),
    ]);
    let site = "src/Amq/Topology/IPublishTopology.cs";
    let g = with_model(
        &files,
        vec![
            unit("src/Amq/Amq.csproj", &[], false),
            unit("src/Rmq/Rmq.csproj", &[], false),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, site),
        vec!["App.Amq.Topology.Options"]
    );
    assert_eq!(
        member_targets(&g, site, "MaintainHierarchy"),
        vec!["App.Amq.Topology.Options.MaintainHierarchy"]
    );
}

const FEATURE_WIDGET_SITE: (&str, &str) = (
    "src/Site/Feature/Panel.cs",
    "namespace App.Feature\n{\n    public class Panel\n    {\n        Widget _widget;\n    }\n}\n",
);

#[test]
fn an_unreachable_enclosing_namespace_type_yields_to_an_outer_level_type() {
    let files = fragments_for(&[
        (
            "src/Other/Feature/Widget.cs",
            "namespace App.Feature { public class Widget { } }",
        ),
        (
            "src/Base/Widget.cs",
            "namespace App { public class Widget { } }",
        ),
        FEATURE_WIDGET_SITE,
    ]);
    let site = FEATURE_WIDGET_SITE.0;
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &["src/Base/Base.csproj"], false),
            unit("src/Base/Base.csproj", &[], false),
            unit("src/Other/Other.csproj", &[], false),
        ],
    );
    assert_eq!(type_edge_targets_from(&g, site), vec!["App.Widget"]);

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(type_edge_targets_from(&g, site), vec!["App.Feature.Widget"]);
}

#[test]
fn an_unreachable_candidate_leaves_one_using_at_the_level_and_no_ambiguity() {
    let files = fragments_for(&[
        ("src/Near/T.cs", "namespace X { public class T { } }"),
        ("src/Far/T.cs", "namespace Y { public class T { } }"),
        (
            "src/Site/Holder.cs",
            "namespace A\n{\n    using X;\n    using Y;\n\n    public class Holder\n    {\n        T _t;\n    }\n}\n",
        ),
    ]);
    let site = "src/Site/Holder.cs";
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &["src/Near/Near.csproj"], false),
            unit("src/Near/Near.csproj", &[], false),
            unit("src/Far/Far.csproj", &[], false),
        ],
    );
    assert_eq!(type_edge_targets_from(&g, site), vec!["X.T"]);
    assert!(ambiguous_edges_from(&g, site).is_empty());
    assert_eq!(g.stats.ambiguous_count, 0);
}

#[test]
fn an_enclosing_namespace_type_still_answers_first_when_both_candidates_are_reachable() {
    let files = fragments_for(&[
        (
            "src/Framework/TestInstance.cs",
            "namespace App.Framework.Sagas { public class TestInstance { public int Key { get; set; } } }",
        ),
        (
            "tests/Tests/Container_Specs.cs",
            "namespace App.Tests\n{\n    namespace ContainerTests\n    {\n        using Framework.Sagas;\n\n        public class TestInstance { public int Key { get; set; } }\n\n        public class Spec\n        {\n            TestInstance _instance;\n\n            public int Read(TestInstance x)\n            {\n                return x.Key;\n            }\n        }\n    }\n}\n",
        ),
    ]);
    let site = "tests/Tests/Container_Specs.cs";
    let g = with_model(
        &files,
        vec![
            unit("src/Framework/Framework.csproj", &[], false),
            unit(
                "tests/Tests/Tests.csproj",
                &["src/Framework/Framework.csproj"],
                true,
            ),
        ],
    );
    let types = type_edge_targets_from(&g, site);
    assert!(
        types.contains(&"App.Tests.ContainerTests.TestInstance"),
        "{types:?}"
    );
    assert!(
        !types.contains(&"App.Framework.Sagas.TestInstance"),
        "{types:?}"
    );
    assert_eq!(
        member_targets(&g, site, "Key"),
        vec!["App.Tests.ContainerTests.TestInstance"]
    );
}

#[test]
fn an_unowned_site_file_keeps_the_unfiltered_answer() {
    let site = ("loose/Panel.cs", FEATURE_WIDGET_SITE.1);
    let files = fragments_for(&[
        (
            "src/Other/Feature/Widget.cs",
            "namespace App.Feature { public class Widget { } }",
        ),
        (
            "src/Base/Widget.cs",
            "namespace App { public class Widget { } }",
        ),
        site,
    ]);
    let g = with_model(
        &files,
        vec![
            unit("src/Base/Base.csproj", &[], false),
            unit("src/Other/Other.csproj", &[], false),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, site.0),
        vec!["App.Feature.Widget"]
    );
}

#[test]
fn an_unowned_declaring_file_keeps_the_unfiltered_answer() {
    let files = fragments_for(&[
        (
            "loose/Feature/Widget.cs",
            "namespace App.Feature { public class Widget { } }",
        ),
        (
            "src/Base/Widget.cs",
            "namespace App { public class Widget { } }",
        ),
        FEATURE_WIDGET_SITE,
    ]);
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &["src/Base/Base.csproj"], false),
            unit("src/Base/Base.csproj", &[], false),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, FEATURE_WIDGET_SITE.0),
        vec!["App.Feature.Widget"]
    );
}

#[test]
fn a_partial_type_with_one_part_in_a_reachable_project_is_not_passed_over() {
    let files = fragments_for(&[
        (
            "src/Other/Feature/Widget.cs",
            "namespace App.Feature { public partial class Widget { } }",
        ),
        (
            "src/Near/Feature/Widget.cs",
            "namespace App.Feature { public partial class Widget { } }",
        ),
        (
            "src/Base/Widget.cs",
            "namespace App { public class Widget { } }",
        ),
        FEATURE_WIDGET_SITE,
    ]);
    let g = with_model(
        &files,
        vec![
            unit(
                "src/Site/Site.csproj",
                &["src/Base/Base.csproj", "src/Near/Near.csproj"],
                false,
            ),
            unit("src/Base/Base.csproj", &[], false),
            unit("src/Near/Near.csproj", &[], false),
            unit("src/Other/Other.csproj", &[], false),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, FEATURE_WIDGET_SITE.0),
        vec!["App.Feature.Widget"]
    );
}

#[test]
fn a_referenced_test_project_is_inside_the_closure_of_a_non_test_site() {
    let files = fragments_for(&[
        (
            "tests/Helpers/Feature/Widget.cs",
            "namespace App.Feature { public class Widget { } }",
        ),
        (
            "src/Base/Widget.cs",
            "namespace App { public class Widget { } }",
        ),
        FEATURE_WIDGET_SITE,
    ]);
    let g = with_model(
        &files,
        vec![
            unit(
                "src/Site/Site.csproj",
                &["src/Base/Base.csproj", "tests/Helpers/Helpers.csproj"],
                false,
            ),
            unit("src/Base/Base.csproj", &[], false),
            unit("tests/Helpers/Helpers.csproj", &[], true),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, FEATURE_WIDGET_SITE.0),
        vec!["App.Feature.Widget"]
    );
}

#[test]
fn a_base_named_at_its_declaration_resolves_within_the_declaring_project_closure() {
    let files = fragments_for(&[
        (
            "src/Other/Lib/Base.cs",
            "namespace App.Lib { public class Base { public void Run() { } } }",
        ),
        (
            "src/Lib/Bases/Base.cs",
            "namespace App.Lib.Bases { public class Base { public void Run() { } } }",
        ),
        (
            "src/Lib/Derived.cs",
            "namespace App.Lib\n{\n    using Bases;\n\n    public class Derived : Base { }\n}\n",
        ),
        (
            "src/Site/Runner.cs",
            "namespace App.Site\n{\n    using App.Lib;\n\n    public class Runner\n    {\n        public void Go(Derived d)\n        {\n            d.Run();\n        }\n    }\n}\n",
        ),
    ]);
    let site = "src/Site/Runner.cs";
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &["src/Lib/Lib.csproj"], false),
            unit("src/Lib/Lib.csproj", &[], false),
            unit("src/Other/Other.csproj", &[], false),
        ],
    );
    assert_eq!(member_targets(&g, site, "Run"), vec!["App.Lib.Bases.Base"]);

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_targets(&g, site, "Run"), vec!["App.Lib.Base"]);
}

#[test]
fn a_passed_over_type_with_a_graph_unique_name_still_binds_at_the_unique_name_step() {
    let files = fragments_for(&[
        (
            "src/Other/Feature/Gadget.cs",
            "namespace App.Feature { public class Gadget { } }",
        ),
        (
            "src/Site/Feature/Panel.cs",
            "namespace App.Feature\n{\n    public class Panel\n    {\n        Gadget _gadget;\n    }\n}\n",
        ),
    ]);
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &[], false),
            unit("src/Other/Other.csproj", &[], false),
        ],
    );
    assert_eq!(
        type_edge_targets_from(&g, "src/Site/Feature/Panel.cs"),
        vec!["App.Feature.Gadget"]
    );
}

#[test]
fn a_directive_whose_own_reading_is_passed_over_is_not_read_further_out() {
    let files = fragments_for(&[
        ("src/Far/T.cs", "namespace A.B.Sub { public class T { } }"),
        ("src/Near/T.cs", "namespace Sub { public class T { } }"),
        ("src/Near/Other/T.cs", "namespace Other { public class T { } }"),
        (
            "src/Site/Holder.cs",
            "namespace A.B\n{\n    using Sub;\n\n    public class Holder\n    {\n        T _t;\n    }\n}\n",
        ),
    ]);
    let site = "src/Site/Holder.cs";
    let g = with_model(
        &files,
        vec![
            unit("src/Site/Site.csproj", &["src/Near/Near.csproj"], false),
            unit("src/Near/Near.csproj", &[], false),
            unit("src/Far/Far.csproj", &[], false),
        ],
    );
    assert!(
        type_edge_targets_from(&g, site).is_empty(),
        "{:?}",
        type_edge_targets_from(&g, site)
    );
    let ambiguous = ambiguous_edges_from(&g, site);
    assert_eq!(ambiguous.len(), 1, "{ambiguous:?}");
    assert_eq!(ambiguous[0].2, vec!["Other.T", "Sub.T"]);
}
