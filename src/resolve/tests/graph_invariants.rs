use super::*;

#[test]
fn stage5_schema_every_uses_member_edge_carries_its_member_and_only_heuristic_edges_carry_a_tier() {
    let files = fragments_for(THREE_TIER_FIXTURE);
    let g = resolve_graph(&no_git_root(), &files);

    let member_edges: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| matches!(e, Edge::UsesMember { .. }))
        .collect();
    for e in &member_edges {
        let Edge::UsesMember {
            heuristic,
            tier,
            member,
            ..
        } = e
        else {
            unreachable!()
        };
        assert!(
            member.is_some(),
            "every uses-member edge names its member, precise ones included: {e:?}"
        );
        assert_eq!(
            *heuristic,
            tier.is_some(),
            "the flag and the tier are one fact -- `Edge::uses_member` derives one from the other: {e:?}"
        );
    }

    let rows: Vec<(&str, Option<HeuristicTier>, Option<&str>)> = member_edges
        .iter()
        .map(|e| match e {
            Edge::UsesMember {
                to, tier, member, ..
            } => (to.as_str(), *tier, member.as_deref()),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("App.Core.Widget", None, Some("Render")),
            (
                "App.Ext.WidgetExtensions",
                Some(HeuristicTier::Ext),
                Some("Tally")
            ),
            ("App.Alpha.Config", Some(HeuristicTier::Guess), Some("Load")),
            ("App.Beta.Config", Some(HeuristicTier::Guess), Some("Load")),
        ]
    );

    // One serialized sample per tier, pinned: the precise row gains
    // `member` and nothing else, and the two guess rows spell their tier
    // between the flag and the member.
    let bytes: Vec<String> = member_edges
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert_eq!(
        bytes[0],
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":15,"to":"App.Core.Widget","to_file":"Core/Widget.cs","member":"Render"}"#
    );
    assert_eq!(
        bytes[1],
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":16,"to":"App.Ext.WidgetExtensions","to_file":"Ext/WidgetExtensions.cs","heuristic":true,"tier":"ext","member":"Tally"}"#
    );
    assert_eq!(
        bytes[2],
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":17,"to":"App.Alpha.Config","to_file":"Alpha/Config.cs","heuristic":true,"tier":"guess","member":"Load"}"#
    );

    // And the counters partition the guesses: every heuristic edge is in
    // exactly one tier, so the two add up to the total.
    assert_eq!(
        g.stats.heuristic_by_tier,
        HeuristicByTier { ext: 1, guess: 2 }
    );
    assert_eq!(
        g.stats.heuristic_by_tier.ext + g.stats.heuristic_by_tier.guess,
        g.stats.heuristic_edge_count
    );
    assert_eq!(
        g.schema_version, GRAPH_SCHEMA_VERSION,
        "a graph carrying tier and member is a schema-2 graph"
    );
}

#[test]
fn stage4_stats_heuristic_edge_count_is_appended_last_and_edges_by_kind_never_counts_a_guess() {
    let files = fragments_for(BYTE_IDENTITY_FIXTURE);
    let g = resolve_graph(&no_git_root(), &files);

    // Whole-object bytes rather than a key list: this pins the ORDER the
    // serialized `stats` keys appear in, and the values with them.
    assert_eq!(
        serde_json::to_string(&g.stats).unwrap(),
        r#"{"def_count":9,"file_count":7,"edges_by_kind":{"inherits":1,"uses-type":1,"imports":4,"uses-member":2,"ctor-di":0},"ambiguous_count":0,"ambiguous_pct":0,"unresolved_external_count":0,"heuristic_edge_count":3,"test_def_count":0,"heuristic_by_tier":{"ext":0,"guess":3}}"#,
        "heuristic_by_tier is appended LAST, after test_def_count -- the stats key order graph.json pins"
    );
    assert_eq!(g.stats.heuristic_edge_count, 3);
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 2,
        "the two precise member edges only -- three heuristic ones landed in the same array and moved this number by zero"
    );
    assert_eq!(g.stats.edges_by_kind.inherits, 1);
    assert_eq!(g.stats.edges_by_kind.uses_type, 1);
    assert_eq!(g.stats.edges_by_kind.imports, 4);
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "ambiguous_count semantics are untouched by this stage"
    );
}

// --- partial classes: also_in accumulation + method union -------------

