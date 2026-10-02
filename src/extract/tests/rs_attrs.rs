use super::*;

fn fragment(src: &str) -> RsFragment {
    extract_rs_file(src).fragment
}

fn tests_of(src: &str) -> Vec<String> {
    fragment(src).tests
}

fn paths_of(src: &str) -> Vec<(String, Option<String>)> {
    fragment(src)
        .mod_paths
        .into_iter()
        .map(|m| (m.item, m.path))
        .collect()
}

fn entry(item: &str, path: Option<&str>) -> (String, Option<String>) {
    (item.to_string(), path.map(str::to_string))
}

#[test]
fn a_test_attribute_carried_by_cfg_attr_marks_a_test() {
    let src = concat!(
        "#[cfg_attr(test, test)]\nfn a() {}\n",
        "#[cfg_attr(any(test, doc), test)]\nfn b() {}\n",
        "#[cfg_attr(test, tokio::test)]\nasync fn c() {}\n",
        "#[cfg_attr(test, cfg_attr(all(), test))]\nfn d() {}\n",
        "#[cfg_attr(test, test, ignore)]\nfn e() {}\n",
        "#[cfg_attr(test, ignore, ::tokio::test(flavor = \"x\"),)]\nasync fn f() {}\n",
        "#[allow(dead_code)]\n// a note between\n#[cfg_attr(unix, test)]\nfn g() {}\n",
        "#[cfg_attr(test, test[x])]\nfn h() {}\n",
    );
    assert_eq!(tests_of(src), vec!["a", "b", "c", "d", "e", "f", "g", "h"]);
}

#[test]
fn other_cfg_attr_forms_mark_no_test() {
    let src = concat!(
        "#[cfg_attr(test, derive(Debug))]\nfn a() {}\n",
        "#[cfg_attr(test, allow(dead_code))]\nfn b() {}\n",
        "#[cfg_attr(test, test_case)]\nfn c() {}\n",
        "#[cfg_attr]\nfn d() {}\n",
        "#[cfg_attr(test)]\nfn e() {}\n",
        "#[cfg_attr(test, )]\nfn f() {}\n",
        "#[cfg_attr = \"x\"]\nfn g() {}\n",
        "#[cfg_attr(test test)]\nfn h() {}\n",
        "#[cfg_attr(test, test test)]\nfn i() {}\n",
        "#[cfg_attr(test, test = \"x\")]\nfn j() {}\n",
        "#[cfg_attr(test,, test)]\nfn k() {}\n",
        "#[cfg_attr(test, cfg_attr(test))]\nfn l() {}\n",
        "#[cfg_attr(test, test)]\nstruct NotAFunction;\n",
    );
    assert!(tests_of(src).is_empty(), "{:?}", tests_of(src));
}

#[test]
fn plain_test_attributes_still_mark_a_test() {
    let src = "#[test]\nfn a() {}\n#[tokio::test]\nasync fn b() {}\n#[test_case]\nfn c() {}\n";
    assert_eq!(tests_of(src), vec!["a", "b"]);
}

#[test]
fn the_first_path_attribute_of_a_module_declaration_is_recorded() {
    let src = concat!(
        "#[path = \"x/real.rs\"]\nmod a;\n",
        "#[path = r\"raw.rs\"]\npub mod b;\n",
        "#[path = r#\"hash.rs\"#]\nmod c;\n",
        "#[path = \"one.rs\"]\n#[path = \"two.rs\"]\nmod d;\n",
        "#[allow(dead_code)]\nmod e;\n",
        "mod f;\n",
    );
    assert_eq!(
        paths_of(src),
        vec![
            entry("a", Some("x/real.rs")),
            entry("b", Some("raw.rs")),
            entry("c", Some("hash.rs")),
            entry("d", Some("one.rs")),
        ]
    );
    assert_eq!(fragment(src).mod_files, vec!["a", "b", "c", "d", "e", "f"]);
}

#[test]
fn a_path_whose_file_cannot_be_told_gives_an_entry_without_a_path() {
    let src = concat!(
        "#[path = \"a\\\\b.rs\"]\nmod a;\n",
        "#[path = b\"b.rs\"]\nmod b;\n",
        "#[path = concat!(\"c\", \".rs\")]\nmod c;\n",
        "#[cfg_attr(unix, path = \"u.rs\")]\n#[path = \"d.rs\"]\nmod d;\n",
        "#[path = \"e.rs\"]\n#[cfg_attr(unix, path = \"u.rs\")]\nmod e;\n",
        "#[path]\nmod f;\n",
    );
    assert_eq!(
        paths_of(src),
        vec![
            entry("a", None),
            entry("b", None),
            entry("c", None),
            entry("d", None),
            entry("e", Some("e.rs")),
            entry("f", None),
        ]
    );
}

#[test]
fn children_of_an_inline_module_with_a_path_attribute_have_no_path_at_any_depth() {
    let src = concat!(
        "#[path = \"elsewhere\"]\nmod outer {\n    mod inner {\n        mod deep;\n    }\n",
        "    #[path = \"x.rs\"]\n    mod near;\n}\n",
        "#[cfg_attr(unix, path = \"p\")]\nmod other {\n    mod c;\n}\n",
        "mod plain {\n    #[path = \"k.rs\"]\n    mod kid;\n    mod free;\n}\n",
    );
    assert_eq!(
        paths_of(src),
        vec![
            entry("outer::inner::deep", None),
            entry("outer::near", None),
            entry("other::c", None),
            entry("plain::kid", Some("k.rs")),
        ]
    );
}

#[test]
fn a_repeated_declaration_is_keyed_by_its_suffixed_path() {
    let src = "#[path = \"a.rs\"]\nmod m;\n#[path = \"b.rs\"]\nmod m;\n";
    assert_eq!(
        paths_of(src),
        vec![entry("m", Some("a.rs")), entry("m-1", Some("b.rs"))]
    );
    assert_eq!(fragment(src).mod_files, vec!["m", "m-1"]);
}

#[test]
fn a_file_without_the_new_forms_has_no_mod_paths_and_serializes_as_before() {
    let src = "mod shelf;\n#[test]\nfn lit() {}\n";
    let frag = fragment(src);
    assert!(frag.mod_paths.is_empty());
    let json = serde_json::to_string(&frag).unwrap();
    assert!(!json.contains("modPaths"), "{json}");
}

#[test]
fn deep_cfg_attr_and_path_module_nesting_does_not_exhaust_a_worker_stack() {
    let depth = 2_000;
    let mut src = "#[".to_string();
    src.push_str(&"cfg_attr(test, ".repeat(depth));
    src.push_str("test");
    src.push_str(&")".repeat(depth));
    src.push_str("]\nfn deep() {}\n#[path = \"x\"]\n");
    src.push_str(&"mod m {\n".repeat(depth));
    src.push_str("mod leaf;\n");
    src.push_str(&"}\n".repeat(depth));
    let frag = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || extract_rs_file(&src).fragment)
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    assert_eq!(frag.tests, vec!["deep"]);
    assert_eq!(frag.mod_paths.len(), 1);
    assert!(frag.mod_paths[0].item.ends_with("m::leaf"));
    assert_eq!(frag.mod_paths[0].path, None);
}
