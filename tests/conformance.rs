//! Public conformance suite.
//!
//! These tests exercise the `map` / `find` / `refs` / `impact` / `tests`
//! surface end to end, across both supported languages, over a small,
//! invented fixture that ships in this repository. Anyone can run
//! `cargo test --test conformance` (or plain `cargo test`) locally and get
//! the same pass/fail signal CI does — no private reference implementation
//! or fixture is required. See CONTRIBUTING.md, "Reaching release-gate
//! confidence locally".

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
    init_output: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-conformance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();

        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/conformance");
        let csharp = [
            "ICatalogStore.cs",
            "InMemoryCatalogStore.cs",
            "CatalogController.cs",
            "CatalogStoreTests.cs",
        ];
        for name in csharp {
            fs::copy(base.join("csharp").join(name), root.join(name)).unwrap();
        }
        let typescript = ["catalogTypes.ts", "CatalogBadge.tsx"];
        for name in typescript {
            fs::copy(base.join("typescript").join(name), root.join(name)).unwrap();
        }

        // Kept OUTSIDE `root`: `map` scans the whole fixture root, and a
        // registry file written inside it would show up as an extra,
        // unrelated ".json (present, not indexed)" file in that scan.
        let registry = std::env::temp_dir().join(format!(
            "devscout-conformance-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut fx = Self {
            root,
            registry,
            init_output: String::new(),
        };
        fx.init_output = fx.ok(&["init", "--no-hooks"]);
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
        let _ = fs::remove_file(&self.registry);
    }
}

/// `init`/`map` extracts and graphs every fixture file, C# and TypeScript
/// alike, in one pass.
#[test]
fn map_extracts_and_graphs_both_languages() {
    let fx = Fixture::new();
    assert!(
        fx.init_output.contains("mapped 6 files"),
        "{}",
        fx.init_output
    );
    assert!(
        fx.init_output.contains("graph rebuilt"),
        "{}",
        fx.init_output
    );
}

/// `find` locates a symbol by name in both the C# and the TypeScript file.
#[test]
fn find_locates_symbols_in_both_languages() {
    let fx = Fixture::new();

    let cs = fx.ok(&["find", "InMemoryCatalogStore"]);
    assert!(cs.contains("InMemoryCatalogStore.cs"), "{cs}");

    let ts = fx.ok(&["find", "CatalogItem"]);
    assert!(ts.contains("catalogTypes.ts"), "{ts}");
}

/// `refs` follows an interface implementation and a field's type use.
#[test]
fn refs_follow_interface_and_type_use() {
    let fx = Fixture::new();

    let iface = fx.ok(&["refs", "ICatalogStore"]);
    assert!(iface.contains("InMemoryCatalogStore.cs"), "{iface}");

    let store = fx.ok(&["refs", "InMemoryCatalogStore"]);
    assert!(store.contains("CatalogController.cs"), "{store}");
}

/// `impact` reaches the direct, one-hop consumer of a changed C# file.
#[test]
fn impact_reaches_the_direct_consumer() {
    let fx = Fixture::new();

    let impact = fx.ok(&["impact", "InMemoryCatalogStore.cs", "--hops", "1", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&impact).unwrap();
    let files: Vec<&str> = value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert!(
        files.contains(&"CatalogController.cs"),
        "expected CatalogController.cs in {files:?}"
    );
}

/// `tests` finds the xUnit-attributed test class that reaches the store
/// through a real reference, not a filename convention.
#[test]
fn tests_finds_the_covering_test_class() {
    let fx = Fixture::new();

    let out = fx.ok(&["tests", "InMemoryCatalogStore"]);
    assert!(out.contains("CatalogStoreTests.cs"), "{out}");
}