#[test]
fn partial_class_across_files_merges_into_also_in_with_method_union() {
    let files = vec![
        (
            "A/Product.cs".to_string(),
            frag(
                vec![FragDef {
                    line: 3,
                    ..def_with(
                        "A.Product",
                        "Product",
                        "A",
                        "class",
                        &["Describe"],
                        &["Name"],
                        &["_cache"],
                    )
                }],
                vec![],
                vec![],
            ),
        ),
        (
            "A/Product.Extra.cs".to_string(),
            frag(
                vec![FragDef {
                    line: 3,
                    ..def_with(
                        "A.Product",
                        "Product",
                        "A",
                        "class",
                        &["Refresh"],
                        &["Sku"],
                        &["_extra"],
                    )
                }],
                vec![],
                vec![],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.defs.len(), 1, "one def entry, not two");
    let d = &g.defs[0];
    assert_eq!(
        d.file, "A/Product.cs",
        "first-insertion file wins as the primary site"
    );
    assert_eq!(
        d.methods,
        vec!["Describe".to_string(), "Refresh".to_string()],
        "methods union in encounter order"
    );
    assert_eq!(
        d.also_in,
        vec![AlsoIn {
            file: "A/Product.Extra.cs".into(),
            line: 3
        }]
    );
}

// --- resolution ladder: enclosing-namespace walks at steps 2 and 3 ------

#[test]
fn ladder_step2_a_using_directive_is_itself_read_against_the_enclosing_namespaces() {
    let files = fragments_for(&[
        ("A/Configuration/Setting.cs", "namespace A.Configuration { public class Setting { } }"),
        // The collision partner: without it a bare "Setting" would resolve
        // at step 4 (globally unique simple name) and this test would pass
        // on a ladder that never walked anything. With it, step 4 can only
        // report ambiguous.
        ("Other/Setting.cs", "namespace Other { public class Setting { } }"),
        ("A/B/C/Holder.cs", "\nusing Configuration;\n\nnamespace A.B.C;\n\npublic class Holder\n{\n  private Setting _setting;\n}\n"),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, "A/B/C/Holder.cs"),
        vec!["A.Configuration.Setting"],
        "`using Configuration;` inside A.B.C reaches A.Configuration.Setting"
    );
    assert!(
        !g.edges.iter().any(
            |e| matches!(e, Edge::Ambiguous { from_file, .. } if from_file == "A/B/C/Holder.cs")
        ),
        "step 2 answers, so the ladder never reaches the ambiguous step-4 pool"
    );
}

#[test]
fn ladder_step3_the_ancestor_namespace_rule_walks_every_enclosing_namespace() {
    let files = fragments_for(&[
        ("A/Shared.cs", "namespace A { public class Shared { } }"),
        // Same role as above -- makes step 4 ambiguous, so only a step-3
        // walk can produce a resolved edge here.
        (
            "Other/Shared.cs",
            "namespace Other { public class Shared { } }",
        ),
        (
            "A/B/C/Deep.cs",
            "\nnamespace A.B.C;\n\npublic class Deep\n{\n  private Shared _shared;\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        type_edge_targets_from(&g, "A/B/C/Deep.cs"),
        vec!["A.Shared"],
        "no usings at all -- the ancestor-namespace walk is the only step that can answer"
    );
    assert!(
        !g.edges.iter().any(
            |e| matches!(e, Edge::Ambiguous { from_file, .. } if from_file == "A/B/C/Deep.cs")
        ),
        "a walked step-3 hit resolves and never falls through to the ambiguous step-4 pool"
    );
}

#[test]
fn stage4_scored_a_nested_type_candidate_is_refused_from_outside_its_own_file_and_kept_inside_it() {
    let files = fragments_for(&[
        // Cross-file nested candidate: unreachable from Holder.cs without
        // naming Remote first, so a guess landing on it could never be what
        // the code said.
        (
            "Far/Remote.cs",
            "\nnamespace App.Far;\n\npublic class Remote\n{\n  public class Inner\n  {\n    public void Tally() { }\n  }\n}\n",
        ),
        // Same-file nested candidate + the ref itself. `mystery` is a
        // var-from-call local, so no receiver fact and no qualifier
        // resolution at all -- the only door into the scored tier's
        // uniqueness fallback.
        (
            "Nested/Holder.cs",
            "\nnamespace App.Nested;\n\npublic class Outer\n{\n  public class Nested\n  {\n    public void Tally() { }\n  }\n\n  public string Probe()\n  {\n    var mystery = Fetch();\n    mystery.Tally();\n    return \"x\";\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.Nested.Outer+Nested"],
        "the same-file nested candidate is nameable and stays; the cross-file one is refused"
    );
}

#[test]
fn heuristic_side_dedup_collapses_byte_identical_guesses_and_keeps_an_identical_precise_pair() {
    let files = fragments_for(&[
        ("Widgets/Widget.cs", "namespace App.Widgets { public class Widget { } }"),
        (
            "Ext/Helpers.cs",
            "\nnamespace App.Ext;\n\npublic static class Helpers\n{\n  public static string Slug(this Widget widget)\n  {\n    return \"s\";\n  }\n\n  public static string Tag(this Widget widget)\n  {\n    return \"t\";\n  }\n}\n",
        ),
        // The SAME extension call twice on ONE line: same declaring static
        // class, same member, same line -- two guesses that serialize to
        // the same bytes.
        (
            "Ops/Caller.cs",
            "\nusing App.Ext;\nusing App.Widgets;\n\nnamespace App.Ops;\n\npublic class Caller\n{\n  public string Run()\n  {\n    Widget widget = new Widget();\n    return widget.Tag() + widget.Tag();\n  }\n}\n",
        ),
        ("Enums/Mode.cs", "namespace App.Enums { public enum Mode { On, Off } }"),
        // The precise counterpart: the same enum member read twice on one
        // line. Two identical PRECISE edges are two real occurrences in the
        // source, and dropping either would lose a fact -- so both survive.
        (
            "Ops/Twice.cs",
            "\nusing App.Enums;\n\nnamespace App.Ops;\n\npublic class Twice\n{\n  public bool Both(Mode a, Mode b)\n  {\n    return a == Mode.On && b == Mode.On;\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Ops/Caller.cs"),
        vec![("App.Ext.Helpers", 12)],
        "the second byte-identical guess is dropped, the first kept"
    );
    assert_eq!(
        member_edges_from(&g, "Ops/Twice.cs"),
        vec![("App.Enums.Mode.On", 10), ("App.Enums.Mode.On", 10)],
        "the precise side is untouched: two identical rows are two occurrences, not a duplicate"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 1,
        "the dropped guess leaves the counter too"
    );
    assert_eq!(
        g.stats.heuristic_by_tier,
        HeuristicByTier { ext: 1, guess: 0 },
        "and it leaves ITS tier's counter, not the other one"
    );
}

// The other side of the same rule, and the reason `member` had to join the
// dedup key: two guesses that agree on every key the edge used to carry
// and differ ONLY in the member they name are two facts, not a duplicate.
// Before `member` existed these collapsed into one, and a reader lost a
// call.
#[test]
fn heuristic_side_dedup_keeps_two_guesses_that_name_different_members_of_one_target() {
    let files = fragments_for(&[
        ("Widgets/Widget.cs", "namespace App.Widgets { public class Widget { } }"),
        (
            "Ext/Helpers.cs",
            "\nnamespace App.Ext;\n\npublic static class Helpers\n{\n  public static string Slug(this Widget widget)\n  {\n    return \"s\";\n  }\n\n  public static string Tag(this Widget widget)\n  {\n    return \"t\";\n  }\n}\n",
        ),
        (
            "Ops/Caller.cs",
            "\nusing App.Ext;\nusing App.Widgets;\n\nnamespace App.Ops;\n\npublic class Caller\n{\n  public string Run()\n  {\n    Widget widget = new Widget();\n    return widget.Tag() + widget.Slug();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Ops/Caller.cs"),
        vec![("App.Ext.Helpers", 12), ("App.Ext.Helpers", 12)],
        "two calls, two edges -- identical but for the member each names"
    );
    assert_eq!(
        heuristic_member_names_from(&g, "Ops/Caller.cs"),
        vec![Some("Tag"), Some("Slug")],
        "and the member is what tells them apart, in source order"
    );
    assert_eq!(g.stats.heuristic_edge_count, 2);
    assert_eq!(
        g.stats.heuristic_by_tier,
        HeuristicByTier { ext: 2, guess: 0 }
    );
}

// --- test coverage: test_methods on the merged row + the counter ---

#[test]
fn partial_test_class_unions_its_test_methods_across_both_declaring_files() {
    let files = vec![
        (
            "Tests/WidgetTests.Part1.cs".to_string(),
            frag(
                vec![test_def(
                    "App.Tests.WidgetTests",
                    "WidgetTests",
                    "App.Tests",
                    &["Renders"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Tests/WidgetTests.Part2.cs".to_string(),
            frag(
                vec![test_def(
                    "App.Tests.WidgetTests",
                    "WidgetTests",
                    "App.Tests",
                    &["Renders", "Scales"],
                )],
                vec![],
                vec![],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.defs.len(), 1);
    assert_eq!(
        g.defs[0].test_methods,
        vec!["Renders".to_string(), "Scales".to_string()],
        "union across parts, deduped, first-seen order"
    );
    assert_eq!(
        serde_json::to_string(&g.defs[0]).unwrap(),
        r#"{"id":"App.Tests.WidgetTests","name":"WidgetTests","namespace":"App.Tests","kind":"class","file":"Tests/WidgetTests.Part1.cs","line":1,"methods":[],"testMethods":["Renders","Scales"],"also_in":[{"file":"Tests/WidgetTests.Part2.cs","line":1}]}"#,
        "the graph ROW keeps testMethods -- between methods and also_in"
    );
}

#[test]
fn test_def_count_counts_merged_def_rows_not_fragment_entries() {
    let files = vec![
        (
            "Tests/WidgetTests.Part1.cs".to_string(),
            frag(
                vec![test_def(
                    "App.Tests.WidgetTests",
                    "WidgetTests",
                    "App.Tests",
                    &["Renders"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Tests/WidgetTests.Part2.cs".to_string(),
            frag(
                vec![test_def(
                    "App.Tests.WidgetTests",
                    "WidgetTests",
                    "App.Tests",
                    &["Scales"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Tests/CartTests.cs".to_string(),
            frag(
                vec![test_def(
                    "App.Tests.CartTests",
                    "CartTests",
                    "App.Tests",
                    &["Places"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Src/Widget.cs".to_string(),
            frag(
                vec![def("App.Src.Widget", "Widget", "App.Src", "class")],
                vec![],
                vec![],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        g.stats.test_def_count, 2,
        "the partial class counts once across its two fragments; the production class not at all"
    );
}

// --- imports: always recorded, never resolved --------------------------

#[test]
fn imports_edge_is_recorded_regardless_of_whether_the_target_is_known() {
    let files = vec![(
        "A/Widget.cs".to_string(),
        frag(
            vec![],
            vec![],
            vec![FragRef {
                kind: "imports".into(),
                name: "System.Text".into(),
                qualified: None,
                member: None,
                line: 1,
                namespace: None,
                type_arg_count: None,
                generic: false,
                receiver_type: None,
                arg_count: None,
                receiver_args: None,
                outer_types: Vec::new(),
                args: None,
                receiver_property_owner: None,
                receiver_call_owner: None,
                receiver_call_member: None,
                receiver_base: false,
                receiver_awaited: false,
                receiver_local: false,
                receiver_lambda: None,
            }],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.edges_by_kind.imports, 1);
    assert_eq!(
        g.stats.unresolved_external_count, 0,
        "imports never counts toward unresolved"
    );
}

// --- stats math ---------------------------------------------------------

#[test]
fn ambiguous_pct_only_counts_type_ref_attempts_not_uses_member_or_imports() {
    let files = vec![
        (
            "A/Money.cs".to_string(),
            frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
        ),
        (
            "B/Money.cs".to_string(),
            frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
        ),
        (
            "C/Mixed.cs".to_string(),
            frag(
                vec![def("C.Mixed", "Mixed", "C", "class")],
                vec![],
                vec![
                    type_ref("uses-type", "Money", None, "C"), // ambiguous
                    FragRef {
                        kind: "imports".into(),
                        name: "System".into(),
                        qualified: None,
                        member: None,
                        line: 2,
                        namespace: None,
                        type_arg_count: None,
                        generic: false,
                        receiver_type: None,
                        arg_count: None,
                        receiver_args: None,
                        outer_types: Vec::new(),
                        args: None,
                        receiver_property_owner: None,
                        receiver_call_owner: None,
                        receiver_call_member: None,
                        receiver_base: false,
                        receiver_awaited: false,
                        receiver_local: false,
                        receiver_lambda: None,
                    },
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    // type_ref_attempts = inherits(0) + uses-type(0) + ambiguous(1) = 1
    assert_eq!(g.stats.ambiguous_pct, Percent1::from_ratio(1, 1));
    assert_eq!(
        serde_json::to_string(&g.stats.ambiguous_pct).unwrap(),
        "100"
    );
}

// --- the enclosing-type step ---
