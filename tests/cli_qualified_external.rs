//! CLI coverage for fully qualified names whose qualifier is not in the
//! graph: a qualified reference like `RabbitMQ.Client.ExchangeType.Fanout`
//! no longer falls back to resolving by its bare last segment
//! (`ExchangeType.Fanout`) against an unrelated type the graph does know
//! about (`App.Transports.Fabric.ExchangeType`). A bare name reached through
//! a `using` directive is unaffected and still resolves precisely.

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
            "devscout-qualified-external-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-qualified-external");
        for name in [
            "ExchangeType.cs",
            "JsonSerializer.cs",
            "Member.cs",
            "Configure.cs",
            "Other.cs",
            "Binding.cs",
            "Adapter.cs",
            "DomainConsumer.cs",
            "LegitUser.cs",
        ] {
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

    fn graph(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Edges whose `from_file`/`from_line` match, regardless of `to`.
fn edges_at<'a>(
    graph: &'a serde_json::Value,
    from_file: &str,
    from_line: u64,
) -> Vec<&'a serde_json::Value> {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["from_file"] == from_file && e["from_line"].as_u64() == Some(from_line))
        .collect()
}

/// Edges whose `from_file`/`to` match, regardless of line -- used to prove a
/// destination is reached (or not reached) at all from a given file.
fn edges_to<'a>(
    graph: &'a serde_json::Value,
    from_file: &str,
    to: &str,
) -> Vec<&'a serde_json::Value> {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["from_file"] == from_file && e["to"] == to)
        .collect()
}

#[test]
fn qualified_names_with_unmapped_qualifiers_do_not_fall_back_to_bare_resolution() {
    let fixture = Fixture::new();
    let graph = fixture.graph();

    // Line 7: `RabbitMQ.Client.ExchangeType.Fanout`. `RabbitMQ.Client` is not
    // a namespace the graph knows, so this must NOT resolve -- neither
    // precisely nor heuristically -- to the local
    // `App.Transports.Fabric.ExchangeType.Fanout`, even though the bare last
    // segment matches exactly.
    assert!(
        edges_to(
            &graph,
            "Configure.cs",
            "App.Transports.Fabric.ExchangeType.Fanout"
        )
        .is_empty(),
        "a foreign qualifier must not fall back to a same-named local member: {graph:#}"
    );

    // Line 8: `App.Transports.Fabric.ExchangeType.Topic`. The qualifier IS a
    // known namespace, so this must resolve precisely (no `heuristic` key).
    let own = edges_at(&graph, "Configure.cs", 8);
    let own_precise: Vec<_> = own
        .iter()
        .filter(|e| {
            e["to"] == "App.Transports.Fabric.ExchangeType.Topic" && e["heuristic"].is_null()
        })
        .collect();
    assert_eq!(
        own_precise.len(),
        1,
        "expected exactly one precise uses-member edge at line 8: {own:#?}"
    );
    assert_eq!(own_precise[0]["kind"], "uses-member");

    // Line 9: `System.Text.Json.JsonSerializer.Serialize(...)`. `System.Text.Json`
    // is not a namespace the graph knows, so this must NOT resolve precisely
    // to the local `App.Infra.JsonSerializer`.
    let text_precise: Vec<_> = edges_at(&graph, "Configure.cs", 9)
        .into_iter()
        .filter(|e| e["to"] == "App.Infra.JsonSerializer" && e["heuristic"].is_null())
        .collect();
    assert!(
        text_precise.is_empty(),
        "a foreign qualifier must not resolve precisely to a same-named local type: {text_precise:#?}"
    );
    // Empirically (see fixtures/csharp-qualified-external and the graph this
    // fixture builds), the scored ("guess") tier is untouched by this rule: it
    // still names-match `Serialize` against the one type in the graph that
    // declares it, so a heuristic edge DOES land here. Pin that shape rather
    // than silently allow it to regress unnoticed.
    let text_heuristic: Vec<_> = edges_at(&graph, "Configure.cs", 9)
        .into_iter()
        .filter(|e| e["to"] == "App.Infra.JsonSerializer" && e["heuristic"] == true)
        .collect();
    assert_eq!(
        text_heuristic.len(),
        1,
        "expected exactly one heuristic edge at line 9 (scored tier, untouched by the qualified-name rule): {graph:#}"
    );
    assert_eq!(text_heuristic[0]["tier"], "guess");

    // Line 10: `App.Infra.JsonSerializer.Serialize(...)`. The qualifier IS a
    // known namespace, so this must resolve precisely.
    let local_precise: Vec<_> = edges_at(&graph, "Configure.cs", 10)
        .into_iter()
        .filter(|e| e["to"] == "App.Infra.JsonSerializer" && e["heuristic"].is_null())
        .collect();
    assert_eq!(
        local_precise.len(),
        1,
        "expected exactly one precise uses-member edge at line 10: {graph:#}"
    );
    assert_eq!(local_precise[0]["kind"], "uses-member");

    // Line 11: `expr.Member.Name`. `expr` is a
    // `System.Linq.Expressions.MemberExpression`, an external BCL type the
    // graph never declares, so `.Member` must not resolve precisely to the
    // local `App.Model.Member`.
    let member_precise: Vec<_> = edges_at(&graph, "Configure.cs", 11)
        .into_iter()
        .filter(|e| e["to"] == "App.Model.Member" && e["heuristic"].is_null())
        .collect();
    assert!(
        member_precise.is_empty(),
        "a receiver of unresolved external type must not resolve precisely to a same-named local type: {member_precise:#?}"
    );
    // Empirically, the scored tier still fires here too (same reasoning as
    // line 9): `Member` name-matches the one type in the graph that declares
    // it, so a heuristic edge lands, tier "guess".
    let member_heuristic: Vec<_> = edges_at(&graph, "Configure.cs", 11)
        .into_iter()
        .filter(|e| e["to"] == "App.Model.Member" && e["heuristic"] == true)
        .collect();
    assert_eq!(
        member_heuristic.len(),
        1,
        "expected exactly one heuristic edge at line 11 (scored tier, untouched by the qualified-name rule): {graph:#}"
    );
    assert_eq!(member_heuristic[0]["tier"], "guess");

    // Other.cs: a bare `ExchangeType` reached through `using
    // App.Transports.Fabric;` is unaffected by the qualified-name rule and
    // still resolves precisely, both as the member access and as the
    // return-type usage.
    let other_member: Vec<_> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["from_file"] == "Other.cs"
                && e["kind"] == "uses-member"
                && e["to"] == "App.Transports.Fabric.ExchangeType.Fanout"
                && e["heuristic"].is_null()
        })
        .collect();
    assert_eq!(
        other_member.len(),
        1,
        "expected exactly one precise uses-member edge from Other.cs: {graph:#}"
    );

    let other_type: Vec<_> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["from_file"] == "Other.cs"
                && e["kind"] == "uses-type"
                && e["to"] == "App.Transports.Fabric.ExchangeType"
        })
        .collect();
    assert_eq!(
        other_type.len(),
        1,
        "expected a uses-type edge for the ExchangeType return type: {graph:#}"
    );

    assert_eq!(graph["stats"]["ambiguous_count"], 0);
}

