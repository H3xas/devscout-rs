//! A structurally valid artifact that declares incomplete coverage is
//! admitted together with its per-unit diagnostics and an explicit
//! incomplete state -- never reported as clean or complete, and a sibling
//! unit unrelated to the failing one is retained in full.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-partial-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let registry = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-partial-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self { root, registry };
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fixture
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

    fn artifact_path(&self) -> PathBuf {
        self.root.join(".scout/graph/compiler-facts-v1.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/compiler-facts")
        .join(name)
}

#[test]
fn a_qualified_partial_is_admitted_with_diagnostics_and_incomplete_state() {
    let fx = Fixture::new();
    let candidate = fixture_path("candidate-partial.json");

    let out = fx.run(&["compiler-facts", "import", candidate.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a structurally valid partial artifact must be admitted, not refused: {out:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("incomplete"),
        "status output must say incomplete, never ok/complete: {stdout}"
    );
    assert!(
        !stdout.contains("complete)"),
        "must not read as clean complete: {stdout}"
    );

    let status = fx.ok(&["compiler-facts", "status"]);
    assert!(status.contains("incomplete"), "{status}");

    let published: Value = serde_json::from_slice(&fs::read(fx.artifact_path()).unwrap()).unwrap();
    assert_eq!(published["coverage"]["state"], "incomplete");
    let incomplete_units = published["coverage"]["incompleteUnits"].as_array().unwrap();
    assert_eq!(incomplete_units.len(), 1);
    assert_eq!(incomplete_units[0]["unit"], "Api|net9.0");
    assert!(incomplete_units[0]["reason"]
        .as_str()
        .unwrap()
        .contains("Load"));

    // The sibling unit unrelated to the failing one is retained in full,
    // not discarded wholesale.
    let processed = published["units"]["processed"].as_array().unwrap();
    assert!(processed.iter().any(|u| u == "Shared|net9.0"));
    let symbols = published["symbols"].as_array().unwrap();
    assert!(symbols
        .iter()
        .any(|s| s["assembly"] == "Shared" && s["member"] == "Publish"));

    assert_eq!(
        out.status.code(),
        Some(0),
        "exit code must never be non-zero for an admitted partial"
    );
}
