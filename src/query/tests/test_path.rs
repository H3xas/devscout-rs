use super::*;
use crate::query::test_path::{is_test_path, test_via};

#[test]
fn the_suffixes_and_a_tests_directory_segment_name_a_test_file() {
    for file in [
        "a/b.spec.ts",
        "a/b.spec.tsx",
        "a/b.test.ts",
        "a/b.test.tsx",
        "a/b.spec.js",
        "a/b.test.js",
        "a/b.spec.mjs",
        "a/b.test.mjs",
        "a/b.spec.cjs",
        "a/b.test.cjs",
        "a/__tests__/b.ts",
        "a/__tests__/b.js",
        "a/__tests__/b.mjs",
        "a/__tests__/b.cjs",
        "__tests__/x/y/b.tsx",
    ] {
        assert!(is_test_path(file), "{file}");
    }
}

#[test]
fn near_misses_are_not_test_files() {
    for file in [
        "a/b.spec.jsx",
        "a/b.test.jsx",
        "a/__tests__/b.jsx",
        "a/b.spec.mts",
        "a/b.test.cts",
        "tests/b.ts",
        "a/my__tests__/b.ts",
        "a/cart__tests__.ts",
        "a/__tests__.ts",
        "a/b.Spec.ts",
        "a/b.TEST.tsx",
        "a/b.TEST.js",
        "a/__Tests__/b.ts",
        "a/__tests__x/b.ts",
        "a/__tests__/b.spec.ts.snap",
        "a/b.spec.ts.snap",
        "a/b.spec.cs",
        "a/b.spec",
    ] {
        assert!(!is_test_path(file), "{file}");
    }
}

fn index_over(
    defs: Vec<graph::Def>,
    units: Vec<graph::GraphUnit>,
    label: &str,
) -> (graph::Graph, PathBuf) {
    let root = stage4_root(&defs, &[], label);
    (make_graph_with_units(defs, Vec::new(), units), root)
}

#[test]
fn an_attributed_def_and_a_test_project_outrank_the_file_name() {
    let defs = vec![
        test_def("App.Specs", "Specs", "app/a.spec.ts", 1, &["runs"]),
        test_def("App.Specs", "SpecsMjs", "app/e.spec.mjs", 1, &["runs"]),
        def("app/b.spec.ts#b", "b", "", "function", "app/b.spec.ts", 1),
        def("app/c.spec.ts#c", "c", "", "function", "app/c.spec.ts", 1),
        def("app/h.spec.cjs#h", "h", "", "function", "app/h.spec.cjs", 1),
        def("lib/d.ts#d", "d", "", "function", "lib/d.ts", 1),
        def("lib/i.spec.mjs#i", "i", "", "function", "lib/i.spec.mjs", 1),
    ];
    let units = vec![graph::GraphUnit {
        id: "app/App.Tests.csproj".to_string(),
        name: "App.Tests".to_string(),
        refs: Vec::new(),
        test: true,
    }];
    let (g, root) = index_over(defs, units, "test-path-precedence");
    let index = load_graph_index(&g, &root);
    assert_eq!(test_via(&index, "app/a.spec.ts"), Some(TestVia::Attribute));
    assert_eq!(test_via(&index, "app/e.spec.mjs"), Some(TestVia::Attribute));
    assert_eq!(test_via(&index, "app/b.spec.ts"), Some(TestVia::Project));
    assert_eq!(test_via(&index, "app/h.spec.cjs"), Some(TestVia::Project));
    assert_eq!(test_via(&index, "lib/i.spec.mjs"), Some(TestVia::Path));
    assert_eq!(test_via(&index, "lib/d.ts"), None);

    let (g, root) = index_over(
        vec![
            def("app/c.spec.ts#c", "c", "", "function", "app/c.spec.ts", 1),
            def("app/j.test.js#j", "j", "", "function", "app/j.test.js", 1),
        ],
        Vec::new(),
        "test-path-name-only",
    );
    let index = load_graph_index(&g, &root);
    assert_eq!(test_via(&index, "app/c.spec.ts"), Some(TestVia::Path));
    assert!(index.is_test_file("app/c.spec.ts"));
    assert_eq!(test_via(&index, "app/j.test.js"), Some(TestVia::Path));
    assert!(index.is_test_file("app/j.test.js"));
}
