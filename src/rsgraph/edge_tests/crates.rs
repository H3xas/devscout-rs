use super::*;
use crate::rsgraph::crates::{sidecar_differs, CrateRoot, RootKind, RustPackage};

fn roots(package: &RustPackage) -> Vec<(RootKind, &str)> {
    package
        .roots
        .iter()
        .map(|r| (r.kind, r.file.as_str()))
        .collect()
}

#[test]
fn default_target_files_that_exist_are_roots() {
    let got = crates_of(&[
        ("Cargo.toml", "[package]\nname = \"lamp-post\"\n"),
        ("src/lib.rs", ""),
        ("src/main.rs", ""),
        ("tests/a.rs", ""),
        ("tests/sub/b.rs", ""),
        ("examples/e.rs", ""),
        ("benches/k.rs", ""),
        ("build.rs", ""),
        ("src/other.rs", ""),
    ])
    .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0].dir.as_str(), got[0].name.as_str()),
        ("", "lamp-post")
    );
    assert_eq!(got[0].lib.as_deref(), Some("lamp_post"));
    assert_eq!(
        roots(&got[0]),
        vec![
            (RootKind::Lib, "src/lib.rs"),
            (RootKind::Bin, "src/main.rs"),
            (RootKind::Test, "tests/a.rs"),
            (RootKind::Example, "examples/e.rs"),
            (RootKind::Bench, "benches/k.rs"),
            (RootKind::Build, "build.rs"),
        ],
        "a nested tests/ file and a plain source file are no roots"
    );
}

#[test]
fn absent_default_files_give_no_root_and_no_library_name() {
    let got = crates_of(&[
        ("Cargo.toml", "[package]\nname = \"solo\"\n"),
        ("src/main.rs", ""),
    ])
    .unwrap();
    assert_eq!(roots(&got[0]), vec![(RootKind::Bin, "src/main.rs")]);
    assert_eq!(got[0].lib, None);
}

#[test]
fn path_keys_and_the_library_name_come_from_their_sections() {
    let manifest = "[package]\r\nname = 'wick-set' # the set\r\n\r\n[lib]\r\nname = \"wicks\"\r\npath = \"core/lib.rs\"\r\n\r\n[[bin]]\r\nname = \"x\"\r\npath = \"tools/x.rs\"\r\n\r\n[[test]]\r\npath = \"./it/y.rs\"\r\n[[example]]\r\npath = \"../outside.rs\"\r\n[[bench]]\r\npath = \"unwalked.rs\"\r\nnot a key line\r\n";
    let got = crates_of(&[
        ("pkg/Cargo.toml", manifest),
        ("pkg/core/lib.rs", ""),
        ("pkg/tools/x.rs", ""),
        ("pkg/it/y.rs", ""),
        ("outside.rs", ""),
    ])
    .unwrap();
    assert_eq!(got[0].dir, "pkg");
    assert_eq!(got[0].name, "wick-set");
    assert_eq!(got[0].lib.as_deref(), Some("wicks"));
    assert_eq!(
        roots(&got[0]),
        vec![
            (RootKind::Lib, "pkg/core/lib.rs"),
            (RootKind::Bin, "pkg/tools/x.rs"),
            (RootKind::Test, "pkg/it/y.rs"),
        ],
        "a path leaving the package and a path to no walked file add nothing"
    );
}

#[test]
fn a_workspace_only_manifest_names_no_root_and_members_are_their_own_packages() {
    assert_eq!(
        crates_of(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"a\"]\n"),
            ("src/lib.rs", ""),
        ]),
        None
    );
    let got = crates_of(&[
        ("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n"),
        ("b/Cargo.toml", "[package]\nname = \"b\"\n"),
        ("b/src/lib.rs", ""),
        (
            "a/Cargo.toml",
            "[package]\nname = \"a\"\n[package.metadata]\nname = \"not-it\"\n",
        ),
        ("a/src/main.rs", ""),
    ])
    .unwrap();
    let names: Vec<(&str, &str)> = got
        .iter()
        .map(|p| (p.dir.as_str(), p.name.as_str()))
        .collect();
    assert_eq!(names, vec![("a", "a"), ("b", "b")]);
}

#[test]
fn a_manifest_without_a_package_section_or_name_is_skipped() {
    assert_eq!(
        crates_of(&[("Cargo.toml", "name = \"loose\"\n"), ("src/lib.rs", "")]),
        None
    );
    assert_eq!(
        crates_of(&[("Cargo.toml", "[package]\nversion = \"1\"\n")]),
        None
    );
    assert_eq!(crates_of(&[("src/lib.rs", "")]), None);
}

#[test]
fn the_sidecar_differs_unless_both_sides_agree() {
    let dir = scratch(&[]);
    let model = vec![RustPackage {
        dir: String::new(),
        name: "a".into(),
        lib: Some("a".into()),
        roots: vec![CrateRoot {
            file: "src/lib.rs".into(),
            kind: RootKind::Lib,
        }],
    }];
    assert!(!sidecar_differs(&dir, None));
    assert!(sidecar_differs(&dir, Some(&model)));
    let path = crate::graph::rust_crates_path(&dir);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&model).unwrap()).unwrap();
    assert!(!sidecar_differs(&dir, Some(&model)));
    assert!(sidecar_differs(&dir, None));
    let mut renamed = model.clone();
    renamed[0].name = "b".into();
    assert!(sidecar_differs(&dir, Some(&renamed)));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"[{"dir":"","name":"a","lib":"a","roots":[{"file":"src/lib.rs","kind":"lib"}]}]"#
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_rebuild_writes_the_sidecar_and_removes_it_when_no_package_remains() {
    let dir = scratch(&[]);
    let model = vec![RustPackage {
        dir: String::new(),
        name: "a".into(),
        lib: None,
        roots: Vec::new(),
    }];
    let files: Vec<crate::graph::GraphFile> = Vec::new();
    let fresh = std::collections::HashMap::new();
    crate::graph::rebuild_graph_with_crates(&dir, &files, &fresh, true, None, None, Some(&model))
        .unwrap();
    let path = crate::graph::rust_crates_path(&dir);
    assert!(path.exists());
    crate::graph::rebuild_graph(&dir, &files, &fresh, true, None, None).unwrap();
    assert!(!path.exists());
    let _ = fs::remove_dir_all(&dir);
}
