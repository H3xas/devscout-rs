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

    // `ICatalogue<T>` (arity 1, declares `Shelve`) and `ICatalogue` (arity 0)
    // share one id, and the generic sibling is declared first, so it holds
    // the shared qualified-name slot an arity-blind lookup lands on.
    // `WideConsumer.shelf` writes a THIRD arity that neither sibling has; no
    // in-tree arity-2 def exists, so the call must stay external rather than
    // settle for the member-declaring sibling.
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

// Two static siblings, `Volumes.Anthology` and `Volumes.Anthology<T>`, each
// declaring the same member name (`Collate`) -- so the member-list check
// alone cannot tell the two consumer lines apart and the arity carried on
// the qualifier itself has to do the work. `AnthologyConsumers.cs` writes
// the generic qualifier (arity 1, matches `Anthology<T>`), the bare
// qualifier (arity 0, matches `Anthology`), and a two-argument qualifier
// that names an arity no sibling declares.
fn anthology_fixture() -> Fixture {
    Fixture::with_files(&[
        "AnthologyBare.cs",
        "AnthologyGeneric.cs",
        "AnthologyConsumers.cs",
    ])
}

#[test]
fn a_generic_member_qualifier_keeps_its_precise_edge_at_the_matching_arity() {
    let fx = anthology_fixture();
    let graph = fx.graph();
    let edges = graph["edges"].as_array().unwrap();

    // `to_file` is the one field that still tells the two same-id siblings
    // apart in a serialized edge, since a `uses-member` edge's `to` is the
    // qualifier's simple id text and both siblings share it.
    let precise_lines_at = |to_file: &str| -> Vec<i64> {
        edges
            .iter()
            .filter(|e| {
                e["kind"] == "uses-member"
                    && e["to"] == "Volumes.Anthology"
                    && e["to_file"] == to_file
                    && e.get("heuristic").is_none()
            })
            .map(|e| e["from_line"].as_i64().unwrap())
            .collect()
    };

    assert_eq!(
        precise_lines_at("AnthologyGeneric.cs"),
        vec![13],
        "the arity-1 sibling's precise refs must be exactly the generic-qualifier line: {graph}"
    );
    assert_eq!(
        precise_lines_at("AnthologyBare.cs"),
        vec![14],
        "the arity-0 sibling's precise refs must be exactly the bare-qualifier line: {graph}"
    );
    assert_eq!(
        graph["stats"]["edges_by_kind"]["uses-member"], 2,
        "both matching-arity lines stay precise: {graph}"
    );
}

#[test]
fn a_member_qualifier_whose_arity_has_no_sibling_earns_no_precise_edge() {
    let fx = anthology_fixture();
    let graph = fx.graph();
    let edges = graph["edges"].as_array().unwrap();

    let has_precise_uses_member_at = |line: i64| {
        edges.iter().any(|e| {
            e["kind"] == "uses-member" && e["from_line"] == line && e.get("heuristic").is_none()
        })
    };

    assert!(
        !has_precise_uses_member_at(15),
        "the arity-2 qualifier names an arity neither sibling declares and must earn no precise edge: {graph}"
    );
    assert!(
        has_precise_uses_member_at(13),
        "the matching-arity generic line must keep its edge: {graph}"
    );
    assert!(
        has_precise_uses_member_at(14),
        "the matching-arity bare line must keep its edge: {graph}"
    );
    assert_eq!(
        graph["stats"]["edges_by_kind"]["uses-member"], 2,
        "the drop is scoped to the mismatched line, not its matching-arity siblings: {graph}"
    );
}

// A second sibling pair, `Volumes.Codex` / `Volumes.Codex<T>`, whose GENERIC
// file sorts before its bare file -- the opposite order from the Anthology
// pair above. A bare qualifier's own arity must bind its own sibling
// independent of which file the def index happens to walk first.
fn codex_fixture() -> Fixture {
    Fixture::with_files(&["CodexGeneric.cs", "CodexPlain.cs", "CodexConsumers.cs"])
}

#[test]
fn a_bare_member_qualifier_binds_its_own_sibling_independent_of_index_order() {
    let fx = codex_fixture();
    let graph = fx.graph();
    let edges = graph["edges"].as_array().unwrap();

    let precise_line_to_file = |line: i64| -> Option<&str> {
        edges
            .iter()
            .find(|e| {
                e["kind"] == "uses-member" && e["from_line"] == line && e.get("heuristic").is_none()
            })
            .and_then(|e| e["to_file"].as_str())
    };

    assert_eq!(
        precise_line_to_file(11),
        Some("CodexPlain.cs"),
        "the bare line must bind the arity-0 sibling even though the generic file's own is indexed first: {graph}"
    );
    assert_eq!(
        precise_line_to_file(12),
        Some("CodexGeneric.cs"),
        "the generic line must keep binding the arity-1 sibling: {graph}"
    );
}
