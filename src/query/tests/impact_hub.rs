use super::*;

fn hub_files_of(model: &ImpactModel) -> Vec<String> {
    let mut files: Vec<String> = model.rows.iter().map(|r| r.file.clone()).collect();
    files.sort();
    files
}

#[test]
fn build_impact_model_a_hub_file_is_recorded_classed_infra_and_never_expanded_through() {
    let graph = hub_fixture_graph();
    let root = hub_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        hub_files_of(&model),
        vec![
            "Api/Startup.cs",
            "Core/Plain.cs",
            "Core/PlainUser.cs",
            "Core/S1.cs",
            "Core/S2.cs",
            "Core/S3.cs",
            "Core/S4.cs",
            "Core/S5.cs",
            "Core/Shared.cs"
        ],
        "the entry point is reached but its own four consumers are not"
    );
    let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
    assert!(
        row_of("Api/Startup.cs").infra,
        "the row says why the walk stopped there"
    );
    assert!(
        !row_of("Core/Shared.cs").infra,
        "an ordinary file carries no class key at all"
    );
    assert_eq!(
        model.braked_files,
        vec![BrakedFile {
            file: "Api/Startup.cs".to_string(),
            indegree: 4
        }]
    );
    assert!(model.braked.is_empty(), "no interface was braked here");
}

#[test]
fn build_impact_model_hub_max_indegree_brakes_a_file_no_name_pattern_matches_and_zero_disables_that_half_only(
) {
    let graph = hub_fixture_graph();
    let root = hub_fixture_root();
    let index = load_graph_index(&graph, &root);
    let tight = match build_impact_model(
        &index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        5,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        hub_files_of(&tight),
        vec![
            "Api/Startup.cs",
            "Core/Plain.cs",
            "Core/PlainUser.cs",
            "Core/Shared.cs"
        ],
        "the in-degree-5 file stops expanding too"
    );
    assert_eq!(
        tight.braked_files,
        vec![
            BrakedFile {
                file: "Core/Shared.cs".to_string(),
                indegree: 5
            },
            BrakedFile {
                file: "Api/Startup.cs".to_string(),
                indegree: 4
            },
        ],
        "widest-first, then by path"
    );

    let off = match build_impact_model(
        &index,
        "Widget",
        2,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        0,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        off.braked_files,
        vec![BrakedFile {
            file: "Api/Startup.cs".to_string(),
            indegree: 4
        }],
        "0 disables the threshold; the name-pattern classification is not a threshold and stays on"
    );
    assert!(
        hub_files_of(&off).contains(&"Core/S1.cs".to_string()),
        "the in-degree hub widens again"
    );
    assert!(
        !hub_files_of(&off).contains(&"Api/A.cs".to_string()),
        "an entry point is still an entry point at --hub-max-indegree 0"
    );
}

#[test]
fn build_impact_model_a_hub_reached_on_the_last_hop_is_never_reported_as_braked() {
    let graph = hub_fixture_graph();
    let root = hub_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "Widget",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        hub_files_of(&model),
        vec!["Api/Startup.cs", "Core/Plain.cs", "Core/Shared.cs"]
    );
    assert!(
        model.rows.iter().find(|r| r.file == "Api/Startup.cs").unwrap().infra,
        "the classification is a fact about the file, not about whether the walk had another hop left"
    );
    assert!(
        model.braked_files.is_empty(),
        "no hop remained, so no widening was refused"
    );
}

#[test]
fn is_infra_file_matches_the_four_shapes_and_nothing_that_merely_resembles_them() {
    for f in [
        "src/Program.cs",
        "Startup.cs",
        "a/CatalogServiceExtensions.cs",
        "a/FooServiceCollectionExtensions.cs",
        "a/JobQueueRegistration.cs",
        "a/DependencyResolution/Wire.cs",
        "a/CompositionRootTests.cs",
        "a/GroupControllerTestsBase.cs",
        "a/ControllerTestBase.cs",
        "a/BaseFixture.cs",
    ] {
        assert!(is_infra_file(f), "{f} must classify as infra");
    }
    for f in [
        "src/ProgramManager.cs",
        "src/StartupRunner.cs",
        "src/Registrations.cs",
        "src/DependencyResolutionHelper.cs",
        "src/CompositionRoot.Extra.cs",
        "src/Widget.cs",
    ] {
        assert!(!is_infra_file(f), "{f} must NOT classify as infra");
    }
}
