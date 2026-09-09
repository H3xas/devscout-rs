use super::*;

#[test]
fn stage3_tier_f_an_extension_call_resolves_to_the_static_class_when_its_namespace_is_imported() {
    let files = fragments_for(&[
        WIDGET_SRC,
        WIDGET_EXTENSIONS_SRC,
        (
            "Consumers/UsesExtension.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class UsesExtension\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/UsesExtension.cs"),
        vec![("App.Ext.WidgetExtensions", 9)],
        "Widget does not declare Render -- only the extension tier can claim this call, and the edge targets the DECLARING static class"
    );
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        // Tier (f) is a HEURISTIC tier: it emits exactly this one edge, and
        // the edge declares itself a guess, because the instance-member veto
        // that would disprove it cannot see members of an out-of-graph
        // receiver and never will without a build.
        Edge::UsesMember {
            to_file, heuristic, ..
        } => {
            assert_eq!(to_file, "Ext/WidgetExtensions.cs");
            assert!(*heuristic, "tier (f) emits heuristic edges");
        }
        _ => unreachable!(),
    }
    assert_eq!(
        serde_json::to_string(edge).unwrap(),
        r#"{"kind":"uses-member","from_file":"Consumers/UsesExtension.cs","from_line":9,"to":"App.Ext.WidgetExtensions","to_file":"Ext/WidgetExtensions.cs","heuristic":true,"tier":"ext","member":"Render"}"#,
        "heuristic, then tier, then member -- appended in that order after the shared prefix"
    );
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 0,
        "edges_by_kind counts PRECISE edges only, so a heuristic tier cannot inflate it"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 1,
        "guesses are counted in their own stat instead"
    );
}

#[test]
fn stage3_tier_f_an_extension_class_in_the_refs_own_namespace_is_admitted_with_no_using_at_all() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Consumers/WidgetExtensions.cs",
            "namespace App.Consumers { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
        ),
        (
            "Consumers/SameNamespace.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class SameNamespace\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/SameNamespace.cs"),
        vec![("App.Consumers.WidgetExtensions", 8)]
    );
    assert!(
        member_edges_from(&g, "Consumers/SameNamespace.cs").is_empty(),
        "admission by own-namespace is still tier (f), so still a guess"
    );
}

#[test]
fn stage3_tier_f_an_extension_class_whose_namespace_is_not_imported_earns_no_edge() {
    // Deliberately no `using App.Ext;` -- in real C# this file would not
    // compile, and the resolver must not paper over that with a name match.
    let files = fragments_for(&[
        WIDGET_SRC,
        WIDGET_EXTENSIONS_SRC,
        (
            "Consumers/NoUsing.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class NoUsing\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/NoUsing.cs").is_empty(),
        "visibility is the admission rule -- an unimported extension class is not a candidate"
    );
    assert!(
        !g.edges
            .iter()
            .any(|e| matches!(e, Edge::Ambiguous { origin, .. } if origin == "uses-member")),
        "a declined extension lookup is still never ambiguous noise"
    );
}

// The positive half of the same rule, and the one thing tier (f)'s
// namespace test learned in stage 6: an ENCLOSING namespace needs no
// using directive, because in C# it is already in scope. Until global
// usings became per-project this gap was invisible -- any `global using`
// for the namespace, declared in any file anywhere in the repo, admitted
// the class here by accident.
#[test]
fn stage6_tier_f_an_extension_class_in_an_enclosing_namespace_needs_no_using() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Ext/Registration.cs",
            "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
        ),
        (
            "Ext/Deep/DeepRunner.cs",
            "\nusing App.Other;\n\nnamespace App.Ext.Deep;\n\npublic class DeepRunner\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
        (
            "Sibling/SiblingRunner.cs",
            "\nusing App.Other;\n\nnamespace App.Sibling;\n\npublic class SiblingRunner\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);

    assert_eq!(
        heuristic_member_edges_from(&g, "Ext/Deep/DeepRunner.cs"),
        vec![("App.Ext.WidgetExtensions", 8)],
        "App.Ext encloses App.Ext.Deep, so the extension class is in scope with no import"
    );
    assert_eq!(
        heuristic_member_tiers_from(&g, "Ext/Deep/DeepRunner.cs"),
        vec![Some(HeuristicTier::Ext)],
        "and tier (f) is what claims it -- not the scored tier's weaker second look"
    );
    assert!(
        heuristic_member_edges_from(&g, "Sibling/SiblingRunner.cs").is_empty(),
        "nothing wider than the lexical rule: a SIBLING namespace still needs the import"
    );
}

#[test]
fn stage3_tier_f_two_admitted_candidates_earn_no_edge_and_no_ambiguous_increment() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "ExtA/AExtensions.cs",
            "namespace App.ExtA { public static class AExtensions { public static void Render(this Widget w) { } } }",
        ),
        (
            "ExtB/BExtensions.cs",
            "namespace App.ExtB { public static class BExtensions { public static void Render(this Widget w) { } } }",
        ),
        (
            "Consumers/TwoCandidates.cs",
            "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class TwoCandidates\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/TwoCandidates.cs").is_empty(),
        "never pick a winner between two visible extension classes"
    );
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "a refused extension lookup does not touch the ambiguous stats"
    );
}

