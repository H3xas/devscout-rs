use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::refs_tables::edge_loc;
use crate::graph;

mod coverage;
mod dispatch;
mod find;
mod heuristic_ordering;
mod impact_from_lines;
mod impact_hub;
mod impact_iface;
mod impact_iface_brake;
mod impact_misc;
mod impact_ranking;
mod index;
mod member;
mod read;
mod refs_bare_member;
mod refs_enum;
mod refs_tables;
mod symbol;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_repo_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("scout-query-test-{label}-{nanos}-{n}"));
    fs::create_dir_all(dir.join(".git")).unwrap();
    dir
}

/// Mirrors `fixtureRoot`'s manifest half: every file in `manifestFiles`
/// gets `{purpose:'x', mtime:1, source:'ast'}`.
fn write_manifest_fixture(root: &Path, files: &[&str]) {
    let dir = root.join(".git").join("scout");
    fs::create_dir_all(&dir).unwrap();
    let mut entries = serde_json::Map::new();
    for f in files {
        entries.insert(
            (*f).to_string(),
            serde_json::json!({"purpose": "x", "mtime": 1, "source": "ast"}),
        );
    }
    let manifest =
        serde_json::json!({"built_at_head": "deadbeef", "scoped_dirs": ["."], "entries": entries});
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
}

fn dummy_stats() -> graph::Stats {
    graph::Stats {
        def_count: 0,
        file_count: 0,
        edges_by_kind: graph::EdgesByKind::default(),
        ambiguous_count: 0,
        ambiguous_pct: graph::Percent1::zero(),
        unresolved_external_count: 0,
        heuristic_edge_count: 0,
        test_def_count: 0,
        heuristic_by_tier: graph::HeuristicByTier::default(),
        // Incidental: `ts` has no bearing on this C#-only query-layer
        // fixture.
        ts: None,
    }
}

fn make_graph(defs: Vec<graph::Def>, edges: Vec<graph::Edge>) -> graph::Graph {
    graph::Graph {
        schema_version: graph::GRAPH_SCHEMA_VERSION,
        built_at_head: Some("deadbeef".to_string()),
        defs,
        edges,
        stats: dummy_stats(),
        names: Vec::new(),
        units: Vec::new(),
    }
}

/// `make_graph` plus a `.csproj` project model -- `load_graph_index`
/// rebuilds a `ProjectModel` from `graph.units` (via
/// `project::units_from_graph`/`ProjectModel::from_units`) exactly the
/// way it would from a real `graph.json`, so a test never has to reach
/// into `GraphIndex.project` by hand.
fn make_graph_with_units(
    defs: Vec<graph::Def>,
    edges: Vec<graph::Edge>,
    units: Vec<graph::GraphUnit>,
) -> graph::Graph {
    let mut g = make_graph(defs, edges);
    g.units = units;
    g
}

fn def(id: &str, name: &str, namespace: &str, kind: &str, file: &str, line: usize) -> graph::Def {
    graph::Def {
        id: id.into(),
        name: name.into(),
        namespace: namespace.into(),
        kind: kind.into(),
        file: file.into(),
        line,
        methods: vec![],
        test_methods: vec![],
        also_in: vec![],
        end_line: 0,
    }
}

fn def_also(
    id: &str,
    name: &str,
    namespace: &str,
    kind: &str,
    file: &str,
    line: usize,
    also_in: Vec<(&str, usize)>,
) -> graph::Def {
    graph::Def {
        id: id.into(),
        name: name.into(),
        namespace: namespace.into(),
        kind: kind.into(),
        file: file.into(),
        line,
        methods: vec![],
        test_methods: vec![],
        also_in: also_in
            .into_iter()
            .map(|(f, l)| graph::AlsoIn {
                file: f.into(),
                line: l,
            })
            .collect(),
        end_line: 0,
    }
}

fn inherits(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::Inherits {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: false,
    }
}

fn uses_type(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::UsesType {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: false,
    }
}

fn uses_member(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::UsesMember {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: false,
        tier: None,
        member: None,
    }
}

// The same edge, tagged as a scored guess -- the only difference the query
// layer is allowed to see.
fn heuristic_uses_member(
    from_file: &str,
    from_line: usize,
    to: &str,
    to_file: &str,
) -> graph::Edge {
    graph::Edge::UsesMember {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: true,
        tier: Some(graph::HeuristicTier::Guess),
        member: None,
    }
}

// And the same edge tagged as the OTHER heuristic tier: extension-method
// lookup, which the query surface reports apart from a name guess.
fn ext_uses_member(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::UsesMember {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: true,
        tier: Some(graph::HeuristicTier::Ext),
        member: None,
    }
}

