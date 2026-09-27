// Projects Rust fragments into graph definitions and names.
//
// A Rust fragment carries items with spans and nothing else, so there is
// nothing to resolve: each item becomes one def (which `read` quotes by its
// span) and one name row (which `find` prints). The rows are appended after
// every C# and TypeScript row, so a tree without a `.rs` file serializes
// exactly as it did before Rust was indexed.
//
// An id is `<file>#<path>`, the repository-relative file and the item's
// file-local path. A crate path would need the module tree (`mod`
// declarations, `mod.rs`, the crate roots); the file path is exact per file
// and matches how TypeScript ids are formed.

use crate::extract::RsFragment;
use crate::graph::{Def, Graph, GraphName};

/// The graph id of a Rust item: `<file>#<path>`.
pub fn rs_def_id(file: &str, path: &str) -> String {
    format!("{file}#{path}")
}

// `<file>` for a file-level item, `<file>#<parent path>` otherwise. The
// parent is the path up to its last `::`; a `-N` suffix sits on the last
// segment only, so it never reaches a namespace.
fn rs_namespace(file: &str, path: &str) -> String {
    match path.rfind("::") {
        Some(i) => rs_def_id(file, &path[..i]),
        None => file.to_string(),
    }
}

/// Appends one def and one name row per item, in fragment order and then
/// item order, and counts them in `stats`. No fragment leaves `graph`
/// untouched.
pub fn append_rust(graph: &mut Graph, fragments: &[(String, RsFragment)]) {
    if fragments.is_empty() {
        return;
    }
    let mut added = 0;
    for (file, fragment) in fragments {
        for item in &fragment.items {
            graph.defs.push(Def {
                id: rs_def_id(file, &item.path),
                name: item.name.clone(),
                namespace: rs_namespace(file, &item.path),
                kind: item.kind.clone(),
                file: file.clone(),
                line: item.line,
                methods: Vec::new(),
                test_methods: Vec::new(),
                also_in: Vec::new(),
                end_line: item.end_line,
            });
            graph.names.push(GraphName {
                name: item.name.clone(),
                kind: item.kind.clone(),
                file: file.clone(),
                line: item.name_line,
                owner: String::new(),
            });
            added += 1;
        }
    }
    graph.stats.def_count += added;
    graph.stats.file_count += fragments.len();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::RsItem;

    fn empty_graph() -> Graph {
        let root = std::env::temp_dir().join(format!("scout-rsgraph-test-{}", std::process::id()));
        crate::resolve::resolve_graph_with_model(&root, &[], &[], None, None)
    }

    fn item(
        name: &str,
        kind: &str,
        path: &str,
        line: usize,
        name_line: usize,
        end_line: usize,
    ) -> RsItem {
        RsItem {
            name: name.into(),
            kind: kind.into(),
            path: path.into(),
            line,
            name_line,
            end_line,
        }
    }

    fn fragment(items: Vec<RsItem>) -> RsFragment {
        RsFragment {
            rs: 1,
            items,
            ..RsFragment::default()
        }
    }

    #[test]
    fn ids_and_namespaces_follow_the_path() {
        assert_eq!(rs_def_id("src/a.rs", "Widget::new"), "src/a.rs#Widget::new");
        assert_eq!(rs_namespace("src/a.rs", "polish"), "src/a.rs");
        assert_eq!(
            rs_namespace("src/a.rs", "cabinet::Hinge"),
            "src/a.rs#cabinet"
        );
        assert_eq!(
            rs_namespace("src/a.rs", "cabinet::Hinge::oil"),
            "src/a.rs#cabinet::Hinge"
        );
        assert_eq!(rs_namespace("src/a.rs", "Gadget::fmt-1"), "src/a.rs#Gadget");
        assert_eq!(rs_namespace("src/a.rs", "home_shelf-1"), "src/a.rs");
    }

    #[test]
    fn no_fragment_leaves_the_serialized_graph_unchanged() {
        let mut g = empty_graph();
        let before = serde_json::to_string(&g).unwrap();
        append_rust(&mut g, &[]);
        assert_eq!(serde_json::to_string(&g).unwrap(), before);
    }

    #[test]
    fn rows_follow_every_existing_row_and_stats_count_them() {
        let mut g = empty_graph();
        g.defs.push(Def {
            id: "App.Existing".into(),
            name: "Existing".into(),
            namespace: "App".into(),
            kind: "class".into(),
            file: "src/Existing.cs".into(),
            line: 1,
            methods: Vec::new(),
            test_methods: Vec::new(),
            also_in: Vec::new(),
            end_line: 3,
        });
        g.stats.def_count = 1;
        g.stats.file_count = 1;
        let fragments = vec![
            (
                "src/b.rs".to_string(),
                fragment(vec![
                    item("Widget", "struct", "Widget", 1, 2, 4),
                    item("new", "fn", "Widget::new", 6, 7, 9),
                ]),
            ),
            ("src/empty.rs".to_string(), fragment(Vec::new())),
            (
                "src/a.rs".to_string(),
                fragment(vec![item("polish", "fn", "polish", 3, 3, 5)]),
            ),
        ];
        append_rust(&mut g, &fragments);

        let ids: Vec<&str> = g.defs.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "App.Existing",
                "src/b.rs#Widget",
                "src/b.rs#Widget::new",
                "src/a.rs#polish"
            ]
        );
        assert_eq!(g.stats.def_count, 4);
        assert_eq!(g.stats.file_count, 4);

        let new = &g.defs[2];
        assert_eq!(
            (new.name.as_str(), new.namespace.as_str(), new.kind.as_str()),
            ("new", "src/b.rs#Widget", "fn")
        );
        assert_eq!((new.line, new.end_line), (6, 9));
        assert!(new.methods.is_empty());

        let names: Vec<(&str, &str, &str, usize, &str)> = g
            .names
            .iter()
            .map(|n| {
                (
                    n.name.as_str(),
                    n.kind.as_str(),
                    n.file.as_str(),
                    n.line,
                    n.owner.as_str(),
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ("Widget", "struct", "src/b.rs", 2, ""),
                ("new", "fn", "src/b.rs", 7, ""),
                ("polish", "fn", "src/a.rs", 3, ""),
            ]
        );
    }

    #[test]
    fn a_def_serializes_in_the_shared_shape() {
        let mut g = empty_graph();
        append_rust(
            &mut g,
            &[(
                "src/a.rs".to_string(),
                fragment(vec![item("oil", "fn", "cabinet::Hinge::oil", 4, 5, 7)]),
            )],
        );
        assert_eq!(
            serde_json::to_string(&g.defs[0]).unwrap(),
            r#"{"id":"src/a.rs#cabinet::Hinge::oil","name":"oil","namespace":"src/a.rs#cabinet::Hinge","kind":"fn","file":"src/a.rs","line":4,"methods":[],"endLine":7}"#
        );
        assert_eq!(
            serde_json::to_string(&g.names[0]).unwrap(),
            r#"{"name":"oil","kind":"fn","file":"src/a.rs","line":5}"#
        );
    }
}
