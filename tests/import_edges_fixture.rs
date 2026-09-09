//! Cross-repo reach through imported edges: the direct and composed cases,
//! wholesale re-import, native-row stability, the `--no-imports` escape
//! hatch, and the `--json` answer-contract additions.

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
            "devscout-import-edges-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/import-edges/storefront");
        for name in ["CheckoutController.cs", "CheckoutControllerTests.cs"] {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let registry = std::env::temp_dir().join(format!(
            "devscout-import-edges-fixture-registry-{}-{}.json",
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

    fn ok_json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.ok(args)).unwrap()
    }

    fn artifact_path(&self) -> PathBuf {
        self.root.join(".scout/graph/imported-edges.json")
    }

    fn import(&self, export_name: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/import-edges")
            .join(export_name);
        let out = self.run(&[
            "import-edges",
            path.to_str().unwrap(),
            "--repo",
            "storefront",
        ]);
        assert!(out.status.success(), "import-edges failed: {out:?}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

#[test]
fn direct_and_composed_imported_edges_appear_in_text_compact_and_json() {
    let fx = Fixture::new();
    fx.import("export.json");

    let json = fx.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["importedAffected"], 2);
    assert_eq!(json["provenance"]["id"], "a1b2c3d4e5f60718");
    let rows = json["importedRows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0]["file"], "checkoutClient.ts");
    assert_eq!(rows[0]["repo"], "ledger");
    assert_eq!(rows[0]["importedKind"], "calls");
    assert_eq!(rows[0]["why"], "imported-edge");
    assert_eq!(rows[1]["file"], "orderConsumer.ts");
    assert_eq!(rows[1]["repo"], "ledger");
    assert_eq!(rows[1]["importedKind"], "consumes");
    assert_eq!(rows[1]["why"], "imported-edge");

    let text = fx.ok(&["impact", "CheckoutController.cs"]);
    assert!(text.contains("checkoutClient.ts"), "{text}");
    assert!(text.contains("orderConsumer.ts"), "{text}");
    assert!(text.contains("ledger"), "{text}");
    assert!(text.contains("a1b2c3d4e5f60718"), "{text}");

    let compact = fx.ok(&["impact", "CheckoutController.cs", "--compact"]);
    assert!(compact.contains("checkoutClient.ts"), "{compact}");
    assert!(compact.contains("orderConsumer.ts"), "{compact}");
}

#[test]
fn native_rows_are_unmoved_when_an_import_is_present() {
    let without = Fixture::new();
    let baseline = without.ok_json(&["impact", "CheckoutController.cs", "--json"]);

    let with = Fixture::new();
    with.import("export.json");
    let mut with_import = with.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    let obj = with_import.as_object_mut().unwrap();
    for key in [
        "importedAffected",
        "importedDropped",
        "importedRows",
        "provenance",
    ] {
        obj.remove(key);
    }
    assert_eq!(
        with_import, baseline,
        "every native field must be unchanged by an import"
    );
}

#[test]
fn reimport_replaces_the_prior_set_wholesale_by_provenance() {
    let fx = Fixture::new();

    fx.import("export.json");
    let full = fx.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(full["importedAffected"], 2);
    let consumer_row = full["importedRows"][1].clone();
    assert_eq!(consumer_row["file"], "orderConsumer.ts");

    // Same provenance, one edge retracted: exactly that row disappears, the
    // other survives byte-identical.
    fx.import("export-retracted.json");
    let retracted = fx.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(retracted["provenance"]["id"], "a1b2c3d4e5f60718");
    let rows = retracted["importedRows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0], consumer_row);

    // A different provenance replaces the set wholesale: the surviving
    // consumer row from above is gone, replaced by the new export's own row.
    fx.import("export-different-provenance.json");
    let replaced = fx.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(replaced["provenance"]["id"], "9988776655443322");
    let rows = replaced["importedRows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["file"], "retryClient.ts");
}

#[test]
fn no_imports_flag_matches_the_artifact_deleted_case() {
    let fx = Fixture::new();
    fx.import("export.json");

    let with_flag = fx.ok(&["impact", "CheckoutController.cs", "--no-imports", "--json"]);
    fs::remove_file(fx.artifact_path()).unwrap();
    let artifact_deleted = fx.ok(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(with_flag, artifact_deleted);
}

#[test]
fn answer_contract_documents_imported_edge_and_the_schema_version_stays_one() {
    let contract =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/answer-contract.md"))
            .unwrap();
    assert!(
        contract.contains("`imported-edge`"),
        "the why table must document imported-edge"
    );
    assert!(
        contract.contains("without")
            && contract.contains("schema_version")
            && contract.contains("opaque"),
        "the contract must state that a new why/outcome word never bumps schema_version and \
         that an unrecognised one must be treated as opaque"
    );

    let fx = Fixture::new();
    fx.import("export.json");
    let json = fx.ok_json(&["impact", "CheckoutController.cs", "--json"]);
    assert_eq!(json["schema_version"], 1);
}
