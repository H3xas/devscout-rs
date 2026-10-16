//! The negative-binding fixture's sealed partition.
//!
//! Twenty member-reference sites whose labels were written from the C# rules
//! and sealed against the compiler oracle before devscout output was seen. The
//! subject is the syntax-only graph, so the compiler is never both witness and
//! subject, and a resolver change that recovers more references at the cost of
//! a wrong precise target shows up here instead of as a silent trade. Every
//! run reports the whole partition: the denominator is the point, so a case
//! that starts failing stays in the count rather than being dropped from it.
//!
//! The table is printed on every run; `cargo test --test negative_binding --
//! --nocapture` shows it on a passing one, and a failure repeats it in the
//! message.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

const FIXTURE: &str = "fixtures/csharp-negative-binding";
const LABELS: &str = "labels.json";
const GATE: &str = "gate.json";
const OBSERVED: &str = "observed.json";
const ORACLE: &str = "compiler-facts.json";

const RESOLVABLE_CATEGORIES: &[&str] = &[
    "namespace-shadowing",
    "receiver-overload",
    "extension-instance",
    "generic-binding",
];

static NEXT: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn fixture_dir() -> PathBuf {
    repo_root().join(FIXTURE)
}

fn fixture_text(name: &str) -> String {
    let path = fixture_dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Intent {
    Resolvable,
    Negative,
}

#[derive(Clone, Debug)]
struct Case {
    id: String,
    category: String,
    intent: Intent,
    file: String,
    line: u64,
    member: String,
    declaring_type: String,
    overload_parameters: Vec<String>,
    plausible_wrong: String,
    oracle_state: String,
    diagnostic: String,
    accepted_outcomes: Vec<String>,
}

fn require_str(object: &Value, key: &str, where_: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{where_}: '{key}' is missing or not a string"))
}

fn optional_str(object: &Value, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn require_array(object: &Value, key: &str, where_: &str) -> Result<Vec<String>, String> {
    let items = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{where_}: '{key}' is missing or not an array"))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{where_}: '{key}' holds a non-string entry"))
        })
        .collect()
}

/// A manifest that does not parse into this shape is refused whole, naming the
/// input — a case with a hole in it must never reach the denominator.
fn parse_labels(text: &str) -> Result<Vec<Case>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("{LABELS}: {e}"))?;
    let entries = doc
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{LABELS}: 'cases' is missing or not an array"))?;
    let mut cases = Vec::new();
    for entry in entries {
        let id = require_str(entry, "id", LABELS)?;
        let where_ = format!("{LABELS} case '{id}'");
        let site = entry
            .get("site")
            .ok_or_else(|| format!("{where_}: 'site' is missing"))?;
        let intent = require_str(entry, "intent", &where_)?;
        let intent = match intent.as_str() {
            "resolvable" => Intent::Resolvable,
            "negative" => Intent::Negative,
            other => return Err(format!("{where_}: unknown intent '{other}'")),
        };
        let case = Case {
            id,
            category: require_str(entry, "category", &where_)?,
            intent,
            file: require_str(site, "file", &where_)?,
            line: site
                .get("line")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("{where_}: 'site.line' is missing or not a number"))?,
            member: require_str(site, "member", &where_)?,
            // The two intents carry different fields: a resolvable case names
            // the target it must reach, a negative one names what the compiler
            // reports instead of a target.
            declaring_type: match intent {
                Intent::Resolvable => require_str(entry, "declaringType", &where_)?,
                Intent::Negative => optional_str(entry, "declaringType"),
            },
            overload_parameters: match intent {
                Intent::Resolvable => require_array(entry, "overloadParameters", &where_)?,
                Intent::Negative => Vec::new(),
            },
            plausible_wrong: match intent {
                Intent::Resolvable => require_str(entry, "plausibleWrong", &where_)?,
                Intent::Negative => optional_str(entry, "plausibleWrong"),
            },
            oracle_state: match intent {
                Intent::Negative => require_str(entry, "oracleState", &where_)?,
                Intent::Resolvable => optional_str(entry, "oracleState"),
            },
            diagnostic: match intent {
                Intent::Negative => require_str(entry, "diagnostic", &where_)?,
                Intent::Resolvable => optional_str(entry, "diagnostic"),
            },
            accepted_outcomes: match intent {
                Intent::Negative => require_array(entry, "acceptedOutcomes", &where_)?,
                Intent::Resolvable => Vec::new(),
            },
        };
        cases.push(case);
    }
    Ok(cases)
}

