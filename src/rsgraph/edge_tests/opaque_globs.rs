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
            "uses-member tests/it.rs:4 -> src/lib.rs#shelf::dust",
            "uses-member tests/it.rs:5 -> src/lib.rs#glow",
        ]
    );
}
