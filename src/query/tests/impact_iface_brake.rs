use super::*;

// --- the broad-interface fan-in brake ---

const BROAD_IFACE_MANIFEST_FILES: &[&str] = &[
    "Widgets/IWidgetRepository.cs",
    "Widgets/IWidgetClock.cs",
    "Widgets/WidgetRepository.cs",
    "Widgets/GadgetService.cs",
    "Widgets/WidgetConsumer00.cs",
    "Widgets/WidgetConsumer01.cs",
    "Widgets/WidgetConsumer02.cs",
    "Widgets/WidgetConsumer03.cs",
    "Widgets/WidgetConsumer04.cs",
    "Widgets/WidgetConsumer05.cs",
    "Widgets/WidgetConsumer06.cs",
    "Widgets/WidgetConsumer07.cs",
    "Widgets/WidgetConsumer08.cs",
    "Widgets/ClockConsumer0.cs",
    "Widgets/ClockConsumer1.cs",
];

/// `WidgetRepository` is the SOLE implementor of two contracts:
/// `IWidgetRepository`, ctor-injected by 9 distinct constructors (one over
/// the default threshold of 8), and `IWidgetClock`, injected by 2. Both are
/// ordinary, well-named application interfaces -- nothing about their names
/// or namespaces marks the first as plumbing, which is exactly why the
/// name-pattern `infra` class never catches this shape. `GadgetService`
/// names the concrete class directly.
fn broad_iface_fixture_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "App.Widgets.IWidgetRepository",
                "IWidgetRepository",
                "App.Widgets",
                "interface",
                "Widgets/IWidgetRepository.cs",
                3,
            ),
            def(
                "App.Widgets.IWidgetClock",
                "IWidgetClock",
                "App.Widgets",
                "interface",
                "Widgets/IWidgetClock.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetRepository",
                "WidgetRepository",
                "App.Widgets",
                "class",
                "Widgets/WidgetRepository.cs",
                3,
            ),
            def(
                "App.Widgets.GadgetService",
                "GadgetService",
                "App.Widgets",
                "class",
                "Widgets/GadgetService.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer00",
                "WidgetConsumer00",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer00.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer01",
                "WidgetConsumer01",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer01.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer02",
                "WidgetConsumer02",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer02.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer03",
                "WidgetConsumer03",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer03.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer04",
                "WidgetConsumer04",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer04.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer05",
                "WidgetConsumer05",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer05.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer06",
                "WidgetConsumer06",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer06.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer07",
                "WidgetConsumer07",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer07.cs",
                3,
            ),
            def(
                "App.Widgets.WidgetConsumer08",
                "WidgetConsumer08",
                "App.Widgets",
                "class",
                "Widgets/WidgetConsumer08.cs",
                3,
            ),
            def(
                "App.Widgets.ClockConsumer0",
                "ClockConsumer0",
                "App.Widgets",
                "class",
                "Widgets/ClockConsumer0.cs",
                3,
            ),
            def(
                "App.Widgets.ClockConsumer1",
                "ClockConsumer1",
                "App.Widgets",
                "class",
                "Widgets/ClockConsumer1.cs",
                3,
            ),
        ],
        vec![
            inherits(
                "Widgets/WidgetRepository.cs",
                3,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            inherits(
                "Widgets/WidgetRepository.cs",
                3,
                "App.Widgets.IWidgetClock",
                "Widgets/IWidgetClock.cs",
            ),
            uses_type(
                "Widgets/GadgetService.cs",
                6,
                "App.Widgets.WidgetRepository",
                "Widgets/WidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer00.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer00.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer01.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer01.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer02.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer02.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer03.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer03.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer04.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer04.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer05.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer05.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer06.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer06.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer07.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer07.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/WidgetConsumer08.cs",
                5,
                "IWidgetRepository",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/WidgetConsumer08.cs",
                5,
                "App.Widgets.IWidgetRepository",
                "Widgets/IWidgetRepository.cs",
            ),
            ctor_di_to(
                "Widgets/ClockConsumer0.cs",
                5,
                "IWidgetClock",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/ClockConsumer0.cs",
                5,
                "App.Widgets.IWidgetClock",
                "Widgets/IWidgetClock.cs",
            ),
            ctor_di_to(
                "Widgets/ClockConsumer1.cs",
                5,
                "IWidgetClock",
                "plain",
                "App.Widgets.WidgetRepository",
            ),
            uses_type(
                "Widgets/ClockConsumer1.cs",
                5,
                "App.Widgets.IWidgetClock",
                "Widgets/IWidgetClock.cs",
            ),
        ],
    )
}