// ---------------------------------------------------------------------------
// Partition vocabulary
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tier {
    Precise,
    Ext,
    Guess,
}

impl Tier {
    fn as_str(self) -> &'static str {
        match self {
            Tier::Precise => "precise",
            Tier::Ext => "ext",
            Tier::Guess => "guess",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    PreciseCorrect,
    PreciseWrong,
    Tiered,
    Absent,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::PreciseCorrect => "precise-correct",
            Outcome::PreciseWrong => "precise-wrong",
            Outcome::Tiered => "tiered",
            Outcome::Absent => "absent",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Edge {
    target: String,
    tier: Tier,
}

#[derive(Clone, Debug)]
struct Row {
    id: String,
    category: String,
    outcome: Outcome,
    edges: Vec<Edge>,
}

/// The parameter list convention the oracle writes: `(a,b)->r`. A signature
/// this shape cannot be read from is refused rather than compared loosely.
fn signature_parameters(signature: &str) -> Option<Vec<String>> {
    let open = signature.find('(')?;
    let close = signature[open..].find(')')? + open;
    let inner = &signature[open + 1..close];
    if inner.is_empty() {
        return Some(Vec::new());
    }
    Some(inner.split(',').map(|p| p.trim().to_string()).collect())
}

/// The identifier starting at a UTF-16 code-unit column, matching the span
/// encoding the oracle writes so a line with non-ASCII text still keys.
fn identifier_at(line: &str, column: usize) -> Option<String> {
    let mut utf16 = 0usize;
    for (byte, ch) in line.char_indices() {
        if utf16 == column {
            let rest = &line[byte..];
            let mut chars = rest.chars();
            let first = chars.next()?;
            if !(first.is_alphabetic() || first == '_') {
                return None;
            }
            let mut name = String::from(first);
            for ch in chars {
                if ch.is_alphanumeric() || ch == '_' {
                    name.push(ch);
                } else {
                    break;
                }
            }
            return Some(name);
        }
        if utf16 > column {
            return None;
        }
        utf16 += ch.len_utf16();
    }
    None
}

fn source_line(case: &Case) -> Result<String, String> {
    let path = fixture_dir().join(&case.file);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .nth(case.line as usize - 1)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "{} case '{}': line {} does not exist",
                case.file, case.id, case.line
            )
        })
}

// ---------------------------------------------------------------------------
// The graph
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Workspace {
    root: PathBuf,
    registry: PathBuf,
}

