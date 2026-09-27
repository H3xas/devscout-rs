use super::*;

fn sites(src: &str) -> Vec<RsSite> {
    extract_rs_file(src).fragment.sites
}

fn brief(src: &str) -> Vec<(RsRole, String, usize)> {
    sites(src)
        .into_iter()
        .map(|s| (s.role, s.path, s.line))
        .collect()
}

fn s(role: RsRole, path: &str, line: usize) -> (RsRole, String, usize) {
    (role, path.into(), line)
}

use RsRole::{Call, Impl, Lit, Pat, Type, Value};

#[test]
fn callee_forms_are_call_sites() {
    let src = "fn a() {\n    f();\n    m::f();\n    super::f();\n    crate::m::f();\n    Lamp::new();\n    g::<u8>();\n    ::ext::h();\n}\n";
    assert_eq!(
        brief(src),
        vec![
            s(Call, "f", 2),
            s(Call, "m::f", 3),
            s(Call, "super::f", 4),
            s(Call, "crate::m::f", 5),
            s(Call, "Lamp::new", 6),
            s(Call, "g", 7),
            s(Call, "::ext::h", 8),
        ]
    );
}

#[test]
fn self_rooted_paths_carry_their_impl_or_trait() {
    let src = "struct Lamp;\nimpl Lamp {\n    fn a() { Self::b(); Self {}; }\n    fn b() {}\n}\ntrait Shine {\n    fn c() { Self::d(); }\n    fn d();\n}\nfn e() { Self::f(); }\n";
    let got: Vec<(String, Option<usize>, Option<String>)> = sites(src)
        .into_iter()
        .map(|s| (s.path, s.self_impl, s.self_trait))
        .collect();
    assert_eq!(
        got,
        vec![
            ("Lamp".into(), None, None),
            ("Self::b".into(), Some(0), None),
            ("Self::d".into(), None, Some("Shine".into())),
        ],
        "`Self` alone and a `Self` path outside any impl or trait give no site"
    );
}

#[test]
fn type_positions_literals_and_impls_have_their_roles() {
    let src = "struct W { f: Field }\nstruct T(Tup);\ntype A = Aliased;\nfn g<P: Bound>(x: Param) -> Ret where P: Clause {\n    let y: Ann = x as Cast;\n    let v: Vec<Arg> = Vec::new();\n    let d: &dyn Obj = &y;\n    Lit { a: 1 };\n}\nimpl Tr for W {}\nimpl W {}\n";
    assert_eq!(
        brief(src),
        vec![
            s(Type, "Field", 1),
            s(Type, "Tup", 2),
            s(Type, "Aliased", 3),
            s(Type, "Bound", 4),
            s(Type, "Param", 4),
            s(Type, "Ret", 4),
            s(Type, "Clause", 4),
            s(Type, "Ann", 5),
            s(Type, "Cast", 5),
            s(Type, "Vec", 6),
            s(Type, "Arg", 6),
            s(Call, "Vec::new", 6),
            s(Type, "Obj", 7),
            s(Lit, "Lit", 8),
            s(Impl, "Tr", 10),
            s(Type, "W", 10),
            s(Type, "W", 11),
        ]
    );
}

#[test]
fn values_and_pattern_qualifiers() {
    let src = "fn g(e: u8) {\n    let h = Unit;\n    let k = m::CONST;\n    match e { E::V(x) => {}, E::W { y, .. } => {}, S => {} }\n    let z = Tuple(1);\n}\n";
    assert_eq!(
        brief(src),
        vec![
            s(Value, "Unit", 2),
            s(Value, "m::CONST", 3),
            s(Pat, "E::V", 4),
            s(Pat, "E::W", 4),
            s(Call, "Tuple", 5),
        ],
        "a parameter, a binding and an uppercase binding-shaped pattern give no site"
    );
}

#[test]
fn locals_closures_nested_fns_and_generics_shadow() {
    let src = "fn f() {}\nfn g<T: Tr>(p: u8) {\n    let x = 0;\n    let c = |q| q + 1;\n    c(1);\n    p(2);\n    if let Some(w) = None { w(3); }\n    for z in it { z(4); }\n    match 1 { v => v(5) }\n    fn f() {}\n    f();\n    T::make();\n    self::f();\n    x.f();\n}\n";
    let got: Vec<String> = sites(src).into_iter().map(|s| s.path).collect();
    assert_eq!(
        got,
        vec!["Tr", "Some", "None", "it", "self::f"],
        "a local, a closure, a nested fn, a generic and a method name never give a site"
    );
}

#[test]
fn a_path_split_over_lines_records_each_segment_line() {
    let src = "fn a() {\n    crate::m\n        ::f();\n}\n";
    let site = &sites(src)[0];
    assert_eq!((site.line, site.lines.clone()), (3, vec![2, 2, 3]));
}

#[test]
fn scope_and_block_are_recorded() {
    let src = "mod m {\n    fn a() {\n        use x::y;\n        y();\n        { z(); }\n    }\n}\nfn b() { w(); }\n";
    let got: Vec<(String, String, Option<usize>)> = sites(src)
        .into_iter()
        .map(|s| (s.path, s.scope, s.block))
        .collect();
    assert_eq!(
        got,
        vec![
            ("y".into(), "m".into(), Some(0)),
            ("z".into(), "m".into(), Some(0)),
            ("w".into(), "".into(), None),
        ]
    );
}

#[test]
fn nothing_is_read_from_skipped_regions() {
    let src = "use a::b;\n#[derive(Clone)]\n#[cfg(feature = \"f\")]\nstruct S;\nmacro_rules! m { () => { f() } }\nfn g() { let s = \"f()\"; x.method(); x.field; }\nfn broken( { h(); }\n";
    let got: Vec<String> = sites(src).into_iter().map(|s| s.path).collect();
    assert!(
        !got.iter()
            .any(|p| matches!(p.as_str(), "a::b" | "Clone" | "f" | "method" | "field")),
        "{got:?}"
    );
}

#[test]
fn qualified_paths_bind_nothing_but_their_types_are_type_sites() {
    let src = "fn a() { <Lamp as Shine>::glow(); }\n";
    assert_eq!(brief(src), vec![s(Type, "Lamp", 1), s(Type, "Shine", 1)]);
}

#[test]
fn deep_nesting_is_walked_without_exhausting_a_worker_stack() {
    let depth = 3000;
    let src = format!(
        "fn a() {{ {}f(); {} }}\n",
        "{ let c = || ".repeat(depth),
        "; }".repeat(depth)
    );
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || extract_rs_file(&src).fragment.sites.len())
        .unwrap();
    assert_eq!(handle.join().unwrap(), 1);
}