fn heuristic_uses_type(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
    graph::Edge::UsesType {
        from_file: from_file.into(),
        from_line,
        to: to.into(),
        to_file: to_file.into(),
        heuristic: true,
    }
}

fn imports(from_file: &str, from_line: usize, target: &str) -> graph::Edge {
    graph::Edge::Imports {
        from_file: from_file.into(),
        from_line,
        target: target.into(),
    }
}

fn ambiguous(
    from_file: &str,
    from_line: usize,
    raw: &str,
    candidates: Vec<(&str, &str)>,
) -> graph::Edge {
    let candidate_count = candidates.len();
    graph::Edge::Ambiguous {
        origin: "uses-type".into(),
        from_file: from_file.into(),
        from_line,
        raw: raw.into(),
        candidates: candidates
            .into_iter()
            .map(|(id, file)| graph::Candidate {
                id: id.into(),
                file: file.into(),
            })
            .collect(),
        candidate_count,
    }
}

// --- base fixture ---

const BASE_MANIFEST_FILES: &[&str] = &[
    "Widgets/IWidget.cs",
    "Widgets/Impl/WidgetImpl.cs",
    "Widgets/Impl/OtherImpl.cs",
    "Consumers/TwoHop.cs",
    "Consumers/Holder.cs",
    "One/Config.cs",
    "Two/Config.cs",
    "Outer/Container.cs",
    "Outer/Container.Extra.cs",
    "Three/Consumer.cs",
    // Ghost/NotInManifest.cs deliberately omitted.
];

fn base_fixture_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "App.Widgets.IWidget",
                "IWidget",
                "App.Widgets",
                "interface",
                "Widgets/IWidget.cs",
                3,
            ),
            def(
                "App.Widgets.Impl.WidgetImpl",
                "WidgetImpl",
                "App.Widgets.Impl",
                "class",
                "Widgets/Impl/WidgetImpl.cs",
                5,
            ),
            def(
                "App.Widgets.Impl.OtherImpl",
                "OtherImpl",
                "App.Widgets.Impl",
                "class",
                "Widgets/Impl/OtherImpl.cs",
                5,
            ),
            def(
                "App.Consumers.TwoHop",
                "TwoHop",
                "App.Consumers",
                "class",
                "Consumers/TwoHop.cs",
                3,
            ),
            def(
                "App.One.Config",
                "Config",
                "App.One",
                "class",
                "One/Config.cs",
                1,
            ),
            def(
                "App.Two.Config",
                "Config",
                "App.Two",
                "class",
                "Two/Config.cs",
                1,
            ),
            def_also(
                "App.Outer.Container",
                "Container",
                "App.Outer",
                "class",
                "Outer/Container.cs",
                3,
                vec![("Outer/Container.Extra.cs", 1)],
            ),
            def(
                "App.Outer.Container+Item",
                "Item",
                "App.Outer",
                "class",
                "Outer/Container.cs",
                5,
            ),
            def(
                "App.Ghost.NotInManifest",
                "NotInManifest",
                "App.Ghost",
                "class",
                "Ghost/NotInManifest.cs",
                1,
            ),
        ],
        vec![
            inherits(
                "Widgets/Impl/WidgetImpl.cs",
                5,
                "App.Widgets.IWidget",
                "Widgets/IWidget.cs",
            ),
            inherits(
                "Widgets/Impl/OtherImpl.cs",
                5,
                "App.Widgets.IWidget",
                "Widgets/IWidget.cs",
            ),
            uses_type(
                "Consumers/Holder.cs",
                8,
                "App.Widgets.IWidget",
                "Widgets/IWidget.cs",
            ),
            uses_type(
                "Consumers/TwoHop.cs",
                4,
                "App.Widgets.Impl.WidgetImpl",
                "Widgets/Impl/WidgetImpl.cs",
            ),
            imports("Widgets/Impl/WidgetImpl.cs", 1, "App.Widgets"),
            ambiguous(
                "Three/Consumer.cs",
                4,
                "Config",
                vec![
                    ("App.One.Config", "One/Config.cs"),
                    ("App.Two.Config", "Two/Config.cs"),
                ],
            ),
        ],
    )
}

fn base_fixture_root() -> PathBuf {
    let root = temp_repo_root("base");
    write_manifest_fixture(&root, BASE_MANIFEST_FILES);
    root
}

