// `devscout audit --semantic <refs.jsonl>` -- scores this repo's `uses-member`
// graph edges against a Roslyn-derived oracle (`tools/scout-semantic`,
// documented in the W0 design note). The oracle emits one ground-truth record
// per member reference in a compiled solution; this command joins those
// records to the graph's own `uses-member` edges on `(file, startLine)` and
// reports precision/recall per resolver tier, plus a handful of leak/
// structural-impossibility signals that catch a specific class of resolver
// bug (a guessed edge landing on a member the caller's project can never
// actually see).
//
// Split in two, deliberately: `load` touches the filesystem (graph.json, the
// oracle JSONL files, the manifest) and returns `Inputs`; `score` is a pure
// function from `Inputs` to `AuditReport` with no I/O at all, so every
// scoring rule below is unit-testable without a repo on disk. `graph.json` is
// read as a bare `serde_json::Value`, not through `graph::read_graph` --
// `Edge::UsesMember` does not carry a `tier` field yet (only legacy
// `heuristic: bool`), and reading through the typed struct would silently
// drop a `tier` key the day the resolver starts emitting one. Parsing the
// loose `Value` here means that day requires no change to this file's load
// path, only to `tier_of` below.
//
// `OracleRef` is trimmed to the fields the scoring rules in the design note
// actually consult (file/startLine/shape/receiverKind/target/targetKind/
// targetFile/external/ambiguous) -- `member`, `line`, `ext`, `receiver`,
// `receiverText`, `memberKind`, `targetUnit` and `unit` are part of the
// oracle's on-disk schema but never referenced by any rule here, so declaring
// them would only be dead weight (unknown JSON keys are ignored by serde
// without `deny_unknown_fields`, so dropping them from the struct changes
// nothing about what a real refs.jsonl file parses to).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cli::J;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A `uses-member` edge's resolver tier. `Heuristic` is the legacy shape: a
/// `heuristic: true` edge with no `tier` string at all, which is every
/// non-precise edge the resolver emits today (the `tier` key, and the
/// `Ext`/`Guess` split it carries, land in a later change -- see the W0
/// design note's decision #3). `tier_of` below is the one place that maps
/// either shape onto this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Tier {
    Precise,
    Ext,
    Guess,
    Heuristic,
}

impl Tier {
    /// Fixed, spec-mandated iteration/display order for every tier-keyed
    /// output (text table rows, `tiers` JSON keys): precise, ext, guess,
    /// heuristic.
    const ORDER: [Tier; 4] = [Tier::Precise, Tier::Ext, Tier::Guess, Tier::Heuristic];

    fn key(self) -> &'static str {
        match self {
            Tier::Precise => "precise",
            Tier::Ext => "ext",
            Tier::Guess => "guess",
            Tier::Heuristic => "heuristic",
        }
    }
}

/// One `uses-member` edge out of graph.json, reduced to the fields scoring
/// needs. `to_file`/`to` are the graph's own def id and its declaring file --
/// `graph.rs`'s `Def.file` is the def's first-insertion file for a partial
/// class, so a member declared in a second `also_in` file is joined on that
/// first file here too (the design note's fact table: "audit joins on def id
/// only").
struct EdgeRow {
    from_file: String,
    from_line: usize,
    to: String,
    to_file: String,
    tier: Tier,
}

/// A graph.json def, reduced to what scoring needs -- and, doubling as the
/// shape `--defs <defs.jsonl>` deserializes into directly (that oracle
/// record has the same four field names; see `load`). `test` is "does this
/// def carry at least one method with a devscout-recognized test attribute"
/// (graph.json's `testMethods` array, non-empty) for a graph-sourced row, or
/// the oracle's own equivalent Roslyn-derived flag for an oracle-sourced one.
#[derive(Debug, Clone, serde::Deserialize)]
struct DefRow {
    id: String,
    file: String,
    // Carried through for parity with the design note's struct shape and
    // with the oracle's on-disk defs.jsonl record; no scoring rule below
    // reads it back (grouping by kind uses the oracle ref's own
    // `targetKind`, not this).
    #[allow(dead_code)]
    kind: String,
    #[serde(default)]
    test: bool,
}

/// One line of `refs.jsonl`, the oracle's ground-truth member-reference
/// records -- reduced (see the module header) to what scoring reads.
/// `target`/`targetKind`/`targetFile` are `Option` because the oracle writes
/// `null` for an unknown value (its own §3.6 rule); every other field here is
/// always present on a real oracle record.
#[derive(Debug, Clone, serde::Deserialize)]
struct OracleRef {
    file: String,
    #[serde(rename = "startLine")]
    start_line: usize,
    shape: String,
    #[serde(rename = "receiverKind")]
    receiver_kind: String,
    target: Option<String>,
    #[serde(rename = "targetKind")]
    target_kind: Option<String>,
    #[serde(rename = "targetFile")]
    target_file: Option<String>,
    external: bool,
    ambiguous: bool,
}

/// One line of `units.jsonl` -- a compiled project, reduced to what the
/// structural check needs: `status` (ok/failed unit counts, and whether a
/// file belongs to a failed unit at all -- excluded from the audit universe),
/// `refs` (other unit names this unit's `ProjectReferences` name, the raw
/// material for the reachability closure `reach` builds), and `files` (the
/// file->unit membership map `file_to_unit` builds), and `name` (the key
/// `refs`/`file_to_unit` join on). `test`/`tfm`/`diagnostics` from the
/// oracle's own schema are not carried: nothing here reads a unit's own
/// `test` flag (only a *def's* `test` flag, via `DefRow`, matters to the
/// fallback structural check).
#[derive(Debug, Clone, serde::Deserialize)]
struct Unit {
    name: String,
    status: String,
    #[serde(default)]
    refs: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

/// Filesystem inputs to one audit run -- everything `load` gathers and
/// `score` consumes. Held together so `score` stays a single-argument pure
/// function: no field here is touched by any I/O once this value exists.
struct Inputs {
    root: PathBuf,
    /// Defs from graph.json (`devscout`'s own understanding of the in-tree
    /// type/enum-member universe).
    graph_defs: Vec<DefRow>,
    /// Defs from `--defs <defs.jsonl>` (Roslyn's), if given; empty otherwise
    /// -- the structural fallback's `test`/`file_has_test_def` lookups prefer
    /// this source when a def id appears in both (see `score`).
    oracle_defs: Vec<DefRow>,
    /// `uses-member` edges from graph.json.
    edges: Vec<EdgeRow>,
    /// Every record `--semantic <refs.jsonl>` carried, BEFORE the universe
    /// filter `score` applies -- `score` is the one place that drops a
    /// record and counts the drop, so the raw set is what a caller-supplied
    /// `Inputs` should also carry.
    records: Vec<OracleRef>,
    /// Units from `--units <units.jsonl>`, if given; empty otherwise.
    units: Vec<Unit>,
    /// File universe an oracle record's `file` must fall inside to be
    /// scored: the manifest's mapped-file set (or, with no manifest, every
    /// file graph.json mentions), minus the files of any unit whose `status`
    /// is not `"ok"`.
    universe: HashSet<String>,
}

/// `load`'s filesystem arguments -- the resolved, already-`-C`-aware paths
/// for the three input files `cmd_audit` accepts.
struct AuditOptions<'a> {
    semantic: &'a Path,
    units: Option<&'a Path>,
    defs: Option<&'a Path>,
}

// ---------------------------------------------------------------------------
// `load` -- filesystem. Returns a plain `String` error, the shape every
// `cmd_*` in cli.rs already wraps as `"error: {msg}"`.
// ---------------------------------------------------------------------------

