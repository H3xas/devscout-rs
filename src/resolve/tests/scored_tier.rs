use super::*;

#[test]
fn stage4_scored_an_ambiguous_qualifier_names_every_member_declaring_candidate_and_only_those() {
    let files = fragments_for(&[
        ("One/Config.cs", "namespace App.One { public class Config { public void Load() { } } }"),
        ("Two/Config.cs", "namespace App.Two { public class Config { public void Load() { } } }"),
        // Same simple name, so it IS one of the ambiguous candidates the
        // ladder hands over -- but it declares nothing called Load, so the
        // member filter drops it. The pool is never "everything the ladder
        // was confused by".
        ("Three/Config.cs", "namespace App.Three { public class Config { public void Save() { } } }"),
        (
            "Consumers/AmbiguousQualifier.cs",
            "\nnamespace App.Consumers;\n\npublic class AmbiguousQualifier\n{\n  public void Run() => Config.Load();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/AmbiguousQualifier.cs").is_empty(),
        "the precise tiers still refuse to pick"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/AmbiguousQualifier.cs"),
        vec![("App.One.Config", 6), ("App.Two.Config", 6)],
        "both member-declaring candidates, ordered by def id since nothing separates their scores"
    );
}

#[test]
fn stage4_scored_same_namespace_beats_usings_visible_beats_global_and_that_is_the_emitted_order() {
    let files = fragments_for(&[
        ("Consumers/LocalStore.cs", "namespace App.Consumers { public class LocalStore { public void Persist() { } } }"),
        ("Imported/ImportedStore.cs", "namespace App.Imported { public class ImportedStore { public void Persist() { } } }"),
        ("Far/FarStore.cs", "namespace App.Far { public class FarStore { public void Persist() { } } }"),
        (
            "Consumers/Caller.cs",
            "\nusing App.Imported;\n\nnamespace App.Consumers;\n\npublic class Caller\n{\n  public void Run()\n  {\n    var s = Build();\n    s.Persist();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.Consumers.LocalStore", "App.Imported.ImportedStore", "App.Far.FarStore"],
        "score 3 then 2 then 1 -- NOT def-id order, which would have put App.Consumers, App.Far, App.Imported"
    );
}

#[test]
fn stage4_scored_the_uniqueness_fallback_emits_at_two_member_declaring_defs() {
    let files = fragments_for(&[
        ("A/Counter.cs", "namespace App.A { public class Counter { public void Tally() { } } }"),
        ("B/Ledger.cs", "namespace App.B { public class Ledger { public void Tally() { } } }"),
        (
            "Consumers/Unknown.cs",
            "\nnamespace App.Consumers;\n\npublic class Unknown\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.A.Counter", "App.B.Ledger"]
    );
}

#[test]
fn stage4_scored_a_member_name_carried_by_four_defs_is_too_common_to_guess_from() {
    let files = fragments_for(&[
        ("A/Counter.cs", "namespace App.A { public class Counter { public void Tally() { } } }"),
        ("B/Ledger.cs", "namespace App.B { public class Ledger { public void Tally() { } } }"),
        ("C/Register.cs", "namespace App.C { public class Register { public void Tally() { } } }"),
        ("D/Book.cs", "namespace App.D { public class Book { public void Tally() { } } }"),
        (
            "Consumers/TooCommon.cs",
            "\nnamespace App.Consumers;\n\npublic class TooCommon\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        heuristic_member_edges_from(&g, "Consumers/TooCommon.cs").is_empty(),
        "the refusal is total, not a top-three slice: past the threshold the name carries no information at all"
    );
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage4_scored_an_ambiguous_pool_larger_than_the_emit_cap_yields_exactly_three() {
    let files = fragments_for(&[
        ("A/Repo.cs", "namespace App.A { public class Repo { public void Save() { } } }"),
        ("B/Repo.cs", "namespace App.B { public class Repo { public void Save() { } } }"),
        ("C/Repo.cs", "namespace App.C { public class Repo { public void Save() { } } }"),
        ("D/Repo.cs", "namespace App.D { public class Repo { public void Save() { } } }"),
        ("E/Repo.cs", "namespace App.E { public class Repo { public void Save() { } } }"),
        ("Consumers/Many.cs", "\nnamespace App.Consumers;\n\npublic class Many\n{\n  public void Run() => Repo.Save();\n}\n"),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    let guesses = heuristic_member_edges_from(&g, "Consumers/Many.cs");
    assert_eq!(
        guesses.len(),
        3,
        "the emit cap binds on the AMBIGUOUS pool, which has no size threshold of its own"
    );
    assert_eq!(
        guesses.iter().map(|(to, _)| *to).collect::<Vec<_>>(),
        vec!["App.A.Repo", "App.B.Repo", "App.C.Repo"],
        "all five score 1 at the global ladder step, so the def-id tiebreak alone decides which three survive"
    );
}

#[test]
fn stage4_scored_a_ref_a_precise_tier_already_answered_never_gets_a_heuristic_duplicate() {
    let files = fragments_for(&[
        // Two more defs declaring Render, so the uniqueness fallback WOULD
        // have a pool to draw from if it were ever reached for this ref.
        ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
        ("Other/Gadget.cs", "namespace App.Other { public class Gadget { public void Render() { } } }"),
        (
            "Consumers/Precise.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class Precise\n{\n  private Widget _widget;\n\n  public void Run() => _widget.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edges_from(&g, "Consumers/Precise.cs"),
        vec![("App.Other.Widget", 10)],
        "one ref, one answer -- a fact is never restated as a guess"
    );
    assert!(heuristic_member_edges_from(&g, "Consumers/Precise.cs").is_empty());
    assert_eq!(g.stats.heuristic_edge_count, 0);
}

#[test]
fn stage4_scored_a_qualifier_that_resolved_but_vouched_for_nothing_is_left_alone() {
    let files = fragments_for(&[
        // Widget resolves uniquely and simply does not declare Render. That
        // is a KNOWN answer ("not here"), not an unknown one, so the scored
        // tier -- which only ever reads AMBIGUOUS or nothing-at-all
        // outcomes -- must not fire, even though Gadget would be a tidy
        // single-candidate guess.
        ("Other/Widget.cs", "namespace App.Other { public class Widget { } }"),
        ("Other/Gadget.cs", "namespace App.Other { public class Gadget { public void Render() { } } }"),
        (
            "Consumers/ResolvedMiss.cs",
            "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class ResolvedMiss\n{\n  public void Run() => Widget.Render();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edges_from(&g, "Consumers/ResolvedMiss.cs").is_empty());
    assert!(heuristic_member_edges_from(&g, "Consumers/ResolvedMiss.cs").is_empty());
}

// --- stage 5: the call-shape rule -------------------------------------
//
// A property or field is undeniable evidence for a READ of its own name,
// but no evidence at all for a CALL of that name -- C# simply has no
// overload-resolution path from `entity.Property(x => x.Id)` to a
// property or a field. Letting one vouch for a call anyway is exactly the
// false-positive shape a corpus audit surfaced: 41% of all heuristic
// edges were a call landing on a property/field-only def.

#[test]
fn stage5_shape_rule_a_call_never_vouches_through_a_property_or_field_in_the_uniqueness_pool() {
    let files = fragments_for(&[
        (
            "Model/Customer.cs",
            "namespace App.Model { public class Customer { public string Property { get; set; } } }",
        ),
        (
            "Model/Order.cs",
            "namespace App.Model { public class Order { public int Property; } }",
        ),
        (
            "Consumers/CallShape.cs",
            "\nnamespace App.Consumers;\n\npublic class CallShape\n{\n  public void Run()\n  {\n    var e = Entity();\n    e.Property(x => x.Id);\n    var p = e.Property;\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/CallShape.cs"),
        vec![("App.Model.Customer", 10), ("App.Model.Order", 10)],
        "the call at line 9 names neither a property nor a field -- only the read at line 10, which both a property and a field vouch for, survives"
    );
}

#[test]
fn stage5_shape_rule_filters_the_ambiguous_pool_the_same_way() {
    let files = fragments_for(&[
        (
            "One/Config.cs",
            "namespace App.One { public class Config { public void Load() { } } }",
        ),
        (
            "Two/Config.cs",
            "namespace App.Two { public class Config { public string Load { get; } } }",
        ),
        (
            "Consumers/AmbiguousCall.cs",
            "\nnamespace App.Consumers;\n\npublic class AmbiguousCall\n{\n  public void Run() => Config.Load();\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edges_from(&g, "Consumers/AmbiguousCall.cs").is_empty(),
        "the precise tiers still refuse to pick"
    );
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/AmbiguousCall.cs"),
        vec![("App.One.Config", 6)],
        "Config.Load() is a call -- App.Two.Config only ever declared Load as a property, so the shape rule drops it out of the ambiguous pool before scoring"
    );
}

#[test]
fn stage5_shape_rule_a_method_or_extension_name_still_vouches_for_a_call() {
    let files = fragments_for(&[
        (
            "A/Counter.cs",
            "namespace App.A { public class Counter { public void Tally() { } } }",
        ),
        ("Other/Foo.cs", "namespace App.Other { public class Foo { } }"),
        (
            "Ext/FooExtensions.cs",
            "namespace App.Ext { public static class FooExtensions { public static void Tally(this Foo f) { } } }",
        ),
        (
            "Consumers/CallShapeOk.cs",
            "\nnamespace App.Consumers;\n\npublic class CallShapeOk\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
        ),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.A.Counter", "App.Ext.FooExtensions"],
        "a method name and an extension-method name both still vouch for a call -- the shape rule only ever removes candidates, never adds one"
    );
}

// --- stage 5: the receiver-assignability rule --------------------------
//
// The scored tier's uniqueness pool is drawn by member NAME alone, so a
// ref whose receiver is typed but EXTERNAL (`private ILogger _logger;`
// where ILogger is a NuGet interface) used to name any in-graph class
// carrying a method of that name -- a log adapter implementing an
// unrelated interface, say. The receiver's type is a fact the extractor
// already recorded, and C# will only bind that call to a member of a type
// the receiver is assignable to, so a candidate the in-graph inheritance
// closure cannot connect to the receiver type is not a weak guess, it is a
// disproved one. The rule below refuses it.
//
// The connection is NOMINAL and deliberately shallow: a candidate answers
// when it IS the receiver type, when a def in its in-graph base closure
// is, or when any def in that closure merely NAMES the receiver type in
// its raw base list -- the last case being the one that matters, since the
// receiver type is usually external and so has no def to walk to.
