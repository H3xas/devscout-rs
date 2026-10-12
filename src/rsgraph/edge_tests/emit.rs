use super::*;

const LAMP: &str = "pub struct Lamp;\npub trait Shine { fn beam(); }\npub type Alias = Lamp;\n";

#[test]
fn each_callee_form_gives_one_uses_member_edge_with_the_model_fields() {
    let files = [
        ("src/lib.rs", "mod m;\nmod lamp;\nmod glow;\nfn f() {}\nfn t() {\n    f();\n    m::g();\n    crate::m::g();\n    lamp::Lamp::glow();\n    lamp::Shine::beam();\n    m::h::<u8>();\n}\n"),
        ("src/m.rs", "pub fn g() {}\npub fn h<T>() {}\npub fn back() { super::f(); }\n"),
        ("src/lamp.rs", LAMP),
        ("src/glow.rs", "impl crate::lamp::Lamp {\n    pub fn glow() { Self::dim(); }\n    pub fn dim() {}\n}\n"),
    ];
    let graph = graph_for(&files);
    let json: Vec<String> = graph
        .edges
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert_eq!(
        json,
        vec![
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":6,"to":"src/lib.rs#f","to_file":"src/lib.rs","member":"f"}"#,
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":7,"to":"src/m.rs#g","to_file":"src/m.rs","member":"g"}"#,
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":8,"to":"src/m.rs#g","to_file":"src/m.rs","member":"g"}"#,
            r#"{"kind":"uses-type","from_file":"src/lib.rs","from_line":9,"to":"src/lamp.rs#Lamp","to_file":"src/lamp.rs"}"#,
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":9,"to":"src/glow.rs#Lamp::glow","to_file":"src/glow.rs","member":"glow"}"#,
            r#"{"kind":"uses-type","from_file":"src/lib.rs","from_line":10,"to":"src/lamp.rs#Shine","to_file":"src/lamp.rs"}"#,
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":10,"to":"src/lamp.rs#Shine::beam","to_file":"src/lamp.rs","member":"beam"}"#,
            r#"{"kind":"uses-member","from_file":"src/lib.rs","from_line":11,"to":"src/m.rs#h","to_file":"src/m.rs","member":"h"}"#,
            r#"{"kind":"uses-member","from_file":"src/m.rs","from_line":3,"to":"src/lib.rs#f","to_file":"src/lib.rs","member":"f"}"#,
            r#"{"kind":"uses-type","from_file":"src/lamp.rs","from_line":3,"to":"src/lamp.rs#Lamp","to_file":"src/lamp.rs"}"#,
            r#"{"kind":"uses-type","from_file":"src/glow.rs","from_line":1,"to":"src/lamp.rs#Lamp","to_file":"src/lamp.rs"}"#,
            r#"{"kind":"uses-member","from_file":"src/glow.rs","from_line":2,"to":"src/glow.rs#Lamp::dim","to_file":"src/glow.rs","member":"dim"}"#,
        ]
    );
    assert_eq!(graph.stats.edges_by_kind.uses_member, 8);
    assert_eq!(graph.stats.edges_by_kind.uses_type, 4);
}

#[test]
fn no_edge_from_methods_shadows_generics_aliases_or_std() {
    let src = "pub fn glow() {}\npub struct Lamp;\nimpl Lamp { pub fn make() {} }\npub type Alias = Lamp;\nfn t<T: Default>(glow: u8, x: Lamp) {\n    x.glow();\n    glow;\n    let c = |glow: u8| glow;\n    T::default();\n    Alias::make();\n    std::mem::drop(1);\n    Vec::<u8>::new();\n}\nfn u() {\n    fn glow() {}\n    glow();\n}\n";
    let got = edges_for(&[("src/lib.rs", src)]);
    assert!(!got.iter().any(|e| e.starts_with("uses-member")), "{got:?}");
    assert!(
        got.contains(&"uses-type src/lib.rs:10 -> src/lib.rs#Alias".to_string()),
        "an alias qualifier gives uses-type and binds nothing after it: {got:?}"
    );
}

#[test]
fn type_positions_literals_constructors_and_impls_give_their_edges() {
    let src = "pub struct Lamp;\npub struct Wick(u8);\npub struct Shade { a: u8 }\npub trait Shine {}\npub enum Mode { On }\npub type Alias = Lamp;\npub struct Holder { f: Lamp }\nfn t(p: Lamp) -> Shade {\n    let a: Alias = Lamp;\n    let w = Wick(1);\n    let m = Mode::On;\n    let g: Vec<Shade> = Vec::new();\n    let s = Shade { a: 1 };\n    s\n}\nimpl Lamp {}\nimpl Shine for Lamp {}\nimpl std::fmt::Display for Lamp { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) } }\nfn b<T: Shine>(x: &dyn Shine) where T: Shine {}\n";
    let got = edges_for(&[("src/lib.rs", src)]);
    let want = [
        "uses-type src/lib.rs:6 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:7 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:8 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:8 -> src/lib.rs#Shade",
        "uses-type src/lib.rs:9 -> src/lib.rs#Alias",
        "uses-type src/lib.rs:9 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:10 -> src/lib.rs#Wick",
        "uses-type src/lib.rs:11 -> src/lib.rs#Mode",
        "uses-type src/lib.rs:12 -> src/lib.rs#Shade",
        "uses-type src/lib.rs:13 -> src/lib.rs#Shade",
        "uses-type src/lib.rs:16 -> src/lib.rs#Lamp",
        "implements src/lib.rs:17 -> src/lib.rs#Shine",
        "uses-type src/lib.rs:17 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:18 -> src/lib.rs#Lamp",
        "uses-type src/lib.rs:19 -> src/lib.rs#Shine",
        "uses-type src/lib.rs:19 -> src/lib.rs#Shine",
        "uses-type src/lib.rs:19 -> src/lib.rs#Shine",
    ];
    assert_eq!(got, want);
    let graph = graph_for(&[("src/lib.rs", src)]);
    let implements: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| matches!(e, Edge::Implements { .. }))
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert_eq!(
        implements,
        vec![
            r#"{"kind":"implements","from_file":"src/lib.rs","from_line":17,"to":"src/lib.rs#Shine","to_file":"src/lib.rs"}"#
        ]
    );
    assert_eq!(graph.stats.edges_by_kind.implements, 1);
}