/// Reads graph.json, the oracle's `refs.jsonl`/`units.jsonl`/`defs.jsonl`,
/// and the manifest, and assembles `Inputs`. No scoring happens here.
fn load(root: &Path, opts: &AuditOptions) -> Result<Inputs, String> {
    let graph_path = crate::graph::graph_json_path(root);
    let graph_text = std::fs::read_to_string(&graph_path).map_err(|_| {
        "no graph.json for this repo — run `devscout map` on a C# scope first".to_string()
    })?;
    let graph_value: serde_json::Value = serde_json::from_str(&graph_text).map_err(|e| {
        format!(
            "graph.json at {} is not valid JSON: {e}",
            graph_path.display()
        )
    })?;
    let (graph_defs, edges) = parse_graph(&graph_value)?;

    let records = read_jsonl::<OracleRef>(opts.semantic)?;
    let units = match opts.units {
        Some(p) => read_jsonl::<Unit>(p)?,
        None => Vec::new(),
    };
    let oracle_defs = match opts.defs {
        Some(p) => read_jsonl::<DefRow>(p)?,
        None => Vec::new(),
    };
    let universe = build_universe(root, &graph_value, &units);

    Ok(Inputs {
        root: root.to_path_buf(),
        graph_defs,
        oracle_defs,
        edges,
        records,
        units,
        universe,
    })
}

/// Parses one non-empty line per record via `T`'s `Deserialize`. The error
/// carries the file and 1-based line number, matching how a human would
/// point at a bad line in a `.jsonl` file.
fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: T =
            serde_json::from_str(line).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))?;
        out.push(rec);
    }
    Ok(out)
}

/// Walks graph.json's `defs`/`edges` arrays by hand (a `serde_json::Value`,
/// not the typed `graph::Graph` -- see the module header) into the reduced
/// `DefRow`/`EdgeRow` shapes scoring needs. A missing/malformed field reads
/// as its type's default rather than erroring: graph.json is devscout's own
/// artifact and any shape it can produce is one this function should survive
/// (an edge with the wrong `kind` is simply filtered out, never a hard
/// error).
fn parse_graph(v: &serde_json::Value) -> Result<(Vec<DefRow>, Vec<EdgeRow>), String> {
    let defs_arr = v
        .get("defs")
        .and_then(|d| d.as_array())
        .ok_or("graph.json has no 'defs' array")?;
    let mut defs = Vec::with_capacity(defs_arr.len());
    for d in defs_arr {
        defs.push(DefRow {
            id: str_field(d, "id"),
            file: str_field(d, "file"),
            kind: str_field(d, "kind"),
            test: d
                .get("testMethods")
                .and_then(|x| x.as_array())
                .is_some_and(|a| !a.is_empty()),
        });
    }

    let edges_arr = v
        .get("edges")
        .and_then(|e| e.as_array())
        .ok_or("graph.json has no 'edges' array")?;
    let mut edges = Vec::new();
    for e in edges_arr {
        if e.get("kind").and_then(|k| k.as_str()) != Some("uses-member") {
            continue;
        }
        edges.push(EdgeRow {
            from_file: str_field(e, "from_file"),
            from_line: e.get("from_line").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            to: str_field(e, "to"),
            to_file: str_field(e, "to_file"),
            tier: tier_of(e),
        });
    }
    Ok((defs, edges))
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Tier from an edge's raw JSON: the `tier` string if present (only `"ext"`
/// and `"guess"` are meaningful spellings today -- anything else falls
/// through to the `heuristic` bool, the same as no `tier` key at all), else
/// legacy `heuristic: true` -> `Heuristic`, else `Precise`.
fn tier_of(e: &serde_json::Value) -> Tier {
    match e.get("tier").and_then(|x| x.as_str()) {
        Some("ext") => return Tier::Ext,
        Some("guess") => return Tier::Guess,
        _ => {}
    }
    if e.get("heuristic")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        Tier::Heuristic
    } else {
        Tier::Precise
    }
}

/// The audit's file universe: the manifest's mapped-file set (`entries`
/// object keys) if a manifest is present and non-empty; otherwise every file
/// named by a graph.json def or edge. Either way, the files of any unit whose
/// `status` is not `"ok"` are then removed -- a failed unit's own files carry
/// no reliable oracle ground truth (the compilation that would have produced
/// it never succeeded). A corrupt manifest fails open (falls back to the
/// graph-files set) rather than aborting the whole audit over an unrelated
/// artifact.
fn build_universe(root: &Path, graph_value: &serde_json::Value, units: &[Unit]) -> HashSet<String> {
    let mut universe: HashSet<String> = HashSet::new();
    if let Ok(Some(m)) = crate::manifest::read_manifest(root) {
        if let Some(entries) = m.get("entries").and_then(crate::manifest::Value::as_object) {
            universe.extend(entries.iter().map(|(k, _)| k.clone()));
        }
    }
    if universe.is_empty() {
        for key in ["defs", "edges"] {
            if let Some(arr) = graph_value.get(key).and_then(|x| x.as_array()) {
                for row in arr {
                    for file_key in ["file", "from_file", "to_file"] {
                        if let Some(f) = row.get(file_key).and_then(|x| x.as_str()) {
                            universe.insert(f.to_string());
                        }
                    }
                }
            }
        }
    }
    for u in units {
        if u.status != "ok" {
            for f in &u.files {
                universe.remove(f);
            }
        }
    }
    universe
}

// ---------------------------------------------------------------------------
// Scoring primitives -- pure, no I/O, shared by `score` and its tests.
// ---------------------------------------------------------------------------

/// The target-match rule (design note §5.3): an oracle record matches an
/// edge's `to` either literally, or -- for an enum-member record -- when the
/// edge names the bare enum type (`to` == the record's target with its last
/// `.member` segment dropped). The second arm is the "two-spelling" case:
/// `resolve.rs` writes an enum-member edge as `Ns.Enum.Member` when that
/// exact def exists and as bare `Ns.Enum` otherwise (fact table, graph.rs
/// :1121-1138), and either spelling is a correct answer to the same oracle
/// record.
fn target_matches(r: &OracleRef, edge_to: &str) -> bool {
    let Some(t) = &r.target else { return false };
    if t == edge_to {
        return true;
    }
    if r.target_kind.as_deref() == Some("enum-member") {
        if let Some((prefix, _)) = t.rsplit_once('.') {
            if prefix == edge_to {
                return true;
            }
        }
    }
    false
}

/// Whether an oracle record's target is a def devscout's own graph knows
/// about -- the recall-D eligibility rule's third clause. Same two-spelling
/// allowance as `target_matches`: an enum-member record is "known" if either
/// its exact id or its bare enum id is a graph def.
fn target_known(defs_by_id: &HashMap<&str, &DefRow>, r: &OracleRef) -> bool {
    let Some(t) = &r.target else { return false };
    if defs_by_id.contains_key(t.as_str()) {
        return true;
    }
    if r.target_kind.as_deref() == Some("enum-member") {
        if let Some((prefix, _)) = t.rsplit_once('.') {
            if defs_by_id.contains_key(prefix) {
                return true;
            }
        }
    }
    false
}

/// An id's short name -- its last `.`- or `+`-separated segment (`+` splits a
/// nested type, e.g. `Ns.Outer+Inner` -> `Inner`) -- for the "top fp targets"
/// table, which reports by short name rather than the full id (the design
/// note's §5.4 example: `FilterConfig`, not `Fixture.Domain.FilterConfig`).
fn short_name(id: &str) -> &str {
    id.rsplit(['.', '+']).next().unwrap_or(id)
}

/// `unit(file)` -- first-match file->unit-name map built from every unit's
/// `files` list.
fn file_to_unit(units: &[Unit]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for u in units {
        for f in &u.files {
            m.entry(f.clone()).or_insert_with(|| u.name.clone());
        }
    }
    m
}