#[test]
fn a_bare_qualifier_found_only_by_global_name_uniqueness_earns_no_precise_member_edge() {
    let fixture = Fixture::new();
    let graph = fixture.graph();

    // Adapter.cs imports only `App.Vendor`, a namespace no fixture file
    // declares, and never brings `App.Domain` into scope by any other
    // means. `Binding` is nonetheless the only in-tree def of that bare
    // name, so the old ladder answered it at the global-uniqueness step --
    // a rule C# does not have. Line 9 (`Binding.Stitched`, a member the
    // enum does not declare) and line 10 (`Binding.Loose`, a member it
    // does) must both stay external; the fix does not distinguish them,
    // because neither ever legally reaches `App.Domain` at all.
    assert!(
        edges_at(&graph, "Adapter.cs", 9).is_empty(),
        "a global-uniqueness-only qualifier must not earn a member edge for an undeclared member: {graph:#}"
    );
    assert!(
        edges_at(&graph, "Adapter.cs", 10).is_empty(),
        "a global-uniqueness-only qualifier must not earn a member edge even for a member the enum declares: {graph:#}"
    );

    // DomainConsumer.cs reaches the same enum through the ancestor-namespace
    // step (it shares App.Domain with the declaration), which this fix does
    // not touch, so its own `Binding.Loose` at line 7 must keep its precise
    // edge -- the guard that steps 2 and 3 are unaffected.
    let guard: Vec<_> = edges_at(&graph, "DomainConsumer.cs", 7)
        .into_iter()
        .filter(|e| e["to"] == "App.Domain.Binding.Loose" && e["heuristic"].is_null())
        .collect();
    assert_eq!(
        guard.len(),
        1,
        "same-namespace access to the enum must still resolve precisely: {graph:#}"
    );
}

#[test]
fn an_enum_qualifier_earns_no_precise_edge_for_a_member_the_enum_does_not_declare() {
    let fixture = Fixture::new();
    let graph = fixture.graph();

    // LegitUser.cs reaches `Binding` legitimately through `using
    // App.Domain;` (step 2), never through global uniqueness -- but the
    // enum does not declare `Bogus`, so line 9 must earn no edge at all
    // rather than fall back to an edge on the enum type itself.
    assert!(
        edges_at(&graph, "LegitUser.cs", 9).is_empty(),
        "an enum member the enum does not declare must not fall back to an edge on the enum type: {graph:#}"
    );
}

#[test]
fn refs_for_the_qualified_enum_member_excludes_the_foreign_qualifier_site() {
    let fixture = Fixture::new();

    let refs = fixture.ok(&["refs", "App.Transports.Fabric.ExchangeType.Fanout"]);
    assert!(
        !refs.contains("Configure.cs"),
        "the RabbitMQ.Client-qualified reference must not show up as a ref: {refs}"
    );
    assert!(
        refs.contains("Other.cs"),
        "the using-qualified bare reference must still show up as a ref: {refs}"
    );
}
