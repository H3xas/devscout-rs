//! Integration tests for the test-double refusal's extension-form parameter
//! alignment, driven against `fixtures/bus-extension-refusal/`.
//!
//! `fixtures/bus-signals/`, `fixtures/bus-vocabulary/`,
//! `fixtures/bus-vocabulary-admission/` and `fixtures/bus-array-identity/`
//! are untouched -- every case here lives in its own directory and file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-extension-refusal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-extension-refusal");
        for entry in fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if name.ends_with(".cs") {
                fs::copy(entry.path(), root.join(name)).unwrap();
            }
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
        fx.ok(&["map", "."]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn graph(&self) -> serde_json::Value {
        let bytes = fs::read(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Every bus hop's own publishing file, as the fixture-relative path the
    // graph records it under.
    fn hop_files(&self) -> Vec<String> {
        let graph = self.graph();
        let mut out: Vec<String> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| e["from_file"].as_str().unwrap().to_string())
            .collect();
        out.sort();
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

#[test]
fn a_publish_inside_a_repository_extension_helper_verify_lambda_emits_no_hop() {
    let fx = Fixture::new();
    let files = fx.hop_files();
    assert!(
        !files
            .iter()
            .any(|f| f.contains("ExtensionFormVerifyLambda.cs")),
        "neither the `VerifiedOnce` nor the `NeverCalled` extension-form verify lambda may \
         earn a hop: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.contains("SignalHandler.cs")),
        "the real, non-test-double publish site in the same fixture must keep its hop: {files:?}"
    );
}

#[test]
fn a_publish_inside_a_repository_extension_helper_called_in_static_form_emits_no_hop() {
    let fx = Fixture::new();
    let files = fx.hop_files();
    assert!(
        !files
            .iter()
            .any(|f| f.contains("StaticFormVerifyLambda.cs")),
        "the static-form call already aligns its parameters directly and must still be \
         refused: {files:?}"
    );
}

#[test]
fn a_publish_inside_an_extension_method_taking_a_plain_delegate_keeps_its_hop() {
    let fx = Fixture::new();
    let files = fx.hop_files();
    assert!(
        files
            .iter()
            .any(|f| f.contains("PlainDelegateExtension.cs")),
        "an extension helper whose parameter is a plain delegate, not an expression tree, is \
         real dispatch and must keep its hop: {files:?}"
    );
}