impl Workspace {
    /// A fresh copy of the fixture, so a run never shares state with another
    /// and the committed tree is never written to.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-negative-binding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        for entry in fs::read_dir(fixture_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                fs::copy(&path, root.join(path.file_name().unwrap())).unwrap();
            }
        }
        let registry = root.join("registry.json");
        let workspace = Self { root, registry };
        workspace.ok(&["init", "--no-hooks"]);
        workspace.ok(&["map", "."]);
        workspace
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .env("SCOUT_CONTENT_DB", self.root.join("content.db"))
            .env("HOME", &self.root)
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

    fn graph(&self) -> Result<Value, String> {
        let path = self.root.join(".scout/graph/graph.json");
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let graph: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if graph.get("edges").and_then(Value::as_array).is_none()
            || graph.get("defs").and_then(Value::as_array).is_none()
        {
            return Err(format!("{}: no 'defs' and 'edges' arrays", path.display()));
        }
        Ok(graph)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn graph_files(graph: &Value) -> Vec<String> {
    graph["defs"]
        .as_array()
        .map(|defs| {
            defs.iter()
                .filter_map(|def| def.get("file").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `uses-member` edges at one site, keyed by file, line and member name.
fn site_edges(graph: &Value, case: &Case) -> Result<Vec<Edge>, String> {
    let graph_name = ".scout/graph/graph.json";
    if !graph_files(graph).iter().any(|file| *file == case.file) {
        return Err(format!(
            "{graph_name} case '{}': {} is not in the graph",
            case.id, case.file
        ));
    }
    let mut edges = Vec::new();
    for edge in graph["edges"].as_array().unwrap() {
        if edge.get("kind").and_then(Value::as_str) != Some("uses-member") {
            continue;
        }
        if edge.get("from_file").and_then(Value::as_str) != Some(case.file.as_str())
            || edge.get("from_line").and_then(Value::as_u64) != Some(case.line)
            || edge.get("member").and_then(Value::as_str) != Some(case.member.as_str())
        {
            continue;
        }
        let target = edge
            .get("to")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "{graph_name} case '{}': an edge at {}:{} has no target",
                    case.id, case.file, case.line
                )
            })?
            .to_string();
        let heuristic = edge
            .get("heuristic")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let tier = match edge.get("tier").and_then(Value::as_str) {
            None | Some("ext") if !heuristic => Tier::Precise,
            Some("ext") => Tier::Ext,
            Some("guess") => Tier::Guess,
            None => Tier::Precise,
            Some(other) => {
                return Err(format!(
                    "{graph_name} case '{}': unknown tier '{other}'",
                    case.id
                ))
            }
        };
        if let Some(recorded) = edge.get("overload_signature").and_then(Value::as_str) {
            if let Some(parameters) = signature_parameters(recorded) {
                if !case.overload_parameters.is_empty() && parameters != case.overload_parameters {
                    edges.push(Edge {
                        target: format!("{target} {}", recorded),
                        tier: tier,
                    });
                    continue;
                }
            }
        }
        edges.push(Edge { target, tier });
    }
    Ok(edges)
}

fn targets_at(edges: &[Edge], tiers: &[Tier]) -> Vec<String> {
    let mut targets: Vec<String> = edges
        .iter()
        .filter(|edge| tiers.contains(&edge.tier))
        .map(|edge| edge.target.clone())
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

fn classify(case: &Case, edges: &[Edge]) -> Outcome {
    let precise = targets_at(edges, &[Tier::Precise]);
    let wrong = precise.iter().any(|target| target != &case.declaring_type);
    match case.intent {
        // A negative case has no precise target to be right about, so any
        // precise edge at its site is a wrong one.
        Intent::Negative => {
            if !precise.is_empty() {
                Outcome::PreciseWrong
            } else if !edges.is_empty() {
                Outcome::Tiered
            } else {
                Outcome::Absent
            }
        }
        Intent::Resolvable => {
            if wrong {
                Outcome::PreciseWrong
            } else if !precise.is_empty() {
                Outcome::PreciseCorrect
            } else if !edges.is_empty() {
                Outcome::Tiered
            } else {
                Outcome::Absent
            }
        }
    }
}

fn partition(cases: &[Case], graph: &Value) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for case in cases {
        let edges = site_edges(graph, case)?;
        rows.push(Row {
            id: case.id.clone(),
            category: case.category.clone(),
            outcome: classify(case, &edges),
            edges,
        });
    }
    Ok(rows)
}

/// Resolvable cases whose site carries an edge to the labelled target at the
/// precise or extension tier. `guess` is excluded: it is not a binding.
fn recovered(cases: &[Case], rows: &[Row]) -> usize {
    cases
        .iter()
        .filter(|case| case.intent == Intent::Resolvable)
        .zip(rows)
        .filter(|(case, row)| {
            row.edges
                .iter()
                .any(|edge| edge.target == case.declaring_type && edge.tier != Tier::Guess)
        })
        .count()
}

fn precise_only(cases: &[Case], rows: &[Row]) -> usize {
    cases
        .iter()
        .filter(|case| case.intent == Intent::Resolvable)
        .zip(rows)
        .filter(|(case, row)| {
            row.edges
                .iter()
                .any(|edge| edge.target == case.declaring_type && edge.tier == Tier::Precise)
        })
        .count()
}

fn partition_json(cases: &[Case], rows: &[Row]) -> String {
    let outcomes: Vec<Value> = [
        Outcome::PreciseCorrect,
        Outcome::PreciseWrong,
        Outcome::Tiered,
        Outcome::Absent,
    ]
    .iter()
    .map(|outcome| {
        json!({
            "outcome": outcome.as_str(),
            "count": rows.iter().filter(|row| row.outcome == *outcome).count(),
        })
    })
    .collect();
    let entries: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "category": row.category,
                "outcome": row.outcome.as_str(),
                "edges": row
                    .edges
                    .iter()
                    .map(|edge| json!({ "target": edge.target, "tier": edge.tier.as_str() }))
                    .collect::<Vec<Value>>(),
            })
        })
        .collect();
    let doc = json!({
        "contract": "csharp-negative-binding-observed-v1",
        "cases": entries,
        "totals": {
            "cases": rows.len(),
            "resolvable": cases.iter().filter(|c| c.intent == Intent::Resolvable).count(),
            "negative": cases.iter().filter(|c| c.intent == Intent::Negative).count(),
            "recovered": recovered(cases, rows),
            "preciseOnly": precise_only(cases, rows),
            "byOutcome": outcomes,
        },
    });
    let mut out = serde_json::to_string_pretty(&doc).unwrap();
    out.push('\n');
    out
}

