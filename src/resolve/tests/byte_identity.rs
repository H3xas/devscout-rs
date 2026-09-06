use super::*;

#[test]
fn stage4_byte_identity_dropping_the_heuristic_edges_reproduces_the_pre_stage4_edge_array() {
    let files = fragments_for(BYTE_IDENTITY_FIXTURE);
    let g = resolve_graph(&no_git_root(), &files);

    let precise: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| {
            !matches!(
                e,
                Edge::Inherits {
                    heuristic: true,
                    ..
                } | Edge::UsesType {
                    heuristic: true,
                    ..
                } | Edge::UsesMember {
                    heuristic: true,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        serde_json::to_string(&precise).unwrap(),
        format!("[{}]", PRE_STAGE4_EDGE_ROWS.join(",")),
        "stage 4 is emission-only: it may ADD tagged edges and may never move, drop or re-key a precise one"
    );

    // And the addition really happened -- otherwise the assertion above
    // would pass just as well on a scored tier that emits nothing at all.
    assert_eq!(
        heuristic_member_edges_from(&g, "Consumers/Consumer.cs"),
        vec![
            ("App.Alpha.Config", 16),
            ("App.Beta.Config", 16),
            ("App.Solo.Counter", 18)
        ]
    );
}

// Stage 6 adds a project model the resolver may consult; a repo that
// declares no `.csproj` has none, and for such a repo the WHOLE artifact
// -- not just the edge array -- must serialize exactly as it did before
// stage 6 existed. Whole-graph bytes rather than a spot check on `units`:
// an omitted key is only half the guarantee, the other half is that
// threading the model through moved nothing else.
#[test]
fn stage6_without_a_project_model_the_byte_identity_fixture_serializes_exactly_as_before() {
    let files = fragments_for(BYTE_IDENTITY_FIXTURE);
    let root = no_git_root();

    let legacy = serde_json::to_string(&resolve_graph(&root, &files)).unwrap();
    let modelled =
        serde_json::to_string(&resolve_graph_with_model(&root, &files, &[], None)).unwrap();
    assert_eq!(
        legacy, modelled,
        "a None model must leave the artifact byte-identical, key for key"
    );

    assert!(
        !legacy.contains(r#""units""#),
        "no `.csproj`, no `units` key -- it is omit-when-empty precisely so a \
         csproj-less repo's graph.json is unchanged: {legacy}"
    );
}

// --- stage 6: the admission gate on the two heuristic tiers -----------
//
// A heuristic tier guesses from NAMES; the project model is the one fact
// that can disprove such a guess structurally -- a def the site's assembly
// could not reference even if the name were right. The gate is a filter
// like every other heuristic-tier rule: purely subtractive, and it fails
// OPEN (a file or a def outside every project admits everything), because
// an ownership answer this resolver cannot compute must never delete an
// edge it would otherwise have emitted.
