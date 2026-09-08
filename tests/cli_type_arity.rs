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
        Self::with_files(&["Definitions.cs", "Consumers.cs"])
    }

    fn with_files(names: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-type-arity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-arity");
        for name in names {
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

    fn graph(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

/// Precise `uses-member` edges out of `(from_file, from_line)` naming `to`.
fn precise_member_edges_to<'a>(
    graph: &'a serde_json::Value,
    from_file: &str,
    from_line: u64,
    to: &str,
) -> Vec<&'a serde_json::Value> {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == from_file
                && e["from_line"].as_u64() == Some(from_line)
                && e["to"] == to
                && e["heuristic"].is_null()
        })
        .collect()
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

#[test]
fn a_receiver_written_with_two_type_arguments_never_binds_a_one_type_argument_sibling() {
    let fx = Fixture::with_files(&["Catalogue.cs", "CatalogueConsumers.cs"]);
    let graph = fx.graph();

    // `ICatalogue` (arity 0) and `ICatalogue<T>` (arity 1) share one id, and
    // `WideConsumer.shelf` writes a THIRD arity that neither sibling has --
    // the shape where the arity-blind fallback used to answer with
    // whichever sibling the index happened to meet first. No in-tree
    // arity-2 def exists, so the call must stay external.
    assert!(
        precise_member_edges_to(&graph, "CatalogueConsumers.cs", 11, "Catalog.ICatalogue").is_empty(),
        "a receiver written with two type arguments must never bind the one-type-argument sibling: {graph:#}"
    );

    // NarrowConsumer.shelf writes the arity the generic sibling actually
    // has, and must keep its precise edge -- the guard that an exact-arity
    // match still binds.
    let narrow = precise_member_edges_to(&graph, "CatalogueConsumers.cs", 21, "Catalog.ICatalogue");
    assert_eq!(
        narrow.len(),
        1,
        "an exact one-type-argument receiver must still resolve precisely: {graph:#}"
    );
    assert_eq!(narrow[0]["member"], "Shelve");
}