/// Every row is printed, passing or failing, because the denominator is the
/// claim this fixture makes: a case that starts failing is never dropped from
/// the report to make the totals look better.
fn partition_table(cases: &[Case], rows: &[Row]) -> String {
    let mut lines = vec![format!(
        "{:<46} {:<19} {:<14} {}",
        "case", "category", "outcome", "targets"
    )];
    for (case, row) in cases.iter().zip(rows) {
        let targets = targets_at(&row.edges, &[Tier::Precise, Tier::Ext, Tier::Guess]);
        lines.push(format!(
            "{:<46} {:<19} {:<14} {}",
            case.id,
            row.category,
            row.outcome.as_str(),
            if targets.is_empty() {
                "-".to_string()
            } else {
                targets.join(", ")
            }
        ));
        if case.intent == Intent::Resolvable && row.outcome != Outcome::PreciseCorrect {
            lines.push(format!("  labelled target: {}", case.declaring_type));
        }
    }
    let resolvable = cases
        .iter()
        .filter(|c| c.intent == Intent::Resolvable)
        .count();
    lines.push(format!(
        "denominator {} cases ({} resolvable, {} negative); recovered {}/{resolvable}; precise-only {}",
        rows.len(),
        resolvable,
        cases.iter().filter(|c| c.intent == Intent::Negative).count(),
        recovered(cases, rows),
        precise_only(cases, rows),
    ));
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// The ratchet
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Deviation {
    case: String,
    outcome: String,
}

#[derive(Clone, Debug)]
struct Gate {
    recall_floor: usize,
    deviations: Vec<Deviation>,
}

fn parse_gate(text: &str) -> Result<Gate, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("{GATE}: {e}"))?;
    let recall_floor = doc
        .get("recallFloor")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{GATE}: 'recallFloor' is missing or not a number"))?
        as usize;
    let entries = doc
        .get("deviations")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{GATE}: 'deviations' is missing or not an array"))?;
    let mut deviations = Vec::new();
    for entry in entries {
        let case = require_str(entry, "case", GATE)?;
        deviations.push(Deviation {
            outcome: require_str(entry, "outcome", &format!("{GATE} deviation '{case}'"))?,
            case,
        });
    }
    Ok(Gate {
        recall_floor,
        deviations,
    })
}

fn parse_observed(text: &str) -> Result<Value, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("{OBSERVED}: {e}"))?;
    if doc.get("cases").and_then(Value::as_array).is_none() {
        return Err(format!("{OBSERVED}: 'cases' is missing or not an array"));
    }
    Ok(doc)
}

fn oracle_document(text: &str) -> Result<Value, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("{ORACLE}: {e}"))?;
    for key in ["occurrences", "context", "diagnostics"] {
        if doc.get(key).is_none() {
            return Err(format!("{ORACLE}: '{key}' is missing"));
        }
    }
    Ok(doc)
}

/// The oracle's resolution states a negative label may name, read from the
/// protocol document rather than from a fixed list here.
fn oracle_site(doc: &Value, case: &Case) -> Result<Value, String> {
    let where_ = format!("{ORACLE} case '{}'", case.id);
    let sites = doc["occurrences"]["sites"]
        .as_array()
        .ok_or_else(|| format!("{ORACLE}: 'occurrences.sites' is missing or not an array"))?;
    let source = source_line(case)?;
    let mut found = Vec::new();
    for site in sites {
        if site.get("file").and_then(Value::as_str) != Some(case.file.as_str()) {
            continue;
        }
        let name = site.get("name").cloned().unwrap_or(Value::Null);
        if name.get("line").and_then(Value::as_u64) != Some(case.line) {
            continue;
        }
        let column = name.get("char").and_then(Value::as_u64).ok_or_else(|| {
            format!(
                "{where_}: an occurrence at {}:{} has no name column",
                case.file, case.line
            )
        })?;
        if identifier_at(&source, column as usize).as_deref() == Some(case.member.as_str()) {
            found.push(site.clone());
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!(
            "{where_}: no occurrence for '{}' at {}:{}",
            case.member, case.file, case.line
        )),
        n => Err(format!(
            "{where_}: {n} occurrences for '{}' at {}:{}",
            case.member, case.file, case.line
        )),
    }
}