/// `reach(unit)` for every unit: the transitive closure of `refs` plus the
/// unit itself (design note §5.3: "transitive closure of refs ∪ self"). A
/// `refs` entry naming a unit this set has never heard of (a project
/// reference the oracle could not resolve to one of its own units) is simply
/// a dead end -- it counts as reached but contributes no further edges.
fn reach(units: &[Unit]) -> HashMap<String, HashSet<String>> {
    let by_name: HashMap<&str, &Unit> = units.iter().map(|u| (u.name.as_str(), u)).collect();
    let mut result = HashMap::new();
    for u in units {
        let mut seen: HashSet<String> = HashSet::new();
        let mut stack = vec![u.name.clone()];
        while let Some(n) = stack.pop() {
            if seen.contains(&n) {
                continue;
            }
            seen.insert(n.clone());
            if let Some(unit) = by_name.get(n.as_str()) {
                for r in &unit.refs {
                    if !seen.contains(r) {
                        stack.push(r.clone());
                    }
                }
            }
        }
        result.insert(u.name.clone(), seen);
    }
    result
}

/// The structural check for one edge: `None` when it cannot be checked (the
/// edge is excluded from both `checked` and `impossible`), else `Some(true)`
/// when the edge is structurally impossible.
///
/// `"units"` method (design note §5.3): impossible when `to_file`'s unit is
/// not in `from_file`'s unit's `reach` set. Either file failing to resolve to
/// a unit at all (not listed in any unit's `files`) makes the edge
/// unchecked.
///
/// `"test-defs"` fallback: impossible when the target def is itself
/// test-attributed (`def(to).test`) and the calling file declares no
/// test-attributed def of its own. The target def must be known (found in
/// `test_by_id`) for the edge to be checked at all; an unknown target
/// (`to` not in either def source) is unchecked, not "not impossible".
#[allow(clippy::too_many_arguments)]
fn is_structural(
    e: &EdgeRow,
    units_method: bool,
    file_unit: &HashMap<String, String>,
    reach_map: &HashMap<String, HashSet<String>>,
    test_by_id: &HashMap<String, bool>,
    test_files: &HashSet<String>,
) -> Option<bool> {
    if units_method {
        let u1 = file_unit.get(&e.from_file)?;
        let u2 = file_unit.get(&e.to_file)?;
        let reachable = reach_map.get(u1).is_some_and(|s| s.contains(u2));
        Some(!reachable)
    } else {
        let target_test = *test_by_id.get(&e.to)?;
        if !target_test {
            return Some(false);
        }
        Some(!test_files.contains(&e.from_file))
    }
}

// ---------------------------------------------------------------------------
// `AuditReport` -- `score`'s pure output. Every count is a `usize`; ratios
// are computed at render time (`ratio_text`/`ratio_j`) from a hit count and a
// denominator, both kept, rather than as a pre-divided `f64` -- there is
// exactly one place (each renderer) that has to decide how a zero
// denominator prints, instead of that decision being baked into the data.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct TierStats {
    edges: usize,
    tp: usize,
    fp: usize,
    fp_no_site: usize,
    fp_external_site: usize,
    fp_wrong_target: usize,
    structural: usize,
}

impl TierStats {
    fn precision(&self) -> f64 {
        if self.edges == 0 {
            0.0
        } else {
            self.tp as f64 / self.edges as f64
        }
    }
}

