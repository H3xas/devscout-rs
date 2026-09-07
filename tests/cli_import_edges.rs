//! CLI coverage for the `import-edges` verb: a valid export writes the
//! artifact, and every malformed shape refuses loudly without touching
//! whatever artifact already existed.

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
            "devscout-import-edges-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/import-edges/storefront");
        fs::copy(
            source.join("CheckoutController.cs"),
            root.join("CheckoutController.cs"),
        )
        .unwrap();
        let registry = std::env::temp_dir().join(format!(
            "devscout-import-edges-cli-registry-{}-{}.json",
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

    fn artifact_path(&self) -> PathBuf {
        self.root.join(".scout/graph/imported-edges.json")
    }

    fn export_path(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/import-edges")
            .join(name)
            .to_str()
            .unwrap()
            .to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

#[test]
fn import_edges_writes_the_artifact_on_a_valid_export_and_refuses_loudly_without_partial_writes() {
    let fx = Fixture::new();
    let export = Fixture::export_path("export.json");

    // A valid export exits 0 and writes the artifact.
    let out = fx.run(&["import-edges", &export, "--repo", "storefront"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(fx.artifact_path().exists(), "artifact was not written");
    let written = fs::read(fx.artifact_path()).unwrap();

    // Every malformed shape below exits 1, names the offending value, and
    // leaves the artifact this run already wrote byte-identical.
    let cases: &[(&str, &str)] = &[
        (r#"not json"#, "malformed JSON"),
        (
            r#"{"schemaVersion":1,"format":"something-else","provenance":{"id":"x"},"edges":[]}"#,
            "something-else",
        ),
        (
            r#"{"schemaVersion":2,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[]}"#,
            "schemaVersion",
        ),
        (
            r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"from":{},"to":{}}]}"#,
            "\"kind\"",
        ),
        (
            r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"calls","to":{}}]}"#,
            "\"from\"",
        ),
        (
            r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"calls","from":{}}]}"#,
            "\"to\"",
        ),
        (
            r#"{"schemaVersion":1,"format":"flowtrace-edges","provenance":{"id":"x"},"edges":[{"kind":"deletes","from":{},"to":{}}]}"#,
            "deletes",
        ),
    ];
    for (i, (body, needle)) in cases.iter().enumerate() {
        let bad = fx.root.join(format!("bad-{i}.json"));
        fs::write(&bad, body).unwrap();
        let out = fx.run(&[
            "import-edges",
            bad.to_str().unwrap(),
            "--repo",
            "storefront",
        ]);
        assert_eq!(out.status.code(), Some(1), "case {i}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(needle),
            "case {i}: expected {needle:?} in {stdout:?}"
        );
        assert_eq!(
            fs::read(fx.artifact_path()).unwrap(),
            written,
            "case {i}: a refusal must leave the artifact untouched"
        );
    }
}

#[test]
fn import_edges_on_a_fresh_repo_writes_nothing_on_a_malformed_export() {
    let fx = Fixture::new();
    let bad = fx.root.join("bad.json");
    fs::write(&bad, "not json").unwrap();
    let out = fx.run(&[
        "import-edges",
        bad.to_str().unwrap(),
        "--repo",
        "storefront",
    ]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        !fx.artifact_path().exists(),
        "a refusal must never create the artifact"
    );
}

#[test]
fn import_edges_requires_the_repo_flag_and_a_file_argument() {
    let fx = Fixture::new();
    let export = Fixture::export_path("export.json");

    let missing_repo = fx.run(&["import-edges", &export]);
    assert_eq!(missing_repo.status.code(), Some(2), "{missing_repo:?}");

    let missing_file = fx.run(&["import-edges", "--repo", "storefront"]);
    assert_eq!(missing_file.status.code(), Some(2), "{missing_file:?}");
}