fn gate_failures(
    gate: &Gate,
    cases: &[Case],
    rows: &[Row],
    partition: &str,
    observed: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    if rows.len() != cases.len() {
        failures.push(format!(
            "the partition holds {} cases; {LABELS} declares {}",
            rows.len(),
            cases.len()
        ));
    }
    if partition != observed {
        failures.push(format!(
            "the partition differs from the committed {OBSERVED} -- \
             regenerate it with `BLESS=1 cargo test --test negative_binding` and \
             decide the gate's deviation list by hand"
        ));
    }
    for row in rows {
        if row.outcome != Outcome::PreciseWrong {
            continue;
        }
        let listed = gate
            .deviations
            .iter()
            .any(|deviation| deviation.case == row.id && deviation.outcome == row.outcome.as_str());
        if !listed {
            failures.push(format!(
                "case '{}' is precision-wrong and is not a listed deviation",
                row.id
            ));
        }
    }
    for deviation in &gate.deviations {
        let row = rows.iter().find(|row| row.id == deviation.case);
        match row {
            None => failures.push(format!(
                "listed deviation '{}' is not in the partition",
                deviation.case
            )),
            Some(row) if row.outcome.as_str() != deviation.outcome => failures.push(format!(
                "listed deviation '{}' no longer occurs as '{}' -- it now reads '{}'",
                deviation.case,
                deviation.outcome,
                row.outcome.as_str()
            )),
            Some(_) => {}
        }
    }
    let recovered = recovered(cases, rows);
    let precise = precise_only(cases, rows);
    if recovered < gate.recall_floor {
        failures.push(format!(
            "recall {recovered} is below the floor {} (precise-only {precise})",
            gate.recall_floor
        ));
    }
    if recovered > gate.recall_floor {
        failures.push(format!(
            "recall {recovered} is above the stale floor {} -- raise it (precise-only {precise})",
            gate.recall_floor
        ));
    }
    failures
}

// ---------------------------------------------------------------------------
// Labels the fixture carries but no case does
// ---------------------------------------------------------------------------

/// The identity the oracle document must agree with. A run under another SDK
/// or another engine revision is not the same set of labels, which is why the
/// identity is sealed beside them rather than only in the document.
fn oracle_identity(text: &str) -> Result<Value, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("{LABELS}: {e}"))?;
    let oracle = doc
        .get("oracle")
        .ok_or_else(|| format!("{LABELS}: no 'oracle' identity"))?;
    for key in [
        "producer",
        "mode",
        "sdk",
        "msbuild",
        "compiler",
        "engine",
        "dependencyFingerprint",
    ] {
        if oracle.get(key).and_then(Value::as_str).is_none() {
            return Err(format!("{LABELS}: oracle.'{key}' is missing"));
        }
    }
    Ok(oracle.clone())
}

fn labels() -> Vec<Case> {
    parse_labels(&fixture_text(LABELS)).expect("the committed manifest must parse")
}

fn gate() -> Gate {
    parse_gate(&fixture_text(GATE)).expect("the committed gate must parse")
}

