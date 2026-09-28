use super::*;

fn calls(files: &[(&str, &str)]) -> Vec<String> {
    edges_for(files)
        .into_iter()
        .filter(|e| e.starts_with("uses-member"))
        .collect()
}

#[test]
fn a_block_glob_of_std_or_an_enum_shadows_the_module_items() {
    let src = "pub fn max(a: i32, b: i32) -> i32 { a - b }
pub fn Foo(x: i32) -> i32 { x }
pub enum Opt { Foo(i32), Nothing }
pub fn block_std_glob() -> i32 { use std::cmp::*; max(1, 2) }
pub fn block_enum_glob_call() -> Opt { use Opt::*; Foo(1) }
pub fn block_enum_glob_macro(o: Opt) -> bool { use Opt::*; matches!(o, Foo(_)) }
pub mod inner {
    pub fn max(a: i32, b: i32) -> i32 { a * b }
    pub fn block_crate_glob() -> i32 { use std::cmp::*; max(3, 4) }
    pub fn plain() -> i32 { max(5, 6) }
}
pub fn nested() -> i32 { let a = { use std::cmp::*; max(5, 6) }; a + max(1, 1) }
pub fn if_block() -> i32 { if true { use std::cmp::*; return max(1, 2); } 0 }
pub fn plain() -> i32 { max(7, 8) + Foo(9) }
";
    assert_eq!(
        calls(&[("src/lib.rs", src)]),
        vec![
            "uses-member src/lib.rs:10 -> src/lib.rs#inner::max",
            "uses-member src/lib.rs:12 -> src/lib.rs#max",
            "uses-member src/lib.rs:14 -> src/lib.rs#max",
            "uses-member src/lib.rs:14 -> src/lib.rs#Foo",
        ]
    );
}

#[test]
fn a_block_glob_reaching_an_external_reexport_or_an_opaque_glob_shadows() {
    let src = "pub mod util { pub use std::cmp::max; pub fn other() -> i32 { 0 } }
pub mod enums { pub enum Kind { other(i32) } }
pub mod deep { pub use std::cmp::*; }
pub mod twice { pub use crate::util::max; }
pub fn max(a: i32, b: i32) -> i32 { a - b }
pub fn other() -> i32 { 9 }
pub fn via_reexport_glob() -> i32 { use crate::util::*; max(1, 2) }
pub fn enum_lower() -> enums::Kind { use enums::Kind::*; other(3) }
pub fn via_opaque_glob() -> i32 { use crate::deep::*; max(1, 2) }
pub fn explicit_reexport() -> i32 { use crate::util::max; max(1, 2) }
pub fn explicit_chain() -> i32 { use crate::twice::max; max(1, 2) }
pub fn found_through_glob() -> i32 { use crate::util::*; other() }
";
    assert_eq!(
        calls(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:12 -> src/lib.rs#util::other"]
    );
}

#[test]
fn a_module_glob_nothing_can_enumerate_shadows_the_library_name_but_not_an_import() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"lamp-kit\"\n"),
        (
            "src/lib.rs",
            "pub fn glow() {}\npub mod shelf { pub fn dust() {} }\n",
        ),
        (
            "tests/it.rs",
            "use other_crate::*;\nuse lamp_kit::shelf::*;\nuse lamp_kit::glow as shine;\nfn t() { lamp_kit::glow(); shine(); dust(); }\nfn u() { use std::fmt::*; use lamp_kit::glow; glow(); }\n",
        ),
    ];
    assert_eq!(
        calls(&files),
        vec![
            "uses-member tests/it.rs:4 -> src/lib.rs#glow",
            "uses-member tests/it.rs:5 -> src/lib.rs#glow",
        ]
    );
}

#[test]
fn a_block_glob_of_a_macro_extended_module_shadows_outer_functions() {
    let src = "fn max(a: i32, b: i32) -> i32 { a - b }\nmacro_rules! gen_max { () => { pub fn max(a: i32, b: i32) -> i32 { a + b } }; }\nmod util { gen_max!(); pub fn listed() -> i32 { 1 } }\nfn t() -> i32 { use crate::util::*; listed() + max(1, 2) }\nfn u() -> i32 { use crate::util::*; max(3, 4) }\nfn clear() -> i32 { max(5, 6) }\n";
    assert_eq!(
        calls(&[("src/lib.rs", src)]),
        vec![
            "uses-member src/lib.rs:4 -> src/lib.rs#util::listed",
            "uses-member src/lib.rs:6 -> src/lib.rs#max",
        ]
    );
}

