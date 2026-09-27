use super::*;

#[test]
fn two_functions_of_one_name_each_receive_only_their_own_calls() {
    let files = [
        (
            "src/lib.rs",
            "mod a;\nmod b;\nfn t() { a::glow(); b::glow(); a::glow(); }\n",
        ),
        ("src/a.rs", "pub fn glow() {}\nfn inner() { glow(); }\n"),
        ("src/b.rs", "pub fn glow() {}\n"),
    ];
    assert_eq!(
        edges_to(&files, "src/a.rs#glow"),
        vec![
            "uses-member src/lib.rs:3 -> src/a.rs#glow",
            "uses-member src/lib.rs:3 -> src/a.rs#glow",
            "uses-member src/a.rs:2 -> src/a.rs#glow",
        ]
    );
    assert_eq!(
        edges_to(&files, "src/b.rs#glow"),
        vec!["uses-member src/lib.rs:3 -> src/b.rs#glow"]
    );
}

#[test]
fn a_name_two_globs_provide_is_one_ambiguous_edge_with_both_candidates() {
    let files = [(
        "src/lib.rs",
        "mod a { pub fn glow() {} }\nmod b { pub fn glow() {} }\nuse a::*;\nuse b::*;\nfn t() { glow(); }\n",
    )];
    let graph = graph_for(&files);
    let rust: Vec<&Edge> = graph.edges.iter().collect();
    assert_eq!(rust.len(), 1);
    assert_eq!(
        serde_json::to_string(rust[0]).unwrap(),
        r#"{"kind":"ambiguous","origin":"uses-member","from_file":"src/lib.rs","from_line":5,"raw":"glow","candidates":[{"id":"src/lib.rs#a::glow","file":"src/lib.rs"},{"id":"src/lib.rs#b::glow","file":"src/lib.rs"}],"candidate_count":2}"#
    );
    assert_eq!(graph.stats.edges_by_kind.uses_member, 0);
    assert_eq!(
        graph.stats.ambiguous_count, 0,
        "the C# ambiguity counter keeps its meaning"
    );
}

#[test]
fn candidates_are_capped_and_counted_uncapped() {
    let mut src = String::new();
    for m in ["a", "b", "c", "d", "e", "f", "g"] {
        src.push_str(&format!("mod {m} {{ pub fn glow() {{}} }}\nuse {m}::*;\n"));
    }
    src.push_str("fn t() { glow(); }\n");
    let graph = graph_for(&[("src/lib.rs", src.as_str())]);
    match &graph.edges[0] {
        Edge::Ambiguous {
            candidates,
            candidate_count,
            ..
        } => {
            assert_eq!(candidates.len(), crate::resolve::AMBIGUOUS_CAP);
            assert_eq!(*candidate_count, 7);
        }
        other => panic!("expected an ambiguous edge, got {other:?}"),
    }
}

#[test]
fn a_name_nothing_provides_gives_no_edge() {
    let files = [(
        "src/lib.rs",
        "fn t() { glow(); std::mem::drop(1); Vec::new(); }\n",
    )];
    assert!(edges_for(&files).is_empty());
}

#[test]
fn a_long_pub_use_chain_and_deep_module_nesting_resolve_without_recursion() {
    let depth = 400;
    let mut chain = String::new();
    for i in 0..depth {
        chain.push_str(&format!(
            "mod m{i} {{ pub use crate::m{}::glow; }}\n",
            i + 1
        ));
    }
    chain.push_str(&format!(
        "mod m{depth} {{ pub fn glow() {{}} }}\nfn t() {{ m0::glow(); }}\n"
    ));
    let nested = format!(
        "{}pub fn deep() {{}}{}\nfn u() {{ {}deep(); }}\n",
        "pub mod n { ".repeat(depth),
        " }".repeat(depth),
        "n::".repeat(depth)
    );
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            (
                edges_for(&[("src/lib.rs", chain.as_str())]),
                edges_for(&[("src/lib.rs", nested.as_str())]),
            )
        })
        .unwrap();
    let (chain_edges, nested_edges) = handle.join().unwrap();
    assert_eq!(
        chain_edges,
        vec![format!(
            "uses-member src/lib.rs:{} -> src/lib.rs#m{depth}::glow",
            depth + 2
        )]
    );
    assert_eq!(nested_edges.len(), 1);
    assert!(nested_edges[0].ends_with("::deep"), "{nested_edges:?}");
}
