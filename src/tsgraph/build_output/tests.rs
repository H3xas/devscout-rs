use super::*;

#[test]
fn module_resolution_ac_4_declared_output_maps_to_indexed_source() {
    let outputs = BuildOutputs {
        mappings: HashMap::from([(
            "packages/a".into(),
            Mapping {
                out_dir: "packages/a/dist".into(),
                root_dir: Some("packages/a/src".into()),
            },
        )]),
        refused: vec![],
    };
    let files = HashSet::from(["packages/a/src/index.ts", "packages/a/dist/index.js"]);
    assert_eq!(
        outputs
            .source("packages/a", "packages/a/dist/index.js", &files)
            .as_deref(),
        Some("packages/a/src/index.ts")
    );
    assert!(outputs
        .source("packages/a", "packages/a/other/index.js", &files)
        .is_none());
    assert!(!outputs.covers("packages/a", "packages/a/other/index.js"));
}

#[test]
fn module_resolution_ac_4_output_extensions_follow_typescript_inputs() {
    let outputs = BuildOutputs {
        mappings: HashMap::from([(
            "a".into(),
            Mapping {
                out_dir: "a/dist".into(),
                root_dir: Some("a/src".into()),
            },
        )]),
        refused: vec![],
    };
    let files = HashSet::from(["a/src/one.mts", "a/src/two.cts"]);
    assert_eq!(
        outputs.source("a", "a/dist/one.mjs", &files).as_deref(),
        Some("a/src/one.mts")
    );
    assert_eq!(
        outputs.source("a", "a/dist/two.cjs", &files).as_deref(),
        Some("a/src/two.cts")
    );
}

#[test]
fn module_resolution_ac_4_outdir_without_root_reports_missing_mapping() {
    let outputs = BuildOutputs {
        mappings: HashMap::from([(
            "a".into(),
            Mapping {
                out_dir: "a/dist".into(),
                root_dir: None,
            },
        )]),
        refused: vec![],
    };
    let files = HashSet::from(["a/src/index.ts"]);
    assert!(outputs.covers("a", "a/dist/index.js"));
    assert!(outputs.source("a", "a/dist/index.js", &files).is_none());
}
