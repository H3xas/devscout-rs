use super::*;

#[test]
fn stage2_tier_a_static_property_access_on_a_bare_qualifier_now_emits() {
    // The MessageUrn.Prefix shape: same namespace as the def, so the
    // qualifier answers at the namespace ladder step -- no using, no
    // type-argument list, no dotted qualifier. Such a bare qualifier
    // carries no certainty signal on its own.
    let files = vec![
        (
            "Other/MessageUrn.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Consumers.MessageUrn",
                    "MessageUrn",
                    "App.Consumers",
                    "class",
                    &[],
                    &["Prefix"],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UsesProperty.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UsesProperty",
                    "UsesProperty",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![
                    member_ref("MessageUrn", None, "Prefix", "App.Consumers"),
                    member_ref("MessageUrn", None, "NotDeclared", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edge_targets(&g),
        vec!["App.Consumers.MessageUrn"],
        "the declared property emits; the undeclared member still does not"
    );
}

#[test]
fn stage2_tier_a_const_field_access_on_a_bare_qualifier_emits() {
    let files = vec![
        (
            "Other/Limits.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Limits",
                    "Limits",
                    "App.Other",
                    "class",
                    &[],
                    &[],
                    &["MaxRetries"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UsesField.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UsesField",
                    "UsesField",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![member_ref("Limits", None, "MaxRetries", "App.Consumers")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_edge_targets(&g), vec!["App.Other.Limits"]);
}

#[test]
fn stage2_tier_a_partial_class_contributes_its_own_properties_and_fields() {
    let files = vec![
        (
            "Other/Config.Part1.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Config",
                    "Config",
                    "App.Other",
                    "class",
                    &[],
                    &[],
                    &["Retries"],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Config.Part2.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Config",
                    "Config",
                    "App.Other",
                    "class",
                    &[],
                    &["Name"],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UsesBoth.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UsesBoth",
                    "UsesBoth",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![
                    member_ref("Config", None, "Retries", "App.Consumers"),
                    member_ref("Config", None, "Name", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edge_targets(&g),
        vec!["App.Other.Config", "App.Other.Config"],
        "both halves of the partial class vouch for their own member"
    );
}

// --- tier (e): instance receivers ---

#[test]
fn stage2_tier_e_a_declared_local_receiver_resolves_through_the_ladder() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/LocalReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.LocalReceiver",
                    "LocalReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![receiver_ref(
                    "w",
                    "Render",
                    "App.Consumers",
                    "Widget",
                    Some(0),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).expect("uses-member edge present");
    match edge {
        Edge::UsesMember { to, to_file, .. } => {
            assert_eq!(to, "App.Other.Widget");
            assert_eq!(to_file, "Other/Widget.cs");
        }
        _ => unreachable!(),
    }
    assert_eq!(g.stats.edges_by_kind.uses_member, 1);
}

#[test]
fn stage2_tier_e_a_receiver_whose_member_lives_in_the_property_list_also_emits() {
    // Tier (e) reuses the SAME widened membership test as tier (a) --
    // methods ∪ properties ∪ fields, not methods alone.
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &[],
                    &["Name"],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/PropReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.PropReceiver",
                    "PropReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![receiver_ref("w", "Name", "App.Consumers", "Widget", None)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
}

#[test]
fn stage2_tier_e_a_receiver_type_reached_only_through_a_type_alias_resolves_too() {
    let files = vec![
        (
            "One/Item.cs".to_string(),
            frag(
                vec![def_with(
                    "App.One.Item",
                    "Item",
                    "App.One",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Two/Item.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Two.Item",
                    "Item",
                    "App.Two",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/AliasReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.AliasReceiver",
                    "AliasReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Alias {
                    alias: "AliasedItem".into(),
                    target: "App.Two.Item".into(),
                    global: false,
                }],
                vec![receiver_ref(
                    "item",
                    "Go",
                    "App.Consumers",
                    "AliasedItem",
                    Some(0),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edge_targets(&g),
        vec!["App.Two.Item"],
        "the alias pins the receiver type even though the simple name \"Item\" is ambiguous"
    );
}

#[test]
fn stage2_tier_e_a_receiver_whose_type_does_not_declare_the_member_earns_no_edge() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/UnknownMember.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.UnknownMember",
                    "UnknownMember",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![receiver_ref(
                    "w",
                    "Explode",
                    "App.Consumers",
                    "Widget",
                    Some(0),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edge_targets(&g).is_empty(),
        "stage 3/4 territory, not an edge"
    );
}

#[test]
fn stage2_tier_e_an_ambiguous_receiver_type_earns_no_edge_and_no_ambiguous_noise() {
    let files = vec![
        (
            "One/Handler.cs".to_string(),
            frag(
                vec![def_with(
                    "App.One.Handler",
                    "Handler",
                    "App.One",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Two/Handler.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Two.Handler",
                    "Handler",
                    "App.Two",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/AmbiguousReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.AmbiguousReceiver",
                    "AmbiguousReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![receiver_ref("h", "Go", "App.Consumers", "Handler", Some(0))],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edge_targets(&g).is_empty(),
        "never pick a winner between two same-named receiver types"
    );
    assert_eq!(
        g.stats.ambiguous_count, 0,
        "a uses-member miss is still never reported as ambiguous"
    );
    assert_eq!(g.stats.unresolved_external_count, 0);
    // Refusing to PICK is not the same as having nothing to say. Both
    // candidates declare Go, so both are named as guesses -- the
    // strong scored case, where the right answer is provably one of the
    // two. Neither is same-namespace and the file has no usings, so both
    // score 1 and the def id breaks the tie.
    assert_eq!(
        heuristic_member_edge_targets(&g),
        vec!["App.One.Handler", "App.Two.Handler"]
    );
    assert_eq!(g.stats.heuristic_edge_count, 2);
}

#[test]
fn stage2_tier_e_an_unresolvable_receiver_type_earns_no_edge() {
    let files = vec![(
        "Consumers/ExternalReceiver.cs".to_string(),
        frag(
            vec![def(
                "App.Consumers.ExternalReceiver",
                "ExternalReceiver",
                "App.Consumers",
                "class",
            )],
            vec![],
            vec![receiver_ref(
                "s",
                "Trim",
                "App.Consumers",
                "StringBuilder",
                Some(0),
            )],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(member_edge_targets(&g).is_empty());
    assert_eq!(
        g.stats.unresolved_external_count, 0,
        "a uses-member miss is never counted as external either"
    );
}

#[test]
fn stage2_tier_e_a_ref_with_no_fact_at_all_is_untouched_by_the_new_tier() {
    // The extraction-side negatives (predefined-type receiver, conflicting
    // duplicate locals, var-from-call) all arrive here as the SAME thing:
    // a member ref with no receiverType. One resolver-side pin covers the
    // whole family -- their extraction-side halves are pinned in
    // extract.rs's own stage2b tests.
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/NoFact.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.NoFact",
                    "NoFact",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![member_ref("widget", None, "Render", "App.Consumers")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        member_edge_targets(&g).is_empty(),
        "no fact was recorded, so there is nothing to resolve"
    );
    // With no fact of any kind the qualifier `widget` resolves to nothing
    // at all, which is the only door into the scored
    // tier's uniqueness fallback -- and Widget is the one def graph-wide
    // declaring `Render`, so it is named as a GUESS. Pinned deliberately:
    // this is the weakest evidence the resolver acts on, it is exactly why
    // the fallback is capped and tagged rather than emitted as fact, and it
    // must never leak into the precise set above.
    assert_eq!(heuristic_member_edge_targets(&g), vec!["App.Other.Widget"]);
}

#[test]
fn stage2_tier_e_shadowing_is_settled_at_extraction_so_the_edge_follows_the_recorded_fact() {
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Other/Gadget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Gadget",
                    "Gadget",
                    "App.Other",
                    "class",
                    &["Go"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/ShadowReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.ShadowReceiver",
                    "ShadowReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                // The parameter shadows the same-named field, so the
                // extractor recorded Gadget (see extract.rs's own test).
                vec![receiver_ref(
                    "handler",
                    "Go",
                    "App.Consumers",
                    "Gadget",
                    Some(0),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edge_targets(&g),
        vec!["App.Other.Gadget"],
        "the innermost declaration wins"
    );
}

#[test]
fn stage2_tier_e_never_adds_a_second_edge_for_a_ref_an_earlier_tier_already_claimed() {
    // `public void Run(Widget Widget) => Widget.Render();` -- the
    // qualifier resolves as a TYPE (tier (a)) AND carries a receiver fact.
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Render"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/OneEdge.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.OneEdge",
                    "OneEdge",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![receiver_ref(
                    "Widget",
                    "Render",
                    "App.Consumers",
                    "Widget",
                    Some(0),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        g.stats.edges_by_kind.uses_member, 1,
        "still exactly one edge"
    );
    assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
}

#[test]
fn stage2_tier_e_never_fires_for_a_dotted_chain_tail_because_it_carries_no_fact() {
    // Chain-tail regression, resolver half: "w.Inner.Tail()" flattens to a
    // DOTTED qualifier, which the extractor's bare-only guard refuses a
    // fact for. Even though Widget declares Tail, no edge is earned here.
    let files = vec![
        (
            "Other/Widget.cs".to_string(),
            frag(
                vec![def_with(
                    "App.Other.Widget",
                    "Widget",
                    "App.Other",
                    "class",
                    &["Tail"],
                    &["Inner"],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Consumers/Chain.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.Chain",
                    "Chain",
                    "App.Consumers",
                    "class",
                )],
                vec![FragUsing::Plain {
                    text: "App.Other".into(),
                    global: false,
                }],
                vec![
                    receiver_ref("w", "Inner", "App.Consumers", "Widget", None),
                    member_ref("Inner", Some("w.Inner"), "Tail", "App.Consumers"),
                ],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        member_edge_targets(&g),
        vec!["App.Other.Widget"],
        "only the head access earns a PRECISE edge"
    );
    // The tail's dotted qualifier ("w.Inner") resolves to nothing at all,
    // so the uniqueness fallback reaches it and -- Widget
    // being the only def declaring `Tail` -- names Widget as a GUESS. That
    // is the opposite of the bug this test pins: the tail may never inherit
    // the head's fact and emit a PRECISE edge, but it is allowed to be
    // guessed at by name, tagged, from far weaker evidence.
    assert_eq!(heuristic_member_edge_targets(&g), vec!["App.Other.Widget"]);
}

// --- tier (e) end-to-end: real C# through extract -> resolve ---
//
// The tier tests above hand-build fragments, which pins the RESOLVER in
// isolation but takes the extractor's word for what it records. These four
// run real fixtures through this crate's own extractor, so a fact that never
// gets recorded (or gets recorded on the wrong line) fails here rather than
// passing vacuously.
