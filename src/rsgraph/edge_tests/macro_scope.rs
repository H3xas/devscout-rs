use super::*;

fn calls(src: &str) -> Vec<String> {
    edges_for(&[("src/lib.rs", src)])
        .into_iter()
        .filter(|e| e.starts_with("uses-member"))
        .collect()
}

#[test]
fn a_binding_inside_a_macro_argument_shadows_the_calls_beside_it() {
    let src = "pub fn f(x: i32) -> i32 { x }
pub fn g(x: i32) -> i32 { x }
pub fn closure_param(v: Vec<fn(i32) -> i32>) -> bool { assert!(v.iter().all(|f| f(1) > 0)); true }
pub fn let_in_block() { assert_eq!({ let g = |x: i32| x; g(1) }, 1); }
pub fn match_arm(o: Option<fn(i32) -> i32>) { assert!(match o { Some(f) => f(1), None => 0 } >= 0); }
pub fn if_let(o: Option<fn(i32) -> i32>) -> Vec<i32> { vec![if let Some(g) = o { g(2) } else { 0 }] }
pub fn guard(o: Option<fn(i32) -> i32>) -> bool { matches!(o, Some(f) if f(1) > 0) }
pub fn for_loop(v: Vec<fn(i32) -> i32>) { assert!({ let mut s = 0; for g in v { s += g(1); } s } > 0); }
pub fn while_let(mut v: Vec<fn(i32) -> i32>) { assert!({ while let Some(f) = v.pop() { f(1); } true }); }
pub fn tuple_param(v: Vec<(fn(i32) -> i32, u8)>) { assert!(v.iter().all(|(g, _)| g(1) > 0)); }
pub struct W { pub f: fn(i32) -> i32 }
pub fn shorthand(w: W) { assert!(match w { W { f } => f(1) } > 0); }
macro_rules! make { ($($t:tt)*) => { $($t)* }; }
make! { pub fn made() -> i32 { f(1) } }
make! { pub fn made_shadow(f: fn(i32) -> i32) -> i32 { f(2) } }
pub fn plain() { assert!(f(1) > g(2)); }
";
    assert_eq!(
        calls(src),
        vec![
            "uses-member src/lib.rs:14 -> src/lib.rs#f",
            "uses-member src/lib.rs:16 -> src/lib.rs#f",
            "uses-member src/lib.rs:16 -> src/lib.rs#g",
        ]
    );
}

#[test]
fn a_statement_macro_that_binds_a_name_shadows_the_calls_after_it() {
    let src = "pub fn f(x: i32) -> i32 { x }
macro_rules! bind { ($n:ident) => { let $n = |x: i32| x + 1; }; }
macro_rules! make { ($($t:tt)*) => { $($t)* }; }
pub fn t() -> i32 {
    bind!(f);
    f(1)
}
pub fn u() -> i32 {
    make! { fn f(x: i32) -> i32 { x } }
    f(2)
}
pub fn v() -> i32 { make!(let f = |x: i32| x;); f(3) }
pub fn w() -> i32 { let n = vec![f]; f(4) + n.len() as i32 }
pub fn x(v: Vec<fn(i32) -> i32>) -> i32 { assert!(v.iter().all(|f| f(1) > 0)); f(5) }
";
    assert_eq!(
        calls(src),
        vec![
            "uses-member src/lib.rs:13 -> src/lib.rs#f",
            "uses-member src/lib.rs:14 -> src/lib.rs#f",
        ]
    );
}

#[test]
fn generics_items_imports_and_impls_declared_in_a_token_tree_shadow_it() {
    let src = "pub struct Lamp;
impl Lamp { pub fn default() -> Lamp { Lamp } }
pub fn max(a: i32, b: i32) -> i32 { a + b }
pub fn f() {}
macro_rules! make { ($($t:tt)*) => { $($t)* }; }
make! { pub fn build<Lamp: Default>() -> Lamp { Lamp::default() } }
make! { pub fn user() -> i32 { use std::cmp::max; max(1, 2) } }
pub fn outer() { make! { fn f() {} } f(); }
pub struct Other;
impl Other { pub fn b() {} pub fn x() { make! { impl Lamp { fn c() { Self::b(); } } } } }
pub fn inner_items() { make! { fn f() {} fn g() { f(); } } }
pub fn ok() { assert!(max(1, 2) > 0); make! { fn h() { f(); } } }
";
    assert_eq!(
        calls(src),
        vec![
            "uses-member src/lib.rs:12 -> src/lib.rs#max",
            "uses-member src/lib.rs:12 -> src/lib.rs#f",
        ]
    );
}

#[test]
fn an_item_a_module_level_macro_declares_shadows_the_library_name() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"lamp-kit\"\n"),
        ("src/lib.rs", "pub fn glow() {}\n"),
        (
            "tests/it.rs",
            "macro_rules! make { ($($t:tt)*) => { $($t)* }; }\nmake! { mod lamp_kit { pub fn glow() {} } }\nfn t() { lamp_kit::glow(); }\n",
        ),
        ("tests/other.rs", "fn t() { lamp_kit::glow(); }\n"),
    ];
    let got: Vec<String> = edges_for(&files)
        .into_iter()
        .filter(|e| e.starts_with("uses-member"))
        .collect();
    assert_eq!(got, vec!["uses-member tests/other.rs:1 -> src/lib.rs#glow"]);
}