fn enum_fixture_graph() -> graph::Graph {
    make_graph(
        vec![
            def(
                "App.Enums.PostType",
                "PostType",
                "App.Enums",
                "enum",
                "Enums/PostType.cs",
                3,
            ),
            def(
                "App.Enums.PostType.Post",
                "Post",
                "App.Enums",
                "enum-member",
                "Enums/PostType.cs",
                5,
            ),
            def(
                "App.Enums.PostType.Question",
                "Question",
                "App.Enums",
                "enum-member",
                "Enums/PostType.cs",
                6,
            ),
            def(
                "App.Consumers.Reader",
                "Reader",
                "App.Consumers",
                "class",
                "Consumers/Reader.cs",
                3,
            ),
            def(
                "App.Consumers.TwoHop",
                "TwoHop",
                "App.Consumers",
                "class",
                "Consumers/TwoHop.cs",
                3,
            ),
        ],
        vec![
            uses_member(
                "Consumers/Reader.cs",
                8,
                "App.Enums.PostType.Question",
                "Enums/PostType.cs",
            ),
            uses_type(
                "Consumers/TwoHop.cs",
                4,
                "App.Consumers.Reader",
                "Consumers/Reader.cs",
            ),
        ],
    )
}

fn enum_fixture_root() -> PathBuf {
    let root = temp_repo_root("enum");
    write_manifest_fixture(
        &root,
        &[
            "Enums/PostType.cs",
            "Consumers/Reader.cs",
            "Consumers/TwoHop.cs",
        ],
    );
    root
}

// --- the interface hop ---

fn ctor_di_to(
    from_file: &str,
    from_line: usize,
    iface: &str,
    resolution: &str,
    to: &str,
) -> graph::Edge {
    graph::Edge::CtorDi {
        from_file: from_file.into(),
        from_line,
        iface: iface.into(),
        resolution: resolution.into(),
        args: None,
        to: Some(to.into()),
        candidates: vec![],
    }
}

// --- the hub-file brake ---

