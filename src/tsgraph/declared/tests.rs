use super::*;
use crate::tsgraph::read_ts_resolution;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "devscout-declared-resolution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, content: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn resolve(root: &Temp, files: &[&str], spec: &str, usage: Usage) -> Resolution {
    let (alias, config) = read_ts_resolution(&root.0, files.iter().copied());
    let file_set: HashSet<&str> = files.iter().copied().collect();
    Resolver {
        files: &file_set,
        alias: &alias,
        config: &config,
    }
    .resolve(files[0], spec, usage)
}

fn target(answer: Resolution) -> Option<String> {
    if let Resolution::Resolved(target) = answer {
        Some(target)
    } else {
        None
    }
}

#[test]
fn module_resolution_ac_1_node16_esm_requires_extension() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    root.write("package.json", r#"{"type":"module"}"#);
    let files = ["src/use.ts", "src/x.ts"];
    assert!(matches!(
        resolve(&root, &files, "./x", Usage::Import),
        Resolution::Unresolved(Reason::EsmExtensionRequired)
    ));
    assert_eq!(
        target(resolve(&root, &files, "./x.js", Usage::Import)).as_deref(),
        Some("src/x.ts")
    );
}

#[test]
fn module_resolution_ac_1_bundler_allows_extensionless_relative_import() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    let files = ["src/use.ts", "src/x.ts"];
    assert_eq!(
        target(resolve(&root, &files, "./x", Usage::Import)).as_deref(),
        Some("src/x.ts")
    );
}