struct AuditReport {
    root: String,
    oracle_records: usize,
    oracle_sites: usize,
    oracle_external_sites: usize,
    oracle_ambiguous: usize,
    oracle_dropped: usize,
    units_ok: usize,
    units_failed: usize,
    structural_method: &'static str,
    /// Only tiers with `edges > 0`, in `Tier::ORDER`.
    tiers: Vec<(Tier, TierStats)>,
    recall_denominator: usize,
    recall_precise: usize,
    recall_precise_ext: usize,
    recall_all: usize,
    /// Fixed order: ident, qualified, this, base, call (design note §5.4's
    /// example order) -- `None` when that receiver kind has no D-eligible
    /// record at all (printed `-` in text, `null` in JSON), `Some(hits as a
    /// fraction of that bucket's denominator)` otherwise.
    by_receiver: Vec<(&'static str, Option<f64>)>,
    recall_conditional: usize,
    recall_bare: usize,
    silent_correct: usize,
    silent_leak: usize,
    structural_impossible: usize,
    structural_checked: usize,
    /// Sites-with-N-edges histogram, buckets `[1, 2, 3, "4+"]`.
    fanout: [usize; 4],
    /// `(targetKind, distinct-target count)`, sorted by count desc then kind
    /// asc.
    unknown_targets: Vec<(String, usize)>,
    /// `(short name, count)`, sorted by count desc then name asc, top 20.
    top_fp: Vec<(String, usize)>,
    /// `(target id, count)`, sorted by count desc then id asc, top 20.
    top_missed: Vec<(String, usize)>,
    partial_file_mismatch: usize,
}

/// Scores `inputs` into a full `AuditReport`. Pure: every branch below reads
/// only `inputs` and locally built indexes over it.
fn score(inputs: Inputs) -> AuditReport {
    let root = inputs.root.display().to_string();

    // Universe filter: an oracle record whose file is outside `universe` is
    // dropped and counted, never scored.
    let mut oracle_dropped = 0usize;
    let records: Vec<&OracleRef> = inputs
        .records
        .iter()
        .filter(|r| {
            let keep = inputs.universe.contains(&r.file);
            if !keep {
                oracle_dropped += 1;
            }
            keep
        })
        .collect();

    let graph_defs_by_id: HashMap<&str, &DefRow> = inputs
        .graph_defs
        .iter()
        .map(|d| (d.id.as_str(), d))
        .collect();

    // Site indexes: every kept record, and every uses-member edge, grouped
    // by `(file, startLine)`/`(from_file, from_line)`.
    let mut by_site: HashMap<(String, usize), Vec<&OracleRef>> = HashMap::new();
    for r in &records {
        by_site
            .entry((r.file.clone(), r.start_line))
            .or_default()
            .push(r);
    }
    let mut edges_by_site: HashMap<(String, usize), Vec<&EdgeRow>> = HashMap::new();
    for e in &inputs.edges {
        edges_by_site
            .entry((e.from_file.clone(), e.from_line))
            .or_default()
            .push(e);
    }

    let oracle_sites = by_site.len();
    let oracle_ambiguous = records.iter().filter(|r| r.ambiguous).count();

    // Silent-correct / leak: sites where every `shape == "access"` record is
    // external.
    let mut access_by_site: HashMap<(String, usize), Vec<&OracleRef>> = HashMap::new();
    for r in records.iter().filter(|r| r.shape == "access") {
        access_by_site
            .entry((r.file.clone(), r.start_line))
            .or_default()
            .push(r);
    }
    let mut silent_correct = 0usize;
    let mut silent_leak = 0usize;
    for (site, recs) in &access_by_site {
        if recs.iter().all(|r| r.external) {
            if edges_by_site.contains_key(site) {
                silent_leak += 1;
            } else {
                silent_correct += 1;
            }
        }
    }
    let oracle_external_sites = silent_correct + silent_leak;

    // Structural check setup: "units" method when `--units` produced at
    // least one unit, else the "test-defs" fallback.
    let units_method = !inputs.units.is_empty();
    let structural_method: &'static str = if units_method { "units" } else { "test-defs" };
    let file_unit = file_to_unit(&inputs.units);
    let reach_map = reach(&inputs.units);
    // `test_by_id`/`test_files`: graph defs first, oracle `--defs` entries
    // layered on top (a later insert overwrites an earlier one for the same
    // id) -- an oracle def, when given, is the more authoritative "is this a
    // test-attributed def" signal (see `DefRow`'s doc comment).
    let mut test_by_id: HashMap<String, bool> = HashMap::new();
    let mut test_files: HashSet<String> = HashSet::new();
    for d in inputs.graph_defs.iter().chain(inputs.oracle_defs.iter()) {
        test_by_id.insert(d.id.clone(), d.test);
        if d.test {
            test_files.insert(d.file.clone());
        }
    }
    let mut structural_impossible = 0usize;
    let mut structural_checked = 0usize;
    let edge_structural: Vec<bool> = inputs
        .edges
        .iter()
        .map(|e| {
            match is_structural(
                e,
                units_method,
                &file_unit,
                &reach_map,
                &test_by_id,
                &test_files,
            ) {
                Some(flag) => {
                    structural_checked += 1;
                    if flag {
                        structural_impossible += 1;
                    }
                    flag
                }
                None => false,
            }
        })
        .collect();

    // Per-tier TP/FP classification, fan-out and top-fp-targets tallies.
    let mut tiers: HashMap<Tier, TierStats> = HashMap::new();
    let mut fp_targets: HashMap<String, usize> = HashMap::new();
    let mut fanout_sites: HashMap<(String, usize), usize> = HashMap::new();
    for e in &inputs.edges {
        *fanout_sites
            .entry((e.from_file.clone(), e.from_line))
            .or_insert(0) += 1;
    }
    let mut partial_file_mismatch = 0usize;

    for (i, e) in inputs.edges.iter().enumerate() {
        let stats = tiers.entry(e.tier).or_default();
        stats.edges += 1;
        let site_key = (e.from_file.clone(), e.from_line);
        match by_site.get(&site_key) {
            None => {
                stats.fp += 1;
                stats.fp_no_site += 1;
                *fp_targets.entry(short_name(&e.to).to_string()).or_insert(0) += 1;
            }
            Some(recs) => {
                let tp_record = recs
                    .iter()
                    .find(|r| !r.external && target_matches(r, &e.to));
                if let Some(r) = tp_record {
                    stats.tp += 1;
                    if let Some(tf) = &r.target_file {
                        if tf != &e.to_file {
                            partial_file_mismatch += 1;
                        }
                    }
                } else if recs.iter().all(|r| r.external) {
                    stats.fp += 1;
                    stats.fp_external_site += 1;
                    *fp_targets.entry(short_name(&e.to).to_string()).or_insert(0) += 1;
                } else {
                    stats.fp += 1;
                    stats.fp_wrong_target += 1;
                    *fp_targets.entry(short_name(&e.to).to_string()).or_insert(0) += 1;
                }
            }
        }
        if edge_structural[i] {
            stats.structural += 1;
        }
    }
    let tiers_ordered: Vec<(Tier, TierStats)> = Tier::ORDER
        .into_iter()
        .filter_map(|t| tiers.get(&t).filter(|ts| ts.edges > 0).map(|ts| (t, *ts)))
        .collect();

    // Recall D: shape == "access", non-external, target known to the graph.
    let site_hit = |r: &OracleRef, tier_ok: &dyn Fn(Tier) -> bool| -> bool {
        edges_by_site
            .get(&(r.file.clone(), r.start_line))
            .is_some_and(|es| {
                es.iter()
                    .any(|e| tier_ok(e.tier) && target_matches(r, &e.to))
            })
    };
    let d_records: Vec<&&OracleRef> = records
        .iter()
        .filter(|r| r.shape == "access" && !r.external && target_known(&graph_defs_by_id, r))
        .collect();
    let recall_denominator = d_records.len();
    let recall_precise = d_records
        .iter()
        .filter(|r| site_hit(r, &|t| t == Tier::Precise))
        .count();
    let recall_precise_ext = d_records
        .iter()
        .filter(|r| site_hit(r, &|t| t == Tier::Precise || t == Tier::Ext))
        .count();
    let recall_all = d_records.iter().filter(|r| site_hit(r, &|_| true)).count();

    let mut top_missed: HashMap<String, usize> = HashMap::new();
    for r in &d_records {
        if !site_hit(r, &|_| true) {
            // `target_known` guarantees `target` is `Some`.
            *top_missed.entry(r.target.clone().unwrap()).or_insert(0) += 1;
        }
    }

    let by_receiver: Vec<(&'static str, Option<f64>)> =
        ["ident", "qualified", "this", "base", "call"]
            .into_iter()
            .map(|k| {
                let subset: Vec<_> = d_records.iter().filter(|r| r.receiver_kind == k).collect();
                if subset.is_empty() {
                    (k, None)
                } else {
                    let hits = subset.iter().filter(|r| site_hit(r, &|_| true)).count();
                    (k, Some(hits as f64 / subset.len() as f64))
                }
            })
            .collect();

    let recall_conditional = records
        .iter()
        .filter(|r| r.shape == "conditional" && !r.external && target_known(&graph_defs_by_id, r))
        .count();
    let recall_bare = records
        .iter()
        .filter(|r| r.shape == "bare" && !r.external && target_known(&graph_defs_by_id, r))
        .count();

    // Unknown targets: in-solution (per the record's own `external == false`)
    // but not a def devscout's graph knows about, grouped by targetKind.
    let mut unknown_by_kind: HashMap<String, HashSet<String>> = HashMap::new();
    for r in &records {
        if r.external {
            continue;
        }
        let Some(t) = &r.target else { continue };
        if target_known(&graph_defs_by_id, r) {
            continue;
        }
        let kind = r
            .target_kind
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        unknown_by_kind.entry(kind).or_default().insert(t.clone());
    }
    let mut unknown_targets: Vec<(String, usize)> = unknown_by_kind
        .into_iter()
        .map(|(k, set)| (k, set.len()))
        .collect();
    unknown_targets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut fanout = [0usize; 4];
    for n in fanout_sites.values() {
        match n {
            1 => fanout[0] += 1,
            2 => fanout[1] += 1,
            3 => fanout[2] += 1,
            _ => fanout[3] += 1,
        }
    }

    let mut top_fp: Vec<(String, usize)> = fp_targets.into_iter().collect();
    top_fp.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_fp.truncate(20);
    let mut top_missed: Vec<(String, usize)> = top_missed.into_iter().collect();
    top_missed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_missed.truncate(20);

    let units_ok = inputs.units.iter().filter(|u| u.status == "ok").count();
    let units_failed = inputs.units.len() - units_ok;

    AuditReport {
        root,
        oracle_records: records.len(),
        oracle_sites,
        oracle_external_sites,
        oracle_ambiguous,
        oracle_dropped,
        units_ok,
        units_failed,
        structural_method,
        tiers: tiers_ordered,
        recall_denominator,
        recall_precise,
        recall_precise_ext,
        recall_all,
        by_receiver,
        recall_conditional,
        recall_bare,
        silent_correct,
        silent_leak,
        structural_impossible,
        structural_checked,
        fanout,
        unknown_targets,
        top_fp,
        top_missed,
        partial_file_mismatch,
    }
}

// ---------------------------------------------------------------------------
// Rendering -- text (default) and `--json` (via cli.rs's `J`).
// ---------------------------------------------------------------------------

/// `-`  when `denom == 0` (no eligible record at all -- an undefined ratio,
/// not a zero one), else the hit rate to 3 decimals.
fn ratio_text(hits: usize, denom: usize) -> String {
    if denom == 0 {
        "-".to_string()
    } else {
        format!("{:.3}", hits as f64 / denom as f64)
    }
}

/// Same rule as `ratio_text`, JSON-shaped: a bare `null` (via `J::RawNum`,
/// which writes its string argument through unescaped -- `J` has no `Null`
/// variant, and this is the one place a ratio has no value to report)
/// instead of `-`.
fn ratio_j(hits: usize, denom: usize) -> J {
    if denom == 0 {
        J::RawNum("null".to_string())
    } else {
        J::RawNum(format!("{:.3}", hits as f64 / denom as f64))
    }
}

fn render_text(r: &AuditReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "devscout audit --semantic  root {}  oracle {} records / {} sites  units ok {} failed {}  method {}",
        r.root, r.oracle_records, r.oracle_sites, r.units_ok, r.units_failed, r.structural_method
    ));

