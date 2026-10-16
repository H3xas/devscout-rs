use super::*;

use crate::query::impact::impact_model_for_seed;

// A member name two owners share, one of whose ids is a dotted SUFFIX of the
// other's: the qualifier `Nautical.Crew.Galley` admits both, and an
// already-chosen candidate must admit one.
fn suffix_pair_fixture() -> (graph::Graph, PathBuf) {
    let root = temp_repo_root("pick-suffix-pair");
    fs::create_dir_all(root.join("Crew")).unwrap();
    fs::write(
        root.join("Crew/Galley.cs"),
        "namespace Nautical.Crew\n{\n    public class Galley\n    {\n        public void Simmer(int pots) { }\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Crew/FleetGalley.cs"),
        "namespace Fleet.Nautical.Crew\n{\n    public class Galley\n    {\n        public void Simmer(int pots) { }\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Crew/Cook.cs"),
        "namespace Nautical.Crew\n{\n    public class Cook\n    {\n        public void Boil(Galley galley, Fleet.Nautical.Crew.Galley reliefGalley)\n        {\n            galley.Simmer(2);\n            reliefGalley.Simmer(4);\n        }\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Crew/Solo.cs"),
        "namespace Nautical.Crew\n{\n    public class Solo\n    {\n        public void Stand() { }\n    }\n}\n",
    )
    .unwrap();
    write_manifest_fixture(
        &root,
        &[
            "Crew/Galley.cs",
            "Crew/FleetGalley.cs",
            "Crew/Cook.cs",
            "Crew/Solo.cs",
        ],
    );
    let graph = make_graph(
        vec![
            def(
                "Nautical.Crew.Galley",
                "Galley",
                "Nautical.Crew",
                "class",
                "Crew/Galley.cs",
                3,
            ),
            def(
                "Fleet.Nautical.Crew.Galley",
                "Galley",
                "Fleet.Nautical.Crew",
                "class",
                "Crew/FleetGalley.cs",
                3,
            ),
            def(
                "Nautical.Crew.Cook",
                "Cook",
                "Nautical.Crew",
                "class",
                "Crew/Cook.cs",
                3,
            ),
            def(
                "Nautical.Crew.Solo",
                "Solo",
                "Nautical.Crew",
                "class",
                "Crew/Solo.cs",
                3,
            ),
        ],
        vec![
            uses_member("Crew/Cook.cs", 7, "Nautical.Crew.Galley", "Crew/Galley.cs"),
            uses_member(
                "Crew/Cook.cs",
                8,
                "Fleet.Nautical.Crew.Galley",
                "Crew/FleetGalley.cs",
            ),
            uses_type("Crew/Cook.cs", 5, "Nautical.Crew.Solo", "Crew/Solo.cs"),
        ],
    );
    (graph, root)
}

fn member(name: &str, kind: &str, file: &str, line: usize, owner: &str) -> graph::GraphName {
    graph::GraphName {
        name: name.into(),
        kind: kind.into(),
        file: file.into(),
        line,
        owner: owner.into(),
    }
}

fn with_member_names(mut g: graph::Graph) -> graph::Graph {
    g.names = vec![
        member(
            "Simmer",
            "method",
            "Crew/FleetGalley.cs",
            5,
            "Fleet.Nautical.Crew.Galley",
        ),
        member(
            "Simmer",
            "method",
            "Crew/Galley.cs",
            5,
            "Nautical.Crew.Galley",
        ),
    ];
    g
}

#[test]
fn candidate_sections_ac_5_exact_owner_narrows_where_the_typed_seed_admits_the_suffix_pair() {
    let (graph, root) = suffix_pair_fixture();
    let named = with_member_names(graph);
    let index = load_graph_index(&named, &root);

    let owners = qualified_member_owners(&index, "Simmer", Some("Nautical.Crew.Galley"));
    assert_eq!(
        owners.len(),
        2,
        "a typed qualifier admits both owners whose ids end with the same segments"
    );

    let exact = exact_member_owner(&index, "Simmer", "Fleet.Nautical.Crew.Galley")
        .expect("the relief galley declares the member");
    assert_eq!(exact.0, "Fleet.Nautical.Crew.Galley");
    assert_eq!(exact.1.len(), 1);

    let picked = PickedSeed::Member(MemberCandidate {
        owner: "Fleet.Nautical.Crew.Galley".to_string(),
        name: "Simmer".to_string(),
        file: "Crew/FleetGalley.cs".to_string(),
        line: 5,
    });
    let RefsResult::Members(models) = pick_refs(
        &index,
        &picked,
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) else {
        panic!("one exact owner must answer one model, never a second ambiguity");
    };
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "Fleet.Nautical.Crew.Galley.Simmer");

    // The seed a caller could have typed for that same owner is the one the
    // suffix rule sends back to the refusal.
    assert!(matches!(
        build_refs_model(
            &index,
            "Nautical.Crew.Galley.Simmer",
            false,
            DEFAULT_CAP,
            INBOUND_CAP,
            OUTBOUND_CAP,
            false,
        ),
        RefsResult::MemberAmbiguous(_)
    ));
}

#[test]
fn candidate_sections_ac_5_a_picked_owner_the_graph_does_not_declare_answers_nothing() {
    let (graph, root) = suffix_pair_fixture();
    let named = with_member_names(graph);
    let index = load_graph_index(&named, &root);
    assert!(
        exact_member_owner(&index, "Simmer", "Nautical.Crew.Larder").is_none(),
        "an owner that declares no such member admits nothing"
    );

    // The pick path answers `NotFound` rather than panicking or claiming an
    // empty resolved answer, which is what a seed the graph does not hold
    // gives too.
    let picked = PickedSeed::Member(MemberCandidate {
        owner: "Nautical.Crew.Larder".to_string(),
        name: "Simmer".to_string(),
        file: "Crew/Galley.cs".to_string(),
        line: 5,
    });
    assert!(matches!(
        pick_refs(
            &index,
            &picked,
            false,
            DEFAULT_CAP,
            INBOUND_CAP,
            OUTBOUND_CAP,
            false,
        ),
        RefsResult::NotFound
    ));
}

#[test]
fn candidate_sections_ac_4_a_type_candidate_answers_as_its_def_id_would() {
    let (graph, root) = suffix_pair_fixture();
    let index = load_graph_index(&graph, &root);
    let picked = PickedSeed::Type("Nautical.Crew.Solo".to_string());

    let picked_model = pick_impact(
        &index,
        &picked,
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    );
    let by_id = impact_model_for_seed(
        &index,
        SeedKind::Symbol,
        vec!["Nautical.Crew.Solo".to_string()],
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    );
    let by_unique_name = build_impact_model(
        &index,
        "Solo",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    );

    assert_eq!(
        picked_model, by_id,
        "a picked type candidate is the walk the def id as a single resolved symbol seed builds"
    );
    assert_eq!(
        picked_model, by_unique_name,
        "which is the path a unique type name already took"
    );
}