#[test]
fn stage3_tier_f_an_extension_whose_this_type_differs_from_the_receiver_type_earns_no_edge() {
    let files = fragments_for(&[
        WIDGET_SRC,
        ("Other/Gadget.cs", "namespace App.Other { public class Gadget { } }"),
        (
            "Ext/GadgetExtensions.cs",
            "namespace App.Ext { public static class GadgetExtensions { public static void Render(this Gadget g) { } } }",
        ),
        (
            "Consumers/WrongReceiver.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class WrongReceiver\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/WrongReceiver.cs").is_empty(),
        "the method name matches but the this-type does not -- no edge"
    );
}

#[test]
fn stage3_tier_f_an_instance_member_shadows_a_visible_extension_of_the_same_name() {
    // MUTATION-CRITICAL: this is what tier (e)'s `emitted = true` buys.
    // Drop that assignment and BOTH tiers claim the ref, producing two
    // edges -- the count assertion below is the one that catches it.
    let files = fragments_for(&[
        ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
        WIDGET_EXTENSIONS_SRC,
        (
            "Consumers/Shadowed.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Shadowed\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Shadowed.cs"),
        vec![("App.Other.Widget", 9)],
        "exactly one edge, and C#'s shadowing rule falls out of tier order: the instance member wins"
    );
    // Tier (e) is precise -- only tier (f) emits heuristic edges. And the
    // ref is claimed, so the scored tier never runs on it either: one ref,
    // one answer.
    assert!(heuristic_member_edges_from(&g, "Consumers/Shadowed.cs").is_empty());
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 1,
        "a precise edge still counts in edges_by_kind"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage3_tier_f_a_ref_with_no_receiver_type_never_enters_the_extension_tier() {
    let files = fragments_for(&[
        WIDGET_SRC,
        WIDGET_EXTENSIONS_SRC,
        (
            "Consumers/NoReceiverFact.cs",
            // A TYPE-name qualifier resolves to App.Other.Widget through
            // the ladder, but extension methods are instance-call syntax
            // only, so "Widget.Render()" must never be claimed here. And a
            // receiver the extractor refused to vouch for (var + a call)
            // carries no receiverType, so no lookup key exists at all.
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class NoReceiverFact\n{\n  public void Static() => Widget.Render();\n\n  public void Unknown()\n  {\n    var w = Compute();\n    w.Render();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/NoReceiverFact.cs").is_empty(),
        "neither shape carries a receiver fact, so neither can reach the extension tier"
    );
    // The scored tier draws the line between the two shapes tier (f)
    // treated alike. `Widget.Render()` RESOLVED -- a resolved qualifier is a fact
    // the precise tiers already judged, so the scored tier refuses to
    // second-guess it and emits nothing. `w.Render()` resolved to nothing
    // at all, which is the only door into the uniqueness fallback, and the
    // extension class is a candidate there because the fallback counts
    // extension-method names too (`member_vouched`, not `declares_member`).
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/NoReceiverFact.cs"),
        vec![("App.Ext.WidgetExtensions", 14)]
    );
}

#[test]
fn stage3_tier_f_bound_this_type_matching_is_exact_so_a_base_class_param_never_claims_a_derived_receiver(
) {
    let files = fragments_for(&[
        ("Other/BaseWidget.cs", "namespace App.Other { public class BaseWidget { } }"),
        ("Other/Widget.cs", "namespace App.Other { public class Widget : BaseWidget { } }"),
        (
            "Ext/BaseExtensions.cs",
            "namespace App.Ext { public static class BaseExtensions { public static void Render(this BaseWidget b) { } } }",
        ),
        (
            "Consumers/Derived.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Derived\n{\n  public void Run(Widget w) => w.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Derived.cs").is_empty(),
        "documented limitation: no inheritance walking and no interface widening -- real C# WOULD bind this, the resolver stays narrower rather than guessing"
    );
}

// --- tighten amendment: arity is part of the match ---
//
// Real fixtures run through this crate's own extractor, so an arity or
// arg_count that never gets recorded fails here rather than passing
// vacuously.

#[test]
fn stage3_tighten_regression_a_three_argument_call_never_binds_to_a_one_parameter_extension() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Ext/WidgetExtensions.cs",
            "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
        ),
        (
            "Consumers/ArityMismatch.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class ArityMismatch\n{\n  public void Wrong(Widget w) => w.Render(1, 2, 3);\n  public void Right(Widget w) => w.Render(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/ArityMismatch.cs"),
        vec![("App.Ext.WidgetExtensions", 10)],
        "corpus audit round 1 found 3/20 wrong edges of exactly this shape: an arity-blind index let a 3-argument call (line 9) claim a 1-parameter extension, stealing the edge from the real instance method. Only the arity-MATCHED call on line 10 survives"
    );
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "the refused arity mismatch is silent, like every other uses-member miss"
    );
}

#[test]
fn stage3_tighten_a_property_read_never_enters_the_extension_tier() {
    // A 0-arity extension is exactly what an argCount-blind tier would have
    // matched a property read against, since a property read has no
    // arguments to disagree about. It carries no argCount AT ALL, which is
    // the actual gate: an extension method is reachable through call syntax
    // only.
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Ext/SlugExtensions.cs",
            "namespace App.Ext { public static class SlugExtensions { public static string Slug(this Widget w) => \"s\"; } }",
        ),
        (
            "Consumers/PropertyRead.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class PropertyRead\n{\n  public string Read(Widget w) => w.Slug;\n}\n",
        ),
    ]);
    let consumer = &files
        .iter()
        .find(|(rel, _)| rel == "Consumers/PropertyRead.cs")
        .expect("consumer fragment")
        .1;
    let r = consumer
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Slug"))
        .expect("Slug ref present");
    assert_eq!(
        r.receiver_type.as_deref(),
        Some("Widget"),
        "the receiver fact still fires -- it is the argCount that is absent"
    );
    assert_eq!(
        r.arg_count, None,
        "a property read is not an invocation, so it records no argCount"
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/PropertyRead.cs").is_empty(),
        "no argCount, no key, no candidate lookup at all"
    );
}

