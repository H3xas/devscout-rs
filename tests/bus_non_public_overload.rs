//! Integration tests for which declared overloads the test-double refusal
//! reads for a verb, driven against `fixtures/bus-non-public-overload/`.
//!
//! Every other bus fixture directory is untouched -- the case here lives in
//! its own directory, so the private `Setup` it declares cannot reach any
//! other fixture's setup lambdas.

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
            "devscout-bus-non-public-overload-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-non-public-overload");
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
fn a_mocking_setup_lambda_emits_no_hop_when_an_unrelated_class_declares_a_private_setup_taking_a_plain_delegate(
) {
    let fx = Fixture::new();
    let files = fx.hop_files();
    assert!(
        !files.iter().any(|f| f.contains("LibrarySetupLambda.cs")),
        "a private `Setup(Action<…>)` on an unrelated class is not an overload of the library's \
         `Setup` call, so the setup lambda must stay refused: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.contains("FlareWatch.cs")),
        "the real publish in the same fixture must keep its hop: {files:?}"
    );
}