/// Two hub shapes reaching the same seed, each with its own consumers so
/// the two brakes can be told apart: `Api/Startup.cs` is a hub by NAME (an
/// entry point, whatever its in-degree), `Core/Shared.cs` only by
/// IN-DEGREE. `Core/Plain.cs` is neither and must keep expanding.
/// `Core/S5.cs`'s edge into `Core/Shared.cs` is a heuristic guess, so the
/// in-degree it contributes is the proof that the index spans both edge
/// kinds.
fn hub_fixture_graph() -> graph::Graph {
    let mut defs = vec![
        def(
            "App.Core.Widget",
            "Widget",
            "App.Core",
            "class",
            "Core/Widget.cs",
            3,
        ),
        def(
            "App.Api.Startup",
            "Startup",
            "App.Api",
            "class",
            "Api/Startup.cs",
            3,
        ),
        def(
            "App.Core.Shared",
            "Shared",
            "App.Core",
            "class",
            "Core/Shared.cs",
            3,
        ),
        def(
            "App.Core.Plain",
            "Plain",
            "App.Core",
            "class",
            "Core/Plain.cs",
            3,
        ),
        def("App.Core.S5", "S5", "App.Core", "class", "Core/S5.cs", 3),
        def(
            "App.Core.PlainUser",
            "PlainUser",
            "App.Core",
            "class",
            "Core/PlainUser.cs",
            3,
        ),
    ];
    let mut edges = vec![
        uses_type("Api/Startup.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
        uses_type("Core/Shared.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
        uses_type("Core/Plain.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
        heuristic_uses_type("Core/S5.cs", 4, "App.Core.Shared", "Core/Shared.cs"),
        uses_type("Core/PlainUser.cs", 4, "App.Core.Plain", "Core/Plain.cs"),
    ];
    for n in ["A", "B", "C", "D"] {
        defs.push(def(
            &format!("App.Api.{n}"),
            n,
            "App.Api",
            "class",
            &format!("Api/{n}.cs"),
            3,
        ));
        edges.push(uses_type(
            &format!("Api/{n}.cs"),
            4,
            "App.Api.Startup",
            "Api/Startup.cs",
        ));
    }
    for n in ["S1", "S2", "S3", "S4"] {
        defs.push(def(
            &format!("App.Core.{n}"),
            n,
            "App.Core",
            "class",
            &format!("Core/{n}.cs"),
            3,
        ));
        edges.push(uses_type(
            &format!("Core/{n}.cs"),
            4,
            "App.Core.Shared",
            "Core/Shared.cs",
        ));
    }
    make_graph(defs, edges)
}

fn hub_fixture_root() -> PathBuf {
    let root = temp_repo_root("hub-file");
    let mut files: Vec<String> = vec![
        "Core/Widget.cs".into(),
        "Api/Startup.cs".into(),
        "Core/Shared.cs".into(),
        "Core/Plain.cs".into(),
        "Core/PlainUser.cs".into(),
        "Core/S5.cs".into(),
    ];
    for n in ["A", "B", "C", "D"] {
        files.push(format!("Api/{n}.cs"));
    }
    for n in ["S1", "S2", "S3", "S4"] {
        files.push(format!("Core/{n}.cs"));
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    write_manifest_fixture(&root, &refs);
    root
}

// ========================================================================
// The CLI contract for heuristic edges. The shapes pinned here (row order,
// the shared cap, which edges seed a hop, the count line) live in this
// module plus render.rs, so these tests drive THOSE directly -- same
// fixtures, same expected literals, no process spawn.
//
// A hand-written graph rather than a `devscout map` of C# sources on
// purpose: driving these from source would mean building a fixture whose
// resolution happens to produce 51 inbound edges. The resolver's own tests
// own the question of WHICH edges get tagged; these own what the
// query+render layers do once they are.
// ========================================================================

fn widget_def() -> graph::Def {
    graph::Def {
        id: "App.Core.Widget".into(),
        name: "Widget".into(),
        namespace: "App.Core".into(),
        kind: "class".into(),
        file: "Core/Widget.cs".into(),
        line: 3,
        methods: vec!["Render".to_string()],
        test_methods: vec![],
        also_in: vec![],
        end_line: 0,
    }
}

/// A manifest listing every def file and every edge's from_file, so
/// `manifest_gap` stays 0 and the rendered bytes carry no trailing gap line.
fn stage4_root(defs: &[graph::Def], edges: &[graph::Edge], label: &str) -> PathBuf {
    let root = temp_repo_root(label);
    let mut files: Vec<String> = Vec::new();
    for d in defs {
        if !files.contains(&d.file) {
            files.push(d.file.clone());
        }
    }
    for e in edges {
        let f = edge_loc(e).0.to_string();
        if !files.contains(&f) {
            files.push(f);
        }
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    write_manifest_fixture(&root, &refs);
    root
}

// ========================================================================
// Test-coverage: test_defs_by_file, build_tests_model, impact tests_affected.
// ========================================================================

/// `def()` carrying a test-method list -- what makes its file a TEST file.
fn test_def(id: &str, name: &str, file: &str, line: usize, test_methods: &[&str]) -> graph::Def {
    graph::Def {
        test_methods: test_methods.iter().map(|s| s.to_string()).collect(),
        ..def(id, name, "App.Orders.Tests", "class", file, line)
    }
}

const TESTS_MANIFEST_FILES: &[&str] = &[
    "src/OrderService.cs",
    "src/Untested.cs",
    "tests/OrderServiceTests.cs",
    "tests/Fakes.cs",
    "tests/Partial.cs",
    "tests/Partial.Extra.cs",
];

/// One production type referenced twice from a real test file, once from a
/// non-test neighbour, and once by a GUESS from a partial test class's
/// second declaring file -- the four cases the model has to tell apart.
fn tests_fixture_graph() -> graph::Graph {
    let mut partial = test_def(
        "App.Orders.Tests.PartialTests",
        "PartialTests",
        "tests/Partial.cs",
        3,
        &["Scales"],
    );
    partial.also_in = vec![graph::AlsoIn {
        file: "tests/Partial.Extra.cs".into(),
        line: 3,
    }];
    make_graph(
        vec![
            def(
                "App.Orders.OrderService",
                "OrderService",
                "App.Orders",
                "class",
                "src/OrderService.cs",
                3,
            ),
            def(
                "App.Orders.Untested",
                "Untested",
                "App.Orders",
                "class",
                "src/Untested.cs",
                3,
            ),
            test_def(
                "App.Orders.Tests.OrderServiceTests",
                "OrderServiceTests",
                "tests/OrderServiceTests.cs",
                5,
                &["Totals"],
            ),
            def(
                "App.Orders.Tests.Fakes",
                "Fakes",
                "App.Orders.Tests",
                "class",
                "tests/Fakes.cs",
                3,
            ),
            partial,
        ],
        vec![
            uses_type(
                "tests/OrderServiceTests.cs",
                11,
                "App.Orders.OrderService",
                "src/OrderService.cs",
            ),
            uses_type(
                "tests/OrderServiceTests.cs",
                10,
                "App.Orders.OrderService",
                "src/OrderService.cs",
            ),
            uses_type(
                "tests/OrderServiceTests.cs",
                10,
                "App.Orders.OrderService",
                "src/OrderService.cs",
            ),
            uses_type(
                "tests/Fakes.cs",
                7,
                "App.Orders.OrderService",
                "src/OrderService.cs",
            ),
            heuristic_uses_member(
                "tests/Partial.Extra.cs",
                9,
                "App.Orders.OrderService",
                "src/OrderService.cs",
            ),
        ],
    )
}

fn tests_fixture_root() -> PathBuf {
    let root = temp_repo_root("tests-model");
    write_manifest_fixture(&root, TESTS_MANIFEST_FILES);
    root
}
