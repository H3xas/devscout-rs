use super::*;

fn lib_with(body: &str) -> Vec<String> {
    edges_for(&[
        ("src/lib.rs", body),
        (
            "src/shelf.rs",
            "pub fn dust() {}\npub fn wipe() {}\npub mod deep { pub fn sweep() {} }\n",
        ),
    ])
}

#[test]
fn groups_nested_groups_and_self_in_a_group_bind() {
    let got = lib_with("mod shelf;\nuse shelf::{dust, deep::{sweep}, self as s};\nfn t() { dust(); sweep(); s::wipe(); }\n");
    assert_eq!(
        got,
        vec![
            "uses-member src/lib.rs:3 -> src/shelf.rs#dust",
            "uses-member src/lib.rs:3 -> src/shelf.rs#deep::sweep",
            "uses-member src/lib.rs:3 -> src/shelf.rs#wipe",
        ]
    );
    let none = lib_with("mod shelf;\nuse shelf::{dust};\nfn t() { wipe(); }\n");
    assert!(
        none.is_empty(),
        "a name the group does not import binds nothing: {none:?}"
    );
}

#[test]
fn a_glob_brings_in_every_name_and_a_private_item() {
    let got = lib_with("mod shelf;\nuse shelf::*;\nfn t() { dust(); deep::sweep(); }\n");
    assert_eq!(
        got,
        vec![
            "uses-member src/lib.rs:3 -> src/shelf.rs#dust",
            "uses-member src/lib.rs:3 -> src/shelf.rs#deep::sweep",
        ]
    );
    assert!(lib_with("mod shelf;\nuse shelf::deep::*;\nfn t() { dust(); }\n").is_empty());
}

#[test]
fn renames_bind_the_new_name_and_underscore_binds_nothing() {
    let got = lib_with("mod shelf;\nuse shelf::dust as polish;\nuse shelf::wipe as _;\nfn t() { polish(); dust(); wipe(); }\n");
    assert_eq!(got, vec!["uses-member src/lib.rs:4 -> src/shelf.rs#dust"]);
}

#[test]
fn super_chains_crate_and_self_name_modules() {
    let src = "mod a {\n    pub mod b {\n        pub fn t() { super::super::top(); super::mid(); crate::top(); self::low(); super::super::gone(); }\n        pub fn low() {}\n    }\n    pub fn mid() {}\n}\nfn top() {}\n";
    assert_eq!(
        edges_for(&[("src/lib.rs", src)]),
        vec![
            "uses-member src/lib.rs:3 -> src/lib.rs#top",
            "uses-member src/lib.rs:3 -> src/lib.rs#a::mid",
            "uses-member src/lib.rs:3 -> src/lib.rs#top",
            "uses-member src/lib.rs:3 -> src/lib.rs#a::b::low",
        ]
    );
}

#[test]
fn the_library_name_binds_from_tests_and_main_but_not_from_build_or_orphans() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"lamp-kit\"\n"),
        ("src/lib.rs", "pub fn glow() {}\n"),
        ("src/main.rs", "fn main() { lamp_kit::glow(); }\n"),
        (
            "tests/it.rs",
            "use lamp_kit::glow;\nfn t() { glow(); ::lamp_kit::glow(); }\n",
        ),
        ("build.rs", "fn main() { lamp_kit::glow(); }\n"),
        ("stray/loose.rs", "fn s() { lamp_kit::glow(); }\n"),
    ];
    assert_eq!(
        edges_for(&files),
        vec![
            "uses-member src/main.rs:1 -> src/lib.rs#glow",
            "uses-member tests/it.rs:2 -> src/lib.rs#glow",
            "uses-member tests/it.rs:2 -> src/lib.rs#glow",
        ]
    );
}

#[test]
fn a_pub_use_chain_reaches_the_defining_item_and_a_cycle_ends() {
    let src = "mod a { pub use crate::b::glow; }\nmod b { pub use crate::c::glow; }\nmod c { pub fn glow() {} }\nmod x { pub use crate::y::spin; }\nmod y { pub use crate::x::spin; }\nfn t() { a::glow(); x::spin(); }\n";
    assert_eq!(
        edges_for(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:6 -> src/lib.rs#c::glow"]
    );
}

#[test]
fn a_block_level_use_binds_inside_its_block_only() {
    let src = "mod shelf { pub fn dust() {} }\nfn t() {\n    {\n        use shelf::dust;\n        dust();\n    }\n    dust();\n}\n";
    assert_eq!(
        edges_for(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:5 -> src/lib.rs#shelf::dust"]
    );
}

#[test]
fn an_explicit_import_wins_over_a_glob_and_an_external_one_shadows_it() {
    let src = "mod a { pub fn glow() {} pub fn spin() {} }\nmod b { pub fn glow() {} }\nuse a::*;\nuse b::glow;\nuse std::thread::spin;\nfn t() { glow(); spin(); }\n";
    assert_eq!(
        edges_for(&[("src/lib.rs", src)]),
        vec!["uses-member src/lib.rs:6 -> src/lib.rs#b::glow"]
    );
}

#[test]
fn a_leading_double_colon_names_only_an_external_crate() {
    let src = "mod shelf { pub fn dust() {} }\nfn t() { ::shelf::dust(); }\n";
    assert!(edges_for(&[("src/lib.rs", src)]).is_empty());
}

#[test]
fn a_use_declaration_gives_no_edge() {
    let src = "mod shelf { pub struct Lamp; pub fn dust() {} }\nuse shelf::{Lamp, dust};\n";
    assert!(edges_for(&[("src/lib.rs", src)]).is_empty());
}
