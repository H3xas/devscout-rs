//! The export's fact views: one struct per family, one line each.
//!
//! A view is deliberately not a storage type. The index's own structs carry
//! fields the export does not publish and omit the two the export must always
//! state (`family` and `evidence`), and a view keeps the contract's own key
//! order stable where a storage struct's order is free to move. Field
//! declaration order IS the serialized key order, and the two leading keys are
//! always `family` then `evidence`.
//!
//! Nothing here resolves anything: every field is read off what the index
//! records, so the export can state the same facts the query verbs answer from
//! without becoming a second resolver.

use serde::Serialize;

use crate::graph::Def;
use crate::graph::{AlsoIn, Candidate, Edge, FragEnclosingCall, FragHandlerRegistration};
use crate::graph::{HeuristicTier, SemanticProvenance};
use crate::query::TestVia;

use super::evidence;
use super::snapshot::Snapshot;

/// The families in canonical order, which is also the order of the trailer's
/// counts and of the coverage table.
pub(crate) const FAMILY_ORDER: [&str; 7] = [
    "definition",
    "edge",
    "publish-site",
    "handler-registration",
    "di-registration",
    "unresolved-import",
    "test-file",
];

/// One serialized fact line plus the three keys the canonical order sorts on.
pub(crate) struct Fact {
    /// The family's index in [`FAMILY_ORDER`].
    pub(crate) rank: usize,
    /// The file the fact is recorded in.
    pub(crate) file: String,
    /// The recorded 1-based line, or 0 for a fact the index records none for.
    pub(crate) line: usize,
    /// The compact JSON line, without its terminating newline.
    pub(crate) json: String,
}

