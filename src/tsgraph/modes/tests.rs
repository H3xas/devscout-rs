use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(std::path::PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "devscout-module-modes-{}-{}",
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

#[test]
fn module_resolution_ac_1_modes_follow_nearest_declaring_chain() {
    let temp = Temp::new();
    temp.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"NoDe10"}}"#,
    );
    temp.write(
        "apps/a/tsconfig.json",
        r#"{"extends":"./base","compilerOptions":{}}"#,
    );
    temp.write(
        "apps/a/base.json",
        r#"{"compilerOptions":{"moduleResolution":"NoDe16"}}"#,
    );
    temp.write(
        "apps/b/tsconfig.json",
        r#"{"compilerOptions":{"module":"preserve"}}"#,
    );
    let scopes = read_modes(&temp.0, ["apps/a/src/x.ts", "apps/b/x.ts", "apps/c/x.ts"]);
    assert_eq!(scopes.get("apps/a/src/x.ts").unwrap().mode, Mode::Node16);
    assert_eq!(scopes.get("apps/b/x.ts").unwrap().mode, Mode::Bundler);
    assert_eq!(scopes.get("apps/c/x.ts").unwrap().mode, Mode::Node10);
}

#[test]
fn module_resolution_ac_1_classic_and_unrecognised_module_do_not_infer_a_mode() {
    let temp = Temp::new();
    temp.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"module":"esnext"}}"#,
    );
    temp.write(
        "apps/a/tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"CLASSIC"}}"#,
    );
    let scopes = read_modes(&temp.0, ["apps/a/x.ts", "apps/b/x.ts"]);
    assert_eq!(scopes.get("apps/a/x.ts").unwrap().mode, Mode::Classic);
    assert!(scopes.get("apps/b/x.ts").is_none());
}

#[test]
fn module_resolution_ac_6_refused_option_leaves_chain_without_a_mode() {
    let temp = Temp::new();
    temp.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    temp.write(
        "apps/a/tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"nodee"}}"#,
    );
    let scopes = read_modes(&temp.0, ["apps/a/x.ts"]);
    assert_eq!(scopes.get("apps/a/x.ts").unwrap().mode, Mode::Node16);
    assert_eq!(
        scopes.refused,
        vec![RefusedConfig {
            file: "apps/a/tsconfig.json".into(),
            field: "compilerOptions.moduleResolution".into()
        }]
    );
}
