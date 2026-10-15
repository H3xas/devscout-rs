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