impl Fact {
    fn new(rank: usize, file: String, line: usize, line_struct: &impl Serialize) -> Fact {
        Fact {
            rank,
            file,
            line,
            json: serde_json::to_string(line_struct).expect("a fact line always serializes"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DefinitionLine {
    family: &'static str,
    evidence: &'static str,
    id: String,
    name: String,
    namespace: String,
    kind: String,
    file: String,
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
    methods: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    test_methods: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    also_in: Vec<AlsoIn>,
}

/// One row of an `ambiguous` or `ctor-di` edge's tie list.
#[derive(Serialize)]
struct CandidateRow {
    id: String,
    file: String,
}

impl From<&Candidate> for CandidateRow {
    fn from(c: &Candidate) -> CandidateRow {
        CandidateRow {
            id: c.id.clone(),
            file: c.file.clone(),
        }
    }
}

// One row shape for every edge kind. A kind sets the fields it records and
// leaves the rest absent, which is what keeps one line grammar for all of
// them. The declaration order below IS the contract's canonical key order,
// documented beside the verb.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EdgeLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    line: usize,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tier: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heuristic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    member: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overload_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    via: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bound_defs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<CandidateRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    // The bus-hop variant's own `evidence` word, which the line's leading
    // `evidence` key has already taken: the recorded word names the consumer
    // shape the hop was established through, not the fact's evidence class.
    #[serde(skip_serializing_if = "Option::is_none")]
    hop_evidence: Option<String>,
}

impl EdgeLine {
    fn new(evidence: &'static str, kind: &'static str, file: String, line: usize) -> EdgeLine {
        EdgeLine {
            family: "edge",
            evidence,
            file,
            line,
            kind,
            to: None,
            to_file: None,
            tier: None,
            heuristic: None,
            source: None,
            member: None,
            overload_signature: None,
            target: None,
            via: None,
            bound_defs: Vec::new(),
            origin: None,
            raw: None,
            candidates: Vec::new(),
            candidate_count: None,
            iface: None,
            resolution: None,
            args: Vec::new(),
            message: None,
            hop_evidence: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    line: usize,
    verb: String,
    message: String,
    namespace: String,
    #[serde(rename = "outerTypes", skip_serializing_if = "Vec::is_empty")]
    outer_types: Vec<String>,
    #[serde(rename = "enclosingMethod", skip_serializing_if = "Option::is_none")]
    enclosing_method: Option<String>,
    #[serde(rename = "argCount", skip_serializing_if = "is_zero")]
    arg_count: usize,
    #[serde(rename = "enclosingCall", skip_serializing_if = "Option::is_none")]
    enclosing_call: Option<FragEnclosingCall>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HandlerRegistrationLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    line: usize,
    handler: String,
    namespace: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiRegistrationLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    line: usize,
    service: String,
    implementation: String,
    namespace: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnresolvedImportLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    line: usize,
    target: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TestFileLine {
    family: &'static str,
    evidence: &'static str,
    file: String,
    via: &'static str,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

// `0` is the index's own "no end line recorded" value, so it has to become an
// absent key rather than a line number no file has.
fn end_line_of(def: &Def) -> Option<usize> {
    (def.end_line > 0).then_some(def.end_line)
}

fn tier_word(tier: HeuristicTier) -> &'static str {
    match tier {
        HeuristicTier::Ext => "ext",
        HeuristicTier::Guess => "guess",
    }
}

fn source_word(source: SemanticProvenance) -> &'static str {
    match source {
        SemanticProvenance::Semantic => "semantic",
        SemanticProvenance::SemanticDiscovered => "semantic-discovered",
    }
}

// The edge view is built in three small passes rather than one long match: the
// position every kind carries, the kind's own word, and the fields that kind
// adds. Each match is exhaustive with no wildcard arm, so a new `Edge` variant
// fails to compile until all three know about it.
fn edge_position(edge: &Edge) -> (&str, usize) {
    match edge {
        Edge::Inherits {
            from_file,
            from_line,
            ..
        }
        | Edge::UsesType {
            from_file,
            from_line,
            ..
        }
        | Edge::UsesMember {
            from_file,
            from_line,
            ..
        }
        | Edge::Imports {
            from_file,
            from_line,
            ..
        }
        | Edge::Ambiguous {
            from_file,
            from_line,
            ..
        }
        | Edge::CtorDi {
            from_file,
            from_line,
            ..
        }
        | Edge::Import {
            from_file,
            from_line,
            ..
        }
        | Edge::Call {
            from_file,
            from_line,
            ..
        }
        | Edge::JsxUse {
            from_file,
            from_line,
            ..
        }
        | Edge::Dispatch {
            from_file,
            from_line,
            ..
        }
        | Edge::Implements {
            from_file,
            from_line,
            ..
        }
        | Edge::Overrides {
            from_file,
            from_line,
            ..
        }
        | Edge::BusHop {
            from_file,
            from_line,
            ..
        } => (from_file, *from_line),
    }
}

fn edge_kind(edge: &Edge) -> &'static str {
    match edge {
        Edge::Inherits { .. } => "inherits",
        Edge::UsesType { .. } => "uses-type",
        Edge::UsesMember { .. } => "uses-member",
        Edge::Imports { .. } => "imports",
        Edge::Ambiguous { .. } => "ambiguous",
        Edge::CtorDi { .. } => "ctor-di",
        Edge::Import { .. } => "import",
        Edge::Call { .. } => "call",
        Edge::JsxUse { .. } => "jsx-use",
        Edge::Dispatch { .. } => "dispatch",
        Edge::Implements { .. } => "implements",
        Edge::Overrides { .. } => "overrides",
        Edge::BusHop { .. } => "bus-hop",
    }
}

/// The target pair most kinds carry, set in one place so the arms below stay
/// short.
fn set_target(row: &mut EdgeLine, to: &str, to_file: &str) {
    row.to = Some(to.to_string());
    row.to_file = Some(to_file.to_string());
}

fn fill_kind_fields(edge: &Edge, row: &mut EdgeLine) {
    match edge {
        Edge::Inherits {
            to,
            to_file,
            heuristic,
            ..
        }
        | Edge::UsesType {
            to,
            to_file,
            heuristic,
            ..
        } => {
            set_target(row, to, to_file);
            row.heuristic = Some(*heuristic);
        }
        Edge::UsesMember {
            to,
            to_file,
            heuristic,
            tier,
            member,
            source,
            overload_signature,
            ..
        } => set_uses_member(
            row,
            (to, to_file),
            (*heuristic, *tier),
            (member, *source, overload_signature),
        ),
        Edge::Imports { target, .. } => row.target = Some(target.clone()),
        Edge::Ambiguous {
            origin,
            raw,
            candidates,
            candidate_count,
            ..
        } => set_ambiguous(row, origin, raw, candidates, *candidate_count),
        Edge::CtorDi {
            iface,
            resolution,
            args,
            to,
            candidates,
            ..
        } => set_ctor_di(row, iface, resolution, args, to, candidates),
        Edge::Import {
            target,
            to_file,
            via,
            bound_defs,
            ..
        } => {
            row.target = Some(target.clone());
            row.to_file = Some(to_file.clone());
            row.via = via.clone();
            row.bound_defs = bound_defs.clone();
        }
        Edge::Call { to, to_file, .. }
        | Edge::JsxUse { to, to_file, .. }
        | Edge::Dispatch { to, to_file, .. } => set_target(row, to, to_file),
        Edge::Implements {
            to,
            to_file,
            member,
            ..
        }
        | Edge::Overrides {
            to,
            to_file,
            member,
            ..
        } => {
            set_target(row, to, to_file);
            row.member = member.clone();
        }
        Edge::BusHop {
            message,
            to,
            to_file,
            evidence,
            ..
        } => {
            row.message = Some(message.clone());
            set_target(row, to, to_file);
            row.hop_evidence = Some(evidence.clone());
        }
    }
}

// The kinds whose arm would otherwise be the longest in the match. Each takes
// the fields the sub-pattern bound, so the dispatch itself stays one line per
// kind.
fn set_uses_member(
    row: &mut EdgeLine,
    target: (&String, &String),
    guess: (bool, Option<HeuristicTier>),
    fields: (&Option<String>, Option<SemanticProvenance>, &Option<String>),
) {
    set_target(row, target.0, target.1);
    row.heuristic = Some(guess.0);
    row.tier = guess.1.map(tier_word);
    row.source = fields.1.map(source_word);
    row.member = fields.0.clone();
    row.overload_signature = fields.2.clone();
}

fn set_ambiguous(
    row: &mut EdgeLine,
    origin: &str,
    raw: &str,
    candidates: &[Candidate],
    candidate_count: usize,
) {
    row.origin = Some(origin.to_string());
    row.raw = Some(raw.to_string());
    row.candidates = candidates.iter().map(CandidateRow::from).collect();
    row.candidate_count = Some(candidate_count);
}

fn set_ctor_di(
    row: &mut EdgeLine,
    iface: &str,
    resolution: &str,
    args: &Option<Vec<String>>,
    to: &Option<String>,
    candidates: &[Candidate],
) {
    row.iface = Some(iface.to_string());
    row.resolution = Some(resolution.to_string());
    row.args = args.clone().unwrap_or_default();
    row.to = to.clone();
    row.candidates = candidates.iter().map(CandidateRow::from).collect();
}

fn edge_line(edge: &Edge) -> Fact {
    let evidence = evidence::edge_evidence(edge);
    let (file, line) = edge_position(edge);
    let mut row = EdgeLine::new(evidence, edge_kind(edge), file.to_string(), line);
    fill_kind_fields(edge, &mut row);
    Fact::new(1, row.file.clone(), row.line, &row)
}

fn test_via_word(via: TestVia) -> &'static str {
    match via {
        TestVia::Attribute => "attribute",
        TestVia::Project => "project",
        TestVia::Path => "path",
    }
}

/// Every fact the snapshot records, one line each, in recorded order. Canonical
/// order is the stream's job; this walks the index's own order so nothing here
/// has to know how a consumer sorts.
pub(crate) fn collect(snapshot: &Snapshot) -> Vec<Fact> {
    let mut facts = Vec::with_capacity(
        snapshot.graph.defs.len()
            + snapshot.graph.edges.len()
            + snapshot.graph.unresolved_imports.len()
            + snapshot.test_via.len(),
    );
    collect_definitions(snapshot, &mut facts);
    collect_edges(snapshot, &mut facts);
    collect_messaging(snapshot, &mut facts);
    collect_unresolved_imports(snapshot, &mut facts);
    collect_test_files(snapshot, &mut facts);
    facts
}

fn collect_definitions(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for def in &snapshot.graph.defs {
        facts.push(Fact::new(
            0,
            def.file.clone(),
            def.line,
            &DefinitionLine {
                family: "definition",
                evidence: evidence::DECLARED,
                id: def.id.clone(),
                name: def.name.clone(),
                namespace: def.namespace.clone(),
                kind: def.kind.clone(),
                file: def.file.clone(),
                line: def.line,
                end_line: end_line_of(def),
                methods: def.methods.clone(),
                test_methods: def.test_methods.clone(),
                also_in: def.also_in.clone(),
            },
        ));
    }
}

fn collect_edges(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for edge in &snapshot.graph.edges {
        facts.push(edge_line(edge));
    }
}

// The messaging families, one line per recorded fact in the cache's own file
// order. A TypeScript or Rust entry carries none of the three arrays.
fn collect_messaging(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for (file, entry) in &snapshot.messaging {
        for publish in &entry.publishes {
            facts.push(Fact::new(
                2,
                file.clone(),
                publish.line,
                &PublishLine {
                    family: "publish-site",
                    evidence: evidence::DECLARED,
                    file: file.clone(),
                    line: publish.line,
                    verb: publish.verb.clone(),
                    message: publish.message.clone(),
                    namespace: publish.namespace.clone(),
                    outer_types: publish.outer_types.clone(),
                    enclosing_method: publish.enclosing_method.clone(),
                    arg_count: publish.arg_count,
                    enclosing_call: publish.enclosing_call.clone(),
                },
            ));
        }
        for registration in &entry.handler_registrations {
            facts.push(handler_registration_fact(file, registration));
        }
        for registration in &entry.registrations {
            facts.push(Fact::new(
                4,
                file.clone(),
                registration.line,
                &DiRegistrationLine {
                    family: "di-registration",
                    evidence: evidence::DECLARED,
                    file: file.clone(),
                    line: registration.line,
                    service: registration.service.clone(),
                    implementation: registration.implementation.clone(),
                    namespace: registration.namespace.clone(),
                },
            ));
        }
    }
}

fn collect_unresolved_imports(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for import in &snapshot.graph.unresolved_imports {
        facts.push(Fact::new(
            5,
            import.from_file.clone(),
            import.from_line,
            &UnresolvedImportLine {
                family: "unresolved-import",
                evidence: evidence::UNRESOLVED,
                file: import.from_file.clone(),
                line: import.from_line,
                target: import.target.clone(),
                reason: import.reason.clone(),
            },
        ));
    }
}

// One line per file the classification vouches for; a file it does not classify
// gets no line at all, which is what tells "not a test file" from "no data".
fn collect_test_files(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for (file, via) in &snapshot.test_via {
        let evidence = match via {
            TestVia::Attribute | TestVia::Project => evidence::DECLARED,
            TestVia::Path => evidence::CONVENTION,
        };
        facts.push(Fact::new(
            6,
            file.clone(),
            0,
            &TestFileLine {
                family: "test-file",
                evidence,
                file: file.clone(),
                via: test_via_word(*via),
            },
        ));
    }
}

fn handler_registration_fact(file: &str, registration: &FragHandlerRegistration) -> Fact {
    Fact::new(
        3,
        file.to_string(),
        registration.line,
        &HandlerRegistrationLine {
            family: "handler-registration",
            evidence: evidence::DECLARED,
            file: file.to_string(),
            line: registration.line,
            handler: registration.handler.clone(),
            namespace: registration.namespace.clone(),
        },
    )
}
