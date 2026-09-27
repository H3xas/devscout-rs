use super::*;

fn ts_def(file: &str, name: &str) -> graph::Def {
    def(&format!("{file}#{name}"), name, "", "function", file, 1)
}

fn call(from: &str, line: usize, to_file: &str, name: &str) -> graph::Edge {
    graph::Edge::Call {
        from_file: from.into(),
        from_line: line,
        to: format!("{to_file}#{name}"),
        to_file: to_file.into(),
    }
}

fn jsx_use(from: &str, line: usize, to_file: &str, name: &str) -> graph::Edge {
    graph::Edge::JsxUse {
        from_file: from.into(),
        from_line: line,
        to: format!("{to_file}#{name}"),
        to_file: to_file.into(),
    }
}

fn dispatch_edge(from: &str, line: usize, to_file: &str, name: &str) -> graph::Edge {
    graph::Edge::Dispatch {
        from_file: from.into(),
        from_line: line,
        to: format!("{to_file}#{name}"),
        to_file: to_file.into(),
    }
}

fn import(from: &str, line: usize, to_file: &str, bound: &[&str]) -> graph::Edge {
    graph::Edge::Import {
        from_file: from.into(),
        from_line: line,
        target: "./x".into(),
        to_file: to_file.into(),
        via: None,
        bound_defs: bound.iter().map(|n| format!("{to_file}#{n}")).collect(),
    }
}

fn graph_and_root(label: &str) -> (graph::Graph, PathBuf) {
    let defs = vec![ts_def("lib/a.ts", "alpha"), ts_def("lib/a.ts", "beta")];
    let edges = vec![
        call("app/one.ts", 3, "lib/a.ts", "alpha"),
        jsx_use("app/two.tsx", 4, "lib/a.ts", "alpha"),
        dispatch_edge("app/three.ts", 5, "lib/a.ts", "beta"),
        import("app/one.ts", 1, "lib/a.ts", &["alpha", "beta"]),
        import("app/ns.ts", 1, "lib/a.ts", &[]),
        call("app/four.js", 2, "lib/a.ts", "beta"),
    ];
    let root = stage4_root(&defs, &edges, label);
    (make_graph(defs, edges), root)
}

#[test]
fn each_ts_kind_is_admitted_inbound_under_its_definition_and_outbound_under_its_file() {
    let (g, root) = graph_and_root("ts-admission");
    let index = load_graph_index(&g, &root);
    let alpha = &index.inbound["lib/a.ts#alpha"].ts;
    assert_eq!(
        (&alpha.call, &alpha.jsx_use, &alpha.dispatch, &alpha.import),
        (&vec![0], &vec![1], &vec![], &vec![3])
    );
    let beta = &index.inbound["lib/a.ts#beta"].ts;
    assert_eq!(
        (&beta.call, &beta.dispatch, &beta.import),
        (&vec![5], &vec![2], &vec![3]),
        "a JavaScript caller is admitted like any other"
    );
    let one = &index.outbound_by_file["app/one.ts"].ts;
    assert_eq!((&one.call, &one.import), (&vec![0], &vec![3]));
    assert_eq!(
        index.outbound_by_file["app/one.ts"].uses_member,
        Vec::<usize>::new()
    );
}

#[test]
fn an_import_binding_no_definition_is_not_admitted() {
    let (g, root) = graph_and_root("ts-unbound-import");
    let index = load_graph_index(&g, &root);
    assert!(
        !index.outbound_by_file.contains_key("app/ns.ts"),
        "a namespace import earns no adjacency"
    );
    assert!(index
        .inbound
        .values()
        .all(|entry| !entry.ts.import.contains(&4)));
}

#[test]
fn no_dispatch_no_guess_and_no_bus_leave_the_ts_adjacency_alone() {
    let (g, root) = graph_and_root("ts-flags");
    let full = load_graph_index(&g, &root);
    for opts in [
        IndexOptions {
            include_dispatch: false,
            ..IndexOptions::default()
        },
        IndexOptions {
            include_guesses: false,
            ..IndexOptions::default()
        },
        IndexOptions {
            include_bus: false,
            ..IndexOptions::default()
        },
    ] {
        let narrowed = load_graph_index_with(&g, &root, opts);
        for id in ["lib/a.ts#alpha", "lib/a.ts#beta"] {
            assert_eq!(
                narrowed.inbound[id].ts.lists(),
                full.inbound[id].ts.lists(),
                "{opts:?} changed {id}"
            );
        }
    }
}

#[test]
fn admitted_ts_edges_count_toward_the_hub_indegree_and_unbound_imports_do_not() {
    let (g, root) = graph_and_root("ts-hub");
    let index = load_graph_index(&g, &root);
    assert_eq!(
        index.hub_indegree.get("lib/a.ts").copied(),
        Some(4),
        "one, two, three and four reference it; the namespace-only file does not"
    );
}
