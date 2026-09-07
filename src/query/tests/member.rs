use super::*;

// `impact`/`tests` have no member-shaped answer of their own (unlike
// `refs`/`read`): a member seed answers AS its unique declaring type, so
// these fixtures exercise `resolve_impact_seed`/`build_impact_model` and
// `build_tests_model` directly rather than duplicating `refs_bare_member.rs`'s
// edge-verified fallback.

fn name_row(name: &str, kind: &str, file: &str, line: usize, owner: &str) -> graph::GraphName {
    graph::GraphName {
        name: name.into(),
        kind: kind.into(),
        file: file.into(),
        line,
        owner: owner.into(),
    }
}

// `Cellar` and `Cask` both declare `Rack` (the ambiguous case); `Cellar`
// alone declares `Uncork` (the unique case) and, oddly but on purpose, a
// SECOND name row spells `Cellar` again as a method on `Sommelier` -- this is
// what a query for the bare name `Cellar` must never reach, proving the type
// ladder runs to completion before the member ladder is tried at all (C1).
fn member_seed_fixture() -> (graph::Graph, PathBuf) {
    let mut g = make_graph(
        vec![
            def(
                "App.Store.Cellar",
                "Cellar",
                "App.Store",
                "class",
                "Store/Cellar.cs",
                3,
            ),
            def(
                "App.Store.Cask",
                "Cask",
                "App.Store",
                "class",
                "Store/Cask.cs",
                3,
            ),
            def(
                "App.Store.Sommelier",
                "Sommelier",
                "App.Store",
                "class",
                "Store/Sommelier.cs",
                3,
            ),
        ],
        vec![
            uses_member(
                "Store/Sommelier.cs",
                5,
                "App.Store.Cellar",
                "Store/Cellar.cs",
            ),
            uses_member("Store/Sommelier.cs", 6, "App.Store.Cask", "Store/Cask.cs"),
        ],
    );
    g.names = vec![
        name_row("Cellar", "class", "Store/Cellar.cs", 3, ""),
        name_row("Rack", "method", "Store/Cellar.cs", 5, "App.Store.Cellar"),
        name_row("Uncork", "method", "Store/Cellar.cs", 7, "App.Store.Cellar"),
        name_row("Cask", "class", "Store/Cask.cs", 3, ""),
        name_row("Rack", "method", "Store/Cask.cs", 5, "App.Store.Cask"),
        name_row("Sommelier", "class", "Store/Sommelier.cs", 3, ""),
        name_row(
            "Cellar",
            "method",
            "Store/Sommelier.cs",
            9,
            "App.Store.Sommelier",
        ),
    ];
    let root = temp_repo_root("member-seed");
    write_manifest_fixture(
        &root,
        &["Store/Cellar.cs", "Store/Cask.cs", "Store/Sommelier.cs"],
    );
    (g, root)
}

#[test]
fn resolve_member_seed_finds_a_unique_bare_member() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    assert_eq!(
        resolve_member_seed(&index, "Uncork"),
        MemberSeedResolution::Resolved("App.Store.Cellar".to_string())
    );
}

#[test]
fn resolve_member_seed_lists_candidates_when_two_types_declare_the_name() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    let MemberSeedResolution::Ambiguous(candidates) = resolve_member_seed(&index, "Rack") else {
        panic!("expected Ambiguous");
    };
    let owners: Vec<&str> = candidates.iter().map(|c| c.owner.as_str()).collect();
    assert_eq!(
        owners,
        vec!["App.Store.Cellar", "App.Store.Cask"],
        "name-index order, never a bare type list: each row still carries its own file and line"
    );
    assert_eq!(candidates[0].file, "Store/Cellar.cs");
    assert_eq!(candidates[0].line, 5);
}

#[test]
fn resolve_member_seed_accepts_type_dot_member_and_namespace_dot_type_dot_member() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    assert_eq!(
        resolve_member_seed(&index, "Cellar.Rack"),
        MemberSeedResolution::Resolved("App.Store.Cellar".to_string()),
        "Type.Member narrows the otherwise-ambiguous bare name"
    );
    assert_eq!(
        resolve_member_seed(&index, "App.Store.Cellar.Rack"),
        MemberSeedResolution::Resolved("App.Store.Cellar".to_string()),
        "Namespace.Type.Member reaches the same def"
    );
}

#[test]
fn resolve_member_seed_refuses_a_qualifier_naming_the_wrong_type() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    assert_eq!(
        resolve_member_seed(&index, "Cask.Uncork"),
        MemberSeedResolution::NotFound,
        "Cask declares no Uncork -- the qualifier admits no owner at all"
    );
}

#[test]
fn resolve_member_seed_finds_nothing_for_a_name_the_graph_does_not_hold() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    assert_eq!(
        resolve_member_seed(&index, "Xylophone"),
        MemberSeedResolution::NotFound
    );
}

// C1: the type ladder runs to completion, and wins, before the member ladder
// is ever consulted. `Cellar` is both a type AND (via the fixture's odd
// second name row) a method name on `Sommelier`; every verb must still
// answer as the type.
#[test]
fn a_name_that_resolves_to_a_type_never_falls_through_to_the_member_ladder() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);

    match resolve_impact_seed(&index, "Cellar") {
        SeedResolution::Resolved { kind, ids } => {
            assert_eq!(kind, SeedKind::Symbol);
            assert_eq!(ids, vec!["App.Store.Cellar".to_string()]);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }

    match build_tests_model(&index, "Cellar") {
        TestsResult::Resolved(model) => {
            assert_eq!(model.symbol, "App.Store.Cellar");
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

// C4: an exact member match resolves instead of a caller ever seeing a
// zero-hit "fall back to text search" note for it (the note itself is
// `cli.rs`'s job; what this pins is that resolution SUCCEEDS here, on both
// verbs that previously had no member ladder at all).
#[test]
fn impact_and_tests_answer_a_bare_member_seed_instead_of_reporting_zero_hits() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);

    let ImpactResult::Resolved(model) = build_impact_model(
        &index,
        "Uncork",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) else {
        panic!("a member seed with a real inbound reference must resolve, not zero-hit");
    };
    assert_eq!(model.seed_files, vec!["Store/Cellar.cs".to_string()]);

    let TestsResult::Resolved(model) = build_tests_model(&index, "Uncork") else {
        panic!("a member seed must resolve on `tests` too");
    };
    assert_eq!(model.symbol, "App.Store.Cellar");
}

#[test]
fn impact_and_tests_answer_ambiguous_member_ambiguous_not_a_bare_type_list() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);

    match build_impact_model(
        &index,
        "Rack",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::MemberAmbiguous(candidates) => {
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected MemberAmbiguous, got {other:?}"),
    }

    match build_tests_model(&index, "Rack") {
        TestsResult::MemberAmbiguous(candidates) => {
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected MemberAmbiguous, got {other:?}"),
    }
}

// `--pick`'s own re-resolution builds exactly this string and feeds it back
// in, so the seed it constructs must itself resolve the same way a caller
// typing it by hand would.
#[test]
fn qualified_seed_of_a_candidate_resolves_back_to_that_same_owner() {
    let (g, root) = member_seed_fixture();
    let index = load_graph_index(&g, &root);
    let MemberSeedResolution::Ambiguous(candidates) = resolve_member_seed(&index, "Rack") else {
        panic!("expected Ambiguous");
    };
    assert_eq!(
        resolve_member_seed(&index, &qualified_seed(&candidates[0])),
        MemberSeedResolution::Resolved(candidates[0].owner.clone())
    );
}
