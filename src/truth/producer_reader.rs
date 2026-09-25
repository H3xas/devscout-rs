//! Reads actual producer output into observations, rather than hand-building
//! them from a manifest's own expectations.
//!
//! Three readers, each grounded in bytes already committed to this
//! repository: the Roslyn oracle's `refs.jsonl`/`units.jsonl`
//! (`fixtures/csharp-semantic/oracle/`), the flow-tracer's `facts.json`
//! (`fixtures/csharp-flowtrace/facts.json`), and devscout's own native
//! extract-and-resolve pipeline (`crate::extract`, `crate::graph`,
//! `crate::resolve`), run here exactly as `devscout map` runs it -- offline,
//! no compiler subprocess, no network. [`native_dispatch_implements_edges`]
//! is what grades the
//! `type-argument-pair-produces-implements-edges-on-a-clean-build` red row:
//! the edges it returns are not asserted, they are the resolver's own
//! output for the fixture it is handed.

use std::path::Path;

use serde::Deserialize;

use crate::graph::Edge;

/// One line of the Roslyn oracle's `refs.jsonl`.
///
/// A ground-truth reference the oracle exporter recorded for one
/// call/access site. Only the fields this harness grades against are kept;
/// the exporter's own richer shape is read past rather than mirrored field
/// for field.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OracleRef {
    /// The repository-relative source file.
    pub file: String,
    /// The one-based source line the exporter recorded.
    pub line: u32,
    /// The member name the reference resolved to.
    pub member: String,
    /// The resolved target's fully qualified name, when the exporter
    /// resolved one.
    pub target: Option<String>,
    /// Whether the exporter classified this reference as external.
    pub external: bool,
    /// Whether the exporter classified this reference as ambiguous.
    pub ambiguous: bool,
    /// Which compiled unit (project) this reference belongs to.
    pub unit: String,
}

/// Parses `refs.jsonl`: one [`OracleRef`] per non-empty line, in file order.
/// A malformed line aborts the parse and names which one.
pub fn read_oracle_refs(text: &str) -> Result<Vec<OracleRef>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|e| format!("refs.jsonl line {}: {e}", i + 1))
        })
        .collect()
}

/// One line of the Roslyn oracle's `units.jsonl`: one compiled project and
/// the build health the exporter observed for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OracleUnit {
    /// The project's name.
    pub name: String,
    /// The target framework moniker this unit was built for.
    pub tfm: String,
    /// The build status the exporter recorded (`"ok"` or otherwise).
    pub status: String,
    /// How many compiler diagnostics the exporter recorded for this unit.
    pub diagnostics: u32,
    /// Every source file this unit compiled.
    pub files: Vec<String>,
}

/// Parses `units.jsonl`: one [`OracleUnit`] per non-empty line.
pub fn read_oracle_units(text: &str) -> Result<Vec<OracleUnit>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|e| format!("units.jsonl line {}: {e}", i + 1))
        })
        .collect()
}

/// One flow-tracer fact from `facts.json`.
///
/// Shape varies by `kind` (`"type"` on the wire); every field the eight
/// registered fact kinds can carry is kept optional here rather than
/// modeled as one enum per kind, so a fact kind this reader does not
/// specifically grade against still parses rather than aborting the whole
/// document.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowtraceFact {
    /// The fact's kind (`"publish"`, `"consume"`, `"route"`,
    /// `"di_binding"`, `"message_class"`, `"iface_impl"`, `"ctor_field"`,
    /// `"method_span"`).
    #[serde(rename = "type")]
    pub kind: String,
    /// The repository-relative source file.
    pub file: String,
    /// The one-based source line the tracer recorded.
    pub line: u32,
    /// The declaring class, when this fact kind carries one.
    #[serde(default)]
    pub class: Option<String>,
    /// The message/event type name, for `publish`/`consume`/`message_class`
    /// facts.
    #[serde(default)]
    pub message: Option<String>,
    /// The fully qualified name the tracer resolved, when it carries one.
    #[serde(default)]
    pub fqn: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FlowtraceDocument {
    facts: Vec<FlowtraceFact>,
}

/// Parses `facts.json`'s flat `facts` array, dropping the header fields this
/// harness does not grade against.
pub fn read_flowtrace_facts(text: &str) -> Result<Vec<FlowtraceFact>, String> {
    let doc: FlowtraceDocument =
        serde_json::from_str(text).map_err(|e| format!("facts.json: {e}"))?;
    Ok(doc.facts)
}

