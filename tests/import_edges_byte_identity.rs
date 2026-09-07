//! Byte identity when no import is configured: on the public conformance
//! fixture, `impact` in every output form and a fresh `map` both answer
//! exactly as they did before this feature existed.

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
            "devscout-import-edges-byte-identity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/conformance");
        for name in [
            "ICatalogStore.cs",
            "InMemoryCatalogStore.cs",
            "CatalogController.cs",
            "CatalogStoreTests.cs",
        ] {
            fs::copy(base.join("csharp").join(name), root.join(name)).unwrap();
        }
        let registry = std::env::temp_dir().join(format!(
            "devscout-import-edges-byte-identity-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self { root, registry };
        let init = fixture.run(&["init", "--no-hooks"]);
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

// The exact `impact ICatalogStore --json` bytes this fixture has always
// produced, pinned in `docs/answer-contract.md`. No import is ever
// configured in this test, so these bytes must not move by one byte.
const EXPECTED_JSON: &str = concat!(
    r#"{"schema_version":1,"query":"ICatalogStore","status":"resolved","kind":"symbol","#,
    r#""seedFiles":["ICatalogStore.cs"],"hops":2,"totalAffected":3,"rows":["#,
    r#"{"file":"InMemoryCatalogStore.cs","hop":1,"viaCount":1,"ambiguousCount":0,"#,
    r#""topSymbols":["ICatalogStore"],"topSymbolsMore":0,"score":0.3195320656137759,"#,
    r#""fromLines":{"direct":5},"why":"inherits"},"#,
    r#"{"file":"CatalogController.cs","hop":2,"viaCount":3,"ambiguousCount":0,"#,
    r#""topSymbols":["InMemoryCatalogStore"],"topSymbolsMore":0,"score":0.15227392859677333,"#,
    r#""fromLines":{"direct":5},"why":"uses-type"},"#,
    r#"{"file":"CatalogStoreTests.cs","hop":2,"viaCount":2,"ambiguousCount":0,"#,
    r#""topSymbols":["InMemoryCatalogStore"],"topSymbolsMore":0,"score":0.15227392859677333,"#,
    r#""fromLines":{"direct":11},"why":"uses-type"}],"#,
    r#""dropped":0,"manifestGap":0,"heuristicAffected":0,"testsAffected":1,"outcome":"hit"}"#,
);

#[test]
fn impact_and_map_stay_byte_identical_with_no_import_artifact_present() {
    let fx = Fixture::new();

    let artifact = fx.root.join(".scout/graph/imported-edges.json");
    assert!(
        !artifact.exists(),
        "no import was ever configured, so no artifact should exist"
    );

    let json = fx.ok(&["impact", "ICatalogStore", "--json"]);
    assert_eq!(json.trim_end_matches('\n'), EXPECTED_JSON);
    for key in [
        "importedRows",
        "importedAffected",
        "importedDropped",
        "\"provenance\"",
    ] {
        assert!(
            !json.contains(key),
            "an unconfigured import must add no key: {key} in {json}"
        );
    }

    let graph_json = fs::read_to_string(fx.root.join(".scout/graph/graph.json")).unwrap();
    assert!(
        graph_json.starts_with(r#"{"schema_version":2,"#),
        "GRAPH_SCHEMA_VERSION stays 2: {graph_json}"
    );
}