fn identifiers(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn negative_binding_ac_1_manifest_shape() {
    let cases = labels();
    assert_eq!(cases.len(), 20, "the manifest must declare twenty cases");

    let mut ids = std::collections::BTreeSet::new();
    let mut sites = std::collections::BTreeSet::new();
    for case in &cases {
        assert!(
            case.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "case '{}' is not a lowercase id",
            case.id
        );
        assert!(
            ids.insert(case.id.clone()),
            "duplicate case id '{}'",
            case.id
        );
        assert!(
            sites.insert((case.file.clone(), case.line)),
            "two cases share {}:{}",
            case.file,
            case.line
        );
        let line = source_line(case).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            identifiers(&line).iter().any(|token| *token == case.member),
            "{}:{} does not hold member '{}'",
            case.file,
            case.line,
            case.member
        );
        match case.intent {
            Intent::Resolvable => {
                assert!(
                    case.category != "negative",
                    "case '{}' is resolvable but filed as negative",
                    case.id
                );
                assert!(!case.declaring_type.is_empty());
                assert!(
                    !case.plausible_wrong.is_empty(),
                    "case '{}' names no plausible wrong target",
                    case.id
                );
            }
            Intent::Negative => {
                assert_eq!(case.category, "negative", "case '{}'", case.id);
                assert!(
                    case.diagnostic.starts_with("CS")
                        && case.diagnostic[2..].chars().all(|c| c.is_ascii_digit()),
                    "case '{}' names diagnostic '{}'",
                    case.id,
                    case.diagnostic
                );
                assert!(
                    !case.accepted_outcomes.is_empty()
                        && case
                            .accepted_outcomes
                            .iter()
                            .all(|o| o == "absent" || o == "tiered"),
                    "case '{}' accepts outcomes {:?}",
                    case.id,
                    case.accepted_outcomes
                );
            }
        }
    }

    for category in RESOLVABLE_CATEGORIES {
        assert_eq!(
            cases.iter().filter(|c| c.category == *category).count(),
            4,
            "category '{category}' must hold four cases"
        );
    }
    let negative: Vec<&Case> = cases
        .iter()
        .filter(|c| c.intent == Intent::Negative)
        .collect();
    assert_eq!(negative.len(), 4);
    assert!(negative.iter().any(|c| c.oracle_state == "ambiguous"));
    assert!(negative.iter().any(|c| c.oracle_state == "inaccessible"));
}

#[test]
fn negative_binding_ac_3_oracle_pinned_and_in_agreement() {
    let text = fixture_text(ORACLE);
    let doc = oracle_document(&text).unwrap();
    let identity = oracle_identity(&fixture_text(LABELS)).unwrap();

    assert_eq!(
        doc["dependencyFingerprint"].as_str().unwrap_or(""),
        identity["dependencyFingerprint"].as_str().unwrap()
    );
    let compilations = doc["context"]["envelope"]["compilations"]
        .as_array()
        .expect("the document must carry its compilation envelope");
    assert!(!compilations.is_empty());
    for compilation in compilations {
        let versions = &compilation["versions"];
        for key in ["sdk", "compiler", "engine"] {
            assert_eq!(
                versions[key].as_str().unwrap_or(""),
                identity[key].as_str().unwrap(),
                "compilation versions.{key} does not match the sealed oracle identity"
            );
        }
    }

    for case in labels() {
        let site = oracle_site(&doc, &case).unwrap_or_else(|e| panic!("{e}"));
        if case.intent == Intent::Resolvable {
            let target = &site["target"];
            assert_eq!(
                site["resolution"].as_str(),
                Some("confirmed"),
                "case '{}' is not confirmed by the compiler",
                case.id
            );
            assert_eq!(target["type"].as_str(), Some(case.declaring_type.as_str()));
            assert_eq!(target["member"].as_str(), Some(case.member.as_str()));
            let parameters =
                signature_parameters(target["overloadSignature"].as_str().unwrap_or(""))
                    .unwrap_or_else(|| panic!("case '{}': unreadable signature", case.id));
            assert_eq!(
                parameters, case.overload_parameters,
                "case '{}': the bound overload differs from the label",
                case.id
            );
        } else {
            assert_eq!(
                site["resolution"].as_str(),
                Some(case.oracle_state.as_str()),
                "case '{}': the compiler reports another resolution state",
                case.id
            );
            let codes: Vec<String> = doc["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| {
                    d["file"].as_str() == Some(case.file.as_str())
                        && d["line"].as_u64() == Some(case.line)
                })
                .filter_map(|d| d["code"].as_str().map(str::to_string))
                .collect();
            assert!(
                codes.contains(&case.diagnostic),
                "case '{}': {}:{} carries {codes:?}, not {}",
                case.id,
                case.file,
                case.line,
                case.diagnostic
            );
        }
    }
}

