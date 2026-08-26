//! Integration tests for command-line generic-type arity handling.

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
            "devscout-type-arity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-arity");
        for name in ["Definitions.cs", "Consumers.cs"] {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn refs_and_impact_only_follow_exact_type_arity() {
    let fx = Fixture::new();

    let generic = fx.ok(&["refs", "Generic.Widget"]);
    assert!(generic.contains("Consumers.cs:13"), "{generic}");
    assert!(!generic.contains("Consumers.cs:8"), "{generic}");
    assert!(!generic.contains("Consumers.cs:18"), "{generic}");

    let plain = fx.ok(&["refs", "Plain.Widget"]);
    assert!(plain.contains("Consumers.cs:8"), "{plain}");
    assert!(!plain.contains("Consumers.cs:13"), "{plain}");
    assert!(!plain.contains("Consumers.cs:18"), "{plain}");

    let impact = fx.ok(&["impact", "Definitions.cs", "--hops", "1", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&impact).unwrap();
    assert_eq!(
        value["rows"][0]["viaCount"], 4,
        "open Foo arities and both Widget arities are precise: {impact}"
    );

    let graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(fx.root.join(".scout/graph/graph.json")).unwrap())
            .unwrap();
    assert_eq!(
        graph["stats"]["unresolved_external_count"], 1,
        "Widget<T,U> must remain unresolved"
    );
}
