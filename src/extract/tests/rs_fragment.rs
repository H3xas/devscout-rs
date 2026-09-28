use super::*;

fn rows(src: &str) -> Vec<(String, String, usize, usize, usize)> {
    extract_rs_file(src)
        .fragment
        .items
        .into_iter()
        .map(|i| (i.kind, i.path, i.line, i.name_line, i.end_line))
        .collect()
}

fn paths(src: &str) -> Vec<String> {
    extract_rs_file(src)
        .fragment
        .items
        .into_iter()
        .map(|i| i.path)
        .collect()
}

#[test]
fn carried_file_context_preserves_nested_paths_marks_spans_and_impl_owner() {
    let src = "/// Root item.\npub struct Root;\npub mod m {\n    pub struct Nested;\n    impl Nested { pub fn act(&self) {} }\n    pub trait Job { fn perform(&self); }\n    extern \"C\" { fn ffi() -> i32; }\n}\n";
    let extracted = extract_rs_file(src);
    assert_eq!(extracted.purpose.as_deref(), Some("struct Root | mod m"));
    assert_eq!(
        extracted
            .fragment
            .items
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        [
            "Root",
            "m",
            "m::Nested",
            "m::Nested::act",
            "m::Job",
            "m::Job::perform",
            "m::ffi"
        ]
    );
    assert_eq!(extracted.fragment.items[0].line, 1);
    assert_eq!(extracted.fragment.items[0].name_line, 2);
    assert_eq!(extracted.fragment.items[1].end_line, 8);
    assert_eq!(extracted.fragment.ctors, ["Root", "m::Nested"]);
    assert_eq!(extracted.fragment.impls.len(), 1);
    assert_eq!(extracted.fragment.impls[0].scope, "m");
    assert_eq!(extracted.fragment.impls[0].self_type, "Nested");
    assert_eq!(extracted.fragment.impls[0].items, ["m::Nested::act"]);
}

fn row(
    kind: &str,
    path: &str,
    line: usize,
    name_line: usize,
    end_line: usize,
) -> (String, String, usize, usize, usize) {
    (
        kind.to_string(),
        path.to_string(),
        line,
        name_line,
        end_line,
    )
}

#[test]
fn every_kind_in_every_walked_context() {
    let src = "\
mod drawers;
pub struct Widget { a: u32 }
enum Tone { Red, Blue }
union Bits { a: u32, b: f32 }
trait Render {
    fn draw(&self);
    fn outline(&self) {}
    type Canvas;
    const LAYERS: u32;
}
type Shade = u8;
fn polish() {}
const DEPTH: u32 = 3;
static LABEL: &str = \"shelf\";
macro_rules! stack_of { () => {}; }
pub mod cabinet {
    mod fittings;
    struct Hinge;
    enum Finish { Oak }
    union Latch { a: u8 }
    trait Swing { fn swing(&self); }
    type Screw = u16;
    fn assemble() {}
    const WIDTH: u32 = 1;
    static NAME: &str = \"c\";
    macro_rules! twice { () => {}; }
    impl Hinge {
        const TURNS: u32 = 2;
        type Pin = u8;
        fn oil(&self) {}
    }
}
impl Widget {
    fn new() -> Self { Widget { a: 0 } }
}
extern \"C\" {
    fn shelf_count() -> i32;
    static SHELF_ERRNO: i32;
    type ShelfHandle;
}
";
    assert_eq!(
        rows(src),
        vec![
            row("mod", "drawers", 1, 1, 1),
            row("struct", "Widget", 2, 2, 2),
            row("enum", "Tone", 3, 3, 3),
            row("union", "Bits", 4, 4, 4),
            row("trait", "Render", 5, 5, 10),
            row("fn", "Render::draw", 6, 6, 6),
            row("fn", "Render::outline", 7, 7, 7),
            row("type", "Render::Canvas", 8, 8, 8),
            row("const", "Render::LAYERS", 9, 9, 9),
            row("type", "Shade", 11, 11, 11),
            row("fn", "polish", 12, 12, 12),
            row("const", "DEPTH", 13, 13, 13),
            row("static", "LABEL", 14, 14, 14),
            row("macro_rules", "stack_of", 15, 15, 15),
            row("mod", "cabinet", 16, 16, 32),
            row("mod", "cabinet::fittings", 17, 17, 17),
            row("struct", "cabinet::Hinge", 18, 18, 18),
            row("enum", "cabinet::Finish", 19, 19, 19),
            row("union", "cabinet::Latch", 20, 20, 20),
            row("trait", "cabinet::Swing", 21, 21, 21),
            row("fn", "cabinet::Swing::swing", 21, 21, 21),
            row("type", "cabinet::Screw", 22, 22, 22),
            row("fn", "cabinet::assemble", 23, 23, 23),
            row("const", "cabinet::WIDTH", 24, 24, 24),
            row("static", "cabinet::NAME", 25, 25, 25),
            row("macro_rules", "cabinet::twice", 26, 26, 26),
            row("const", "cabinet::Hinge::TURNS", 28, 28, 28),
            row("type", "cabinet::Hinge::Pin", 29, 29, 29),
            row("fn", "cabinet::Hinge::oil", 30, 30, 30),
            row("fn", "Widget::new", 34, 34, 34),
            row("fn", "shelf_count", 37, 37, 37),
            row("static", "SHELF_ERRNO", 38, 38, 38),
            row("type", "ShelfHandle", 39, 39, 39),
        ]
    );
}