/// Runs devscout's own native extract-and-resolve pipeline over one C#
/// source file -- the same two calls `devscout map` makes on every mapped
/// file -- and returns every `implements` edge it produced.
///
/// This is real analyzer output, not an assertion about what the resolver
/// would do: a caller grading the
/// `type-argument-pair-produces-implements-edges-on-a-clean-build` red row
/// against this function's result is grading the resolver's own decision,
/// the same one `devscout map` would persist to `graph.json` for this file.
pub fn native_dispatch_implements_edges(root: &Path, rel_path: &str, source: &str) -> Vec<Edge> {
    let extraction = crate::extract::extract(source);
    let fragment = crate::graph::fragment_from_extraction(&extraction);
    let fragments = vec![(rel_path.to_string(), fragment)];
    let graph = crate::resolve::resolve_graph(root, &fragments);
    graph
        .edges
        .into_iter()
        .filter(|e| matches!(e, Edge::Implements { .. }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REFS: &str = "{\"file\":\"src/App/Repo.cs\",\"line\":7,\"member\":\"Load\",\"target\":\"Fixture.Domain.Order\",\"external\":false,\"ambiguous\":false,\"unit\":\"App\"}\n";

    #[test]
    fn oracle_refs_parse_one_record_per_line() {
        let refs = read_oracle_refs(SAMPLE_REFS).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].member, "Load");
        assert!(!refs[0].external);
    }

    #[test]
    fn oracle_refs_skip_blank_lines_and_name_a_malformed_one() {
        let text = format!("{SAMPLE_REFS}\n{{not json}}\n");
        let err = read_oracle_refs(&text).unwrap_err();
        assert!(
            err.contains("line 3"),
            "error must name the bad line: {err}"
        );
    }

    #[test]
    fn oracle_units_parse_status_and_diagnostics() {
        let text = "{\"name\":\"App\",\"tfm\":\"net9.0\",\"status\":\"ok\",\"diagnostics\":0,\"files\":[\"src/App/Repo.cs\"]}\n";
        let units = read_oracle_units(text).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].status, "ok");
        assert_eq!(units[0].diagnostics, 0);
    }

    #[test]
    fn flowtrace_facts_parse_the_flat_facts_array() {
        let text = r#"{"schemaVersion":1,"facts":[{"type":"publish","file":"src/Api/Program.cs","line":25,"message":"ParcelDispatchedMessage","fqn":"Courier.Api.Messaging.Messages.ParcelDispatchedMessage"}]}"#;
        let facts = read_flowtrace_facts(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].kind, "publish");
        assert_eq!(facts[0].message.as_deref(), Some("ParcelDispatchedMessage"));
    }

    #[test]
    fn native_dispatch_reads_the_committed_oracle_and_flowtrace_bytes() {
        // Grounds this reader in the bytes already committed to the
        // repository, not just synthetic strings above.
        let refs_text =
            std::fs::read_to_string("fixtures/csharp-semantic/oracle/refs.jsonl").unwrap();
        let refs = read_oracle_refs(&refs_text).unwrap();
        assert!(refs.iter().any(|r| r.member == "Load" && !r.external));

        let units_text =
            std::fs::read_to_string("fixtures/csharp-semantic/oracle/units.jsonl").unwrap();
        let units = read_oracle_units(&units_text).unwrap();
        assert!(units.iter().any(|u| u.name == "App" && u.status == "ok"));

        let facts_text = std::fs::read_to_string("fixtures/csharp-flowtrace/facts.json").unwrap();
        let facts = read_flowtrace_facts(&facts_text).unwrap();
        assert!(facts.iter().any(|f| f.kind == "publish"));
    }

    #[test]
    fn native_dispatch_reproduces_the_counterexample_from_real_resolver_output() {
        let source =
            std::fs::read_to_string("fixtures/csharp-truth/src/NativeDispatchCounterexample.cs")
                .unwrap();
        let edges = native_dispatch_implements_edges(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            "src/NativeDispatchCounterexample.cs",
            &source,
        );
        assert!(
            !edges.is_empty(),
            "the two-type-argument generic call on an unrelated class must still \
             produce implements edges from today's resolver -- if this now emits \
             nothing, the fifth red-baseline row is stale and must be re-reviewed, \
             not silently kept"
        );
    }
}