#[test]
fn calls_in_macro_arguments_give_the_edges_the_same_call_would() {
    let src = "mod m { pub fn g(x: u8) -> u8 { x } }\npub struct Lamp;\nimpl Lamp { pub fn new() -> u8 { 1 } }\nfn f(x: u8) -> u8 { x }\nfn t() {\n    assert_eq!(f(1), m::g(2));\n    assert!(f(3) > 0);\n    let v = vec![f(m::g(4))];\n    assert!(matches!(f(5), 5));\n    let s = format!(\"{} f(9)\", Lamp::new());\n}\n";
    assert_eq!(
        edges_for(&[("src/lib.rs", src)]),
        vec![
            "uses-type src/lib.rs:3 -> src/lib.rs#Lamp",
            "uses-member src/lib.rs:6 -> src/lib.rs#f",
            "uses-member src/lib.rs:6 -> src/lib.rs#m::g",
            "uses-member src/lib.rs:7 -> src/lib.rs#f",
            "uses-member src/lib.rs:8 -> src/lib.rs#f",
            "uses-member src/lib.rs:8 -> src/lib.rs#m::g",
            "uses-member src/lib.rs:9 -> src/lib.rs#f",
            "uses-type src/lib.rs:10 -> src/lib.rs#Lamp",
            "uses-member src/lib.rs:10 -> src/lib.rs#Lamp::new",
        ]
    );
}

#[test]
fn rust_edges_follow_every_existing_edge_and_no_fragment_changes_nothing() {
    let root = std::env::temp_dir().join("scout-rsgraph-edges-order");
    let mut graph = crate::resolve::resolve_graph_with_model(&root, &[], &[], None, None);
    graph.edges.push(Edge::UsesType {
        from_file: "src/A.cs".into(),
        from_line: 1,
        to: "App.B".into(),
        to_file: "src/B.cs".into(),
        heuristic: false,
    });
    let before = serde_json::to_string(&graph).unwrap();
    crate::rsgraph::append_rust_edges(&mut graph, &[], None);
    assert_eq!(serde_json::to_string(&graph).unwrap(), before);
    let fragments = vec![(
        "src/lib.rs".to_string(),
        extract_rs_file("fn f() {}\nfn t() { f(); }\n").fragment,
    )];
    crate::rsgraph::append_rust(&mut graph, &fragments);
    crate::rsgraph::append_rust_edges(&mut graph, &fragments, None);
    let kinds: Vec<String> = graph.edges.iter().map(brief).collect();
    assert_eq!(kinds[1], "uses-member src/lib.rs:2 -> src/lib.rs#f");
    assert!(kinds[0].starts_with("UsesType") || kinds[0].starts_with("uses-type src/A.cs"));
}

#[test]
fn each_test_function_carries_its_own_name_and_a_helper_file_carries_none() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"p\"\n"),
        (
            "src/lib.rs",
            "pub fn glow() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn lit() { super::glow(); }\n}\n#[cfg(test)]\nmod more;\n",
        ),
        ("src/more.rs", "#[test]\nfn dim() {}\nfn helper() {}\n"),
        ("tests/it.rs", "mod common;\n#[test]\nfn from_outside() { common::setup(); p::glow(); }\n"),
        ("tests/common/mod.rs", "pub fn setup() {}\n"),
    ];
    let graph = graph_for(&files);
    let carriers: Vec<(&str, &[String])> = graph
        .defs
        .iter()
        .filter(|d| !d.test_methods.is_empty())
        .map(|d| (d.id.as_str(), d.test_methods.as_slice()))
        .collect();
    assert_eq!(
        carriers,
        vec![
            ("src/lib.rs#tests::lit", &["lit".to_string()][..]),
            ("src/more.rs#dim", &["dim".to_string()][..]),
            (
                "tests/it.rs#from_outside",
                &["from_outside".to_string()][..]
            ),
        ]
    );
    assert_eq!(graph.stats.test_def_count, 3);
    assert_eq!(
        serde_json::to_string(graph.defs.iter().find(|d| d.name == "dim").unwrap()).unwrap(),
        r#"{"id":"src/more.rs#dim","name":"dim","namespace":"src/more.rs","kind":"fn","file":"src/more.rs","line":1,"methods":[],"testMethods":["dim"],"endLine":2}"#
    );
}