#[test]
fn stage3_range_an_optional_parameter_makes_the_entry_a_range_and_every_count_inside_it_binds() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Ext/OptExtensions.cs",
            "namespace App.Ext { public static class OptExtensions { public static void Render(this Widget w, int depth, string label = null) { } } }",
        ),
        (
            "Consumers/OptionalRange.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class OptionalRange\n{\n  public void One(Widget w) => w.Render(1);\n  public void Two(Widget w) => w.Render(1, \"a\");\n  public void Three(Widget w) => w.Render(1, \"a\", 3);\n}\n",
        ),
    ]);
    let ext = &files
        .iter()
        .find(|(rel, _)| rel == "Ext/OptExtensions.cs")
        .expect("ext fragment")
        .1;
    let d = ext
        .defs
        .iter()
        .find(|d| d.id == "App.Ext.OptExtensions")
        .expect("OptExtensions def present");
    assert_eq!(
        d.extension_methods
            .iter()
            .map(|e| (
                e.name.as_str(),
                e.this_type.as_str(),
                e.arity_min,
                e.arity_max
            ))
            .collect::<Vec<_>>(),
        vec![("Render", "Widget", 1, 2)],
        "arityMin skips the defaulted parameter; arityMax still counts every DECLARED one"
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/OptionalRange.cs"),
        vec![("App.Ext.OptExtensions", 9), ("App.Ext.OptExtensions", 10)],
        "one argument and two both fall inside [1, 2]; three falls outside and earns nothing"
    );
}