#[test]
fn macro_extended_types_and_glob_chains_shadow_outer_types() {
    let src = "pub struct Lamp; impl Lamp { pub fn on() {} }\nmacro_rules! gen_lamp { () => { pub struct Lamp; impl Lamp { pub fn on() {} } }; }\nmod util { gen_lamp!(); }\nmod chain { pub use crate::util::*; }\nfn t() { use crate::chain::*; let _l: Lamp = Lamp; Lamp::on(); }\nfn clear() { let _l: Lamp = Lamp; Lamp::on(); }\n";
    let got = edges_for(&[("src/lib.rs", src)]);
    assert!(!got.iter().any(|edge| edge.contains("src/lib.rs:5 ->")));
    assert!(got
        .iter()
        .any(|edge| edge == "uses-member src/lib.rs:6 -> src/lib.rs#Lamp::on"));
}

#[test]
fn include_makes_a_module_glob_opaque_at_block_and_module_scope() {
    let src = "fn max(a: i32, b: i32) -> i32 { a - b }\nmod util { include!(\"items.rs\"); }\nmod from_module { use crate::util::*; fn t() { max(1, 2); } }\nfn from_block() { use crate::util::*; max(3, 4); }\nfn clear() { max(5, 6); }\n";
    assert_eq!(
        calls(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:5 -> src/lib.rs#max"]
    );
}

#[test]
fn an_unlisted_glob_can_conflict_with_a_name_another_glob_lists() {
    let src = "mod known { pub fn max() {} }\nmod unknown { include!(\"items.rs\"); }\nfn t() { use crate::known::*; use crate::unknown::*; max(); }\nfn clear() { use crate::known::*; max(); }\n";
    assert_eq!(
        calls(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:4 -> src/lib.rs#known::max"]
    );
}

#[test]
fn macro_extended_module_items_precede_its_fallback_glob_on_every_route() {
    let src = "pub mod fallback { pub fn max() -> i32 { 1 } pub struct Lamp; impl Lamp { pub fn on() -> i32 { 1 } } }\nmacro_rules! generate { () => { pub fn max() -> i32 { 2 } pub struct Lamp; impl Lamp { pub fn on() -> i32 { 2 } } }; }\npub mod generated { pub use crate::fallback::*; generate!(); pub fn inside() -> i32 { max() + Lamp::on() } pub fn known() -> i32 { 7 } }\npub fn qualified() -> i32 { generated::max() + generated::Lamp::on() }\npub fn imported() -> i32 { use crate::generated::{max as chosen, Lamp as Chosen}; chosen() + Chosen::on() }\npub fn globbed() -> i32 { use crate::generated::*; max() + Lamp::on() }\npub fn listed() -> i32 { fallback::max() + fallback::Lamp::on() + generated::known() }\npub mod chain { pub use crate::generated::{max as pick, Lamp as Light, known}; }\npub fn through_chain() -> i32 { chain::pick() + chain::Light::on() + chain::known() }\n#[test] fn runtime() { assert_eq!(generated::inside(), 4); assert_eq!(qualified(), 4); assert_eq!(imported(), 4); assert_eq!(globbed(), 4); assert_eq!(listed(), 9); assert_eq!(through_chain(), 11); }\n";
    let got = edges_for(&[("src/lib.rs", src)]);
    for line in [3, 4, 5, 6, 9] {
        assert!(
            !got.iter()
                .any(|edge| edge.contains(&format!("src/lib.rs:{line} -> src/lib.rs#fallback::"))),
            "line {line}: {got:?}"
        );
    }
    assert!(got
        .iter()
        .any(|edge| edge == "uses-member src/lib.rs:7 -> src/lib.rs#fallback::max"));
    assert!(got
        .iter()
        .any(|edge| edge == "uses-type src/lib.rs:7 -> src/lib.rs#fallback::Lamp"));
    assert!(got
        .iter()
        .any(|edge| edge == "uses-member src/lib.rs:7 -> src/lib.rs#fallback::Lamp::on"));
    assert!(got
        .iter()
        .any(|edge| edge == "uses-member src/lib.rs:7 -> src/lib.rs#generated::known"));
    assert!(got
        .iter()
        .any(|edge| edge == "uses-member src/lib.rs:9 -> src/lib.rs#generated::known"));
}