    if !r.tiers.is_empty() {
        lines.push(format!(
            "{:<10}{:>7}{:>7}{:>7}{:>12}{:>13}{:>13}{:>10}{:>12}",
            "tier",
            "edges",
            "tp",
            "fp",
            "precision",
            "fp:no-site",
            "fp:external",
            "fp:wrong",
            "structural"
        ));
        for (tier, ts) in &r.tiers {
            lines.push(format!(
                "{:<10}{:>7}{:>7}{:>7}{:>12.3}{:>13}{:>13}{:>10}{:>12}",
                tier.key(),
                ts.edges,
                ts.tp,
                ts.fp,
                ts.precision(),
                ts.fp_no_site,
                ts.fp_external_site,
                ts.fp_wrong_target,
                ts.structural,
            ));
        }
    }

    lines.push(format!(
        "recall ({} in-graph member sites)  precise {}  precise+ext {}  all {}",
        r.recall_denominator,
        ratio_text(r.recall_precise, r.recall_denominator),
        ratio_text(r.recall_precise_ext, r.recall_denominator),
        ratio_text(r.recall_all, r.recall_denominator),
    ));
    let by_receiver = r
        .by_receiver
        .iter()
        .map(|(k, v)| match v {
            Some(x) => format!("{k} {x:.3}"),
            None => format!("{k} -"),
        })
        .collect::<Vec<_>>()
        .join("  ");
    lines.push(format!("  by receiver  {by_receiver}"));

    lines.push(format!(
        "external sites {}  silent-correct {}  leaked {}",
        r.oracle_external_sites, r.silent_correct, r.silent_leak
    ));
    lines.push(format!(
        "fan-out  1: {}  2: {}  3: {}  4+: {}",
        r.fanout[0], r.fanout[1], r.fanout[2], r.fanout[3]
    ));