#[test]
fn negative_binding_ac_4_full_denominator_partition() {
    let cases = labels();
    let workspace = Workspace::new();
    let graph = workspace.graph().expect("the graph must be readable");
    let rows = partition(&cases, &graph).unwrap_or_else(|e| panic!("{e}"));

    println!("{}", partition_table(&cases, &rows));
    assert_eq!(
        rows.len(),
        cases.len(),
        "the denominator must hold every declared case"
    );

    let generated = partition_json(&cases, &rows);
    if std::env::var("BLESS").as_deref() == Ok("1") {
        fs::write(fixture_dir().join(OBSERVED), &generated).expect("write the observation pin");
        return;
    }
    let committed = fs::read_to_string(fixture_dir().join(OBSERVED))
        .unwrap_or_else(|e| panic!("{}: {e}", fixture_dir().join(OBSERVED).display()));
    assert_eq!(
        generated, committed,
        "{} is stale -- regenerate with `BLESS=1 cargo test --test negative_binding`",
        OBSERVED
    );
}

#[test]
fn negative_binding_ac_5_ratchet_gate() {
    let cases = labels();
    let workspace = Workspace::new();
    let graph = workspace.graph().expect("the graph must be readable");
    let rows = partition(&cases, &graph).unwrap_or_else(|e| panic!("{e}"));
    let table = partition_table(&cases, &rows);

    let generated = partition_json(&cases, &rows);
    let observed = fs::read_to_string(fixture_dir().join(OBSERVED)).unwrap_or_default();
    let failures = gate_failures(&gate(), &cases, &rows, &generated, &observed);
    assert!(failures.is_empty(), "{table}\n{}", failures.join("\n"));
}

fn synthetic_case(id: &str, intent: Intent) -> Case {
    Case {
        id: id.to_string(),
        category: "namespace-shadowing".to_string(),
        intent,
        file: "Shadowing.cs".to_string(),
        line: 20,
        member: "Paint".to_string(),
        declaring_type: "NegativeBinding.Shadow.Inner.Panel".to_string(),
        overload_parameters: Vec::new(),
        plausible_wrong: "NegativeBinding.Shadow.Panel".to_string(),
        oracle_state: "ambiguous".to_string(),
        diagnostic: "CS0121".to_string(),
        accepted_outcomes: vec!["absent".to_string(), "tiered".to_string()],
    }
}

fn synthetic_row(id: &str, outcome: Outcome, target: &str) -> Row {
    Row {
        id: id.to_string(),
        category: "namespace-shadowing".to_string(),
        outcome,
        edges: if target.is_empty() {
            Vec::new()
        } else {
            vec![Edge {
                target: target.to_string(),
                tier: Tier::Precise,
            }]
        },
    }
}

/// The gate is driven with observations this run did not produce, so each way
/// it can refuse is exercised without waiting for a resolver regression.
#[test]
fn negative_binding_ac_5_gate_unit_failures() {
    let cases = vec![
        synthetic_case("one", Intent::Resolvable),
        synthetic_case("two", Intent::Resolvable),
    ];
    let unchanged = vec![
        synthetic_row(
            "one",
            Outcome::PreciseCorrect,
            "NegativeBinding.Shadow.Inner.Panel",
        ),
        synthetic_row(
            "two",
            Outcome::PreciseCorrect,
            "NegativeBinding.Shadow.Inner.Panel",
        ),
    ];
    let gate = Gate {
        recall_floor: 2,
        deviations: Vec::new(),
    };
    let pin = partition_json(&cases, &unchanged);
    assert!(gate_failures(&gate, &cases, &unchanged, &pin, &pin).is_empty());

    let wrong = vec![
        synthetic_row("one", Outcome::PreciseWrong, "NegativeBinding.Shadow.Panel"),
        synthetic_row(
            "two",
            Outcome::PreciseCorrect,
            "NegativeBinding.Shadow.Inner.Panel",
        ),
    ];
    let wrong_pin = partition_json(&cases, &wrong);
    let failures = gate_failures(&gate, &cases, &wrong, &wrong_pin, &wrong_pin);
    assert!(
        failures.iter().any(|f| f.contains("precision-wrong")),
        "{failures:?}"
    );
    assert!(failures.iter().any(|f| f.contains("floor")), "{failures:?}");

    let listed = Gate {
        recall_floor: 1,
        deviations: vec![Deviation {
            case: "one".to_string(),
            outcome: "precise-wrong".to_string(),
        }],
    };
    let wrong_gate = gate_failures(&listed, &cases, &wrong, &wrong_pin, &wrong_pin);
    assert!(
        !wrong_gate.iter().any(|f| f.contains("precision-wrong")),
        "{wrong_gate:?}"
    );

    let stale = gate_failures(&listed, &cases, &unchanged, &pin, &pin);
    assert!(
        stale.iter().any(|f| f.contains("no longer occurs")),
        "{stale:?}"
    );

    let stale_floor = gate_failures(&gate, &cases, &unchanged, &pin, &pin);
    assert!(stale_floor.is_empty(), "{stale_floor:?}");
    let raised = gate_failures(
        &Gate {
            recall_floor: 1,
            deviations: Vec::new(),
        },
        &cases,
        &unchanged,
        &pin,
        &pin,
    );
    assert!(
        raised.iter().any(|f| f.contains("stale floor")),
        "{raised:?}"
    );

    let drifted = gate_failures(&gate, &cases, &unchanged, &pin, &format!("{pin}\n"));
    assert!(drifted.iter().any(|f| f.contains(OBSERVED)), "{drifted:?}");

    let short = gate_failures(&gate, &cases, &unchanged[..1], &pin, &pin);
    assert!(short.iter().any(|f| f.contains("declares 2")), "{short:?}");
}

