use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn module_resolution_ac_7_no_mode_does_not_read_package_manifests() {
    let root = std::env::temp_dir().join(format!(
        "devscout-inert-config-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext"}}"#,
    )
    .unwrap();
    let (_, config) = read_ts_resolution(&root, ["src/index.ts"]);
    assert!(config.inert());
    assert!(config.packages.is_none());
    assert!(config.refused.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn module_resolution_ac_6_declared_mode_reads_and_refuses_bad_manifest() {
    let root = std::env::temp_dir().join(format!(
        "devscout-declared-config-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    )
    .unwrap();
    let (_, config) = read_ts_resolution(&root, ["src/index.ts"]);
    assert!(!config.inert());
    assert_eq!(
        config.refused[0],
        RefusedConfig {
            file: "package.json".into(),
            field: "document".into()
        }
    );
    fs::remove_dir_all(root).unwrap();
}