fn broad_iface_fixture_root() -> PathBuf {
    let root = temp_repo_root("broad-iface");
    write_manifest_fixture(&root, BROAD_IFACE_MANIFEST_FILES);
    root
}

#[test]
fn build_impact_model_brakes_a_broad_interface_by_fan_in_while_a_narrow_one_on_the_same_class_still_hops(
) {
    let graph = broad_iface_fixture_graph();
    let root = broad_iface_fixture_root();
    let index = load_graph_index(&graph, &root);
    assert_eq!(
        index.ctor_di_fanin.get("IWidgetRepository"),
        Some(&9),
        "fan-in counts distinct constructor sites"
    );
    assert_eq!(index.ctor_di_fanin.get("IWidgetClock"), Some(&2));

    let model = match build_impact_model(
        &index,
        "WidgetRepository",
        1,
        DEFAULT_CAP,
        true,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
    files.sort();
    assert_eq!(
        files,
        vec!["Widgets/ClockConsumer0.cs", "Widgets/ClockConsumer1.cs", "Widgets/GadgetService.cs"],
        "the broad contract is braked on BOTH widening paths; the narrow one and the direct-name edge are untouched"
    );
    assert_eq!(
        model.braked,
        vec![BrakedIface {
            iface: "IWidgetRepository".to_string(),
            fanin: 9
        }],
        "the narrowing is reported, never silent"
    );
}

#[test]
fn build_impact_model_iface_max_fanin_zero_disables_the_brake_and_restores_the_ds_0050_radius() {
    let graph = broad_iface_fixture_graph();
    let root = broad_iface_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "WidgetRepository",
        1,
        DEFAULT_CAP,
        true,
        0,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model.rows.len(),
        12,
        "every ctor-injected consumer of both contracts, plus the direct-name reference"
    );
    assert!(
        model.braked.is_empty(),
        "a brake that never fired reports nothing"
    );

    // A threshold BELOW the narrow contract's own fan-in brakes it too,
    // widest first in the report -- the brake is a number, not a name list.
    let tight = match build_impact_model(
        &index,
        "WidgetRepository",
        1,
        DEFAULT_CAP,
        true,
        1,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        tight
            .rows
            .iter()
            .map(|r| r.file.as_str())
            .collect::<Vec<_>>(),
        vec!["Widgets/GadgetService.cs"]
    );
    assert_eq!(
        tight.braked,
        vec![
            BrakedIface {
                iface: "IWidgetRepository".to_string(),
                fanin: 9
            },
            BrakedIface {
                iface: "IWidgetClock".to_string(),
                fanin: 2
            },
        ]
    );
}

#[test]
fn build_impact_model_no_iface_keeps_its_ds_0050_meaning_no_hop_at_all_and_no_brake_report() {
    let graph = broad_iface_fixture_graph();
    let root = broad_iface_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_impact_model(
        &index,
        "WidgetRepository",
        1,
        DEFAULT_CAP,
        false,
        DEFAULT_IFACE_MAX_FANIN,
        DEFAULT_HUB_MAX_INDEGREE,
    ) {
        ImpactResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model
            .rows
            .iter()
            .map(|r| r.file.as_str())
            .collect::<Vec<_>>(),
        vec!["Widgets/GadgetService.cs"],
        "the hop is off entirely, so the narrow contract does not widen either"
    );
    assert!(
        model.braked.is_empty(),
        "nothing was braked because nothing was attempted"
    );
}