#[test]
fn stage3_tier_f_a_partial_static_class_declaring_the_same_quadruple_twice_stays_one_candidate() {
    // The Rust index stores def INDEXES in each bucket, so the per-def
    // dedup has to happen BEFORE the push -- otherwise a partial class
    // re-declaring the same (name, thisType, arityMin, arityMax) in a second
    // file would fill its own bucket twice and the one-candidate gate would
    // refuse a call that has exactly one real candidate.
    let files = vec![
        (
            "Ext/Widget.cs".to_string(),
            frag(
                vec![def("App.Other.Widget", "Widget", "App.Other", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Ext/Part1.cs".to_string(),
            frag(
                vec![ext_def(
                    "App.Ext.Helpers",
                    "Helpers",
                    "App.Ext",
                    &[("Render", "Widget", 0, 0)],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Ext/Part2.cs".to_string(),
            frag(
                vec![ext_def(
                    "App.Ext.Helpers",
                    "Helpers",
                    "App.Ext",
                    &[("Render", "Widget", 0, 0), ("Poke", "Widget", 0, 0)],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/PartialExt.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.PartialExt",
                    "PartialExt",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Ext".into(),
                    global: false,
                }],
                // Distinct LINES: the two calls guess the same static class,
                // and the heuristic-side dedup collapses byte-identical
                // guesses -- which a shared synthetic line would make these,
                // hiding the second candidate this test exists to see.
                vec![
                    receiver_ref("w", "Render", "App.Consumers", "Widget", Some(0)),
                    FragRef {
                        line: 2,
                        ..receiver_ref("w", "Poke", "App.Consumers", "Widget", Some(0))
                    },
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.Ext.Helpers", "App.Ext.Helpers"],
        "the duplicate pair is deduped, and the second file's NEW pair still registers"
    );
}

// --- second tighten: instance-member veto, arity range,
// --- generic argument unification ---
//
// Real fixtures run through this crate's own extractor, so a range, a base
// name, or a type-argument descriptor that never gets recorded fails here
// rather than passing vacuously.

#[test]
fn stage3_range_regression_an_exact_arity_class_no_longer_looks_unique_next_to_a_range_class() {
    let files = fragments_for(&[
        ("Other/Bus.cs", "namespace App.Other { public class Bus { } }"),
        // Exactly two parameters -- the shape the arity-keyed index used to
        // hand the edge to, because the range class below was keyed under
        // arity 3 and could not be found at argCount 2 at all.
        (
            "ExtA/WrongSend.cs",
            "namespace App.ExtA { public static class WrongSend { public static void Send(this Bus b, object m, int retries) { } } }",
        ),
        (
            "ExtB/RightSend.cs",
            "namespace App.ExtB { public static class RightSend { public static void Send(this Bus b, object m, string topic, int retries = 0) { } } }",
        ),
        (
            "Consumers/TwoSends.cs",
            "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class TwoSends\n{\n  public void Run(Bus b) => b.Send(1, 2);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/TwoSends.cs").is_empty(),
        "both classes accept two arguments once the range is honoured, so the tier sees TWO candidates and refuses -- the arity-keyed index saw one and picked the wrong class"
    );
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "honest ambiguity here is still silence, not an ambiguous edge"
    );
}

#[test]
fn stage3_range_a_params_array_records_arity_max_minus_one_and_accepts_any_count() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "Ext/ParamsExtensions.cs",
            "namespace App.Ext { public static class ParamsExtensions { public static void All(this Widget w, params int[] xs) { } } }",
        ),
        (
            "Consumers/Spread.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Spread\n{\n  public void None(Widget w) => w.All();\n  public void Five(Widget w) => w.All(1, 2, 3, 4, 5);\n}\n",
        ),
    ]);
    let ext = &files
        .iter()
        .find(|(rel, _)| rel == "Ext/ParamsExtensions.cs")
        .expect("ext fragment")
        .1;
    let d = ext
        .defs
        .iter()
        .find(|d| d.id == "App.Ext.ParamsExtensions")
        .expect("ParamsExtensions def present");
    assert_eq!(
        d.extension_methods
            .iter()
            .map(|e| (e.arity_min, e.arity_max))
            .collect::<Vec<_>>(),
        vec![(0, -1)],
        "a params array is optional AND unbounded: nothing forces it, nothing caps it"
    );

    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Spread.cs"),
        vec![
            ("App.Ext.ParamsExtensions", 9),
            ("App.Ext.ParamsExtensions", 10)
        ],
        "zero arguments and five both bind to the same unbounded entry"
    );
}

#[test]
fn stage3_range_an_unbounded_params_entry_alongside_a_second_visible_class_still_drops() {
    let files = fragments_for(&[
        WIDGET_SRC,
        (
            "ExtA/ParamsExtensions.cs",
            "namespace App.ExtA { public static class ParamsExtensions { public static void All(this Widget w, params int[] xs) { } } }",
        ),
        (
            "ExtB/ExactExtensions.cs",
            "namespace App.ExtB { public static class ExactExtensions { public static void All(this Widget w, int a, int b) { } } }",
        ),
        (
            "Consumers/SpreadTwo.cs",
            "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class SpreadTwo\n{\n  public void Two(Widget w) => w.All(1, 2);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/SpreadTwo.cs").is_empty(),
        "an unbounded range never wins a tie -- two candidates is two candidates"
    );
}

#[test]
fn stage3_veto_a_member_declared_by_the_receivers_interface_beats_a_matching_visible_extension() {
    let files = fragments_for(&[
        ("Other/IWidget.cs", "namespace App.Other { public interface IWidget { void Render(int depth); } }"),
        ("Other/Widget.cs", "namespace App.Other { public class Widget : IWidget { } }"),
        (
            "Ext/WidgetExtensions.cs",
            "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
        ),
        (
            "Consumers/Vetoed.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Vetoed\n{\n  public void Run(Widget w) => w.Render(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/Vetoed.cs").is_empty(),
        "C# binds the instance member the interface declares; the extension is unreachable, so the tier must not claim the ref"
    );
    // Tier (e) must NOT have widened either: it emits only on the exact
    // receiver def, and Widget itself declares nothing.
    assert_eq!(g.stats.ambiguous_count, 0);
}

#[test]
fn stage3_veto_control_the_same_shape_with_the_member_absent_still_earns_its_extension_edge() {
    let files = fragments_for(&[
        ("Other/IWidget.cs", "namespace App.Other { public interface IWidget { void Measure(int depth); } }"),
        ("Other/Widget.cs", "namespace App.Other { public class Widget : IWidget { } }"),
        (
            "Ext/WidgetExtensions.cs",
            "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
        ),
        (
            "Consumers/NotVetoed.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class NotVetoed\n{\n  public void Run(Widget w) => w.Render(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/NotVetoed.cs"),
        vec![("App.Ext.WidgetExtensions", 9)],
        "the closure declares Measure, not Render -- nothing vetoes"
    );
}

#[test]
fn stage3_veto_the_closure_is_transitive_so_a_member_on_the_base_of_the_base_still_vetoes() {
    let files = fragments_for(&[
        ("Other/Root.cs", "namespace App.Other { public class Root { public void Render(int depth) { } } }"),
        ("Other/Middle.cs", "namespace App.Other { public class Middle : Root { } }"),
        ("Other/Leaf.cs", "namespace App.Other { public class Leaf : Middle { } }"),
        (
            "Ext/LeafExtensions.cs",
            "namespace App.Ext { public static class LeafExtensions { public static void Render(this Leaf l, int depth) { } } }",
        ),
        (
            "Consumers/DeepVeto.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class DeepVeto\n{\n  public void Run(Leaf l) => l.Render(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    // Two hops up the chain is still an instance member -- the
    // typed-receiver base walk widens exactly that far: Leaf itself
    // declares nothing, but Root, reached through Leaf's transitive
    // in-graph base closure, does, so the typed-receiver precise tier
    // binds there instead of leaving the extension tier's veto as the
    // only visible effect.
    assert_eq!(
        member_edges_from(&g, "Consumers/DeepVeto.cs"),
        vec![("App.Other.Root", 9)],
        "the precise tier now walks the closure the veto always could see"
    );
    assert_eq!(
        g.stats.heuristic_edge_count, 0,
        "the extension is still unreachable -- precise supersedes it, not joins it"
    );
}

#[test]
fn stage3_veto_a_cycle_in_the_base_closure_terminates_instead_of_hanging() {
    // Not legal C#, but a fragments cache assembled from mid-edit sources
    // can present exactly this, and an unbounded walk is not an acceptable
    // failure mode.
    let files = fragments_for(&[
        ("Other/A.cs", "namespace App.Other { public class A : B { } }"),
        ("Other/B.cs", "namespace App.Other { public class B : A { } }"),
        (
            "Ext/AExtensions.cs",
            "namespace App.Ext { public static class AExtensions { public static void Render(this A a, int depth) { } } }",
        ),
        (
            "Consumers/Cyclic.cs",
            "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Cyclic\n{\n  public void Run(A a) => a.Render(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Cyclic.cs"),
        vec![("App.Ext.AExtensions", 9)],
        "the walk terminates and, finding no Render in the cycle, lets the extension edge stand"
    );
}

#[test]
fn stage3_veto_bound_an_external_receiver_type_can_never_be_vetoed() {
    // No definition of `HttpClient` anywhere in the graph -- the receiver
    // resolves to nothing, so no closure exists to inspect.
    let files = fragments_for(&[
        (
            "Ext/HttpExtensions.cs",
            "namespace App.Ext { public static class HttpExtensions { public static void Ping(this HttpClient c, int n) { } } }",
        ),
        (
            "Consumers/External.cs",
            "\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class External\n{\n  public void Run(HttpClient c) => c.Ping(1);\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/External.cs"),
        vec![("App.Ext.HttpExtensions", 8)],
        "documented bound: an out-of-graph receiver hides whatever members it declares"
    );
}