#[test]
fn names_are_recorded_beside_their_paths() {
    let items = extract_rs_file("mod m { impl W { fn go() {} } }\n")
        .fragment
        .items;
    let names: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i.name.as_str(), i.path.as_str()))
        .collect();
    assert_eq!(names, vec![("m", "m"), ("go", "m::W::go")]);
}

#[test]
fn span_start_takes_outer_docs_and_attributes_and_skips_a_plain_comment_between() {
    let src = "\
fn first() {}
/// Line doc.
// a plain note
#[derive(Debug)]
struct Documented;
/** Block doc. */
#[inline]
fn blocked() {}
";
    assert_eq!(
        rows(src),
        vec![
            row("fn", "first", 1, 1, 1),
            row("struct", "Documented", 2, 5, 5),
            row("fn", "blocked", 6, 8, 8),
        ]
    );
}

#[test]
fn span_start_stops_at_a_plain_comment_above_an_inner_attribute_an_inner_doc_and_the_previous_item()
{
    let src = "\
#![allow(dead_code)]
/// After an inner attribute.
fn a() {}
// plain, above the docs
/// Doc.
fn b() {}
// plain alone
fn c() {}
";
    assert_eq!(
        rows(src),
        vec![
            row("fn", "a", 2, 3, 3),
            row("fn", "b", 5, 6, 6),
            row("fn", "c", 8, 8, 8),
        ]
    );
    let after_inner_doc = "//! Module doc.\nfn d() {}\n";
    assert_eq!(rows(after_inner_doc), vec![row("fn", "d", 2, 2, 2)]);
    let in_a_body = "mod m {\n    //! Inner.\n    #[cfg(test)]\n    fn e() {}\n}\n";
    assert_eq!(
        rows(in_a_body),
        vec![row("mod", "m", 1, 1, 5), row("fn", "m::e", 3, 4, 4)]
    );
}

#[test]
fn nothing_is_recorded_outside_walked_contexts_or_for_unlisted_constructs() {
    let src = "\
use std::collections::BTreeMap as ShelfMap;
extern crate core;
fn outer() {
    fn tucked_helper() {}
    struct Local;
}
const PLANK: u32 = {
    fn hidden_in_const() -> u32 { 3 }
    hidden_in_const()
};
macro_rules! stack_of {
    () => { fn made_by_macro() {} };
}
thread_local! { static TUCKED_COUNTER: u32 = 1; }
enum Tone { Crimson }
struct Widget { spine_width: u32 }
const _: () = ();
impl Widget {}
";
    assert_eq!(
        paths(src),
        vec!["outer", "PLANK", "stack_of", "Tone", "Widget"]
    );
}

#[test]
fn items_outside_a_syntax_error_survive() {
    let src = "\
fn before() {}

struct Broken {
    a: ,
}

pub fn = 3;

fn after() {}
";
    assert_eq!(
        rows(src),
        vec![
            row("fn", "before", 1, 1, 1),
            row("struct", "Broken", 3, 3, 5),
            row("fn", "after", 9, 9, 9),
        ]
    );
}

#[test]
fn a_non_bmp_letter_in_a_doc_comment_keeps_every_line() {
    let src = "/// The \u{1D538} shelf.\nstruct Wide;\n\n/// Next.\nfn after() {}\n";
    assert_eq!(
        rows(src),
        vec![row("struct", "Wide", 1, 2, 2), row("fn", "after", 4, 5, 5)]
    );
}

#[test]
fn an_empty_source_gives_an_empty_fragment() {
    let out = extract_rs_file("");
    assert_eq!(out.fragment, RsFragment::default());
    assert_eq!(out.fragment.rs, 1);
    assert_eq!(out.purpose, None);
}

#[test]
fn an_item_ending_at_a_line_break_does_not_claim_the_next_line() {
    let src = "macro_rules! m {\n    () => {};\n}\nfn f() {}\n";
    assert_eq!(
        rows(src),
        vec![row("macro_rules", "m", 1, 1, 3), row("fn", "f", 4, 4, 4)]
    );
}

#[test]
fn paths_join_module_owner_and_trait_segments() {
    let src = "\
mod outer {
    pub mod inner {
        pub struct Gear;
        impl Gear { pub fn turn(&self) {} }
        impl Spin for Gear { fn spin(&self) {} }
        pub trait Spin { fn spin(&self); }
    }
}
extern \"C\" { fn raw_count() -> i32; }
";
    assert_eq!(
        paths(src),
        vec![
            "outer",
            "outer::inner",
            "outer::inner::Gear",
            "outer::inner::Gear::turn",
            "outer::inner::Gear::spin",
            "outer::inner::Spin",
            "outer::inner::Spin::spin",
            "raw_count",
        ]
    );
}