#[test]
fn negative_binding_ac_6_deterministic() {
    let cases = labels();
    let first = Workspace::new();
    let second = Workspace::new();
    let first_rows = partition(&cases, &first.graph().unwrap()).unwrap();
    let second_rows = partition(&cases, &second.graph().unwrap()).unwrap();
    assert_eq!(
        partition_json(&cases, &first_rows),
        partition_json(&cases, &second_rows),
        "two runs over two fresh copies must agree"
    );
}

#[test]
fn negative_binding_ac_7_malformed_input_fails_closed() {
    let cases = labels();
    let workspace = Workspace::new();
    let graph = workspace.graph().unwrap();

    for (text, input) in [
        ("{", LABELS),
        ("{}", LABELS),
        (
            r#"{"cases":[{"id":"x","category":"negative","intent":"negative"}]}"#,
            LABELS,
        ),
    ] {
        let error = parse_labels(text).unwrap_err();
        assert!(error.starts_with(input), "{error}");
    }
    let broken_case = r#"{"cases":[{"id":"broken","category":"negative","intent":"sideways","site":{"file":"Shadowing.cs","line":20,"member":"Paint"}}]}"#;
    let error = parse_labels(broken_case).unwrap_err();
    assert!(error.contains("broken"), "{error}");

    for (text, input) in [
        ("{", GATE),
        ("{}", GATE),
        (r#"{"recallFloor":0,"deviations":[{"case":"x"}]}"#, GATE),
    ] {
        let error = parse_gate(text).unwrap_err();
        assert!(error.starts_with(input), "{error}");
    }
    for (text, input) in [("{", OBSERVED), ("{}", OBSERVED)] {
        let error = parse_observed(text).unwrap_err();
        assert!(error.starts_with(input), "{error}");
    }
    for (text, input) in [("{", ORACLE), ("{}", ORACLE)] {
        let error = oracle_document(text).unwrap_err();
        assert!(error.starts_with(input), "{error}");
    }

    let doc = oracle_document(&fixture_text(ORACLE)).unwrap();
    let mut elsewhere = cases[0].clone();
    elsewhere.line = 1;
    let error = oracle_site(&doc, &elsewhere).unwrap_err();
    assert!(
        error.contains(ORACLE) && error.contains(&elsewhere.id),
        "{error}"
    );

    let mut unknown_file = cases[0].clone();
    unknown_file.file = "Absent.cs".to_string();
    let error = site_edges(&graph, &unknown_file).unwrap_err();
    assert!(
        error.contains(".scout/graph/graph.json") && error.contains(&unknown_file.id),
        "{error}"
    );

    let mut past_the_end = cases[0].clone();
    past_the_end.line = 100_000;
    let error = source_line(&past_the_end).unwrap_err();
    assert!(
        error.contains(&past_the_end.file) && error.contains(&past_the_end.id),
        "{error}"
    );

    // A refusal never yields a partial case list: the denominator is either
    // the sealed one or the run stops.
    assert!(parse_labels(r#"{"cases":[{"id":"x"}]}"#).is_err());
    assert_eq!(cases.len(), 20);
}
