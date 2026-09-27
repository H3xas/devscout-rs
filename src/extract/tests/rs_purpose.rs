use super::*;

fn purpose(src: &str) -> Option<String> {
    extract_rs_file(src).purpose
}

#[test]
fn an_inner_line_doc_gives_its_first_non_empty_line_as_the_prefix() {
    let src = "//!\n//! Shelves and the widgets on them.\n//! More detail.\nstruct Widget;\nfn polish() {}\n";
    assert_eq!(
        purpose(src).as_deref(),
        Some("Shelves and the widgets on them. — struct Widget | fn polish")
    );
}

#[test]
fn an_inner_block_doc_gives_its_first_non_empty_line() {
    let src = "/*!\n * Cabinet layout.\n * Second line.\n */\nenum Door { Left }\n";
    assert_eq!(purpose(src).as_deref(), Some("Cabinet layout. — enum Door"));
}

#[test]
fn a_plain_leading_comment_gives_its_first_line() {
    assert_eq!(
        purpose("// Tone tables.\n// more\nconst A: u8 = 1;\n").as_deref(),
        Some("Tone tables. — const A")
    );
    assert_eq!(
        purpose("/* Block note. */\nstatic B: u8 = 1;\n").as_deref(),
        Some("Block note. — static B")
    );
    assert_eq!(
        purpose("/*\n  body only\n*/\nstatic B: u8 = 1;\n").as_deref(),
        Some("static B"),
        "an empty first line gives no prefix"
    );
}

#[test]
fn an_outer_doc_first_gives_no_prefix() {
    assert_eq!(
        purpose("/// Widget doc.\nstruct Widget;\n").as_deref(),
        Some("struct Widget")
    );
}

#[test]
fn a_shebang_and_inner_attributes_are_skipped_before_the_prefix() {
    let src = "#!/usr/bin/env run\n#![allow(dead_code)]\n#![deny(missing_docs)]\n//! Tool entry.\nfn main() {}\n";
    assert_eq!(purpose(src).as_deref(), Some("Tool entry. — fn main"));
}

#[test]
fn a_use_first_gives_the_segments_alone() {
    let src = "use std::fmt;\n// not leading\nfn tone() {}\n";
    assert_eq!(purpose(src).as_deref(), Some("fn tone"));
}

#[test]
fn segments_group_by_kind_then_source_order_and_skip_nested_items() {
    let src = "\
mod inner { struct Hidden; }
fn b() {}
macro_rules! m { () => {}; }
static S: u8 = 0;
const C: u8 = 0;
fn a() {}
type T = u8;
trait Tr { fn nested(&self); }
union U { x: u8 }
enum E { V }
struct St;
impl St { fn method() {} }
";
    assert_eq!(
        purpose(src).as_deref(),
        Some("struct St | enum E | union U | trait Tr | type T | fn b | fn a | const C | static S | macro_rules m | mod inner")
    );
}

#[test]
fn a_file_with_neither_gives_none() {
    assert_eq!(purpose("use std::fmt;\nuse std::io;\n"), None);
    assert_eq!(purpose(""), None);
}

#[test]
fn a_long_purpose_is_cut_at_the_shared_cap() {
    let mut src = String::from("//! Many functions.\n");
    for i in 0..40 {
        src.push_str(&format!("fn function_number_{i}() {{}}\n"));
    }
    let p = purpose(&src).expect("a purpose");
    assert_eq!(p.encode_utf16().count(), 200);
    assert!(p.starts_with("Many functions. — fn function_number_0 | "));
    assert!(p.ends_with("..."));
}

#[test]
fn items_of_an_extern_block_are_not_segments() {
    let src = "extern \"C\" {\n    fn host_count() -> i32;\n}\nfn local() {}\n";
    assert_eq!(purpose(src).as_deref(), Some("fn local"));
}
