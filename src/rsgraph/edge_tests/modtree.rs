use super::*;

const DUST: &str = "pub fn dust() {}\n";

#[test]
fn without_a_manifest_lib_and_main_are_roots() {
    let files = [
        ("src/lib.rs", "mod shelf;\nfn a() { shelf::dust(); }\n"),
        ("src/shelf.rs", DUST),
        ("app/main.rs", "mod tool;\nfn b() { tool::dust(); }\n"),
        ("app/tool.rs", DUST),
    ];
    assert_eq!(
        edges_for(&files),
        vec![
            "uses-member src/lib.rs:2 -> src/shelf.rs#dust",
            "uses-member app/main.rs:2 -> app/tool.rs#dust",
        ]
    );
}

#[test]
fn an_unreached_file_is_its_own_root_with_or_without_a_manifest() {
    let lone = (
        "src/lone.rs",
        "mod inner;\nfn a() { inner::dust(); crate::b(); }\nfn b() {}\n",
    );
    let inner = ("src/inner.rs", DUST);
    let want = vec![
        "uses-member src/lone.rs:2 -> src/inner.rs#dust",
        "uses-member src/lone.rs:2 -> src/lone.rs#b",
    ];
    assert_eq!(edges_for(&[lone, inner]), want);
    let manifest = ("Cargo.toml", "[package]\nname = \"p\"\n");
    let lib = ("src/lib.rs", "fn c() {}\n");
    assert_eq!(edges_for(&[manifest, lib, lone, inner]), want);
}

#[test]
fn a_declaration_resolves_beside_a_root_or_mod_rs_and_below_any_other_file() {
    let files = [
        (
            "src/lib.rs",
            "mod a;\nmod foo;\nfn t() { a::b::dust(); foo::bar::dust(); }\n",
        ),
        ("src/a/mod.rs", "pub mod b;\n"),
        ("src/a/b.rs", DUST),
        ("src/foo.rs", "pub mod bar;\n"),
        ("src/foo/bar.rs", DUST),
        ("src/bar.rs", DUST),
        ("src/b.rs", DUST),
    ];
    assert_eq!(
        edges_to(&files, "src/a/b.rs#dust"),
        vec!["uses-member src/lib.rs:3 -> src/a/b.rs#dust"]
    );
    assert_eq!(
        edges_to(&files, "src/foo/bar.rs#dust"),
        vec!["uses-member src/lib.rs:3 -> src/foo/bar.rs#dust"]
    );
    assert!(edges_to(&files, "src/bar.rs#dust").is_empty());
    assert!(edges_to(&files, "src/b.rs#dust").is_empty());
}

#[test]
fn an_inline_module_nests_and_its_out_of_line_child_resolves_one_directory_deeper() {
    let files = [
        ("src/lib.rs", "mod outer {\n    pub mod inner;\n    pub fn here() {}\n}\nfn t() { outer::inner::dust(); outer::here(); }\n"),
        ("src/outer/inner.rs", DUST),
        ("src/inner.rs", DUST),
    ];
    assert_eq!(
        edges_for(&files),
        vec![
            "uses-member src/lib.rs:5 -> src/outer/inner.rs#dust",
            "uses-member src/lib.rs:5 -> src/lib.rs#outer::here",
        ]
    );
}

#[test]
fn a_missing_or_doubled_module_file_reaches_nothing() {
    let files = [
        (
            "src/lib.rs",
            "mod gone;\nmod twice;\nfn t() { gone::dust(); twice::dust(); }\n",
        ),
        ("src/twice.rs", DUST),
        ("src/twice/mod.rs", DUST),
    ];
    assert!(edges_for(&files).is_empty());
}

#[test]
fn the_first_position_to_reach_a_file_places_it() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"p\"\n"),
        ("src/lib.rs", "mod shared;\npub fn origin() {}\n"),
        (
            "src/main.rs",
            "mod shared;\npub fn origin() {}\nfn t() { shared::dust(); }\n",
        ),
        ("src/shared.rs", "pub fn dust() { crate::origin(); }\n"),
    ];
    let got = edges_for(&files);
    assert!(
        got.contains(&"uses-member src/shared.rs:1 -> src/lib.rs#origin".to_string()),
        "{got:?}"
    );
    assert!(
        !got.iter().any(|e| e.ends_with("src/main.rs#origin")),
        "{got:?}"
    );
    assert!(got.contains(&"uses-member src/main.rs:3 -> src/shared.rs#dust".to_string()));
}
