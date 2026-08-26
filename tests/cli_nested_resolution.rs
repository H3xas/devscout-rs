//! CLI coverage for bare references that collide with nested types.

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
            "devscout-nested-resolution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-nested-resolution");
        for name in ["Collisions.cs", "Inherited.cs", "Derived.cs", "Claims.cs"] {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.ok(&["init", "--no-hooks"]);
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
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn refs_and_impact_ignore_unreachable_nested_types_but_keep_inherited_nesting() {
    let fixture = Fixture::new();

    for nested in ["First.Owner+Shared", "Second.Owner+Shared"] {
        let refs = fixture.ok(&["refs", nested]);
        assert!(!refs.contains("Collisions.cs:21"), "{nested}: {refs}");

        let impact = fixture.run(&["impact", nested, "--hops", "1", "--json"]);
        assert_eq!(impact.status.code(), Some(3), "{nested}: {impact:?}");
        let value: serde_json::Value = serde_json::from_slice(&impact.stdout).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 0, "{nested}");
    }

    let inherited_refs = fixture.ok(&["refs", "Inherited.Enclosing+IConverter"]);
    assert!(inherited_refs.contains("Derived.cs:5"), "{inherited_refs}");
    let inherited_impact = fixture.ok(&[
        "impact",
        "Inherited.Enclosing+IConverter",
        "--hops",
        "1",
        "--json",
    ]);
    let inherited_value: serde_json::Value = serde_json::from_str(&inherited_impact).unwrap();
    assert_eq!(inherited_value["rows"][0]["file"], "Derived.cs");

    let claim_refs = fixture.ok(&["refs", "LocalTests.Scenario+Claim"]);
    assert!(!claim_refs.contains("Claims.cs:16"), "{claim_refs}");
    let claim_impact = fixture.run(&[
        "impact",
        "LocalTests.Scenario+Claim",
        "--hops",
        "1",
        "--json",
    ]);
    assert_eq!(claim_impact.status.code(), Some(3), "{claim_impact:?}");

    let graph: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixture.root.join(".scout/graph/graph.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(graph["stats"]["ambiguous_count"], 0);
    assert_eq!(graph["stats"]["unresolved_external_count"], 2);
}