    if !r.top_fp.is_empty() {
        let s = r
            .top_fp
            .iter()
            .map(|(n, c)| format!("{n} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("top fp targets   {s}"));
    }
    if !r.top_missed.is_empty() {
        let s = r
            .top_missed
            .iter()
            .map(|(id, c)| format!("{id} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("top missed       {s}"));
    }
    if !r.unknown_targets.is_empty() {
        let s = r
            .unknown_targets
            .iter()
            .map(|(k, c)| format!("{k} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("unknown targets  {s}"));
    }
    // These three are printed only when non-zero: the common case (a clean
    // run against a well-formed oracle) has all three at zero, and a text
    // report that always carried three "0" lines would bury the signal a
    // real run needs to see.
    if r.partial_file_mismatch > 0 {
        lines.push(format!("partial file mismatch {}", r.partial_file_mismatch));
    }
    if r.oracle_ambiguous > 0 {
        lines.push(format!("ambiguous {}", r.oracle_ambiguous));
    }
    if r.oracle_dropped > 0 {
        lines.push(format!("dropped (outside universe) {}", r.oracle_dropped));
    }

    lines.join("\n")
}

fn render_json(r: &AuditReport) -> String {
    let mut tiers_fields: Vec<(&'static str, J)> = Vec::new();
    for tier in Tier::ORDER {
        if let Some((_, ts)) = r.tiers.iter().find(|(t, _)| *t == tier) {
            tiers_fields.push((
                tier.key(),
                J::Obj(vec![
                    ("edges", J::UInt(ts.edges as u64)),
                    ("tp", J::UInt(ts.tp as u64)),
                    ("fp", J::UInt(ts.fp as u64)),
                    ("precision", J::RawNum(format!("{:.3}", ts.precision()))),
                    ("fp_no_site", J::UInt(ts.fp_no_site as u64)),
                    ("fp_external_site", J::UInt(ts.fp_external_site as u64)),
                    ("fp_wrong_target", J::UInt(ts.fp_wrong_target as u64)),
                    ("structural", J::UInt(ts.structural as u64)),
                ]),
            ));
        }
    }

    let by_receiver_j: Vec<(&'static str, J)> = r
        .by_receiver
        .iter()
        .map(|(k, v)| {
            (
                *k,
                match v {
                    Some(x) => J::RawNum(format!("{x:.3}")),
                    None => J::RawNum("null".to_string()),
                },
            )
        })
        .collect();

    J::Obj(vec![
        ("status", J::Str("ok".to_string())),
        ("root", J::Str(r.root.clone())),
        (
            "oracle",
            J::Obj(vec![
                ("records", J::UInt(r.oracle_records as u64)),
                ("sites", J::UInt(r.oracle_sites as u64)),
                ("external_sites", J::UInt(r.oracle_external_sites as u64)),
                ("ambiguous", J::UInt(r.oracle_ambiguous as u64)),
                ("dropped", J::UInt(r.oracle_dropped as u64)),
            ]),
        ),
        (
            "units",
            J::Obj(vec![
                ("ok", J::UInt(r.units_ok as u64)),
                ("failed", J::UInt(r.units_failed as u64)),
                ("method", J::Str(r.structural_method.to_string())),
            ]),
        ),
        ("tiers", J::Obj(tiers_fields)),
        (
            "recall",
            J::Obj(vec![
                ("denominator", J::UInt(r.recall_denominator as u64)),
                ("precise", ratio_j(r.recall_precise, r.recall_denominator)),
                (
                    "precise_ext",
                    ratio_j(r.recall_precise_ext, r.recall_denominator),
                ),
                ("all", ratio_j(r.recall_all, r.recall_denominator)),
                ("by_receiver", J::Obj(by_receiver_j)),
                ("conditional", J::UInt(r.recall_conditional as u64)),
                ("bare", J::UInt(r.recall_bare as u64)),
            ]),
        ),
        (
            "silent",
            J::Obj(vec![
                ("correct", J::UInt(r.silent_correct as u64)),
                ("leak", J::UInt(r.silent_leak as u64)),
            ]),
        ),
        (
            "structural",
            J::Obj(vec![
                ("impossible", J::UInt(r.structural_impossible as u64)),
                ("checked", J::UInt(r.structural_checked as u64)),
                ("method", J::Str(r.structural_method.to_string())),
            ]),
        ),
        (
            "fanout",
            J::Obj(vec![
                ("1", J::UInt(r.fanout[0] as u64)),
                ("2", J::UInt(r.fanout[1] as u64)),
                ("3", J::UInt(r.fanout[2] as u64)),
                ("4+", J::UInt(r.fanout[3] as u64)),
            ]),
        ),
        (
            "unknown_targets",
            J::Arr(
                r.unknown_targets
                    .iter()
                    .map(|(k, c)| {
                        J::Obj(vec![
                            ("kind", J::Str(k.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "top_fp",
            J::Arr(
                r.top_fp
                    .iter()
                    .map(|(n, c)| {
                        J::Obj(vec![
                            ("name", J::Str(n.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "top_missed",
            J::Arr(
                r.top_missed
                    .iter()
                    .map(|(id, c)| {
                        J::Obj(vec![
                            ("id", J::Str(id.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "partial_file_mismatch",
            J::UInt(r.partial_file_mismatch as u64),
        ),
    ])
    .to_json_string()
}

// ---------------------------------------------------------------------------
// `--assert <file>` -- a flat `{"dotted.metric.path": {"min": x} | {"max":
// y}}` object, evaluated against the SAME JSON this run would print with
// `--json` (parsed back through `serde_json::Value` so a dotted path walks
// it generically, one `.get(segment)` per `.`-separated piece) -- one
// violation line per failing or missing metric, `"assert: {path} = {actual}
// > max {y}"` / `"< min {x}"` / `"{path} missing"`.
// ---------------------------------------------------------------------------

fn lookup_metric<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// `13`, not `13.0`; `0.381`, not `0.38100000000000001` -- the same
/// whole-number-drops-its-decimal rule `cli.rs`'s `js_float_string` applies,
/// reimplemented locally rather than reused (that function is private to
/// cli.rs and out of this ticket's scope to touch beyond `J`/
/// `to_json_string`/`require_repo`).
fn fmt_num(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// Evaluates `assert_text` (the `--assert` file's contents) against
/// `report_json` (this run's `--json` shape). `Ok(violations)`, empty when
/// every threshold holds; `Err` only for a malformed assert file or a
/// threshold entry that is not `{"min": _}`/`{"max": _}`.
fn evaluate_assert(report_json: &str, assert_text: &str) -> Result<Vec<String>, String> {
    let report_value: serde_json::Value = serde_json::from_str(report_json)
        .map_err(|e| format!("internal: audit report is not valid JSON: {e}"))?;
    let assert_value: serde_json::Value = serde_json::from_str(assert_text)
        .map_err(|e| format!("assert file is not valid JSON: {e}"))?;
    let obj = assert_value.as_object().ok_or_else(|| {
        "assert file must be a JSON object of {\"path\": {\"min\"|\"max\": n}}".to_string()
    })?;

    let mut violations = Vec::new();
    for (path, spec) in obj {
        let spec_obj = spec.as_object().ok_or_else(|| {
            format!("assert entry '{path}' must be an object with a \"min\" or \"max\" key")
        })?;
        let min = spec_obj.get("min").and_then(serde_json::Value::as_f64);
        let max = spec_obj.get("max").and_then(serde_json::Value::as_f64);
        if min.is_none() && max.is_none() {
            return Err(format!(
                "assert entry '{path}' must carry a numeric \"min\" or \"max\""
            ));
        }

        let actual = lookup_metric(&report_value, path).and_then(serde_json::Value::as_f64);
        let Some(actual) = actual else {
            violations.push(format!("assert: {path} missing"));
            continue;
        };
        if let Some(min) = min {
            if actual < min {
                violations.push(format!(
                    "assert: {path} = {} < min {}",
                    fmt_num(actual),
                    fmt_num(min)
                ));
                continue;
            }
        }
        if let Some(max) = max {
            if actual > max {
                violations.push(format!(
                    "assert: {path} = {} > max {}",
                    fmt_num(actual),
                    fmt_num(max)
                ));
            }
        }
    }
    Ok(violations)
}

// ---------------------------------------------------------------------------
// `cmd_audit` -- the CLI entry point `cli.rs` dispatches `audit` to.
// ---------------------------------------------------------------------------

/// `devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json]
/// [--assert F]`. Exit 0 with the report (text, or one JSON object with
/// `--json`); exit 1 on any error (bad arguments, an unreadable/malformed
/// input file, no `.scout`/`.git` root, no graph.json) or on any `--assert`
/// violation, in which case the violation lines are appended after the
/// report.
pub(crate) fn cmd_audit(cwd: &Path, args: &[String]) -> (i32, String) {
    const USAGE: &str = "usage: devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json] [--assert F]";

    let mut semantic: Option<String> = None;
    let mut units: Option<String> = None;
    let mut defs: Option<String> = None;
    let mut assert_path: Option<String> = None;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if matches!(flag, "--semantic" | "--units" | "--defs" | "--assert") {
            let Some(val) = args.get(i + 1) else {
                return (1, format!("error: missing value for '{flag}'\n{USAGE}"));
            };
            match flag {
                "--semantic" => semantic = Some(val.clone()),
                "--units" => units = Some(val.clone()),
                "--defs" => defs = Some(val.clone()),
                "--assert" => assert_path = Some(val.clone()),
                _ => unreachable!(),
            }
            i += 2;
        } else if flag == "--json" {
            json = true;
            i += 1;
        } else {
            return (1, format!("error: unrecognized argument '{flag}'\n{USAGE}"));
        }
    }
    let Some(semantic) = semantic else {
        return (
            1,
            format!("error: --semantic <refs.jsonl> is required\n{USAGE}"),
        );
    };

    let root = match crate::cli::require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };

    let semantic_path = crate::repo::resolve_from(cwd, Path::new(&semantic));
    let units_path = units.map(|u| crate::repo::resolve_from(cwd, Path::new(&u)));
    let defs_path = defs.map(|d| crate::repo::resolve_from(cwd, Path::new(&d)));
    let opts = AuditOptions {
        semantic: &semantic_path,
        units: units_path.as_deref(),
        defs: defs_path.as_deref(),
    };

    let inputs = match load(&root, &opts) {
        Ok(i) => i,
        Err(e) => return (1, format!("error: {e}")),
    };
    let report = score(inputs);
    let json_string = render_json(&report);
    let mut out = if json {
        json_string.clone()
    } else {
        render_text(&report)
    };

    if let Some(assert_path) = assert_path {
        let abs = crate::repo::resolve_from(cwd, Path::new(&assert_path));
        let text = match std::fs::read_to_string(&abs) {
            Ok(t) => t,
            Err(e) => return (1, format!("error: failed to read {}: {e}", abs.display())),
        };
        let violations = match evaluate_assert(&json_string, &text) {
            Ok(v) => v,
            Err(e) => return (1, format!("error: {e}")),
        };
        if !violations.is_empty() {
            out.push('\n');
            out.push_str(&violations.join("\n"));
            return (1, out);
        }
    }
    (0, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extract, graph, resolve};

    // --- fixtures --------------------------------------------------------

    /// The same real-extractor/real-resolver technique `resolve.rs`'s own
    /// `fragments_for` test helper uses (not reusable from here -- it is
    /// private to that module's `#[cfg(test)]`), serialized and re-parsed
    /// into a bare `Value` -- the design note's "serialise with
    /// `serde_json::to_string`, feed the Value loader".
    fn fragments_for(files: &[(&str, &str)]) -> Vec<(String, graph::Fragment)> {
        files
            .iter()
            .map(|(rel, src)| {
                (
                    (*rel).to_string(),
                    graph::fragment_from_extraction(&extract::extract(src)),
                )
            })
            .collect()
    }

    /// Not a real git repo -- `resolve_graph`'s single I/O call (`git_head`)
    /// fails closed to `None` here, same as `resolve.rs`'s own helper of the
    /// same name.
    fn no_git_root() -> PathBuf {
        std::env::temp_dir().join("scout-audit-test-not-a-repo")
    }

    fn graph_value_for(files: &[(&str, &str)]) -> serde_json::Value {
        let frags = fragments_for(files);
        let g = resolve::resolve_graph(&no_git_root(), &frags);
        let text = serde_json::to_string(&g).expect("graph serializes");
        serde_json::from_str(&text).expect("graph.json round-trips through Value")
    }

    fn oracle_ref(
        file: &str,
        start_line: usize,
        shape: &str,
        receiver_kind: &str,
        target: Option<&str>,
        target_kind: Option<&str>,
        external: bool,
    ) -> OracleRef {
        OracleRef {
            file: file.to_string(),
            start_line,
            shape: shape.to_string(),
            receiver_kind: receiver_kind.to_string(),
            target: target.map(str::to_string),
            target_kind: target_kind.map(str::to_string),
            target_file: None,
            external,
            ambiguous: false,
        }
    }

    // --- precise TP --------------------------------------------------------

    #[test]
    fn precise_edge_at_a_matching_in_solution_site_scores_as_a_true_positive() {
        let files = [
            (
                "Other/MessageUrn.cs",
                "namespace App.Consumers { public static class MessageUrn { public static string Prefix { get; } } }",
            ),
            (
                "Consumers/UsesProperty.cs",
                "\nnamespace App.Consumers;\n\npublic class UsesProperty\n{\n  public object Get() => MessageUrn.Prefix;\n}\n",
            ),
        ];
        let value = graph_value_for(&files);
        let (graph_defs, edges) = parse_graph(&value).expect("graph.json parses");
        assert_eq!(edges.len(), 1, "exactly one uses-member edge expected");
        assert_eq!(edges[0].tier, Tier::Precise);

        let record = oracle_ref(
            &edges[0].from_file,
            edges[0].from_line,
            "access",
            "ident",
            Some(&edges[0].to),
            Some("class"),
            false,
        );
        let universe: HashSet<String> = [edges[0].from_file.clone()].into_iter().collect();
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs,
            oracle_defs: Vec::new(),
            edges,
            records: vec![record],
            units: Vec::new(),
            universe,
        });

        assert_eq!(report.tiers.len(), 1);
        let (tier, ts) = &report.tiers[0];
        assert_eq!(*tier, Tier::Precise);
        assert_eq!(ts.tp, 1);
        assert_eq!(ts.fp, 0);
        assert_eq!(report.oracle_dropped, 0);
    }

    // --- guess FP at an external site (leak) --------------------------------

    #[test]
    fn guessed_edge_at_an_all_external_site_scores_as_a_leaked_false_positive() {
        let files = [
            (
                "Other/Widget.cs",
                "namespace App.Other { public class Widget { public void Frob() { } } }",
            ),
            (
                "Consumers/Guess.cs",
                "\nnamespace App.Consumers;\n\npublic class Guess\n{\n  public void Unknown()\n  {\n    var w = Compute();\n    w.Frob();\n  }\n  private object Compute() => null;\n}\n",
            ),
        ];
        let value = graph_value_for(&files);
        let (graph_defs, edges) = parse_graph(&value).expect("graph.json parses");
        let heuristic_count = edges.iter().filter(|e| e.tier == Tier::Guess).count();
        assert_eq!(heuristic_count, 1, "expected exactly one guessed edge");
        let g_edge = edges.iter().find(|e| e.tier == Tier::Guess).unwrap();

        // The oracle saw a genuinely external member at this same site (e.g.
        // an extension method from a package devscout never indexed) --
        // external, so no guessed target can ever be a true positive here.
        let record = oracle_ref(
            &g_edge.from_file,
            g_edge.from_line,
            "access",
            "ident",
            Some("Some.External.Type"),
            Some("class"),
            true,
        );
        let universe: HashSet<String> = [g_edge.from_file.clone()].into_iter().collect();
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs,
            oracle_defs: Vec::new(),
            edges,
            records: vec![record],
            units: Vec::new(),
            universe,
        });

        assert_eq!(report.tiers.len(), 1);
        let (tier, ts) = &report.tiers[0];
        assert_eq!(*tier, Tier::Guess);
        assert_eq!(ts.tp, 0);
        assert_eq!(ts.fp, 1);
        assert_eq!(ts.fp_external_site, 1);
        assert_eq!(report.silent_leak, 1);
        assert_eq!(report.silent_correct, 0);
        assert_eq!(report.oracle_external_sites, 1);
    }

    // --- any-match with two records on one site -----------------------------

    #[test]
    fn a_matching_record_among_several_at_one_site_still_earns_a_true_positive() {
        let edge = EdgeRow {
            from_file: "F.cs".into(),
            from_line: 5,
            to: "Ns.Right".into(),
            to_file: "F.cs".into(),
            tier: Tier::Precise,
        };
        let wrong = oracle_ref(
            "F.cs",
            5,
            "access",
            "ident",
            Some("Ns.Wrong"),
            Some("class"),
            false,
        );
        let right = oracle_ref(
            "F.cs",
            5,
            "access",
            "ident",
            Some("Ns.Right"),
            Some("class"),
            false,
        );
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs: vec![DefRow {
                id: "Ns.Right".into(),
                file: "F.cs".into(),
                kind: "class".into(),
                test: false,
            }],
            oracle_defs: Vec::new(),
            edges: vec![edge],
            records: vec![wrong, right],
            units: Vec::new(),
            universe: ["F.cs".to_string()].into_iter().collect(),
        });
        let (_, ts) = &report.tiers[0];
        assert_eq!(ts.tp, 1);
        assert_eq!(ts.fp, 0);
    }

    // --- enum member, both spellings ----------------------------------------

    #[test]
    fn enum_member_edge_matches_both_the_full_and_the_bare_spelling() {
        let edge_full = EdgeRow {
            from_file: "F.cs".into(),
            from_line: 10,
            to: "Ns.OrderStatus.Open".into(),
            to_file: "Ns/OrderStatus.cs".into(),
            tier: Tier::Precise,
        };
        let edge_bare = EdgeRow {
            from_file: "F.cs".into(),
            from_line: 20,
            to: "Ns.OrderStatus".into(),
            to_file: "Ns/OrderStatus.cs".into(),
            tier: Tier::Precise,
        };
        let rec_at_full = oracle_ref(
            "F.cs",
            10,
            "access",
            "ident",
            Some("Ns.OrderStatus.Open"),
            Some("enum-member"),
            false,
        );
        let rec_at_bare = oracle_ref(
            "F.cs",
            20,
            "access",
            "ident",
            Some("Ns.OrderStatus.Open"),
            Some("enum-member"),
            false,
        );
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs: vec![DefRow {
                id: "Ns.OrderStatus".into(),
                file: "Ns/OrderStatus.cs".into(),
                kind: "enum".into(),
                test: false,
            }],
            oracle_defs: Vec::new(),
            edges: vec![edge_full, edge_bare],
            records: vec![rec_at_full, rec_at_bare],
            units: Vec::new(),
            universe: ["F.cs".to_string()].into_iter().collect(),
        });
        let (_, ts) = &report.tiers[0];
        assert_eq!(ts.tp, 2);
        assert_eq!(ts.fp, 0);
    }

    // --- legacy heuristic:true, and tier:"ext"/"guess" strings --------------

    #[test]
    fn tier_is_read_from_the_tier_string_when_present_else_from_legacy_heuristic_bool() {
        let text = r#"{"defs":[],"edges":[
            {"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.A","to_file":"A.cs","heuristic":true},
            {"kind":"uses-member","from_file":"F.cs","from_line":2,"to":"Ns.B","to_file":"B.cs","tier":"ext"},
            {"kind":"uses-member","from_file":"F.cs","from_line":3,"to":"Ns.C","to_file":"C.cs","tier":"guess"},
            {"kind":"uses-member","from_file":"F.cs","from_line":4,"to":"Ns.D","to_file":"D.cs"},
            {"kind":"inherits","from_file":"F.cs","from_line":5,"to":"Ns.E","to_file":"E.cs"}
        ]}"#;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let (_, edges) = parse_graph(&value).unwrap();
        assert_eq!(
            edges.len(),
            4,
            "the 'inherits' edge is not uses-member and must be filtered out"
        );
        let tiers: Vec<Tier> = edges.iter().map(|e| e.tier).collect();
        assert_eq!(
            tiers,
            vec![Tier::Heuristic, Tier::Ext, Tier::Guess, Tier::Precise]
        );
    }

    // --- structural: units method, and test-defs fallback -------------------

    #[test]
    fn structural_check_via_units_flags_an_edge_the_caller_project_cannot_reach() {
        let edge = EdgeRow {
            from_file: "App/A.cs".into(),
            from_line: 9,
            to: "Tests.Foo".into(),
            to_file: "Tests/Foo.cs".into(),
            tier: Tier::Heuristic,
        };
        let units = vec![
            Unit {
                name: "App".into(),
                status: "ok".into(),
                refs: vec!["Domain".into()],
                files: vec!["App/A.cs".into()],
            },
            Unit {
                name: "Domain".into(),
                status: "ok".into(),
                refs: vec![],
                files: vec![],
            },
            Unit {
                name: "Tests".into(),
                status: "ok".into(),
                refs: vec!["App".into()],
                files: vec!["Tests/Foo.cs".into()],
            },
        ];
        // Also a genuine (non-external) match at the site, to show the
        // structural flag is orthogonal to TP/FP: App can never reach
        // Tests, so this edge is structurally impossible EVEN THOUGH it is
        // also the right answer to the oracle record at its site.
        let record = oracle_ref(
            "App/A.cs",
            9,
            "access",
            "ident",
            Some("Tests.Foo"),
            Some("class"),
            false,
        );
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs: vec![DefRow {
                id: "Tests.Foo".into(),
                file: "Tests/Foo.cs".into(),
                kind: "class".into(),
                test: false,
            }],
            oracle_defs: Vec::new(),
            edges: vec![edge],
            records: vec![record],
            units,
            universe: ["App/A.cs".to_string()].into_iter().collect(),
        });
        assert_eq!(report.structural_method, "units");
        assert_eq!(report.structural_checked, 1);
        assert_eq!(report.structural_impossible, 1);
        let (_, ts) = &report.tiers[0];
        assert_eq!(ts.tp, 1, "structural is orthogonal to TP/FP");
        assert_eq!(ts.structural, 1);
    }

    #[test]
    fn structural_fallback_flags_a_non_test_caller_reaching_a_test_attributed_def() {
        let edge_bad = EdgeRow {
            from_file: "App/A.cs".into(),
            from_line: 3,
            to: "Tests.Helper".into(),
            to_file: "Tests/Helper.cs".into(),
            tier: Tier::Heuristic,
        };
        // A second edge from a file that DOES declare a test-attributed def
        // of its own -- the fallback's second clause ("from_file has no
        // test def") should clear this one.
        let edge_ok = EdgeRow {
            from_file: "Tests/Caller.cs".into(),
            from_line: 4,
            to: "Tests.Helper".into(),
            to_file: "Tests/Helper.cs".into(),
            tier: Tier::Heuristic,
        };
        let report = score(Inputs {
            root: PathBuf::from("/repo"),
            graph_defs: vec![
                DefRow {
                    id: "Tests.Helper".into(),
                    file: "Tests/Helper.cs".into(),
                    kind: "class".into(),
                    test: true,
                },
                DefRow {
                    id: "Tests.Caller".into(),
                    file: "Tests/Caller.cs".into(),
                    kind: "class".into(),
                    test: true,
                },
            ],
            oracle_defs: Vec::new(),
            edges: vec![edge_bad, edge_ok],
            records: Vec::new(),
            units: Vec::new(), // empty -> "test-defs" fallback
            universe: HashSet::new(),
        });
        assert_eq!(report.structural_method, "test-defs");
        assert_eq!(report.structural_checked, 2);
        assert_eq!(report.structural_impossible, 1);
    }

    // --- --assert: pass, fail, missing path ----------------------------------

    #[test]
    fn assert_thresholds_report_pass_fail_and_missing_path_distinctly() {
        let report_json = r#"{"tiers":{"guess":{"fp":13}},"recall":{"all":0.867}}"#;

        let pass = evaluate_assert(report_json, r#"{"recall.all":{"min":0.8}}"#).unwrap();
        assert!(pass.is_empty());

        let fail = evaluate_assert(report_json, r#"{"tiers.guess.fp":{"max":0}}"#).unwrap();
        assert_eq!(fail, vec!["assert: tiers.guess.fp = 13 > max 0"]);

        let missing =
            evaluate_assert(report_json, r#"{"tiers.precise.precision":{"min":1.0}}"#).unwrap();
        assert_eq!(missing, vec!["assert: tiers.precise.precision missing"]);
    }

    // --- JSON key-order snapshot ---------------------------------------------

    #[test]
    fn json_output_key_order_is_pinned() {
        let report = AuditReport {
            root: "/repo".to_string(),
            oracle_records: 34,
            oracle_sites: 25,
            oracle_external_sites: 8,
            oracle_ambiguous: 1,
            oracle_dropped: 2,
            units_ok: 6,
            units_failed: 0,
            structural_method: "units",
            tiers: vec![
                (
                    Tier::Precise,
                    TierStats {
                        edges: 6,
                        tp: 6,
                        fp: 0,
                        fp_no_site: 0,
                        fp_external_site: 0,
                        fp_wrong_target: 0,
                        structural: 0,
                    },
                ),
                (
                    Tier::Heuristic,
                    TierStats {
                        edges: 21,
                        tp: 8,
                        fp: 13,
                        fp_no_site: 0,
                        fp_external_site: 13,
                        fp_wrong_target: 0,
                        structural: 2,
                    },
                ),
            ],
            recall_denominator: 15,
            recall_precise: 6,
            recall_precise_ext: 7,
            recall_all: 13,
            by_receiver: vec![
                ("ident", Some(0.917)),
                ("qualified", None),
                ("this", Some(0.0)),
                ("base", None),
                ("call", Some(0.0)),
            ],
            recall_conditional: 1,
            recall_bare: 0,
            silent_correct: 4,
            silent_leak: 4,
            structural_impossible: 2,
            structural_checked: 21,
            fanout: [20, 4, 0, 0],
            unknown_targets: vec![("class".to_string(), 2)],
            top_fp: vec![("FilterConfig".to_string(), 2), ("Mailer".to_string(), 1)],
            top_missed: vec![("Fixture.Domain.Order".to_string(), 2)],
            partial_file_mismatch: 0,
        };
        let json = render_json(&report);
        assert_eq!(
            json,
            concat!(
                "{\"status\":\"ok\",\"root\":\"/repo\",",
                "\"oracle\":{\"records\":34,\"sites\":25,\"external_sites\":8,\"ambiguous\":1,\"dropped\":2},",
                "\"units\":{\"ok\":6,\"failed\":0,\"method\":\"units\"},",
                "\"tiers\":{",
                "\"precise\":{\"edges\":6,\"tp\":6,\"fp\":0,\"precision\":1.000,\"fp_no_site\":0,\"fp_external_site\":0,\"fp_wrong_target\":0,\"structural\":0},",
                "\"heuristic\":{\"edges\":21,\"tp\":8,\"fp\":13,\"precision\":0.381,\"fp_no_site\":0,\"fp_external_site\":13,\"fp_wrong_target\":0,\"structural\":2}",
                "},",
                "\"recall\":{\"denominator\":15,\"precise\":0.400,\"precise_ext\":0.467,\"all\":0.867,",
                "\"by_receiver\":{\"ident\":0.917,\"qualified\":null,\"this\":0.000,\"base\":null,\"call\":0.000},",
                "\"conditional\":1,\"bare\":0},",
                "\"silent\":{\"correct\":4,\"leak\":4},",
                "\"structural\":{\"impossible\":2,\"checked\":21,\"method\":\"units\"},",
                "\"fanout\":{\"1\":20,\"2\":4,\"3\":0,\"4+\":0},",
                "\"unknown_targets\":[{\"kind\":\"class\",\"count\":2}],",
                "\"top_fp\":[{\"name\":\"FilterConfig\",\"count\":2},{\"name\":\"Mailer\",\"count\":1}],",
                "\"top_missed\":[{\"id\":\"Fixture.Domain.Order\",\"count\":2}],",
                "\"partial_file_mismatch\":0}",
            )
        );
    }
}
