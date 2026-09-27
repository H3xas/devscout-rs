use super::*;

fn hits(src: &str) -> Vec<(String, usize)> {
    extract_rs_file(src)
        .fragment
        .sites
        .into_iter()
        .filter(|s| s.mac)
        .inspect(|s| assert_eq!(s.role, RsRole::Call))
        .map(|s| (s.path, s.line))
        .collect()
}

fn h(path: &str, line: usize) -> (String, usize) {
    (path.into(), line)
}

#[test]
fn calls_in_macro_arguments_are_call_sites() {
    let src = "fn t() {\n    assert_eq!(a(1), 2);\n    assert!(b());\n    let v = vec![c(d(1))];\n    assert!(matches!(e(), Some(_)));\n    let s = format!(\"{} {}\", f(), m::g::<u8>(1));\n    println!(\"{}\", crate::k::h(2));\n}\n";
    assert_eq!(
        hits(src),
        vec![
            h("a", 2),
            h("b", 3),
            h("c", 4),
            h("d", 4),
            h("e", 5),
            h("Some", 5),
            h("f", 6),
            h("m::g", 6),
            h("crate::k::h", 7),
        ]
    );
}

#[test]
fn a_contextual_keyword_is_an_ordinary_path_segment() {
    let src = "fn t() {\n    assert!(Shelf::default().is_empty());\n    x!(union::raw(1));\n}\n";
    assert_eq!(hits(src), vec![h("Shelf::default", 2), h("union::raw", 3)]);
}

#[test]
fn a_turbofish_inside_or_closing_a_path_is_passed_over() {
    let src = "fn t() {\n    x!(Vec::<Lamp>::new(), p::q::<Vec<u8>>(1), r::<u8, u16>(2));\n}\n";
    assert_eq!(hits(src), vec![h("Vec::new", 2), h("p::q", 2), h("r", 2)]);
}

#[test]
fn methods_fragments_strings_definitions_and_macro_names_are_no_calls() {
    let src = "macro_rules! m { ($f:ident) => { $f(1); inner(2) } }\nfn t(x: u8) {\n    assert!(x.len() > 0, \"{}\", \"f(1)\");\n    y!(fn z(a: u8) {});\n    outer!(nested!(w(3)));\n}\n#[cfg_attr(test, derive(Debug))]\nstruct S;\n";
    assert_eq!(hits(src), vec![h("w", 5)]);
}

#[test]
fn macro_hits_are_shadowed_like_other_calls() {
    let src = "fn t(p: u8) {\n    let c = |q| q;\n    fn nested() {}\n    assert!(c(p(1)) + nested());\n    assert!(self::c(2));\n}\n";
    assert_eq!(hits(src), vec![h("self::c", 5)]);
}

#[test]
fn hits_keep_source_order_across_nested_trees() {
    let src = "fn t() {\n    assert_eq!(f(g(1)), h(2));\n}\n";
    assert_eq!(hits(src), vec![h("f", 2), h("g", 2), h("h", 2)]);
}

#[test]
fn a_deep_token_tree_is_scanned_without_exhausting_a_worker_stack() {
    let depth = 5000;
    let src = format!(
        "fn t() {{ m!({}f(1){}); }}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            extract_rs_file(&src)
                .fragment
                .sites
                .iter()
                .filter(|s| s.mac)
                .count()
        })
        .unwrap();
    assert_eq!(handle.join().unwrap(), 1);
}

#[test]
fn a_name_the_tree_may_bind_shadows_its_calls_but_a_type_written_in_it_does_not() {
    let src = "fn t() {\n    assert!(0 < v.iter().filter(|f| f(1) > 0).count());\n    x!(Vec::<Lamp>::new(), a as Lamp, Lamp::new());\n}\nfn u() {\n    Lamp::new();\n    y!(Lamp; Lamp::new());\n}\n";
    assert_eq!(hits(src), vec![h("Vec::new", 3), h("Lamp::new", 3)]);
    let calls: Vec<String> = extract_rs_file(src)
        .fragment
        .sites
        .into_iter()
        .filter(|s| s.line > 4)
        .map(|s| s.path)
        .collect();
    assert!(
        calls.is_empty(),
        "an item a statement macro may declare shadows its whole block: {calls:?}"
    );
}