#[test]
fn module_resolution_ac_1_non_esm_modes_probe_dotted_basenames() {
    for (mode, package_type, usage) in [
        ("node10", "module", Usage::Import),
        ("bundler", "module", Usage::Import),
        ("node16", "commonjs", Usage::Require),
    ] {
        let root = Temp::new();
        root.write(
            "tsconfig.json",
            &format!(r#"{{"compilerOptions":{{"moduleResolution":"{mode}"}}}}"#),
        );
        root.write("package.json", &format!(r#"{{"type":"{package_type}"}}"#));
        let files = ["src/use.ts", "src/user.service.ts"];
        assert_eq!(
            target(resolve(&root, &files, "./user.service", usage)).as_deref(),
            Some("src/user.service.ts"),
            "{mode}"
        );
    }
}

#[test]
fn module_resolution_ac_1_esm_alias_does_not_append_extensions_or_probe_directories() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16","baseUrl":".","paths":{"@lib/*":["lib/*"]}}}"#,
    );
    root.write("package.json", r#"{"type":"module"}"#);
    let files = ["src/use.ts", "lib/bar.ts", "lib/foo/index.ts", "lib/qux.js"];
    assert!(target(resolve(&root, &files, "@lib/bar", Usage::Import)).is_none());
    assert!(target(resolve(&root, &files, "@lib/foo", Usage::Import)).is_none());
    assert!(target(resolve(&root, &files, "lib/bar", Usage::Import)).is_none());
    assert_eq!(
        target(resolve(&root, &files, "@lib/qux.js", Usage::Import)).as_deref(),
        Some("lib/qux.js")
    );
}

#[test]
fn module_resolution_ac_2_non_esm_paths_probe_directory_indexes() {
    for mode in ["node10", "bundler"] {
        let root = Temp::new();
        root.write(
            "tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{"moduleResolution":"{mode}","baseUrl":".","paths":{{"@lib/*":["lib/*"]}}}}}}"#
            ),
        );
        let files = ["src/use.ts", "lib/foo/index.ts"];
        assert_eq!(
            target(resolve(&root, &files, "@lib/foo", Usage::Import)).as_deref(),
            Some("lib/foo/index.ts"),
            "{mode}"
        );
        assert_eq!(
            target(resolve(&root, &files, "lib/foo", Usage::Import)).as_deref(),
            Some("lib/foo/index.ts"),
            "{mode}"
        );
    }
}

#[test]
fn module_resolution_ac_2_dotted_alias_and_package_main_probe_sources() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node10","baseUrl":".","paths":{"@app/*":["lib/*"]}}}"#,
    );
    root.write("package.json", r#"{"workspaces":["packages/*"]}"#);
    root.write(
        "packages/cfg/package.json",
        r#"{"name":"cfg","main":"lib/config.prod"}"#,
    );
    let files = [
        "src/use.ts",
        "lib/user.service.ts",
        "packages/cfg/lib/config.prod.ts",
    ];
    assert_eq!(
        target(resolve(&root, &files, "@app/user.service", Usage::Import)).as_deref(),
        Some("lib/user.service.ts")
    );
    assert_eq!(
        target(resolve(&root, &files, "lib/user.service", Usage::Import)).as_deref(),
        Some("lib/user.service.ts")
    );
    assert_eq!(
        target(resolve(&root, &files, "cfg", Usage::Import)).as_deref(),
        Some("packages/cfg/lib/config.prod.ts")
    );
}

#[test]
fn module_resolution_ac_3_esm_maps_do_not_append_extensions_to_dotted_targets() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    root.write(
        "package.json",
        r##"{"type":"module","workspaces":["packages/*"],"imports":{"#own":"./src/own.service"}}"##,
    );
    root.write(
        "packages/pkg/package.json",
        r#"{"name":"pkg","type":"module","exports":"./src/user.service"}"#,
    );
    let files = [
        "src/use.ts",
        "src/own.service.ts",
        "packages/pkg/src/user.service.ts",
    ];
    assert!(target(resolve(&root, &files, "#own", Usage::Import)).is_none());
    assert!(target(resolve(&root, &files, "pkg", Usage::Import)).is_none());
}

#[test]
fn module_resolution_ac_3_esm_package_main_can_probe_dotted_targets() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    root.write(
        "package.json",
        r#"{"type":"module","workspaces":["packages/*"]}"#,
    );
    root.write(
        "packages/pkg/package.json",
        r#"{"name":"pkg","type":"module","main":"./src/user.service"}"#,
    );
    let files = ["src/use.ts", "packages/pkg/src/user.service.ts"];
    assert_eq!(
        target(resolve(&root, &files, "pkg", Usage::Import)).as_deref(),
        Some("packages/pkg/src/user.service.ts")
    );
}

#[test]
fn module_resolution_ac_2_workspace_main_maps_declared_build_output() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    root.write("package.json", r#"{"workspaces":["packages/*"]}"#);
    root.write(
        "packages/pkg/package.json",
        r#"{"name":"@scope/pkg","main":"dist/index.js"}"#,
    );
    root.write(
        "packages/pkg/tsconfig.json",
        r#"{"compilerOptions":{"outDir":"dist","rootDir":"src"}}"#,
    );
    let files = ["app/use.ts", "packages/pkg/src/index.ts"];
    assert_eq!(
        target(resolve(&root, &files, "@scope/pkg", Usage::Require)).as_deref(),
        Some("packages/pkg/src/index.ts")
    );
}

#[test]
fn module_resolution_ac_3_exports_and_imports_choose_declared_targets() {
    let root = Temp::new();
    root.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    root.write(
        "package.json",
        r##"{"type":"module","workspaces":["packages/*"],"imports":{"#own":"./src/own.js"}}"##,
    );
    root.write(
        "packages/pkg/package.json",
        r#"{"name":"pkg","exports":{".":{"import":"./src/esm.js","require":"./src/cjs.js"}}}"#,
    );
    let files = [
        "src/use.ts",
        "src/own.ts",
        "packages/pkg/src/esm.ts",
        "packages/pkg/src/cjs.ts",
    ];
    assert_eq!(
        target(resolve(&root, &files, "#own", Usage::Import)).as_deref(),
        Some("src/own.ts")
    );
    assert_eq!(
        target(resolve(&root, &files, "pkg", Usage::Import)).as_deref(),
        Some("packages/pkg/src/esm.ts")
    );
    assert_eq!(
        target(resolve(&root, &files, "pkg", Usage::Require)).as_deref(),
        Some("packages/pkg/src/cjs.ts")
    );
}