#[test]
fn self_type_heads() {
    let src = "\
impl<T> Widget<T> { fn a() {} }
impl crate::shop::Widget { fn b() {} }
impl<'a> Show for &'a mut Widget { fn c() {} }
impl *const Widget { fn d() {} }
impl dyn Render { fn e() {} }
impl u32 { fn f() {} }
impl<T> Show for [T] { fn g() {} }
impl<A, B> Show for (A, B) { fn h() {} }
impl Show for fn() { fn i() {} }
impl dyn Render + Send { fn j() {} }
impl <T as Tr>::Assoc { fn k() {} }
impl r#Type { fn l() {} }
impl Show for () { fn m() {} }
";
    assert_eq!(
        paths(src),
        vec![
            "Widget::a",
            "Widget::b",
            "Widget::c",
            "Widget::d",
            "Render::e",
            "u32::f",
            "_::g",
            "_::h",
            "_::i",
            "_::j",
            "Assoc::k",
            "Type::l",
            "_::m",
        ]
    );
}

#[test]
fn repeated_paths_take_suffixes_in_source_order() {
    let src = "\
struct Gadget;
impl fmt::Display for Gadget { fn fmt(&self) {} }
impl fmt::Debug for Gadget { fn fmt(&self) {} }
#[cfg(unix)]
fn home_shelf() {}
#[cfg(not(unix))]
fn home_shelf() {}
#[cfg(unix)]
mod plat { fn lookup_path() {} }
#[cfg(not(unix))]
mod plat { fn lookup_path() {} }
impl From<u8> for Gadget { fn from(_: u8) -> Self { Gadget } }
impl From<u16> for Gadget { fn from(_: u16) -> Self { Gadget } }
impl From<u32> for Gadget { fn from(_: u32) -> Self { Gadget } }
";
    assert_eq!(
        paths(src),
        vec![
            "Gadget",
            "Gadget::fmt",
            "Gadget::fmt-1",
            "home_shelf",
            "home_shelf-1",
            "plat",
            "plat::lookup_path",
            "plat-1",
            "plat::lookup_path-1",
            "Gadget::from",
            "Gadget::from-1",
            "Gadget::from-2",
        ]
    );
    let names: Vec<String> = extract_rs_file(src)
        .fragment
        .items
        .into_iter()
        .map(|i| i.name)
        .collect();
    assert_eq!(names[2], "fmt", "the suffix is on the path, never the name");
}

#[test]
fn raw_identifiers_drop_the_prefix() {
    let src = "fn r#match() {}\nmod r#type { struct r#Box; }\n";
    let items = extract_rs_file(src).fragment.items;
    let got: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i.name.as_str(), i.path.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![("match", "match"), ("type", "type"), ("Box", "type::Box")]
    );
}

#[test]
fn metavariable_and_underscore_names_are_not_recorded() {
    let src = "\
macro_rules! make { ($n:ident) => { fn $n() {} }; }
const _: u8 = 0;
static _: u8 = 0;
trait T { const _: u8; }
fn kept() {}
";
    assert_eq!(paths(src), vec!["make", "T", "kept"]);
}

#[test]
fn id_paths_are_shell_safe() {
    let hostile = "\
impl<'a, T: Clone + ?Sized> Show for &'a [T] { fn show(&self) {} }
impl<T> Show for Box<dyn Fn(&T) -> Option<T> + Send> { fn show(&self) {} }
impl Show for [u8; 4] { fn show(&self) {} }
impl Show for *mut (u8, u16) { fn show(&self) {} }
impl Show for ! { fn show(&self) {} }
impl<T> Show for <T as Iterator>::Item { fn show(&self) {} }
impl Show for r#Weird<'static, \"s\"> { fn show(&self) {} }
mod r#async { impl super::Show for u8 { fn show(&self) {} } }
";
    let forbidden = [
        '<', '>', '[', ']', '(', ')', '{', '}', '\'', '"', '$', '*', '?', '!', ';', '&', '|',
    ];
    let items = extract_rs_file(hostile).fragment.items;
    assert!(!items.is_empty());
    for item in items {
        assert!(
            !item
                .path
                .chars()
                .any(|c| c.is_whitespace() || forbidden.contains(&c)),
            "unsafe path {:?}",
            item.path
        );
    }
}

#[test]
fn deep_nesting_does_not_exhaust_a_worker_stack() {
    let depth = 2_000;
    let mut src = "mod m {\n".repeat(depth);
    src.push_str("impl &&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&Gear { fn turn() {} }\n");
    src.push_str(&"}\n".repeat(depth));
    let items = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || extract_rs_file(&src).fragment.items)
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    assert_eq!(items.len(), depth + 1);
    let last = items.last().unwrap();
    assert_eq!(last.name, "turn");
    assert!(last.path.ends_with("m::m::Gear::turn"), "{}", last.path);
    assert_eq!(last.path.matches("m::").count(), depth);
}
