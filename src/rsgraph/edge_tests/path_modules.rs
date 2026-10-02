use super::*;

const SPIN: &str = "pub fn spin() {}\n";

// Every edge, and every edge into one of `files`'s paths.
fn edges_into(files: &[(&str, &str)], file: &str) -> Vec<String> {
    edges_for(files)
        .into_iter()
        .filter(|e| e.contains(&format!("-> {file}#")))
        .collect()
}

#[test]
fn a_file_level_path_reaches_its_file_and_not_the_default_file_beside_it() {
    let files = [
        (
            "src/lib.rs",
            "#[path = \"real.rs\"]\nmod alias;\nfn t() { alias::spin(); }\n",
        ),
        ("src/real.rs", SPIN),
        ("src/alias.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "src/real.rs"),
        vec!["uses-member src/lib.rs:3 -> src/real.rs#spin"]
    );
    assert!(edges_into(&files, "src/alias.rs").is_empty());
}

#[test]
fn dot_dot_reaches_a_sibling_directory_relative_to_a_non_mod_rs_files_directory() {
    let files = [
        ("src/lib.rs", "mod reel;\n"),
        (
            "src/reel.rs",
            "#[path = \"../gear/cog.rs\"]\nmod cog;\nfn t() { cog::spin(); }\n",
        ),
        ("gear/cog.rs", SPIN),
        ("src/gear/cog.rs", SPIN),
        ("src/reel/cog.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "gear/cog.rs"),
        vec!["uses-member src/reel.rs:3 -> gear/cog.rs#spin"]
    );
    assert!(edges_into(&files, "src/gear/cog.rs").is_empty());
    assert!(edges_into(&files, "src/reel/cog.rs").is_empty());
}

#[test]
fn inside_an_inline_module_of_a_mod_rs_file_the_path_is_below_its_folder() {
    let files = [
        (
            "src/lib.rs",
            "mod bay {\n    #[path = \"dock.rs\"]\n    pub mod pier;\n}\nfn t() { bay::pier::spin(); }\n",
        ),
        ("src/bay/dock.rs", SPIN),
        ("src/dock.rs", SPIN),
        ("src/bay/pier.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "src/bay/dock.rs"),
        vec!["uses-member src/lib.rs:5 -> src/bay/dock.rs#spin"]
    );
    assert!(edges_into(&files, "src/dock.rs").is_empty());
    assert!(edges_into(&files, "src/bay/pier.rs").is_empty());
}

#[test]
fn inside_an_inline_module_of_a_non_mod_rs_file_the_path_starts_below_its_stem() {
    let files = [
        ("src/lib.rs", "mod reel;\n"),
        (
            "src/reel.rs",
            "mod spool {\n    #[path = \"thread.rs\"]\n    pub mod line;\n}\nfn t() { spool::line::spin(); }\n",
        ),
        ("src/reel/spool/thread.rs", SPIN),
        ("src/spool/thread.rs", SPIN),
        ("src/reel/spool/line.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "src/reel/spool/thread.rs"),
        vec!["uses-member src/reel.rs:5 -> src/reel/spool/thread.rs#spin"]
    );
    assert!(edges_into(&files, "src/spool/thread.rs").is_empty());
    assert!(edges_into(&files, "src/reel/spool/line.rs").is_empty());
}

#[test]
fn a_file_reached_by_path_owns_its_directory() {
    let files = [
        (
            "src/lib.rs",
            "#[path = \"deep/hub.rs\"]\nmod hub;\nfn t() { hub::spoke::spin(); hub::bay::spin(); }\n",
        ),
        ("src/deep/hub.rs", "pub mod spoke;\npub mod bay {\n    pub mod inner;\n    pub use inner::spin;\n}\n"),
        ("src/deep/spoke.rs", SPIN),
        ("src/deep/bay/inner.rs", SPIN),
        ("src/deep/hub/spoke.rs", SPIN),
        ("src/deep/hub/bay/inner.rs", SPIN),
        ("src/hub.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "src/deep/spoke.rs"),
        vec!["uses-member src/lib.rs:3 -> src/deep/spoke.rs#spin"]
    );
    assert_eq!(
        edges_into(&files, "src/deep/bay/inner.rs"),
        vec!["uses-member src/lib.rs:3 -> src/deep/bay/inner.rs#spin"]
    );
    assert!(edges_into(&files, "src/deep/hub/spoke.rs").is_empty());
    assert!(edges_into(&files, "src/deep/hub/bay/inner.rs").is_empty());
    assert!(edges_into(&files, "src/hub.rs").is_empty());
}

#[test]
fn the_first_of_two_path_attributes_wins() {
    let files = [
        (
            "src/lib.rs",
            "#[path = \"one.rs\"]\n#[path = \"two.rs\"]\nmod m;\nfn t() { m::spin(); }\n",
        ),
        ("src/one.rs", SPIN),
        ("src/two.rs", SPIN),
        ("src/m.rs", SPIN),
    ];
    assert_eq!(
        edges_into(&files, "src/one.rs"),
        vec!["uses-member src/lib.rs:4 -> src/one.rs#spin"]
    );
    assert!(edges_into(&files, "src/two.rs").is_empty());
    assert!(edges_into(&files, "src/m.rs").is_empty());
}

#[test]
fn absolute_root_leaving_and_unwalked_paths_reach_nothing_and_not_the_default_file() {
    for literal in [
        "/abs/a.rs",
        "../../a.rs",
        "gone.rs",
        "a",
        "",
        "a.rs/",
        "a.rs/.",
    ] {
        let lib = format!("#[path = \"{literal}\"]\nmod a;\nfn t() {{ a::spin(); }}\n");
        let files = [
            ("src/lib.rs", lib.as_str()),
            ("src/a.rs", SPIN),
            ("src/a/mod.rs", SPIN),
            ("a.rs", SPIN),
        ];
        let got = edges_for(&files);
        assert!(
            !got.iter().any(|e| e.starts_with("uses-member src/lib.rs:")),
            "{literal}: {got:?}"
        );
    }
}

#[test]
fn a_cfg_attr_path_and_a_child_of_a_path_inline_module_reach_nothing() {
    let files = [
        (
            "src/lib.rs",
            concat!(
                "#[cfg_attr(unix, path = \"real.rs\")]\nmod a;\n",
                "#[path = \"shelf\"]\nmod b {\n    pub mod c;\n}\n",
                "fn t() { a::spin(); b::c::spin(); }\n",
            ),
        ),
        ("src/real.rs", SPIN),
        ("src/a.rs", SPIN),
        ("src/shelf/c.rs", SPIN),
        ("src/b/c.rs", SPIN),
        ("src/c.rs", SPIN),
    ];
    let got = edges_for(&files);
    assert!(
        !got.iter().any(|e| e.starts_with("uses-member src/lib.rs:")),
        "{got:?}"
    );
}

#[test]
fn no_edge_from_a_compiled_file_lands_in_a_bypassed_default_file() {
    let files = [
        (
            "src/lib.rs",
            "#[path = \"real.rs\"]\nmod alias;\nmod side;\nfn t() { alias::spin(); }\n",
        ),
        ("src/real.rs", "pub fn spin() { crate::side::spin(); }\n"),
        ("src/side.rs", "pub fn spin() { crate::alias::spin(); }\n"),
        ("src/alias.rs", "pub fn spin() { crate::side::spin(); }\n"),
    ];
    let got = edges_for(&files);
    for edge in &got {
        if !edge.contains(" src/alias.rs:") {
            assert!(!edge.contains("-> src/alias.rs#"), "{got:?}");
        }
    }
    assert!(got.contains(&"uses-member src/side.rs:1 -> src/real.rs#spin".to_string()));
}

#[test]
fn a_path_naming_its_own_file_or_an_ancestor_ends() {
    let files = [
        (
            "src/lib.rs",
            "#[path = \"lib.rs\"]\nmod again;\nmod shelf;\nfn t() { again::t(); }\n",
        ),
        ("src/shelf.rs", "#[path = \"lib.rs\"]\nmod up;\n"),
    ];
    let got = edges_for(&files);
    assert!(
        got.contains(&"uses-member src/lib.rs:4 -> src/lib.rs#t".to_string()),
        "{got:?}"
    );
}
