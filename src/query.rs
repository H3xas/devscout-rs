// refs/impact query support: enum-member union, `impact_walk`, and
// personalized PageRank. A `tests` block below pins the behavioral contract
// with fixture cases exercising every code path.
//
// Design notes:
//
// - **Two-phase index build.** The index cannot be built in one call that
//   also reads the graph and then hands back a struct borrowing that graph
//   -- the result would be self-referential. So the caller reads the graph
//   first with `graph::read_graph(root)`, which returns `None` when there is
//   no graph, letting the caller check the `Option` before proceeding, then
//   calls `load_graph_index(&graph, root)` here, which owns the manifest
//   join and the index build.
//
// - **Manifest corrupt-JSON fail-open.** A corrupt manifest.json surfaces as
//   `Err(ManifestError::InvalidJson)` from `manifest::read_manifest`.
//   Panicking through a query path for an operator-error edge case (a
//   hand-corrupted manifest) is worse than failing open, so
//   `load_graph_index` treats `Err(_)` the same as "no manifest" -- the same
//   fail-open-on-parse-error convention `graph::read_graph` follows. A
//   manifest present but missing its `entries` key is handled the same way.
//
// - **Insertion-order-preserving scratch structures.** Several of
//   `impact_walk`'s scratch maps/sets are NOT just membership/lookup
//   structures -- their insertion order is directly observable in the final
//   result: `Hit::symbols` feeds `top_symbols`'s first-3 truncation, and
//   `visited` key order feeds the `nodes` array handed to
//   `personalized_page_rank`, whose accumulation loop is float-addition over
//   that exact order -- not associative, so a different iteration order can
//   produce a bit-different (though not wrong) result.
//   `std::collections::{HashMap,HashSet}` make no iteration-order guarantee,
//   so this module has two tiny private order-preserving helpers,
//   `SeqSet`/`SeqMap` (a `Vec` plus an index `HashMap`, first insertion wins
//   the slot). They are pure algorithm scratch state, not artifact structs;
//   `graph::Def`/`Edge`/`Graph` are reused directly from `graph.rs`.
//
// - **`str::cmp` ordering.** Location sorts use plain Unicode-codepoint
//   `str::cmp` rather than locale-aware collation, the same accepted
//   trade-off documented at `resolve.rs`'s `capped_candidates`: the two
//   orderings coincide for every path/id shape this crate actually produces,
//   and diverge only in a pathological mix of leading case or a
//   `+`/`.`-adjacent tie -- flagged, not solved, no new ICU dependency.
//
// - **Result models.** `RefsModel`/`ImpactModel` (and their row/table
//   sub-types) are the typed surface `render.rs` consumes. Field names are
//   `snake_case` (e.g. `model.inbound.uses_type`). Nothing here is
//   `Serialize` -- `--json` output byte-shape is `cli.rs`'s job, not this
//   module's.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::graph;
use crate::manifest;
use crate::suggest::kind_rank;

// The three reference kinds and their fixed order. Not iterated generically
// here -- each kind gets its own struct field (`inherits`/`uses_type`/
// `uses_member`) throughout this module rather than a keyed collection. This
// constant exists only as an anchor for the kind list and its order.
#[allow(dead_code)]
const REF_KINDS: [&str; 3] = ["inherits", "uses-type", "uses-member"];

/// Default per-table row cap for the outbound and ambiguous tables. The three
/// inbound tables share [`INBOUND_CAP`] instead.
pub const DEFAULT_CAP: usize = 50;
/// Default cap shared across all three inbound kinds -- one cap total, not one
/// per table.
pub const INBOUND_CAP: usize = 30;
/// Default cap for the `--out` view: one cap shared across all four outbound
/// kinds (inherits/uses-type/uses-member/imports), the mirror of
/// [`INBOUND_CAP`]. `--all` lifts this cap.
pub const OUTBOUND_CAP: usize = 30;
/// Maximum length of a displayed source line, in UTF-16 code units.
/// Truncation counts code units (see `clip_source`) so the cut is stable
/// regardless of any astral characters on the line.
pub const SOURCE_MAX: usize = 120;
/// Default number of hops for the impact walk.
pub const DEFAULT_HOPS: u32 = 2;

/// Default fan-in brake: an interface injected into more than this many
/// distinct constructors is treated as infrastructure for widening purposes,
/// whatever it is called. Chosen from the constructor-injection in-degree
/// histogram of a large corpus, where 151 of 155 injected contracts (97.4%)
/// sit at or below 8 and nothing sits between 8 and the four estate-wide ones
/// (9, 11, 12, 21). `0` disables the brake (`--iface-max-fanin 0`).
pub const DEFAULT_IFACE_MAX_FANIN: usize = 8;

/// Default hub brake: a file whose in-degree is at or above this stops
/// widening. In-degree here is the number of DISTINCT OTHER FILES that
/// reference a file through a `direct` (inherits/uses-type/uses-member) or
/// heuristic edge. Derived from a corpus histogram rather than guessed: 824
/// files carry at least one referrer, every value from 1 to 33 is populated
/// (holding 777 of the 824, 94.3%), and the first empty slot is at 34, above
/// which the tail is sparse and gapped. The comparison is `>=`, so 34 is the
/// first value that is not an ordinary file. `0` disables the in-degree half
/// of the brake; the name-pattern half ([`is_infra_file`]) is a
/// classification, not a threshold, and stays on.
pub const DEFAULT_HUB_MAX_INDEGREE: usize = 34;

// The name-pattern half of the hub classification, extending the `infra` idea
// from TYPE names to FILE shapes: files the rest of an estate refers to BY
// JOB, not by dependency -- registering the container, composing the
// application, entering the process, or holding the setup every test class in
// a suite inherits.
//
// Spelled as explicit lowercase suffix/segment tests rather than a regex list,
// so the predicate is exact on any path, including ones no fixture covered.
const INFRA_BASENAMES: [&str; 2] = ["program.cs", "startup.cs"];
const INFRA_SUFFIXES: [&str; 8] = [
    "serviceextensions.cs",
    "servicecollectionextensions.cs",
    "registration.cs",
    "testbase.cs",
    "testsbase.cs",
    "basetest.cs",
    "basetests.cs",
    "basefixture.cs",
];
const INFRA_DIR: &str = "dependencyresolution/";
const COMPOSITION_ROOT: &str = "compositionroot";

/// A file whose PATH says it is infrastructure. Always on: unlike the
/// in-degree threshold, this is a classification of what the file is FOR, and
/// `0` on the threshold does not turn a composition root back into an ordinary
/// file.
pub fn is_infra_file(file: &str) -> bool {
    let lower = file.to_lowercase();
    let base = match lower.rfind('/') {
        Some(i) => &lower[i + 1..],
        None => &lower[..],
    };
    if INFRA_BASENAMES.contains(&base) {
        return true;
    }
    if INFRA_SUFFIXES.iter().any(|sfx| lower.ends_with(sfx)) {
        return true;
    }
    if lower.starts_with(INFRA_DIR) || lower.contains(&format!("/{INFRA_DIR}")) {
        return true;
    }
    // `CompositionRoot<Anything alphanumeric>.cs`, anywhere in the path's last
    // segment -- the composition root itself and the test class that asserts it.
    if let Some(at) = base.find(COMPOSITION_ROOT) {
        if base.ends_with(".cs") {
            let middle = &base[at + COMPOSITION_ROOT.len()..base.len() - 3];
            if middle.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    false
}
/// Default damping factor for the personalized PageRank pass.
pub const DEFAULT_DAMPING: f64 = 0.85;
/// Default iteration count for the personalized PageRank pass.
pub const DEFAULT_ITERATIONS: u32 = 20;

// ============================================================================
// The full name index, and the source line a hit points at.
// ============================================================================

/// The literal line a declaration sits on, ASCII-trimmed by `trim_source`.
/// Returns `""` on an unreadable file or an out-of-range line; the caller then
/// prints `file:line` with nothing after it rather than a row ending in a
/// dangling separator.
pub fn source_line(root: &Path, file: &str, line: usize) -> String {
    let Ok(body) = std::fs::read_to_string(root.join(file)) else { return String::new() };
    let lines: Vec<&str> = body.split('\n').collect();
    if line < 1 || line > lines.len() {
        return String::new();
    }
    let mut raw = lines[line - 1];
    // A UTF-8 BOM survives the read and is not in the trim set, so line 1 of a
    // BOM'd file would otherwise print it.
    if line == 1 {
        raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    }
    trim_source(raw).to_string()
}

/// Name search: every whitespace-split token of `query` has to appear,
/// case-insensitively, as a substring of the declared NAME -- path and purpose
/// are the manifest search's haystack, not this one's. The caller owns the
/// graph (see the module header's two-phase note). Results come back in index
/// build order; ranking is the caller's job.
pub fn find_names<'g>(graph: &'g graph::Graph, query: &str) -> Vec<&'g graph::GraphName> {
    let tokens: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    graph
        .names
        .iter()
        .filter(|n| {
            let hay = n.name.to_lowercase();
            tokens.iter().all(|t| hay.contains(t.as_str()))
        })
        .collect()
}

/// Every FILE's own first declaration line, keyed by path: the minimum `line`
/// across every `graph.names` entry that file carries, over the WHOLE index.
/// Unlike [`find_names`], this is never filtered to one query's matches,
/// because a manifest-pool row needs a line to open regardless of the query. A
/// file the name index carries nothing for (no declared symbol at all -- a
/// config file, a doc, an asset) has no entry here; the caller falls back to
/// line 1.
pub fn first_decl_line_by_file(graph: &graph::Graph) -> HashMap<String, usize> {
    let mut lines: HashMap<String, usize> = HashMap::new();
    for n in &graph.names {
        lines
            .entry(n.file.clone())
            .and_modify(|line| *line = (*line).min(n.line))
            .or_insert(n.line);
    }
    lines
}

/// The per-file precise inbound-edge count `find`'s tie-break ranks by.
///
/// File path -> how many PRECISE reference edges land on the definitions that
/// file declares, keyed by the edge's target file. This direct edge count IS
/// the whole centrality measure behind `find`'s tie-break -- it deliberately
/// shares nothing with `impact_walk`'s weighting (no hops, no `PageRank`, no
/// distinct-referrer folding): one precise edge, one count.
///
/// Counted: `inherits`/`uses-type`/`uses-member` without the guess tag, plus
/// the three TS reference kinds `call`/`jsx-use`/`dispatch`, which on a TS repo
/// ARE the reference graph -- leaving them out would rank every TS file at
/// zero. NOT counted: heuristic edges (a guess never enters a table a fact is
/// read from), `imports`/`import` (they name a namespace or module, never a
/// definition), `ctor-di` (DI wiring, not a reference), `ambiguous` (candidates,
/// not resolutions), and a file's references to ITSELF (`from_file ==
/// to_file`) -- how much OTHER code pulls on a file is the measure, so a
/// self-citation is no more centrality here than it is for `hub_indegree`.
pub fn file_inbound_counts(graph: &graph::Graph) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in &graph.edges {
        let (from_file, to_file) = match e {
            graph::Edge::Inherits { from_file, to_file, heuristic, .. }
            | graph::Edge::UsesType { from_file, to_file, heuristic, .. }
            | graph::Edge::UsesMember { from_file, to_file, heuristic, .. } => {
                if *heuristic {
                    continue;
                }
                (from_file, to_file)
            }
            graph::Edge::Call { from_file, to_file, .. }
            | graph::Edge::JsxUse { from_file, to_file, .. }
            | graph::Edge::Dispatch { from_file, to_file, .. } => (from_file, to_file),
            // Listed rather than caught by a wildcard so a future edge kind
            // still fails the exhaustiveness check here.
            graph::Edge::Imports { .. } | graph::Edge::Import { .. } | graph::Edge::CtorDi { .. } | graph::Edge::Ambiguous { .. } => continue,
        };
        if from_file == to_file {
            continue;
        }
        *counts.entry(to_file.clone()).or_insert(0) += 1;
    }
    counts
}

/// Collapses `suggest::kind_rank` into three tiers for `find`'s default
/// view: tier 1 is a code symbol (`kind_rank` 0 -- a type or top-level
/// declaration -- or 1 -- a member), tier 2 is a markup or binding name
/// (`kind_rank` 2), tier 3 is a resource key (`kind_rank` 3). `find` matches
/// tier 1 then 2 by default and demotes tier 3 to a one-line trailer
/// (`--resources` includes it).
pub fn name_tier(kind: &str) -> u8 {
    let rank = kind_rank(kind);
    if rank <= 1 { 1 } else { rank }
}

// ============================================================================
// Order-preserving scratch structures (private -- see module header).
// ============================================================================

/// Insertion-order-preserving set: `insert` is a no-op when the value is
/// already present (first insertion wins the position). Public because it
/// appears in [`ImpactWalkResult`]'s public fields.
#[derive(Debug, Clone)]
pub struct SeqSet<T: Eq + std::hash::Hash + Clone> {
    order: Vec<T>,
    seen: HashSet<T>,
}

impl<T: Eq + std::hash::Hash + Clone> SeqSet<T> {
    pub fn new() -> Self {
        Self { order: Vec::new(), seen: HashSet::new() }
    }
    pub fn insert(&mut self, value: T) {
        if self.seen.insert(value.clone()) {
            self.order.push(value);
        }
    }
    pub fn contains(&self, value: &T) -> bool {
        self.seen.contains(value)
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.order.iter()
    }
    pub fn into_vec(self) -> Vec<T> {
        self.order
    }
}

impl<T: Eq + std::hash::Hash + Clone> Default for SeqSet<T> {
    fn default() -> Self {
        SeqSet::new()
    }
}

/// Insertion-order-preserving `String`-keyed map with a mutable
/// get-or-insert-default accessor. Same shape as `graph::OrderedMap`, kept as
/// a separate small type here because that one exposes no `get_mut`/entry-style
/// API. Public for the same reason as [`SeqSet`] -- appears in
/// [`ImpactWalkResult::visited`].
#[derive(Debug, Clone)]
pub struct SeqMap<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> SeqMap<V> {
    pub fn new() -> Self {
        Self { entries: Vec::new(), index: HashMap::new() }
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }
    pub fn insert(&mut self, key: String, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }
}

impl<V: Default> SeqMap<V> {
    fn get_or_insert_default(&mut self, key: &str) -> &mut V {
        if let Some(&i) = self.index.get(key) {
            return &mut self.entries[i].1;
        }
        self.index.insert(key.to_string(), self.entries.len());
        self.entries.push((key.to_string(), V::default()));
        let last = self.entries.len() - 1;
        &mut self.entries[last].1
    }
}

fn push_ordered_unique(map: &mut HashMap<String, Vec<usize>>, key: &str, value: usize) {
    let v = map.entry(key.to_string()).or_default();
    if !v.contains(&value) {
        v.push(value);
    }
}

// ============================================================================
// GraphIndex -- the built query index.
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct InboundEntry {
    pub inherits: Vec<usize>,
    pub uses_type: Vec<usize>,
    pub uses_member: Vec<usize>,
}

/// The value shape of both `heuristic_inbound` and `heuristic_outbound_by_file`
/// (`inherits`/`uses-type`/`uses-member` -- note no `imports`: an imports edge
/// names a namespace string, never a def, so no tier can guess one).
/// Structurally identical to [`InboundEntry`], and deliberately the same type.
pub type HeuristicEntry = InboundEntry;

#[derive(Debug, Clone, Default)]
pub struct OutboundEntry {
    pub inherits: Vec<usize>,
    pub uses_type: Vec<usize>,
    pub uses_member: Vec<usize>,
    pub imports: Vec<usize>,
}

/// The built query index over a `Graph`. `by_id`/`by_file`/`by_simple_name`/
/// `by_lower_name` store `graph.defs` indices rather than cloned `Def`s, so a
/// def is never copied into every bucket it is reachable from.
pub struct GraphIndex<'g> {
    pub graph: &'g graph::Graph,
    /// The repository root every `file` in the graph is relative to. Held so a
    /// query can read the one source line a hit sits on; nothing else in this
    /// module touches the filesystem.
    pub root: std::path::PathBuf,
    /// Def id -> index into `graph.defs`.
    pub by_id: HashMap<String, usize>,
    /// File -> the def indices declared in it. The `Vec` preserves insertion
    /// order (defs-array order for a def's own file, then `also_in`
    /// cross-references in defs-array order) -- `impact_walk`'s next-frontier
    /// expansion iterates this in order, one of the order-sensitive paths
    /// documented in the module header.
    pub by_file: HashMap<String, Vec<usize>>,
    /// Exact name -> def indices, insertion order = `graph.defs` array order.
    pub by_simple_name: HashMap<String, Vec<usize>>,
    /// Lowercased name -> def indices.
    pub by_lower_name: HashMap<String, Vec<usize>>,
    /// Def id -> its inbound edges by kind (inherits/uses-type/uses-member).
    pub inbound: HashMap<String, InboundEntry>,
    /// File -> its outbound edges by kind (inherits/uses-type/uses-member/imports).
    pub outbound_by_file: HashMap<String, OutboundEntry>,
    /// Candidate def id -> the ambiguous edges naming it as a candidate.
    pub ambiguous_by_candidate: HashMap<String, Vec<usize>>,
    /// Source file -> the ambiguous edges originating in it.
    pub ambiguous_by_file: HashMap<String, Vec<usize>>,
    /// Def id -> its inbound HEURISTIC edges by kind. Heuristic edges live in
    /// their own adjacency, never mixed into the precise one, so every consumer
    /// (the impact walk's frontier, the refs tables, PageRank's edge set) reads
    /// only precise edges unless it asks for guesses by name. Mixing them in
    /// and filtering later is the shape that eventually leaks a guess into a
    /// fact -- a filter forgotten in one call site is silent.
    pub heuristic_inbound: HashMap<String, HeuristicEntry>,
    /// File -> its outbound HEURISTIC edges (same three-kind shape).
    pub heuristic_outbound_by_file: HashMap<String, HeuristicEntry>,
    /// File -> the defs declared in it that carry non-empty `test_methods`. A
    /// file is a TEST file here because a type declared in it carries an
    /// attribute a runner discovers, never because of what it is called. Every
    /// declaring site counts (`def.file` plus each `also_in`), and the per-def
    /// file dedup keeps two partial blocks in ONE file from listing that def
    /// twice. Holds `graph.defs` indices, same convention as the buckets above.
    pub test_defs_by_file: HashMap<String, Vec<usize>>,
    /// Files present in the graph but absent from the manifest.
    pub flagged_files: HashSet<String>,
    /// Whether a manifest was found and parsed.
    pub manifest_present: bool,
    /// Implementor def id -> every `ctor-di` edge the resolver confirmed
    /// resolves TO it. Only an edge carrying a `to` is indexed here --
    /// ambiguous/infra/unresolved edges never do, so an infra leaf like
    /// `ILogger<T>` can never enter this map by construction.
    pub ctor_di_by_to: HashMap<String, Vec<usize>>,
    /// Injected-type BARE NAME -> the number of DISTINCT CONSTRUCTOR SITES
    /// (`from_file` + `from_line`) that inject it. A `ctor-param` ref carries
    /// its constructor's line, so every parameter of one constructor shares one
    /// site; two classes in one file still count twice. This is the graph's
    /// proxy for "distinct consuming classes". Every ctor-di edge counts,
    /// whatever its resolution: how widely a contract is injected is a fact
    /// about the contract, not about whether an implementor was confirmed.
    pub ctor_di_fanin: HashMap<String, usize>,
    /// File -> the number of DISTINCT OTHER FILES that reference it through a
    /// `direct` (inherits/uses-type/uses-member) or heuristic edge. The
    /// file-level mirror of `ctor_di_fanin`, over the two edge kinds
    /// responsible for the reach a fan-in brake cannot see. Distinct REFERRING
    /// FILES, not distinct edges: one neighbour naming a hub fifty times is
    /// still one neighbour. A file's references to ITSELF are excluded -- a
    /// file is never its own dependant.
    pub hub_indegree: HashMap<String, usize>,
}

impl<'g> GraphIndex<'g> {
    /// Look up a def by id, e.g. for rendering an ambiguous candidate's
    /// `{file, line, kind}`. Spelled as a method so callers don't reach through
    /// `by_id` plus index arithmetic themselves.
    pub fn def(&self, id: &str) -> Option<&graph::Def> {
        self.by_id.get(id).map(|&i| &self.graph.defs[i])
    }
}

fn note_file(flagged: &mut HashSet<String>, manifest_paths: Option<&HashSet<String>>, file: &str) {
    if let Some(paths) = manifest_paths {
        if !file.is_empty() && !paths.contains(file) {
            flagged.insert(file.to_string());
        }
    }
}

/// Build the query index, phase two of the two-phase build (see module
/// header). `graph` is what the caller got back from `graph::read_graph(root)`;
/// `root` is used here only for the manifest join.
pub fn load_graph_index<'g>(graph: &'g graph::Graph, root: &Path) -> GraphIndex<'g> {
    let manifest_value = match manifest::read_manifest(root) {
        Ok(v) => v,
        Err(_) => None, // corrupt manifest.json: fail open, see module header
    };
    let manifest_present = manifest_value.is_some();
    let manifest_paths: Option<HashSet<String>> = manifest_value.as_ref().and_then(|m| {
        m.get("entries").and_then(|e| e.as_object()).map(|entries| entries.iter().map(|(k, _)| k.clone()).collect())
    });
    let manifest_paths_ref = manifest_paths.as_ref();

    let mut flagged_files: HashSet<String> = HashSet::new();
    let mut by_id: HashMap<String, usize> = HashMap::with_capacity(graph.defs.len());
    let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_simple_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_lower_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut test_defs_by_file: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, d) in graph.defs.iter().enumerate() {
        by_id.insert(d.id.clone(), i);
        note_file(&mut flagged_files, manifest_paths_ref, &d.file);
        push_ordered_unique(&mut by_file, &d.file, i);
        for also in &d.also_in {
            note_file(&mut flagged_files, manifest_paths_ref, &also.file);
            push_ordered_unique(&mut by_file, &also.file, i);
        }
        if !d.test_methods.is_empty() {
            let mut files: Vec<&str> = vec![d.file.as_str()];
            for also in &d.also_in {
                if !files.contains(&also.file.as_str()) {
                    files.push(also.file.as_str());
                }
            }
            for file in files {
                test_defs_by_file.entry(file.to_string()).or_default().push(i);
            }
        }
        by_simple_name.entry(d.name.clone()).or_default().push(i);
        by_lower_name.entry(d.name.to_lowercase()).or_default().push(i);
    }

    let mut inbound: HashMap<String, InboundEntry> = HashMap::new();
    let mut outbound_by_file: HashMap<String, OutboundEntry> = HashMap::new();
    let mut ambiguous_by_candidate: HashMap<String, Vec<usize>> = HashMap::new();
    let mut ambiguous_by_file: HashMap<String, Vec<usize>> = HashMap::new();
    let mut heuristic_inbound: HashMap<String, HeuristicEntry> = HashMap::new();
    let mut heuristic_outbound_by_file: HashMap<String, HeuristicEntry> = HashMap::new();
    let mut ctor_di_by_to: HashMap<String, Vec<usize>> = HashMap::new();
    let mut ctor_di_sites_by_iface: HashMap<String, HashSet<(String, usize)>> = HashMap::new();
    // File -> the distinct other files that reference it (see `hub_indegree`).
    let mut hub_referrers_by_file: HashMap<String, HashSet<String>> = HashMap::new();

    for (i, e) in graph.edges.iter().enumerate() {
        match e {
            graph::Edge::Imports { from_file, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                outbound_by_file.entry(from_file.clone()).or_default().imports.push(i);
            }
            graph::Edge::Ambiguous { from_file, candidates, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                ambiguous_by_file.entry(from_file.clone()).or_default().push(i);
                for c in candidates {
                    ambiguous_by_candidate.entry(c.id.clone()).or_default().push(i);
                }
            }
            graph::Edge::Inherits { from_file, to, to_file, heuristic, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file.entry(to_file.clone()).or_default().insert(from_file.clone());
                }
                if *heuristic {
                    heuristic_outbound_by_file.entry(from_file.clone()).or_default().inherits.push(i);
                    heuristic_inbound.entry(to.clone()).or_default().inherits.push(i);
                } else {
                    outbound_by_file.entry(from_file.clone()).or_default().inherits.push(i);
                    inbound.entry(to.clone()).or_default().inherits.push(i);
                }
            }
            graph::Edge::UsesType { from_file, to, to_file, heuristic, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file.entry(to_file.clone()).or_default().insert(from_file.clone());
                }
                if *heuristic {
                    heuristic_outbound_by_file.entry(from_file.clone()).or_default().uses_type.push(i);
                    heuristic_inbound.entry(to.clone()).or_default().uses_type.push(i);
                } else {
                    outbound_by_file.entry(from_file.clone()).or_default().uses_type.push(i);
                    inbound.entry(to.clone()).or_default().uses_type.push(i);
                }
            }
            graph::Edge::UsesMember { from_file, to, to_file, heuristic, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file.entry(to_file.clone()).or_default().insert(from_file.clone());
                }
                if *heuristic {
                    heuristic_outbound_by_file.entry(from_file.clone()).or_default().uses_member.push(i);
                    heuristic_inbound.entry(to.clone()).or_default().uses_member.push(i);
                } else {
                    outbound_by_file.entry(from_file.clone()).or_default().uses_member.push(i);
                    inbound.entry(to.clone()).or_default().uses_member.push(i);
                }
            }
            // 'ctor-di' is deliberately NOT one of the kinds `refs`/`impact`
            // render -- that kind list is fixed to
            // inherits/uses-type/uses-member -- so it earns no `inbound`/
            // `outbound_by_file` entry. It DOES earn its own reverse index,
            // keyed by the resolved implementor, when it carries one.
            graph::Edge::CtorDi { from_file, from_line, iface, to, .. } => {
                // Fan-in counts EVERY ctor-di edge, resolved or not; only the
                // reverse index below is restricted to the ones carrying a
                // confirmed implementor.
                ctor_di_sites_by_iface
                    .entry(iface.clone())
                    .or_default()
                    .insert((from_file.clone(), *from_line));
                if let Some(to) = to {
                    ctor_di_by_to.entry(to.clone()).or_default().push(i);
                }
            }
            // The four TS/TSX edge kinds earn no entry in this C#-shaped index,
            // the same stance the `ctor-di` arm above states for its own kind:
            // `refs`/`impact` render a fixed kind list. Listed rather than
            // caught by a wildcard so a future edge kind still fails the
            // exhaustiveness check here.
            graph::Edge::Import { .. }
            | graph::Edge::Call { .. }
            | graph::Edge::JsxUse { .. }
            | graph::Edge::Dispatch { .. } => {}
        }
    }

    GraphIndex {
        graph,
        root: root.to_path_buf(),
        by_id,
        by_file,
        by_simple_name,
        by_lower_name,
        inbound,
        outbound_by_file,
        ambiguous_by_candidate,
        ambiguous_by_file,
        heuristic_inbound,
        heuristic_outbound_by_file,
        test_defs_by_file,
        flagged_files,
        manifest_present,
        ctor_di_by_to,
        ctor_di_fanin: ctor_di_sites_by_iface.into_iter().map(|(name, sites)| (name, sites.len())).collect(),
        hub_indegree: hub_referrers_by_file.into_iter().map(|(file, refs)| (file, refs.len())).collect(),
    }
}

// The interface def id(s) a class def's OWN file(s) declare an `inherits` edge
// to, restricted to a def of kind `"interface"` -- a plain base class is not
// part of the interface hop. Reuses `outbound_by_file`'s file-level union
// rather than attributing an edge to one specific def in a multi-type file (an
// accepted imprecision). Insertion order is `def_files(def_id)` order, then
// that file's own inherits-edge array order -- the first-seen order of the
// `via` labels a widened hit carries.
fn implemented_interfaces(index: &GraphIndex, def_id: &str) -> Vec<String> {
    let mut seen: SeqSet<String> = SeqSet::new();
    for file in def_files(index, def_id) {
        let Some(o) = index.outbound_by_file.get(&file) else { continue };
        for &ei in &o.inherits {
            let graph::Edge::Inherits { to, .. } = &index.graph.edges[ei] else { continue };
            if seen.contains(to) {
                continue;
            }
            if index.def(to).map(|d| d.kind.as_str()) != Some("interface") {
                continue;
            }
            seen.insert(to.clone());
        }
    }
    seen.into_vec()
}

// ============================================================================
// resolve_symbol.
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Resolved(String),
    Ambiguous(Vec<String>),
    NotFound,
}

/// "Never guess" resolution ladder: exact id, then unique exact name, then
/// unique case-insensitive name; two-or-more candidates at any step is reported
/// as ambiguous, not resolved further.
pub fn resolve_symbol(index: &GraphIndex, query: &str) -> Resolution {
    if index.by_id.contains_key(query) {
        return Resolution::Resolved(query.to_string());
    }
    if let Some(exact) = index.by_simple_name.get(query) {
        if exact.len() == 1 {
            return Resolution::Resolved(index.graph.defs[exact[0]].id.clone());
        }
        if exact.len() > 1 {
            return Resolution::Ambiguous(exact.iter().map(|&i| index.graph.defs[i].id.clone()).collect());
        }
    }
    let lower = query.to_lowercase();
    if let Some(ci) = index.by_lower_name.get(&lower) {
        if ci.len() == 1 {
            return Resolution::Resolved(index.graph.defs[ci[0]].id.clone());
        }
        if ci.len() > 1 {
            return Resolution::Ambiguous(ci.iter().map(|&i| index.graph.defs[i].id.clone()).collect());
        }
    }
    // A TAIL of a def id, which is how a caller spells an enum member:
    // `Toggles.EnableX`, never the namespace-qualified
    // `App.Config.Toggles.EnableX` the graph keys it under. Reached only after
    // every step above has missed, so no query that resolved before still
    // resolves differently. Restricted to a dotted query on purpose -- every
    // def is indexed under its simple name, so `by_simple_name` above is
    // already exhaustive for an undotted one and this step could only repeat
    // it. Two or more ids ending in the same tail is an ambiguity, reported as
    // one rather than guessed at. Iterates `graph.defs` directly (its array
    // order) rather than the unordered `by_id` map.
    if query.contains('.') {
        let suffix = format!(".{query}");
        let tail: Vec<String> =
            index.graph.defs.iter().filter(|d| d.id.ends_with(&suffix)).map(|d| d.id.clone()).collect();
        if tail.len() == 1 {
            return Resolution::Resolved(tail.into_iter().next().expect("one tail match"));
        }
        if tail.len() > 1 {
            return Resolution::Ambiguous(tail);
        }
    }
    Resolution::NotFound
}

// ============================================================================
// def_files / def_sites.
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct DefSite {
    pub file: String,
    pub line: usize,
}

/// The distinct files a def is declared in, deduped: a partial type's
/// `also_in` site can share a file with its primary declaration.
pub fn def_files(index: &GraphIndex, def_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(&i) = index.by_id.get(def_id) {
        let d = &index.graph.defs[i];
        if seen.insert(d.file.clone()) {
            out.push(d.file.clone());
        }
        for also in &d.also_in {
            if seen.insert(also.file.clone()) {
                out.push(also.file.clone());
            }
        }
    }
    out
}

/// Every declaring site of a def, NOT deduped (unlike [`def_files`]) -- each
/// site is its own row.
pub fn def_sites(index: &GraphIndex, def_id: &str) -> Vec<DefSite> {
    let mut out = Vec::new();
    if let Some(&i) = index.by_id.get(def_id) {
        let d = &index.graph.defs[i];
        out.push(DefSite { file: d.file.clone(), line: d.line });
        for also in &d.also_in {
            out.push(DefSite { file: also.file.clone(), line: also.line });
        }
    }
    out
}

// ============================================================================
// symbol_refs.
// ============================================================================

/// The immediate inbound/outbound edges of one resolved symbol, holding
/// `graph.edges` indices rather than cloned edges (the same convention
/// [`GraphIndex`] uses).
#[derive(Debug, Clone, Default)]
pub struct SymbolRefs {
    pub inbound_inherits: Vec<usize>,
    pub inbound_uses_type: Vec<usize>,
    pub inbound_uses_member: Vec<usize>,
    pub outbound_inherits: Vec<usize>,
    pub outbound_uses_type: Vec<usize>,
    pub outbound_uses_member: Vec<usize>,
    pub outbound_imports: Vec<usize>,
    /// The same two tables again over the HEURISTIC adjacency, built by the
    /// identical rules (enum members union into the enum's inbound, partial
    /// classes union outbound over every declaring file) so a heuristic row is
    /// never present or absent for a reason a precise row would not have been.
    pub heuristic_inbound_inherits: Vec<usize>,
    pub heuristic_inbound_uses_type: Vec<usize>,
    pub heuristic_inbound_uses_member: Vec<usize>,
    pub heuristic_outbound_inherits: Vec<usize>,
    pub heuristic_outbound_uses_type: Vec<usize>,
    pub heuristic_outbound_uses_member: Vec<usize>,
    pub ambiguous_inbound: Vec<usize>,
    pub ambiguous_outbound: Vec<usize>,
}

/// Immediate (1-hop) inbound/outbound for one resolved symbol. Partial-class
/// defs union outbound over every declaring file. An enum query unions every
/// member's inbound `uses-member` edges in, iterating `index.graph.defs`
/// directly (its array order) rather than the unordered `by_id` map.
pub fn symbol_refs(index: &GraphIndex, def_id: &str) -> SymbolRefs {
    let empty = InboundEntry::default();
    let base = index.inbound.get(def_id).unwrap_or(&empty);
    let inbound_inherits = base.inherits.clone();
    let inbound_uses_type = base.uses_type.clone();
    let mut inbound_uses_member = base.uses_member.clone();

    if let Some(&i) = index.by_id.get(def_id) {
        if index.graph.defs[i].kind == "enum" {
            let prefix = format!("{def_id}.");
            for d in &index.graph.defs {
                if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
                    continue;
                }
                if let Some(m) = index.inbound.get(&d.id) {
                    inbound_uses_member.extend(m.uses_member.iter().copied());
                }
            }
        }
    }

    let mut outbound_inherits = Vec::new();
    let mut outbound_uses_type = Vec::new();
    let mut outbound_uses_member = Vec::new();
    let mut outbound_imports = Vec::new();
    for file in def_files(index, def_id) {
        if let Some(o) = index.outbound_by_file.get(&file) {
            outbound_inherits.extend(o.inherits.iter().copied());
            outbound_uses_type.extend(o.uses_type.iter().copied());
            outbound_uses_member.extend(o.uses_member.iter().copied());
            outbound_imports.extend(o.imports.iter().copied());
        }
    }

    // The heuristic halves, same rules, separate adjacency.
    let heuristic_base = index.heuristic_inbound.get(def_id).unwrap_or(&empty);
    let heuristic_inbound_inherits = heuristic_base.inherits.clone();
    let heuristic_inbound_uses_type = heuristic_base.uses_type.clone();
    let mut heuristic_inbound_uses_member = heuristic_base.uses_member.clone();

    if let Some(&i) = index.by_id.get(def_id) {
        if index.graph.defs[i].kind == "enum" {
            let prefix = format!("{def_id}.");
            for d in &index.graph.defs {
                if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
                    continue;
                }
                if let Some(m) = index.heuristic_inbound.get(&d.id) {
                    heuristic_inbound_uses_member.extend(m.uses_member.iter().copied());
                }
            }
        }
    }

    let mut heuristic_outbound_inherits = Vec::new();
    let mut heuristic_outbound_uses_type = Vec::new();
    let mut heuristic_outbound_uses_member = Vec::new();
    for file in def_files(index, def_id) {
        if let Some(o) = index.heuristic_outbound_by_file.get(&file) {
            heuristic_outbound_inherits.extend(o.inherits.iter().copied());
            heuristic_outbound_uses_type.extend(o.uses_type.iter().copied());
            heuristic_outbound_uses_member.extend(o.uses_member.iter().copied());
        }
    }

    let ambiguous_inbound = index.ambiguous_by_candidate.get(def_id).cloned().unwrap_or_default();
    let ambiguous_outbound: Vec<usize> = def_files(index, def_id)
        .iter()
        .flat_map(|f| index.ambiguous_by_file.get(f).cloned().unwrap_or_default())
        .collect();

    SymbolRefs {
        inbound_inherits,
        inbound_uses_type,
        inbound_uses_member,
        outbound_inherits,
        outbound_uses_type,
        outbound_uses_member,
        outbound_imports,
        heuristic_inbound_inherits,
        heuristic_inbound_uses_type,
        heuristic_inbound_uses_member,
        heuristic_outbound_inherits,
        heuristic_outbound_uses_type,
        heuristic_outbound_uses_member,
        ambiguous_inbound,
        ambiguous_outbound,
    }
}

// ============================================================================
// build_refs_model.
// ============================================================================

fn cap_rows<T>(mut rows: Vec<T>, cap: usize) -> (Vec<T>, usize) {
    if rows.len() <= cap {
        (rows, 0)
    } else {
        let dropped = rows.len() - cap;
        rows.truncate(cap);
        (rows, dropped)
    }
}

// The (from_file, from_line) an edge originates at, the key location sorts use.
// See the module header re: `str::cmp` ordering.
fn edge_loc(e: &graph::Edge) -> (&str, usize) {
    match e {
        graph::Edge::Inherits { from_file, from_line, .. }
        | graph::Edge::UsesType { from_file, from_line, .. }
        | graph::Edge::UsesMember { from_file, from_line, .. }
        | graph::Edge::Imports { from_file, from_line, .. }
        | graph::Edge::Ambiguous { from_file, from_line, .. }
        // Never actually reached: 'ctor-di' edges are never pushed into any
        // structure this helper sorts (see the query-index builder's own
        // CtorDi arm). Included only for exhaustiveness, as are the four
        // TS/TSX kinds below, for the same reason.
        | graph::Edge::CtorDi { from_file, from_line, .. }
        | graph::Edge::Import { from_file, from_line, .. }
        | graph::Edge::Call { from_file, from_line, .. }
        | graph::Edge::JsxUse { from_file, from_line, .. }
        | graph::Edge::Dispatch { from_file, from_line, .. } => (from_file.as_str(), *from_line),
    }
}

fn loc_cmp(a: &graph::Edge, b: &graph::Edge) -> std::cmp::Ordering {
    let (af, al) = edge_loc(a);
    let (bf, bl) = edge_loc(b);
    if af == bf { al.cmp(&bl) } else { af.cmp(bf) }
}

/// One inbound-table row: `file` and `line` of the referencing site, then
/// `heuristic` (whether the edge was guessed) and `source` (the trimmed
/// referencing line). An empty `source` is omitted from `--json`.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundRow {
    pub file: String,
    pub line: usize,
    pub heuristic: bool,
    pub source: String,
}

/// One outbound-table row: `file`/`line` of the referencing site, `to_file`/`to`
/// of the target, then `heuristic` and `source` (same omit-when-empty rule as
/// [`InboundRow`]). `source` is read at the site actually making the reference
/// -- for an outbound edge that is a line in the def's own file, not the
/// target's.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundRow {
    pub file: String,
    pub line: usize,
    pub to_file: String,
    pub to: String,
    pub heuristic: bool,
    pub source: String,
}

/// One imports-table row: `file`/`line`, the imported `target` namespace, and
/// `source`. An imports edge is never a guess, so unlike [`OutboundRow`] this
/// carries no `heuristic` field.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportRow {
    pub file: String,
    pub line: usize,
    pub target: String,
    pub source: String,
}

/// One ambiguous-table row: the referencing `file`/`line`, the `origin` and
/// `raw` text of the reference, and how many candidates it matched.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousRow {
    pub file: String,
    pub line: usize,
    pub origin: String,
    pub raw: String,
    pub candidate_count: usize,
}

/// A capped table: `total` rows before capping, how many were `dropped`, and
/// the surviving `rows`.
#[derive(Debug, Clone, PartialEq)]
pub struct Table<R> {
    pub total: usize,
    pub dropped: usize,
    pub rows: Vec<R>,
}

fn build_table<R>(mut idxs: Vec<usize>, edges: &[graph::Edge], cap: usize, map_row: fn(&graph::Edge) -> R) -> Table<R> {
    idxs.sort_by(|&a, &b| loc_cmp(&edges[a], &edges[b]));
    let total = idxs.len();
    let (shown, dropped) = cap_rows(idxs, cap);
    let rows = shown.into_iter().map(|i| map_row(&edges[i])).collect();
    Table { total, dropped, rows }
}

// The per-table, per-kind outbound builder was removed here: the outbound
// tables now go through `build_outbound_tables`'s shared cap and rank instead
// (below `RankedOutbound`), the same way the inbound tables stopped going
// through a per-kind builder. `build_table` above still backs the two
// ambiguous tables.

fn ambiguous_row(e: &graph::Edge) -> AmbiguousRow {
    match e {
        graph::Edge::Ambiguous { origin, from_file, from_line, raw, candidate_count, .. } => AmbiguousRow {
            file: from_file.clone(),
            line: *from_line,
            origin: origin.clone(),
            raw: raw.clone(),
            candidate_count: *candidate_count,
        },
        _ => unreachable!("ambiguous table only ever holds ambiguous edge indices"),
    }
}

/// The three inbound tables, one per kind (inherits/uses-type/uses-member).
#[derive(Debug, Clone, PartialEq)]
pub struct InboundTables {
    pub inherits: Table<InboundRow>,
    pub uses_type: Table<InboundRow>,
    pub uses_member: Table<InboundRow>,
}

/// The four outbound tables: three by kind plus imports.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundTables {
    pub inherits: Table<OutboundRow>,
    pub uses_type: Table<OutboundRow>,
    pub uses_member: Table<OutboundRow>,
    pub imports: Table<ImportRow>,
}

/// The inbound and outbound ambiguous-reference tables.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousTables {
    pub inbound: Table<AmbiguousRow>,
    pub outbound: Table<AmbiguousRow>,
}

/// One enum member that carries at least one inbound reference.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefEntry {
    pub name: String,
    pub count: usize,
}

/// How many of an ENUM's inbound member edges land on one of its members,
/// split by member.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefs {
    pub total: usize,
    pub member_count: usize,
    pub members: Vec<MemberRefEntry>,
    pub dropped: usize,
}

// Maximum number of per-member rows kept in a `MemberRefs`.
const MEMBER_NAME_CAP: usize = 5;

// The edges themselves are already there (a `Toggles.EnableX` access resolves
// to the member def and `symbol_refs` unions every member's inbound into the
// enum's), but a caller reading `refs Toggles` could not tell a use of the
// TYPE from a use of a member, and the per-member split is the thing an enum
// question is usually actually about.
//
// Member order is def-table order (declaration order), never by count: a
// stable list is what makes two runs print the same line. Iterates
// `graph.defs` directly for the same reason `symbol_refs`'s own enum union
// does -- that array order is the order to preserve.
fn enum_member_refs(index: &GraphIndex, def_id: &str) -> Option<MemberRefs> {
    let prefix = format!("{def_id}.");
    let mut members: Vec<MemberRefEntry> = Vec::new();
    let mut total = 0usize;
    for d in &index.graph.defs {
        if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
            continue;
        }
        let count = index.inbound.get(&d.id).map_or(0, |e| e.uses_member.len())
            + index.heuristic_inbound.get(&d.id).map_or(0, |e| e.uses_member.len());
        if count == 0 {
            continue;
        }
        members.push(MemberRefEntry { name: d.name.clone(), count });
        total += count;
    }
    if total == 0 {
        return None;
    }
    let member_count = members.len();
    let dropped = member_count.saturating_sub(MEMBER_NAME_CAP);
    members.truncate(MEMBER_NAME_CAP);
    Some(MemberRefs { total, member_count, members, dropped })
}

/// The resolved `refs` result for one symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct RefsModel {
    pub query: String,
    pub id: String,
    pub kind: String,
    pub sites: Vec<DefSite>,
    pub inbound: InboundTables,
    /// Present only under `--out`; `None` otherwise, which is what keeps
    /// `--json` and the two text renderers agreeing about whether the outbound
    /// side exists at all.
    pub outbound: Option<OutboundTables>,
    pub ambiguous: AmbiguousTables,
    /// Number of files present in the graph but absent from the manifest.
    pub manifest_gap: usize,
    /// `Some` only for an enum with member-level references; `None` for every
    /// other symbol.
    pub member_refs: Option<MemberRefs>,
}

/// The outcome of a `refs` query: resolved, ambiguous, a bare-member answer,
/// or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum RefsResult {
    Resolved(RefsModel),
    Ambiguous(Vec<String>),
    /// A bare-member answer: one resolved-shaped model per declaring type.
    Members(Vec<RefsModel>),
    NotFound,
}

// A file's project is the first segment of its repo-relative path -- the graph
// carries no other grouping, and the paths it stores are always repo-relative
// with `/` separators. A file at the repo root has the empty project, which
// only ever matches another root file.
fn project_of(file: &str) -> &str {
    match file.find('/') {
        Some(i) => &file[..i],
        None => "",
    }
}

// Trims exactly this ASCII set and nothing else. `str::trim` would strip the
// full Unicode White_Space property (U+00A0 included) but not U+FEFF; a fixed
// explicit set keeps trimming stable and independent of the Unicode tables, so
// a line whose first or last character is one of these is trimmed predictably.
fn trim_source(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n' | '\u{0b}' | '\u{0c}'))
}

/// Per-file cache of source lines, so a widely used type does not re-read one
/// consumer file once per hit. The value is `None` for a file that could not
/// be read.
pub type LineCache = HashMap<String, Option<Vec<String>>>;

fn cached_line(root: &Path, file: &str, line: usize, cache: &mut LineCache) -> String {
    let lines = cache.entry(file.to_string()).or_insert_with(|| {
        std::fs::read_to_string(root.join(file)).ok().map(|body| body.split('\n').map(str::to_string).collect())
    });
    let Some(lines) = lines else { return String::new() };
    if line < 1 || line > lines.len() {
        return String::new();
    }
    let mut raw = lines[line - 1].as_str();
    // A UTF-8 BOM survives `read_to_string` and is not in the trim set, so line
    // 1 of a BOM'd file would otherwise print it.
    if line == 1 {
        raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    }
    trim_source(raw).to_string()
}

// Interior tabs collapse to one space each and the result is cut to `SOURCE_MAX`.
fn clip_source(text: &str) -> String {
    let text = text.replace('\t', " ");
    // Cutting at `SOURCE_MAX` UTF-16 code units can split a surrogate pair; the
    // lone surrogate is then emitted as U+FFFD, which is what `from_utf16_lossy`
    // produces here.
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > SOURCE_MAX {
        String::from_utf16_lossy(&units[..SOURCE_MAX])
    } else {
        text
    }
}

// The one trimmed, clipped line of source a refs hit sits on, so a caller can
// judge the hit without opening the file.
fn hit_source(root: &Path, file: &str, line: usize, cache: &mut LineCache) -> String {
    clip_source(&cached_line(root, file, line, cache))
}

// A `uses-member` edge records the member's declaring TYPE, the file and the
// line, and never the member's own name, so no bare-member answer can be read
// off an edge. This whole-token test stands in for the field that is missing:
// an occurrence with a word character on either side does not count, so `Foo`
// never answers for a line whose only occurrence is `FooEx`. A word character
// is an ASCII letter, an ASCII digit or `_`, and nothing else -- every other
// code point, non-ASCII included, is a boundary. That rule is stated as a
// literal test rather than a character class so it does not depend on any
// regex engine's Unicode handling.
//
// The scan works over UTF-8 bytes: a valid UTF-8 needle can only match at a
// character boundary (lead and continuation byte ranges are disjoint), and
// every byte outside ASCII fails the word-character test on both sides.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn line_has_token(line: &str, token: &str) -> bool {
    let hay = line.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    for at in 0..=(hay.len() - needle.len()) {
        if &hay[at..at + needle.len()] != needle {
            continue;
        }
        let after_at = at + needle.len();
        let before_ok = at == 0 || !is_word_byte(hay[at - 1]);
        let after_ok = after_at >= hay.len() || !is_word_byte(hay[after_at]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

// Every type that declares `name`, in name-index order, each with the sites it
// declares it at. Two overloads are two sites on ONE type, never two
// candidates. Markup and resource rows carry no `owner`, so nothing a markup
// file names can be mistaken for a member.
fn member_owners(index: &GraphIndex, name: &str) -> Vec<(String, Vec<DefSite>)> {
    let mut out: Vec<(String, Vec<DefSite>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for n in &index.graph.names {
        if n.name != name || n.owner.is_empty() || !index.by_id.contains_key(&n.owner) {
            continue;
        }
        let site = DefSite { file: n.file.clone(), line: n.line };
        match at.get(n.owner.as_str()) {
            Some(&i) => out[i].1.push(site),
            None => {
                at.insert(n.owner.as_str(), out.len());
                out.push((n.owner.clone(), vec![site]));
            }
        }
    }
    out
}

// The two non-empty outcomes of `build_member_refs_models`: a plain models
// array, or an ambiguity -- more than one declaring type surviving edge-line
// verification is reported, never turned into several models.
enum MemberRefsOutcome {
    Models(Vec<RefsModel>),
    Ambiguous(Vec<String>),
}

// `refs <bare member>`: the name index names the declaring type(s); each of
// that type's inbound `uses-member` edges survives only if the line it starts
// on carries the member as a whole token. A type whose edges all fail
// verification contributes no model at all, and when none survives the caller
// takes the zero-hit path.
//
// More than one declaring type surviving verification answers
// `Ambiguous(owner_ids)`, in name-index order, rather than several models --
// the house rule of never guessing between candidates. Overloads of one name
// on ONE type are one group (one entry in `owners`, several sites), never an
// ambiguity.
fn build_member_refs_models(index: &GraphIndex, name: &str, inbound_cap: usize) -> Option<MemberRefsOutcome> {
    let owners = member_owners(index, name);
    if owners.is_empty() {
        return None;
    }
    let edges = &index.graph.edges;
    let mut cache: LineCache = HashMap::new();
    let mut groups: Vec<(String, Vec<DefSite>, Vec<(usize, bool)>)> = Vec::new();
    for (owner, sites) in owners {
        let refs = symbol_refs(index, &owner);
        let owner_def = &index.graph.defs[index.by_id[&owner]];
        let owner_project = project_of(&owner_def.file).to_string();
        let mut kept: Vec<(usize, bool)> = Vec::new();
        for &e in &refs.inbound_uses_member {
            kept.push((e, false));
        }
        for &e in &refs.heuristic_inbound_uses_member {
            kept.push((e, true));
        }
        kept.retain(|&(e, _)| {
            let (file, line) = edge_loc(&edges[e]);
            line_has_token(&cached_line(&index.root, file, line, &mut cache), name)
        });
        // Same ranking a type's inbound table uses: precise before guessed, the
        // declaring type's own project before every other, then file, then
        // line.
        let foreign = |e: usize| usize::from(project_of(edge_loc(&edges[e]).0) != owner_project);
        kept.sort_by(|a, b| {
            a.1.cmp(&b.1).then_with(|| foreign(a.0).cmp(&foreign(b.0))).then_with(|| loc_cmp(&edges[a.0], &edges[b.0]))
        });
        if !kept.is_empty() {
            groups.push((owner, sites, kept));
        }
    }
    if groups.is_empty() {
        return None;
    }
    // Reported before the inbound cap is ever spent, so the answer does not
    // depend on `inbound_cap`: an ambiguity is a refusal, not a budgeted,
    // capped multi-block resolution.
    if groups.len() > 1 {
        return Some(MemberRefsOutcome::Ambiguous(groups.into_iter().map(|(owner, _, _)| owner).collect()));
    }

    fn empty<R>() -> Table<R> {
        Table { total: 0, dropped: 0, rows: Vec::new() }
    }
    let mut budget = inbound_cap;
    let mut models = Vec::new();
    for (owner, sites, kept) in groups {
        let take = budget.min(kept.len());
        budget -= take;
        let mut rows = Vec::new();
        for &(e, heuristic) in &kept[..take] {
            let (file, line) = edge_loc(&edges[e]);
            let source = clip_source(&cached_line(&index.root, file, line, &mut cache));
            rows.push(InboundRow { file: file.to_string(), line, heuristic, source });
        }
        let total = kept.len();
        models.push(RefsModel {
            query: name.to_string(),
            id: format!("{owner}.{name}"),
            kind: "member".to_string(),
            sites,
            inbound: InboundTables {
                inherits: empty(),
                uses_type: empty(),
                uses_member: Table { total, dropped: total - rows.len(), rows },
            },
            outbound: None,
            ambiguous: AmbiguousTables { inbound: empty(), outbound: empty() },
            manifest_gap: index.flagged_files.len(),
            member_refs: None,
        });
    }
    Some(MemberRefsOutcome::Models(models))
}

/// One inbound edge awaiting the global cap: which kind's table it belongs to,
/// which edge it is, and whether it was guessed.
struct RankedInbound {
    kind: usize,
    edge: usize,
    heuristic: bool,
}

/// One outbound edge awaiting the shared `--out` cap: which of the four kinds
/// it belongs to (0=inherits, 1=uses-type, 2=uses-member, 3=imports), which
/// edge it is, and whether it was guessed. `imports` is never a guess -- the
/// builder never marks one heuristic, by construction (see
/// `build_outbound_tables` below).
struct RankedOutbound {
    kind: usize,
    edge: usize,
    heuristic: bool,
}

// The three ref kinds name a `to_file` -- ranked same-project/foreign against
// it, exactly as an inbound edge ranks its `from_file`. An imports edge names a
// namespace string, never a file, so nothing proves it shares the def's own
// project: it never earns the same-project rank and always sorts as foreign,
// the never-guess rule applied to ranking rather than to resolution.
fn outbound_foreign(def_project: &str, edges: &[graph::Edge], r: &RankedOutbound) -> usize {
    if r.kind == 3 {
        return 1;
    }
    let to_file = match &edges[r.edge] {
        graph::Edge::Inherits { to_file, .. } | graph::Edge::UsesType { to_file, .. } | graph::Edge::UsesMember { to_file, .. } => {
            to_file.as_str()
        }
        _ => unreachable!("outbound ranked kinds 0-2 only ever hold inherits/uses-type/uses-member edge indices"),
    };
    usize::from(project_of(to_file) != def_project)
}

// The four outbound kinds share ONE cap and ONE ranking under `--out`,
// mirroring `build_refs_model`'s own inbound block below: resolved before
// heuristic, then the def's own project before every other (imports always
// foreign, per `outbound_foreign` above), then file, then line. Each shown hit
// carries the same trimmed source line an inbound hit does, read at the SAME
// `from_line` an inbound row reads -- for an outbound edge that is a line in
// the def's own file, the site actually making the reference, not the caller's.
fn build_outbound_tables(refs: &SymbolRefs, def_project: &str, edges: &[graph::Edge], root: &Path, outbound_cap: usize) -> OutboundTables {
    let mut ranked: Vec<RankedOutbound> = Vec::new();
    for &e in &refs.outbound_inherits {
        ranked.push(RankedOutbound { kind: 0, edge: e, heuristic: false });
    }
    for &e in &refs.heuristic_outbound_inherits {
        ranked.push(RankedOutbound { kind: 0, edge: e, heuristic: true });
    }
    for &e in &refs.outbound_uses_type {
        ranked.push(RankedOutbound { kind: 1, edge: e, heuristic: false });
    }
    for &e in &refs.heuristic_outbound_uses_type {
        ranked.push(RankedOutbound { kind: 1, edge: e, heuristic: true });
    }
    for &e in &refs.outbound_uses_member {
        ranked.push(RankedOutbound { kind: 2, edge: e, heuristic: false });
    }
    for &e in &refs.heuristic_outbound_uses_member {
        ranked.push(RankedOutbound { kind: 2, edge: e, heuristic: true });
    }
    for &e in &refs.outbound_imports {
        ranked.push(RankedOutbound { kind: 3, edge: e, heuristic: false });
    }
    let mut totals = [0usize; 4];
    for r in &ranked {
        totals[r.kind] += 1;
    }
    ranked.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| outbound_foreign(def_project, edges, a).cmp(&outbound_foreign(def_project, edges, b)))
            .then_with(|| loc_cmp(&edges[a.edge], &edges[b.edge]))
    });
    let (shown, _) = cap_rows(ranked, outbound_cap);

    let mut source_cache: LineCache = HashMap::new();
    let mut inherits = Vec::new();
    let mut uses_type = Vec::new();
    let mut uses_member = Vec::new();
    let mut imports = Vec::new();
    for r in shown {
        let (file, line) = edge_loc(&edges[r.edge]);
        let source = hit_source(root, file, line, &mut source_cache);
        if r.kind == 3 {
            let graph::Edge::Imports { target, .. } = &edges[r.edge] else {
                unreachable!("outbound kind 3 only ever holds imports edge indices");
            };
            imports.push(ImportRow { file: file.to_string(), line, target: target.clone(), source });
            continue;
        }
        let (to_file, to) = match &edges[r.edge] {
            graph::Edge::Inherits { to_file, to, .. }
            | graph::Edge::UsesType { to_file, to, .. }
            | graph::Edge::UsesMember { to_file, to, .. } => (to_file.clone(), to.clone()),
            _ => unreachable!("outbound ranked kinds 0-2 only ever hold inherits/uses-type/uses-member edge indices"),
        };
        let row = OutboundRow { file: file.to_string(), line, to_file, to, heuristic: r.heuristic, source };
        match r.kind {
            0 => inherits.push(row),
            1 => uses_type.push(row),
            _ => uses_member.push(row),
        }
    }

    OutboundTables {
        inherits: Table { total: totals[0], dropped: totals[0] - inherits.len(), rows: inherits },
        uses_type: Table { total: totals[1], dropped: totals[1] - uses_type.len(), rows: uses_type },
        uses_member: Table { total: totals[2], dropped: totals[2] - uses_member.len(), rows: uses_member },
        imports: Table { total: totals[3], dropped: totals[3] - imports.len(), rows: imports },
    }
}

/// Build the `refs` model for `query`. `out` includes the outbound tables;
/// `cap`/`inbound_cap`/`outbound_cap` bound the tables; `all_out` (`--all`)
/// lifts the inbound and outbound caps.
pub fn build_refs_model(
    index: &GraphIndex,
    query: &str,
    out: bool,
    cap: usize,
    inbound_cap: usize,
    outbound_cap: usize,
    all_out: bool,
) -> RefsResult {
    build_refs_model_inner(index, query, out, cap, inbound_cap, outbound_cap, all_out, false)
}

#[allow(clippy::too_many_arguments)]
fn build_refs_model_inner(
    index: &GraphIndex,
    query: &str,
    out: bool,
    cap: usize,
    inbound_cap: usize,
    outbound_cap: usize,
    all_out: bool,
    exclude_self_inbound: bool,
) -> RefsResult {
    let id = match resolve_symbol(index, query) {
        Resolution::Resolved(id) => id,
        Resolution::Ambiguous(ids) => return RefsResult::Ambiguous(ids),
        // The member path is a FALLBACK, reached only when nothing in the graph
        // declares `query` as a type, so a name that is both a type and a
        // member still answers as the type. `out` has no member reading (a
        // type's outbound edges are the type's answer, not the member's) and is
        // ignored here.
        Resolution::NotFound => {
            return match build_member_refs_models(index, query, inbound_cap) {
                Some(MemberRefsOutcome::Models(models)) => RefsResult::Members(models),
                Some(MemberRefsOutcome::Ambiguous(ids)) => RefsResult::Ambiguous(ids),
                None => RefsResult::NotFound,
            };
        }
    };
    let def_idx = *index.by_id.get(&id).expect("resolved id must exist in the index it was resolved from");
    let def = &index.graph.defs[def_idx];
    let refs = symbol_refs(index, &id);
    let edges = &index.graph.edges;

    // The three inbound kinds share ONE cap and ONE ranking, so a widely used
    // type spends its whole budget on the most specific edges it has rather
    // than on whichever kind the walk reached first. Rank: precise before
    // heuristic, then the def's own project before every other, then file,
    // then line. Push order (kind by kind, precise then heuristic within a
    // kind) is load-bearing: the sort is stable, so two edges of different
    // kinds at the same file:line keep their push order.
    let def_project = project_of(&def.file);
    let mut ranked: Vec<RankedInbound> = Vec::new();
    let mut totals = [0usize; 3];
    let is_self_inbound = |edge: usize| {
        let (file, line) = edge_loc(&edges[edge]);
        exclude_self_inbound
            && def.end_line >= def.line
            && file == def.file
            && (def.line..=def.end_line).contains(&line)
    };
    for (kind, (precise, heuristic)) in [
        (&refs.inbound_inherits, &refs.heuristic_inbound_inherits),
        (&refs.inbound_uses_type, &refs.heuristic_inbound_uses_type),
        (&refs.inbound_uses_member, &refs.heuristic_inbound_uses_member),
    ]
    .into_iter()
    .enumerate()
    {
        for &e in precise {
            if is_self_inbound(e) { continue; }
            ranked.push(RankedInbound { kind, edge: e, heuristic: false });
            totals[kind] += 1;
        }
        for &e in heuristic {
            if is_self_inbound(e) { continue; }
            ranked.push(RankedInbound { kind, edge: e, heuristic: true });
            totals[kind] += 1;
        }
    }
    let foreign = |e: usize| usize::from(project_of(edge_loc(&edges[e]).0) != def_project);
    ranked.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| foreign(a.edge).cmp(&foreign(b.edge)))
            .then_with(|| loc_cmp(&edges[a.edge], &edges[b.edge]))
    });
    // `all_out` (`--all`, the same flag the outbound cap-lift below reads) lifts
    // the inbound cap too, not just the outbound one: a caller reaching for
    // `--all` on a truncated inbound table is asking for exactly this.
    let (shown_inbound, _) = cap_rows(ranked, if all_out { usize::MAX } else { inbound_cap });

    let mut source_cache: LineCache = HashMap::new();
    let mut rows: [Vec<InboundRow>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for r in shown_inbound {
        let (file, line) = edge_loc(&edges[r.edge]);
        let source = hit_source(&index.root, file, line, &mut source_cache);
        rows[r.kind].push(InboundRow { file: file.to_string(), line, heuristic: r.heuristic, source });
    }
    let [inherits_rows, uses_type_rows, uses_member_rows] = rows;
    let inbound_table = |total: usize, rows: Vec<InboundRow>| Table { total, dropped: total - rows.len(), rows };
    let inbound = InboundTables {
        inherits: inbound_table(totals[0], inherits_rows),
        uses_type: inbound_table(totals[1], uses_type_rows),
        uses_member: inbound_table(totals[2], uses_member_rows),
    };

    let outbound = out.then(|| {
        build_outbound_tables(&refs, def_project, edges, &index.root, if all_out { usize::MAX } else { outbound_cap })
    });
    let ambiguous = AmbiguousTables {
        inbound: build_table(refs.ambiguous_inbound, edges, cap, ambiguous_row),
        outbound: build_table(refs.ambiguous_outbound, edges, cap, ambiguous_row),
    };

    let member_refs = if def.kind == "enum" { enum_member_refs(index, &id) } else { None };

    RefsResult::Resolved(RefsModel {
        query: query.to_string(),
        id: id.clone(),
        kind: def.kind.clone(),
        sites: def_sites(index, &id),
        inbound,
        outbound,
        ambiguous,
        manifest_gap: index.flagged_files.len(),
        member_refs,
    })
}

// ============================================================================
// build_read_model -- the declaration span plus the same inbound answer.
// ============================================================================

/// The declaration span of a resolved def: the file, its 1-based start and
/// end lines, and the VERBATIM source text of those lines (no trim, no clip
///
/// -- a span is quoted, not summarized). `end_line` is what tree-sitter
/// delimited as the whole declaration node; when the file has changed since
/// the map, `end_line` is clamped to the file's current last line rather
/// than guessed upward, and a file that shrank below the span's start yields
/// no span at all.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadSpan {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
}

/// The resolved `read` result for one symbol: everything `refs` answers,
///
/// plus the declaration span when one is on record. `span` is `None` for a
/// def with no recorded end (TS defs, a graph written before end lines were
/// extracted) and never faked from a start line alone.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadModel {
    pub refs: RefsModel,
    pub span: Option<ReadSpan>,
}

/// The outcome of a `read` query -- the `refs` outcome set unchanged, so the
/// ambiguity and zero-hit discipline are literally the same code path's.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadResult {
    Resolved(Box<ReadModel>),
    /// A bare-member answer: one resolved-shaped model per declaring type.
    Members(Vec<RefsModel>),
    Ambiguous(Vec<String>),
    NotFound,
}

// Slices the mapped span out of the current file text. Fails closed: an
// unreadable file or a span past EOF degrades to "no span", which the
// renderer shows as a start-line-only answer, never as invented text.
fn read_span(root: &Path, model: &RefsModel, end_line: usize) -> Option<ReadSpan> {
    let site = model.sites.first()?;
    if end_line < site.line {
        return None;
    }
    let body = std::fs::read_to_string(root.join(&site.file)).ok()?;
    let lines: Vec<&str> = body.split('\n').collect();
    if site.line < 1 || site.line > lines.len() {
        return None;
    }
    // The map may be behind the working tree; quoting past EOF would invent
    // lines, so the end clamps to what the file actually has.
    let end = end_line.min(lines.len());
    let mut start_text = lines[site.line - 1];
    if site.line == 1 {
        start_text = start_text.strip_prefix('\u{feff}').unwrap_or(start_text);
    }
    let mut source = String::from(start_text);
    for line in &lines[site.line..end] {
        source.push('\n');
        source.push_str(line);
    }
    Some(ReadSpan {
        file: site.file.clone(),
        start_line: site.line,
        end_line: end,
        source,
    })
}

/// `read` = `refs` resolution + capped-and-ranked inbound machinery, reused
///
/// wholesale (`out` stays off -- the verb answers "what declares this and
/// what points at it"), plus the one new fact: the declaration span.
pub fn build_read_model(index: &GraphIndex, query: &str) -> ReadResult {
    match build_refs_model_inner(
        index,
        query,
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
        true,
    ) {
        RefsResult::Resolved(m) => {
            let end_line = index.def(&m.id).map(|d| d.end_line).unwrap_or(0);
            let span = if end_line > 0 {
                read_span(&index.root, &m, end_line)
            } else {
                None
            };
            ReadResult::Resolved(Box::new(ReadModel { refs: m, span }))
        }
        RefsResult::Members(models) => ReadResult::Members(models),
        RefsResult::Ambiguous(ids) => ReadResult::Ambiguous(ids),
        RefsResult::NotFound => ReadResult::NotFound,
    }
}

// ============================================================================
// build_tests_model -- test coverage.
// ============================================================================

/// One row of a tests model: a test `file`, the test-carrying `test_defs` in
/// it, the `lines` at which it references the symbol, the `ref_count`, and
/// whether the reference was guessed (`heuristic`).
#[derive(Debug, Clone, PartialEq)]
pub struct TestRow {
    pub file: String,
    pub test_defs: Vec<String>,
    pub lines: Vec<usize>,
    pub ref_count: usize,
    pub heuristic: bool,
}

/// The resolved `tests` result for one symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct TestsModel {
    pub query: String,
    pub symbol: String,
    pub def_files: Vec<String>,
    /// Precise rows first, heuristic rows after -- the same discipline every
    /// other consumer keeps, so a guess never sits inside the list of facts.
    pub rows: Vec<TestRow>,
    pub test_file_count: usize,
    pub ref_count: usize,
    pub heuristic_file_count: usize,
    pub heuristic_ref_count: usize,
}

/// The outcome of a `tests` query: resolved, ambiguous, or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum TestsResult {
    Resolved(TestsModel),
    Ambiguous(Vec<String>),
    NotFound,
}

// The three inbound kinds of one adjacency, walked in `REF_KINDS` order.
fn collect_test_rows(index: &GraphIndex, kinds: [&[usize]; 3], heuristic: bool) -> Vec<TestRow> {
    let mut rows: Vec<TestRow> = Vec::new();
    let mut by_file: HashMap<String, usize> = HashMap::new();
    for idxs in kinds {
        for &i in idxs {
            let (from_file, from_line) = edge_loc(&index.graph.edges[i]);
            let Some(test_defs) = index.test_defs_by_file.get(from_file) else {
                continue;
            };
            let slot = match by_file.get(from_file) {
                Some(&slot) => slot,
                None => {
                    let slot = rows.len();
                    rows.push(TestRow {
                        file: from_file.to_string(),
                        test_defs: test_defs.iter().map(|&d| index.graph.defs[d].id.clone()).collect(),
                        lines: Vec::new(),
                        ref_count: 0,
                        heuristic,
                    });
                    by_file.insert(from_file.to_string(), slot);
                    slot
                }
            };
            rows[slot].lines.push(from_line);
            rows[slot].ref_count += 1;
        }
    }
    for row in &mut rows {
        row.lines.sort_unstable();
    }
    rows
}

/// Which TEST files reference this symbol, at which lines, via which
/// test-carrying defs. Nothing here is a new kind of edge: it is the same
/// inbound set `refs` reports, filtered to the files `test_defs_by_file`
/// vouches for, which is why a symbol nothing tests answers "none" rather than
/// falling back to a name convention.
///
/// Row order is FIRST-SEEN edge order, kind by kind (not sorted), so the file a
/// consumer reads first is the one the graph reached first; the edge array
/// walked is itself built in a pinned order.
pub fn build_tests_model(index: &GraphIndex, query: &str) -> TestsResult {
    let id = match resolve_symbol(index, query) {
        Resolution::Resolved(id) => id,
        Resolution::Ambiguous(ids) => return TestsResult::Ambiguous(ids),
        Resolution::NotFound => return TestsResult::NotFound,
    };
    let refs = symbol_refs(index, &id);

    let precise = collect_test_rows(
        index,
        [&refs.inbound_inherits, &refs.inbound_uses_type, &refs.inbound_uses_member],
        false,
    );
    let heuristic = collect_test_rows(
        index,
        [&refs.heuristic_inbound_inherits, &refs.heuristic_inbound_uses_type, &refs.heuristic_inbound_uses_member],
        true,
    );

    let test_file_count = precise.len();
    let ref_count: usize = precise.iter().map(|r| r.ref_count).sum();
    let heuristic_file_count = heuristic.len();
    let heuristic_ref_count: usize = heuristic.iter().map(|r| r.ref_count).sum();

    let mut rows = precise;
    rows.extend(heuristic);

    TestsResult::Resolved(TestsModel {
        query: query.to_string(),
        symbol: id.clone(),
        def_files: def_files(index, &id),
        rows,
        test_file_count,
        ref_count,
        heuristic_file_count,
        heuristic_ref_count,
    })
}

// ============================================================================
// impact_walk + personalized_page_rank.
// ============================================================================

/// One representative referencing line PER EDGE KIND that reached this file,
/// the lowest line per kind. `0` means "this kind never contributed", which
/// keeps a row a kind never touched free of that key in `--json`. `direct_amb`
/// is the ambiguous half of the `direct` kind, kept apart only so
/// `build_impact_model` can apply the resolved-over-ambiguous tie-break instead
/// of letting an ambiguous line win by being numerically smaller.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KindLines {
    pub direct: usize,
    pub direct_amb: usize,
    pub ctor_di: usize,
    pub heuristic: usize,
    pub iface: usize,
}

// The lowest-line-wins guard applied at each of the walk's five edge sites.
// Every edge of one kind reaching one file shares that file by construction, so
// the minimum line is a total order that depends on nothing about iteration.
fn note_line(slot: &mut usize, line: usize) {
    if line > 0 && (*slot == 0 || line < *slot) {
        *slot = line;
    }
}

#[derive(Debug, Clone, Default)]
struct Hit {
    via_count: u32,
    ambiguous_count: u32,
    heuristic_count: u32,
    symbols: SeqSet<String>,
    // The `via` labels an interface-hop hit at this file carries
    // (`"IFoo (ctor-di)"` or bare `"IFoo"`), first-seen order.
    iface_via: SeqSet<String>,
    // One representative line per edge kind (see `KindLines`).
    lines: KindLines,
}

/// One visited file's entry in an impact walk.
#[derive(Debug, Clone, PartialEq)]
pub struct VisitedEntry {
    pub hop: u32,
    pub via_count: u32,
    pub ambiguous_count: u32,
    pub heuristic_count: u32,
    pub symbols: Vec<String>,
    /// The interface `via` labels for this file's hits.
    pub iface_via: Vec<String>,
    /// One representative line per edge kind that reached this file.
    pub lines: KindLines,
    /// This file is a hub, so the walk recorded it as an affected file and
    /// stopped there instead of expanding through it.
    pub infra: bool,
}

/// An interface the walk refused to widen through, and how many distinct
/// constructors inject it. Sorted widest-first then by name, a total order that
/// depends on nothing about the walk's own iteration.
#[derive(Debug, Clone, PartialEq)]
pub struct BrakedIface {
    pub iface: String,
    pub fanin: usize,
}

/// A hub file the walk refused to widen THROUGH, and how many distinct other
/// files reference it. Kept in its own vec (separate from [`BrakedIface`]) so
/// that type, its tests and its JSON bytes are untouched when the file brake
/// never fires. Sorted widest-first then by path, the same total order, for the
/// same reason.
#[derive(Debug, Clone, PartialEq)]
pub struct BrakedFile {
    pub file: String,
    pub indegree: usize,
}

/// The result of an impact walk.
pub struct ImpactWalkResult {
    /// Visited files in discovery order across hops (the first hop a file is
    /// hit wins its slot). `build_impact_model` reads these keys in order to
    /// build the PPR `nodes` array, so this order is directly
    /// float-accumulation-order-visible downstream (see module header).
    pub visited: SeqMap<VisitedEntry>,
    /// The file-level subgraph in the blast-radius direction. A plain
    /// `HashMap<_, HashSet<_>>` because -- unlike `visited`/`frontier` -- an
    /// out-edge set's iteration order does NOT affect
    /// `personalized_page_rank`'s float accumulation (each target in one node's
    /// out-set gets exactly one independent `+=`), so no ordering wrapper is
    /// needed here.
    pub fwd_adj: HashMap<String, HashSet<String>>,
    /// The seed files, in `seed_ids` order flat-mapped through `def_files`.
    pub seed_files: SeqSet<String>,
    /// The interfaces the walk refused to widen through.
    pub braked: Vec<BrakedIface>,
    /// The hub files the walk refused to widen through.
    pub braked_files: Vec<BrakedFile>,
}

fn add_adj(fwd_adj: &mut HashMap<String, HashSet<String>>, from: &str, to: &str) {
    fwd_adj.entry(from.to_string()).or_default().insert(to.to_string());
}

/// Reverse-edge k-hop walk from a set of seed def ids. Hop N's frontier is
/// every def declared in a file discovered at hop N-1; a file already visited
/// (or a seed's own file) is recorded at most once, at its first (minimum) hop.
/// Ambiguous edges whose candidates include a frontier def count toward
/// via-count/top-symbols too. As a byproduct it builds `fwd_adj`, the
/// file-level subgraph that seeds the PPR ranking pass.
///
/// `iface` (the CLI's `--no-iface`, inverted) gates the interface hop: it is
/// def-id-matched, never name-matched, so `infra`/`ambiguous` ctor-di edges can
/// never widen anything. `iface_max_fanin` brakes the same hop for a BROAD
/// interface: one whose ctor-di in-degree (`ctor_di_fanin`, distinct
/// constructor sites) exceeds the threshold is treated as infrastructure for
/// widening purposes, whatever it is named. Both widening paths are braked
/// together -- a contract injected everywhere is also NAMED everywhere, so
/// braking one alone leaves the radius just as wide. `0` disables the brake and
/// restores unbraked widening.
pub fn impact_walk(
    index: &GraphIndex,
    seed_ids: &[String],
    hops: u32,
    iface: bool,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
) -> ImpactWalkResult {
    let mut visited: SeqMap<VisitedEntry> = SeqMap::new();
    // Interface name -> its fan-in, for every interface this walk actually
    // refused to widen through. Recorded so the narrowing is REPORTED, never
    // silent.
    let mut braked: HashMap<String, usize> = HashMap::new();
    // File -> its in-degree, for every hub file this walk refused to widen
    // THROUGH. Recorded on the same terms as `braked` above.
    let mut braked_files: HashMap<String, usize> = HashMap::new();
    let mut seed_files: SeqSet<String> = SeqSet::new();
    for id in seed_ids {
        for f in def_files(index, id) {
            seed_files.insert(f);
        }
    }

    let mut fwd_adj: HashMap<String, HashSet<String>> = HashMap::new();

    let mut frontier: SeqSet<String> = SeqSet::new();
    for id in seed_ids {
        frontier.insert(id.clone());
    }
    let mut seen_defs: HashSet<String> = seed_ids.iter().cloned().collect();

    let mut hop = 1u32;
    while hop <= hops && frontier.len() > 0 {
        let mut hits: SeqMap<Hit> = SeqMap::new();

        for def_id in frontier.iter() {
            if let Some(inb) = index.inbound.get(def_id) {
                for kind_edges in [&inb.inherits, &inb.uses_type, &inb.uses_member] {
                    for &ei in kind_edges {
                        let (loc_file, loc_line) = edge_loc(&index.graph.edges[ei]);
                        let from_file = loc_file.to_string();
                        {
                            let h = hits.get_or_insert_default(&from_file);
                            h.via_count += 1;
                            h.symbols.insert(def_id.clone());
                            note_line(&mut h.lines.direct, loc_line);
                        }
                        for sf in def_files(index, def_id) {
                            add_adj(&mut fwd_adj, &sf, &from_file);
                        }
                    }
                }
            }
            if let Some(amb_idxs) = index.ambiguous_by_candidate.get(def_id) {
                for &ei in amb_idxs {
                    let (loc_file, loc_line) = edge_loc(&index.graph.edges[ei]);
                    let from_file = loc_file.to_string();
                    {
                        let h = hits.get_or_insert_default(&from_file);
                        h.ambiguous_count += 1;
                        note_line(&mut h.lines.direct_amb, loc_line);
                    }
                    for sf in def_files(index, def_id) {
                        add_adj(&mut fwd_adj, &sf, &from_file);
                    }
                }
            }
            // The interface hop. `seen_sites` dedupes a ctor-injected
            // parameter's own COMPANION plain `uses-type` ref (every ctor-param
            // ref emits one, resolving to the INTERFACE's own def at the
            // identical from_file/from_line) against the direct-reference pass
            // just below, so a ctor-injecting consumer is counted once,
            // labelled via ctor-di, never twice.
            if iface {
                let mut seen_sites: HashSet<(String, usize)> = HashSet::new();
                if let Some(ctor_idxs) = index.ctor_di_by_to.get(def_id) {
                    for &ei in ctor_idxs {
                        let graph::Edge::CtorDi { from_file, from_line, iface: iface_name, .. } = &index.graph.edges[ei] else {
                            continue;
                        };
                        // A braked edge is skipped WHOLE: its site never enters
                        // `seen_sites` either. It does not need to; the same
                        // interface is braked below too.
                        if brake_fanin(index, iface_max_fanin, iface_name, &mut braked) {
                            continue;
                        }
                        let from_file = from_file.clone();
                        {
                            let h = hits.get_or_insert_default(&from_file);
                            h.via_count += 1;
                            h.symbols.insert(def_id.clone());
                            h.iface_via.insert(format!("{iface_name} (ctor-di)"));
                            note_line(&mut h.lines.ctor_di, *from_line);
                        }
                        seen_sites.insert((from_file.clone(), *from_line));
                        for sf in def_files(index, def_id) {
                            add_adj(&mut fwd_adj, &sf, &from_file);
                        }
                    }
                }
                for iface_id in implemented_interfaces(index, def_id) {
                    let Some(iface_inb) = index.inbound.get(&iface_id) else { continue };
                    let iface_name = index.def(&iface_id).map(|d| d.name.clone()).unwrap_or_else(|| iface_id.clone());
                    if brake_fanin(index, iface_max_fanin, &iface_name, &mut braked) {
                        continue;
                    }
                    // 'inherits' is deliberately excluded here: a SIBLING
                    // implementor of the same interface owns its own
                    // base-list edge to it, which is a fact about the
                    // sibling, never a reference to THIS class.
                    for kind_edges in [&iface_inb.uses_type, &iface_inb.uses_member] {
                        for &ei in kind_edges {
                            let (loc_file, loc_line) = edge_loc(&index.graph.edges[ei]);
                            let site = (loc_file.to_string(), loc_line);
                            if seen_sites.contains(&site) {
                                continue;
                            }
                            let from_file = site.0.clone();
                            seen_sites.insert(site);
                            {
                                let h = hits.get_or_insert_default(&from_file);
                                h.via_count += 1;
                                h.symbols.insert(def_id.clone());
                                h.iface_via.insert(iface_name.clone());
                                note_line(&mut h.lines.iface, loc_line);
                            }
                            for sf in def_files(index, def_id) {
                                add_adj(&mut fwd_adj, &sf, &from_file);
                            }
                        }
                    }
                }
            }
            // A heuristic edge REACHES a file and stops there.
            // It never calls `add_adj` (so the PageRank graph, and therefore
            // every precise row's score and ordering, is bit-for-bit what it
            // was) and its file never enters `next_frontier` below, so the walk
            // cannot continue THROUGH a guess. One guess is a guess; a guess
            // used as the premise of the next hop is compound fiction, and
            // blast radius is exactly the answer people act on.
            if let Some(hinb) = index.heuristic_inbound.get(def_id) {
                for kind_edges in [&hinb.inherits, &hinb.uses_type, &hinb.uses_member] {
                    for &ei in kind_edges {
                        let (loc_file, loc_line) = edge_loc(&index.graph.edges[ei]);
                        let from_file = loc_file.to_string();
                        let h = hits.get_or_insert_default(&from_file);
                        h.heuristic_count += 1;
                        h.symbols.insert(def_id.clone());
                        note_line(&mut h.lines.heuristic, loc_line);
                    }
                }
            }
        }

        let mut next_frontier: SeqSet<String> = SeqSet::new();
        for (file, h) in hits.iter() {
            // A hub is a file the rest of the estate refers to BY JOB rather
            // than by dependency. Two independent halves: the path-pattern
            // classification, which is always on, and the in-degree threshold,
            // which `0` disables. A hub file is still an AFFECTED file: it is
            // recorded as a row like any other, carrying `class: infra` so the
            // narrowing is visible on the row itself and not only in the
            // trailer.
            let indegree = index.hub_indegree.get(file).copied().unwrap_or(0);
            let hub = is_infra_file(file) || (hub_max_indegree > 0 && indegree >= hub_max_indegree);
            if !visited.contains_key(file) && !seed_files.contains(file) {
                visited.insert(
                    file.clone(),
                    VisitedEntry {
                        hop,
                        via_count: h.via_count,
                        ambiguous_count: h.ambiguous_count,
                        heuristic_count: h.heuristic_count,
                        symbols: h.symbols.clone().into_vec(),
                        iface_via: h.iface_via.clone().into_vec(),
                        lines: h.lines.clone(),
                        infra: hub,
                    },
                );
            }
            // Reached ONLY by heuristic edges: recorded above as an affected
            // file, never expanded. A file with even one precise or ambiguous
            // hit expands exactly as it always did.
            if h.via_count == 0 && h.ambiguous_count == 0 {
                continue;
            }
            // The brake, at the SAME point in the walk a broad interface is
            // excluded: the hop that produced this file still fired, and every
            // interface path still fired. What stops here is expansion THROUGH
            // the hub. Reported only when the walk was actually going to expand
            // (`hop < hops`); on the last hop nothing expands, so nothing was
            // held back and a trailer naming this file would claim a narrowing
            // that never happened.
            if hub {
                if hop < hops {
                    braked_files.insert(file.clone(), indegree);
                }
                continue;
            }
            if let Some(def_idxs) = index.by_file.get(file) {
                for &di in def_idxs {
                    let def_id = &index.graph.defs[di].id;
                    if !seen_defs.contains(def_id) {
                        seen_defs.insert(def_id.clone());
                        next_frontier.insert(def_id.clone());
                    }
                }
            }
        }
        frontier = next_frontier;
        hop += 1;
    }

    // Sorted widest-first, then by name -- a total order that depends on
    // nothing about the walk's own iteration, so the output is deterministic
    // here without needing an ordered set.
    let mut braked: Vec<BrakedIface> =
        braked.into_iter().map(|(iface, fanin)| BrakedIface { iface, fanin }).collect();
    braked.sort_by(|a, b| b.fanin.cmp(&a.fanin).then_with(|| a.iface.cmp(&b.iface)));
    let mut braked_files: Vec<BrakedFile> =
        braked_files.into_iter().map(|(file, indegree)| BrakedFile { file, indegree }).collect();
    braked_files.sort_by(|a, b| b.indegree.cmp(&a.indegree).then_with(|| a.file.cmp(&b.file)));

    ImpactWalkResult { visited, fwd_adj, seed_files, braked, braked_files }
}

// Returns true when this injected type is braked, recording it on the way out
// -- the single point of decision, so no widening path can skip an interface
// without also declaring it.
fn brake_fanin(
    index: &GraphIndex,
    iface_max_fanin: usize,
    name: &str,
    braked: &mut HashMap<String, usize>,
) -> bool {
    if iface_max_fanin == 0 {
        return false;
    }
    let fanin = index.ctor_di_fanin.get(name).copied().unwrap_or(0);
    if fanin <= iface_max_fanin {
        return false;
    }
    braked.insert(name.to_string(), fanin);
    true
}

/// Personalized PageRank over the file-level subgraph. Plain power iteration,
/// no dependencies. Ranking only -- never removes a node.
///
/// **Bit-exactness**: `nodes`' array order drives `idx` (node -> array
/// position) and the `for i in 0..n` iteration order of the main loop, which
/// accumulates into `next[k]` via non-associative `f64` addition (both the
/// per-edge `next[j] += share` and, especially, the dangling-node
/// redistribution `next[k] += damping * r * teleport[k]` summed across every
/// dangling `i`). A different `nodes` order can therefore produce a
/// bit-different (not wrong) result -- callers MUST build `nodes` in a fixed
/// order (`seed_files` then `visited` keys, deduped) for a stable result; see
/// `build_impact_model`. Iterating one node's own out-edge set
/// (`fwd_adj.get(id)`) is NOT order-sensitive: every target in that set is
/// distinct, so each gets exactly one independent `+=` regardless of scan order
/// -- `fwd_adj`'s plain `HashMap<_, HashSet<_>>` is therefore safe.
pub fn personalized_page_rank(
    nodes: &[String],
    fwd_adj: &HashMap<String, HashSet<String>>,
    seeds: &[String],
    damping: f64,
    iterations: u32,
) -> HashMap<String, f64> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }
    let idx: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();

    let mut teleport = vec![0f64; n];
    let mut wsum = 0f64;
    for s in seeds {
        if let Some(&i) = idx.get(s.as_str()) {
            teleport[i] = 1.0;
            wsum += 1.0;
        }
    }
    if wsum == 0.0 {
        let v = 1.0 / n as f64;
        for t in teleport.iter_mut() {
            *t = v;
        }
    } else {
        for t in teleport.iter_mut() {
            *t /= wsum;
        }
    }

    let out_lists: Vec<Vec<usize>> = nodes
        .iter()
        .map(|id| match fwd_adj.get(id) {
            None => Vec::new(),
            Some(s) => s.iter().filter_map(|t| idx.get(t.as_str()).copied()).collect(),
        })
        .collect();

    let mut rank = teleport.clone();
    for _ in 0..iterations {
        let mut next = vec![0f64; n];
        for i in 0..n {
            let r = rank[i];
            if r == 0.0 {
                continue;
            }
            let outs = &out_lists[i];
            if outs.is_empty() {
                for k in 0..n {
                    next[k] += damping * r * teleport[k]; // dangling node: no rank leakage
                }
                continue;
            }
            let share = (damping * r) / outs.len() as f64;
            for &j in outs {
                next[j] += share;
            }
        }
        for k in 0..n {
            next[k] += (1.0 - damping) * teleport[k];
        }
        rank = next;
    }

    let mut result = HashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        result.insert(id.clone(), rank[i]);
    }
    result
}

// ============================================================================
// resolve_impact_seed + build_impact_model.
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    File,
    Symbol,
}

/// Whether `arg` looks like a file path rather than a symbol query: a bare
/// simple/qualified name with no path separator or file extension is a symbol
/// query; anything else is a file path. Equivalent to `/\.[A-Za-z0-9]+$/` by
/// checking the LAST `.` only (if an earlier dot's suffix were all-alnum, the
/// last dot's suffix -- which contains that earlier dot -- could not be, since
/// `.` is not `[A-Za-z0-9]`). Note a fully-qualified symbol id like
/// `App.Widgets.IWidget` (trailing segment alphanumeric) DOES look like a file
/// path under this heuristic.
pub fn looks_like_file_path(arg: &str) -> bool {
    if arg.contains('/') {
        return true;
    }
    match arg.rfind('.') {
        Some(pos) => {
            let ext = &arg[pos + 1..];
            !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeedResolution {
    Resolved { kind: SeedKind, ids: Vec<String> },
    Ambiguous { kind: SeedKind, ids: Vec<String> },
    NotFound { kind: SeedKind },
}

/// Resolve an impact-walk seed argument to a file's defs or a single symbol.
pub fn resolve_impact_seed(index: &GraphIndex, arg: &str) -> SeedResolution {
    if looks_like_file_path(arg) {
        return match index.by_file.get(arg) {
            Some(ids) if !ids.is_empty() => {
                SeedResolution::Resolved { kind: SeedKind::File, ids: ids.iter().map(|&i| index.graph.defs[i].id.clone()).collect() }
            }
            _ => SeedResolution::NotFound { kind: SeedKind::File },
        };
    }
    match resolve_symbol(index, arg) {
        Resolution::Resolved(id) => SeedResolution::Resolved { kind: SeedKind::Symbol, ids: vec![id] },
        Resolution::Ambiguous(ids) => SeedResolution::Ambiguous { kind: SeedKind::Symbol, ids },
        Resolution::NotFound => SeedResolution::NotFound { kind: SeedKind::Symbol },
    }
}

/// One row of an impact model: a file in the blast radius and its metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactRow {
    pub file: String,
    pub hop: u32,
    pub via_count: u32,
    pub ambiguous_count: u32,
    pub top_symbols: Vec<String>,
    pub top_symbols_more: usize,
    pub score: f64,
    /// "heuristic" is a property of how the file was REACHED, not of the edges
    /// that reached it: one precise or ambiguous hit makes the file an ordinary
    /// affected file no matter how many guesses also point at it. Only a file
    /// reached EXCLUSIVELY by guesses is flagged. On a precise row neither this
    /// nor `heuristic` is emitted in `--json`.
    pub heuristic_count: u32,
    pub heuristic: bool,
    /// Present only on a row the interface hop actually reached, empty (and
    /// omitted from `--json`) on every other row.
    pub iface_via: Vec<String>,
    /// One representative referencing line per EDGE KIND that actually reached
    /// this file, in the fixed order `direct, ctor-di, heuristic, iface`; a
    /// kind that never contributed is absent, and a row no kind could attribute
    /// a line to carries no entry at all. The file is the row's own `file` --
    /// every edge folded into one row is an edge OUT OF that file -- so a line
    /// alone locates the site.
    pub from_lines: Vec<(&'static str, usize)>,
    /// Set only on a hub file. The row is still an affected file; this says the
    /// walk stopped THERE rather than continuing through it. `false` means the
    /// key is absent in `--json`.
    pub infra: bool,
}

/// The resolved `impact` result for one seed.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactModel {
    pub kind: SeedKind,
    pub seed_files: Vec<String>,
    pub hops: u32,
    pub total_affected: usize,
    pub rows: Vec<ImpactRow>,
    pub dropped: usize,
    pub manifest_gap: usize,
    /// Number of files reached exclusively by guesses. Counted over EVERY row
    /// the walk found, capped or not.
    pub heuristic_affected: usize,
    /// How many of the PRECISELY affected files carry a test-flagged def.
    /// Counted over every affected row, not just the capped `shown` slice, and
    /// heuristic-only rows are excluded: "a guess also touched a test file" is
    /// not coverage. Zero is the interesting answer -- a blast radius that
    /// reaches no test file at all is a gap the caller can act on.
    pub tests_affected: usize,
    /// The braked interfaces, present only when the brake actually fired (empty
    /// under `--iface-max-fanin 0` or `--no-iface`).
    pub braked: Vec<BrakedIface>,
    /// The braked hub files, reported alongside `braked`.
    pub braked_files: Vec<BrakedFile>,
}

/// The outcome of an `impact` query: resolved, ambiguous, or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum ImpactResult {
    Resolved(ImpactModel),
    Ambiguous { kind: SeedKind, ids: Vec<String> },
    NotFound { kind: SeedKind },
}

// Assembles a row's per-kind representative lines. The resolved-over-ambiguous
// tie-break decides the `direct` kind: a resolved site outranks an ambiguous
// one, and the ambiguous line is used only when the resolved half never fired.
fn from_lines_of(lines: &KindLines) -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    let direct = if lines.direct != 0 { lines.direct } else { lines.direct_amb };
    if direct != 0 {
        out.push(("direct", direct));
    }
    if lines.ctor_di != 0 {
        out.push(("ctor-di", lines.ctor_di));
    }
    if lines.heuristic != 0 {
        out.push(("heuristic", lines.heuristic));
    }
    if lines.iface != 0 {
        out.push(("iface", lines.iface));
    }
    out
}

/// Blast radius for a seed, ranked. Never filters beyond the stated `cap`:
/// `rows` is the ranked, capped view; `dropped`/`total_affected` account for
/// every row the walk found, capped or not.
pub fn build_impact_model(
    index: &GraphIndex,
    arg: &str,
    hops: u32,
    cap: usize,
    iface: bool,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
) -> ImpactResult {
    let (kind, seed_ids) = match resolve_impact_seed(index, arg) {
        SeedResolution::Resolved { kind, ids } => (kind, ids),
        SeedResolution::Ambiguous { kind, ids } => return ImpactResult::Ambiguous { kind, ids },
        SeedResolution::NotFound { kind } => return ImpactResult::NotFound { kind },
    };

    let walk = impact_walk(index, &seed_ids, hops, iface, iface_max_fanin, hub_max_indegree);

    // `seed_files` then `visited` keys, deduped -- this order is
    // float-accumulation-order-visible in `personalized_page_rank`.
    let mut node_set: SeqSet<String> = SeqSet::new();
    for f in walk.seed_files.iter() {
        node_set.insert(f.clone());
    }
    for f in walk.visited.keys() {
        node_set.insert(f.clone());
    }
    let nodes = node_set.into_vec();
    let seeds: Vec<String> = walk.seed_files.iter().cloned().collect();

    let rank = personalized_page_rank(&nodes, &walk.fwd_adj, &seeds, DEFAULT_DAMPING, DEFAULT_ITERATIONS);

    let mut rows: Vec<ImpactRow> = walk
        .visited
        .iter()
        .map(|(file, h)| {
            let names: Vec<String> =
                h.symbols.iter().map(|id| index.def(id).map(|d| d.name.clone()).unwrap_or_else(|| id.clone())).collect();
            let top_symbols: Vec<String> = names.iter().take(3).cloned().collect();
            let top_symbols_more = names.len().saturating_sub(3);
            let heuristic = h.via_count == 0 && h.ambiguous_count == 0;
            ImpactRow {
                file: file.clone(),
                hop: h.hop,
                via_count: h.via_count,
                ambiguous_count: h.ambiguous_count,
                top_symbols,
                top_symbols_more,
                score: *rank.get(file).unwrap_or(&0.0),
                heuristic_count: if heuristic { h.heuristic_count } else { 0 },
                heuristic,
                iface_via: h.iface_via.clone(),
                from_lines: from_lines_of(&h.lines),
                infra: h.infra,
            }
        })
        .collect();

    // Sort key: heuristic-only rows AFTER every precise row, then by descending
    // score, then hop, then file. Heuristic-only rows sort last regardless of
    // rank -- the CLI contract is positional, and PageRank never saw the
    // heuristic edges, so their scores are not comparable with the precise ones
    // in the first place.
    rows.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.hop.cmp(&b.hop))
            .then_with(|| a.file.cmp(&b.file))
    });

    let heuristic_affected = rows.iter().filter(|r| r.heuristic).count();
    let total_affected = rows.len() - heuristic_affected;
    let tests_affected = rows.iter().filter(|r| !r.heuristic && index.test_defs_by_file.contains_key(&r.file)).count();
    let (shown, dropped) = cap_rows(rows, cap);

    ImpactResult::Resolved(ImpactModel {
        kind,
        seed_files: walk.seed_files.into_vec(),
        hops,
        total_affected,
        rows: shown,
        dropped,
        manifest_gap: index.flagged_files.len(),
        heuristic_affected,
        tests_affected,
        braked: walk.braked,
        braked_files: walk.braked_files,
    })
}

// ============================================================================
// Tests -- fixture-level behavioral contract. Each test builds its
// `graph::Graph` directly in memory (no JSON round-trip needed:
// `load_graph_index` takes an already-parsed `&Graph`) and writes only a
// manifest.json fixture to a temp `.git`-shaped dir (the one piece
// `load_graph_index` still reads from disk).
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_repo_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("scout-query-test-{label}-{nanos}-{n}"));
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    /// Mirrors `fixtureRoot`'s manifest half: every file in `manifestFiles`
    /// gets `{purpose:'x', mtime:1, source:'ast'}`.
    fn write_manifest_fixture(root: &Path, files: &[&str]) {
        let dir = root.join(".git").join("scout");
        fs::create_dir_all(&dir).unwrap();
        let mut entries = serde_json::Map::new();
        for f in files {
            entries.insert((*f).to_string(), serde_json::json!({"purpose": "x", "mtime": 1, "source": "ast"}));
        }
        let manifest = serde_json::json!({"built_at_head": "deadbeef", "scoped_dirs": ["."], "entries": entries});
        fs::write(dir.join("manifest.json"), serde_json::to_string(&manifest).unwrap()).unwrap();
    }

    fn dummy_stats() -> graph::Stats {
        graph::Stats {
            def_count: 0,
            file_count: 0,
            edges_by_kind: graph::EdgesByKind::default(),
            ambiguous_count: 0,
            ambiguous_pct: graph::Percent1::zero(),
            unresolved_external_count: 0,
            heuristic_edge_count: 0,
            test_def_count: 0,
            // Incidental: `ts` has no bearing on this C#-only query-layer
            // fixture.
            ts: None,
        }
    }

    fn make_graph(defs: Vec<graph::Def>, edges: Vec<graph::Edge>) -> graph::Graph {
        graph::Graph {
            schema_version: 1,
            built_at_head: Some("deadbeef".to_string()),
            defs,
            edges,
            stats: dummy_stats(),
            names: Vec::new(),
        }
    }

    fn def(id: &str, name: &str, namespace: &str, kind: &str, file: &str, line: usize) -> graph::Def {
        graph::Def { id: id.into(), name: name.into(), namespace: namespace.into(), kind: kind.into(), file: file.into(), line, methods: vec![], test_methods: vec![], also_in: vec![], end_line: 0 }
    }

    fn def_also(id: &str, name: &str, namespace: &str, kind: &str, file: &str, line: usize, also_in: Vec<(&str, usize)>) -> graph::Def {
        graph::Def {
            id: id.into(),
            name: name.into(),
            namespace: namespace.into(),
            kind: kind.into(),
            file: file.into(),
            line,
            methods: vec![],
            test_methods: vec![],
            also_in: also_in.into_iter().map(|(f, l)| graph::AlsoIn { file: f.into(), line: l }).collect(),
            end_line: 0,
        }
    }

    fn inherits(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
        graph::Edge::Inherits { from_file: from_file.into(), from_line, to: to.into(), to_file: to_file.into(), heuristic: false }
    }
    fn uses_type(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
        graph::Edge::UsesType { from_file: from_file.into(), from_line, to: to.into(), to_file: to_file.into(), heuristic: false }
    }
    fn uses_member(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
        graph::Edge::UsesMember { from_file: from_file.into(), from_line, to: to.into(), to_file: to_file.into(), heuristic: false }
    }
    // The same edge, tagged as a guess -- the only difference the query layer
    // is allowed to see.
    fn heuristic_uses_member(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
        graph::Edge::UsesMember { from_file: from_file.into(), from_line, to: to.into(), to_file: to_file.into(), heuristic: true }
    }
    fn heuristic_uses_type(from_file: &str, from_line: usize, to: &str, to_file: &str) -> graph::Edge {
        graph::Edge::UsesType { from_file: from_file.into(), from_line, to: to.into(), to_file: to_file.into(), heuristic: true }
    }
    fn imports(from_file: &str, from_line: usize, target: &str) -> graph::Edge {
        graph::Edge::Imports { from_file: from_file.into(), from_line, target: target.into() }
    }
    fn ambiguous(from_file: &str, from_line: usize, raw: &str, candidates: Vec<(&str, &str)>) -> graph::Edge {
        let candidate_count = candidates.len();
        graph::Edge::Ambiguous {
            origin: "uses-type".into(),
            from_file: from_file.into(),
            from_line,
            raw: raw.into(),
            candidates: candidates.into_iter().map(|(id, file)| graph::Candidate { id: id.into(), file: file.into() }).collect(),
            candidate_count,
        }
    }

    // --- file_inbound_counts -------------------------------------------------
    //
    // The map `find`'s tie-break reads. Every claim it makes about which edges
    // are facts is pinned here: one wrong kind in (or out) silently reorders
    // every text-tied find answer.

    #[test]
    fn file_inbound_counts_counts_precise_reference_edges_by_target_file() {
        let g = make_graph(
            vec![def("App.IWidget", "IWidget", "App", "interface", "Widgets/IWidget.cs", 3)],
            vec![
                inherits("Widgets/Impl/A.cs", 5, "App.IWidget", "Widgets/IWidget.cs"),
                uses_type("Consumers/B.cs", 4, "App.IWidget", "Widgets/IWidget.cs"),
                uses_member("Consumers/C.cs", 9, "App.IWidget", "Widgets/IWidget.cs"),
            ],
        );
        let counts = file_inbound_counts(&g);
        assert_eq!(counts.get("Widgets/IWidget.cs"), Some(&3), "every precise reference kind counts");
        assert_eq!(counts.len(), 1);
    }

    #[test]
    fn file_inbound_counts_excludes_heuristic_edges_and_non_reference_kinds() {
        let g = make_graph(
            vec![def("App.IWidget", "IWidget", "App", "interface", "Widgets/IWidget.cs", 3)],
            vec![
                heuristic_uses_type("Guessy/Guesser.cs", 7, "App.IWidget", "Widgets/IWidget.cs"),
                heuristic_uses_member("Guessy/Guesser2.cs", 8, "App.IWidget", "Widgets/IWidget.cs"),
                // Names a namespace, never a definition.
                imports("Widgets/IWidget.cs", 1, "App.Widgets"),
                // DI wiring and an unresolved name: neither is a reference to
                // this file's definitions.
                graph::Edge::CtorDi {
                    from_file: "App/Program.cs".into(),
                    from_line: 4,
                    iface: "App.IWidget".into(),
                    resolution: "plain".into(),
                    args: None,
                    to: None,
                    candidates: vec![],
                },
                ambiguous("Ambig/User.cs", 6, "IWidget", vec![("App.IWidget", "Widgets/IWidget.cs")]),
                // A module import that resolves to the file itself is still an
                // import, not a reference to a declaration.
                graph::Edge::Import {
                    from_file: "Consumers/Importer.ts".into(),
                    from_line: 1,
                    target: "./widgets".into(),
                    to_file: "Widgets/IWidget.ts".into(),
                    via: None,
                },
            ],
        );
        assert!(
            file_inbound_counts(&g).is_empty(),
            "guesses, imports, ctor-di wiring, and ambiguity earn no count"
        );
    }

    #[test]
    fn file_inbound_counts_counts_the_ts_reference_kinds() {
        // On a TS repo call/jsx-use/dispatch ARE the reference graph; a count
        // blind to them would rank every TS file at zero.
        let g = make_graph(
            vec![def("ui.Button", "Button", "", "function", "src/Button.tsx", 1)],
            vec![
                graph::Edge::Call { from_file: "src/App.ts".into(), from_line: 10, to: "ui.Button".into(), to_file: "src/Button.tsx".into() },
                graph::Edge::JsxUse { from_file: "src/Page.tsx".into(), from_line: 20, to: "ui.Button".into(), to_file: "src/Button.tsx".into() },
                graph::Edge::Dispatch { from_file: "src/store.ts".into(), from_line: 30, to: "ui.Button".into(), to_file: "src/Button.tsx".into() },
            ],
        );
        let counts = file_inbound_counts(&g);
        assert_eq!(counts.get("src/Button.tsx"), Some(&3));
        assert!(!counts.contains_key("src/App.ts"), "keyed by the TARGET file only");
    }

    #[test]
    fn file_inbound_counts_excludes_a_files_references_to_itself() {
        // This repository treats only references from other files as inbound
        // interest; a file's self-references do not count.
        let g = make_graph(
            vec![def("App.Hub", "Hub", "App", "class", "src/Hub.cs", 3)],
            vec![
                uses_type("src/Hub.cs", 5, "App.Hub", "src/Hub.cs"),
                inherits("src/Hub.cs", 7, "App.Hub", "src/Hub.cs"),
                uses_type("src/Other.cs", 9, "App.Hub", "src/Hub.cs"),
            ],
        );
        let counts = file_inbound_counts(&g);
        assert_eq!(counts.get("src/Hub.cs"), Some(&1), "self-references earn nothing");
        assert_eq!(counts.len(), 1);
    }

    #[test]
    fn file_inbound_counts_on_an_edgeless_graph_is_an_empty_map() {
        assert!(file_inbound_counts(&make_graph(vec![], vec![])).is_empty());
    }

    // --- base fixture ---

    const BASE_MANIFEST_FILES: &[&str] = &[
        "Widgets/IWidget.cs",
        "Widgets/Impl/WidgetImpl.cs",
        "Widgets/Impl/OtherImpl.cs",
        "Consumers/TwoHop.cs",
        "Consumers/Holder.cs",
        "One/Config.cs",
        "Two/Config.cs",
        "Outer/Container.cs",
        "Outer/Container.Extra.cs",
        "Three/Consumer.cs",
        // Ghost/NotInManifest.cs deliberately omitted.
    ];

    fn base_fixture_graph() -> graph::Graph {
        make_graph(
            vec![
                def("App.Widgets.IWidget", "IWidget", "App.Widgets", "interface", "Widgets/IWidget.cs", 3),
                def("App.Widgets.Impl.WidgetImpl", "WidgetImpl", "App.Widgets.Impl", "class", "Widgets/Impl/WidgetImpl.cs", 5),
                def("App.Widgets.Impl.OtherImpl", "OtherImpl", "App.Widgets.Impl", "class", "Widgets/Impl/OtherImpl.cs", 5),
                def("App.Consumers.TwoHop", "TwoHop", "App.Consumers", "class", "Consumers/TwoHop.cs", 3),
                def("App.One.Config", "Config", "App.One", "class", "One/Config.cs", 1),
                def("App.Two.Config", "Config", "App.Two", "class", "Two/Config.cs", 1),
                def_also("App.Outer.Container", "Container", "App.Outer", "class", "Outer/Container.cs", 3, vec![("Outer/Container.Extra.cs", 1)]),
                def("App.Outer.Container+Item", "Item", "App.Outer", "class", "Outer/Container.cs", 5),
                def("App.Ghost.NotInManifest", "NotInManifest", "App.Ghost", "class", "Ghost/NotInManifest.cs", 1),
            ],
            vec![
                inherits("Widgets/Impl/WidgetImpl.cs", 5, "App.Widgets.IWidget", "Widgets/IWidget.cs"),
                inherits("Widgets/Impl/OtherImpl.cs", 5, "App.Widgets.IWidget", "Widgets/IWidget.cs"),
                uses_type("Consumers/Holder.cs", 8, "App.Widgets.IWidget", "Widgets/IWidget.cs"),
                uses_type("Consumers/TwoHop.cs", 4, "App.Widgets.Impl.WidgetImpl", "Widgets/Impl/WidgetImpl.cs"),
                imports("Widgets/Impl/WidgetImpl.cs", 1, "App.Widgets"),
                ambiguous("Three/Consumer.cs", 4, "Config", vec![("App.One.Config", "One/Config.cs"), ("App.Two.Config", "Two/Config.cs")]),
            ],
        )
    }

    fn base_fixture_root() -> PathBuf {
        let root = temp_repo_root("base");
        write_manifest_fixture(&root, BASE_MANIFEST_FILES);
        root
    }

    fn enum_fixture_graph() -> graph::Graph {
        make_graph(
            vec![
                def("App.Enums.PostType", "PostType", "App.Enums", "enum", "Enums/PostType.cs", 3),
                def("App.Enums.PostType.Post", "Post", "App.Enums", "enum-member", "Enums/PostType.cs", 5),
                def("App.Enums.PostType.Question", "Question", "App.Enums", "enum-member", "Enums/PostType.cs", 6),
                def("App.Consumers.Reader", "Reader", "App.Consumers", "class", "Consumers/Reader.cs", 3),
                def("App.Consumers.TwoHop", "TwoHop", "App.Consumers", "class", "Consumers/TwoHop.cs", 3),
            ],
            vec![
                uses_member("Consumers/Reader.cs", 8, "App.Enums.PostType.Question", "Enums/PostType.cs"),
                uses_type("Consumers/TwoHop.cs", 4, "App.Consumers.Reader", "Consumers/Reader.cs"),
            ],
        )
    }

    fn enum_fixture_root() -> PathBuf {
        let root = temp_repo_root("enum");
        write_manifest_fixture(&root, &["Enums/PostType.cs", "Consumers/Reader.cs", "Consumers/TwoHop.cs"]);
        root
    }

    // --- 1: load_graph_index + manifest flagging ---

    #[test]
    fn load_graph_index_joins_def_files_against_the_manifest_and_flags_a_graph_file_missing_from_it() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        assert!(index.by_id.contains_key("App.Ghost.NotInManifest"), "the def must still be indexed, not dropped");
        assert!(index.flagged_files.contains("Ghost/NotInManifest.cs"), "its file must be flagged as missing from the manifest");
        assert_eq!(index.flagged_files.len(), 1, "every other def/edge file is in the manifest and must not be flagged");
        assert!(index.manifest_present);
    }

    // --- 2: resolve_symbol ladder ---

    #[test]
    fn resolve_symbol_exact_id_unique_name_case_insensitive_ambiguous_notfound() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);

        assert_eq!(resolve_symbol(&index, "App.Outer.Container+Item"), Resolution::Resolved("App.Outer.Container+Item".into()));
        assert_eq!(resolve_symbol(&index, "IWidget"), Resolution::Resolved("App.Widgets.IWidget".into()));
        assert_eq!(resolve_symbol(&index, "iwidget"), Resolution::Resolved("App.Widgets.IWidget".into()), "case-insensitive unique match must resolve");

        match resolve_symbol(&index, "Config") {
            Resolution::Ambiguous(mut ids) => {
                ids.sort();
                assert_eq!(ids, vec!["App.One.Config".to_string(), "App.Two.Config".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }

        assert_eq!(resolve_symbol(&index, "NoSuchSymbolAnywhere"), Resolution::NotFound);
    }

    // --- 3: build_refs_model basic inbound/outbound/imports/manifest-gap ---

    #[test]
    fn build_refs_model_def_site_inbound_outbound_grouped_imports_manifest_gap() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "IWidget", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };

        assert_eq!(model.id, "App.Widgets.IWidget");
        assert_eq!(model.sites, vec![DefSite { file: "Widgets/IWidget.cs".into(), line: 3 }]);

        assert_eq!(model.inbound.inherits.total, 2);
        let mut files: Vec<&str> = model.inbound.inherits.rows.iter().map(|r| r.file.as_str()).collect();
        files.sort();
        assert_eq!(files, vec!["Widgets/Impl/OtherImpl.cs", "Widgets/Impl/WidgetImpl.cs"]);
        assert_eq!(model.inbound.uses_type.total, 1);
        assert_eq!(model.inbound.uses_type.rows[0].file, "Consumers/Holder.cs");

        // IWidget's own file makes no outbound reference in the fixture.
        assert_eq!(model.outbound.as_ref().expect("built with out=true").inherits.total, 0);
        assert_eq!(model.outbound.as_ref().expect("built with out=true").imports.total, 0);

        assert_eq!(model.manifest_gap, 1, "the one flagged def file must surface in every refs call, not just the loader");
    }

    // --- 4: outbound inherits+imports from own file; partial-class def sites ---

    #[test]
    fn build_refs_model_outbound_own_file_and_partial_class_sites() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);

        let impl_model = match build_refs_model(&index, "WidgetImpl", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(impl_model.outbound.as_ref().expect("built with out=true").inherits.total, 1);
        assert_eq!(impl_model.outbound.as_ref().expect("built with out=true").inherits.rows[0].to_file, "Widgets/IWidget.cs");
        assert_eq!(impl_model.outbound.as_ref().expect("built with out=true").imports.total, 1);
        assert_eq!(impl_model.outbound.as_ref().expect("built with out=true").imports.rows[0].target, "App.Widgets");
        assert_eq!(impl_model.inbound.uses_type.total, 1, "TwoHop.cs references WidgetImpl");

        let container = match build_refs_model(&index, "App.Outer.Container", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            container.sites,
            vec![DefSite { file: "Outer/Container.cs".into(), line: 3 }, DefSite { file: "Outer/Container.Extra.cs".into(), line: 1 }]
        );
    }

    // --- 5: partial class, second site same file as first -- no outbound double-count ---

    #[test]
    fn build_refs_model_partial_class_same_file_second_site_no_double_count() {
        let graph = make_graph(
            vec![def_also("App.Split.Combo", "Combo", "App.Split", "class", "Split/Combo.cs", 3, vec![("Split/Combo.cs", 20)])],
            vec![uses_type("Split/Combo.cs", 5, "App.Split.Combo", "Split/Combo.cs"), imports("Split/Combo.cs", 1, "System")],
        );
        let root = temp_repo_root("partial-same-file");
        write_manifest_fixture(&root, &["Split/Combo.cs"]);
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Combo", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.outbound.as_ref().expect("built with out=true").uses_type.total, 1, "the single outbound uses-type edge must not be counted twice");
        assert_eq!(model.outbound.as_ref().expect("built with out=true").imports.total, 1, "the single outbound imports edge must not be counted twice");
    }

    // --- 6: ambiguous edges land in a separate trailing section ---

    #[test]
    fn build_refs_model_ambiguous_edges_never_guessed_into_inbound() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "App.One.Config", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };

        assert_eq!(model.inbound.inherits.total, 0);
        assert_eq!(model.inbound.uses_type.total, 0, "the ambiguous ref to \"Config\" must not be counted as a resolved inbound uses-type hit");
        assert_eq!(model.ambiguous.inbound.total, 1);
        assert_eq!(model.ambiguous.inbound.rows[0].raw, "Config");
        assert_eq!(model.ambiguous.inbound.rows[0].candidate_count, 2);
    }

    // --- 7: an ambiguous QUERY name returns candidates, never a guess ---

    #[test]
    fn build_refs_model_ambiguous_query_name_returns_candidates() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        match build_refs_model(&index, "Config", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Ambiguous(mut ids) => {
                ids.sort();
                assert_eq!(ids, vec!["App.One.Config".to_string(), "App.Two.Config".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    // --- 8: caps a table, always reports dropped ---

    #[test]
    fn build_refs_model_caps_a_table_and_reports_dropped() {
        let edges: Vec<graph::Edge> =
            (0..5).map(|i| uses_type(&format!("Consumers/C{i}.cs"), 1, "App.Hot.Popular", "Hot/Popular.cs")).collect();
        let graph = make_graph(vec![def("App.Hot.Popular", "Popular", "App.Hot", "class", "Hot/Popular.cs", 1)], edges);
        let root = temp_repo_root("cap-table");
        let mut files: Vec<String> = (0..5).map(|i| format!("Consumers/C{i}.cs")).collect();
        files.push("Hot/Popular.cs".to_string());
        let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        write_manifest_fixture(&root, &file_refs);
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Popular", true, 2, 2, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.inbound.uses_type.total, 5);
        assert_eq!(model.inbound.uses_type.rows.len(), 2, "must respect the cap");
        assert_eq!(model.inbound.uses_type.dropped, 3, "must always report how many rows were dropped");
    }

    // --- 8b: the outbound tables exist only when asked for ---

    #[test]
    fn build_refs_model_outbound_tables_exist_only_when_asked_for() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let resolved = |out: bool| match build_refs_model(&index, "WidgetImpl", out, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert!(resolved(false).outbound.is_none(), "the default model has no outbound tables at all");
        assert!(resolved(true).outbound.is_some(), "--out brings them back");
    }

    // --- 8c: one cap over the three inbound kinds, ranked ---

    #[test]
    fn build_refs_model_one_inbound_cap_ranked_resolved_then_same_project() {
        // 3 uses-member (2 of them guesses) + 2 uses-type, cap 3. A per-kind
        // cap would show all three uses-member rows; the shared one spends the
        // budget on the facts first, and among facts on the def's own project.
        let graph = make_graph(
            vec![def("App.Hot.Popular", "Popular", "App.Hot", "class", "Hot/Popular.cs", 1)],
            vec![
                uses_member("Hot/Near.cs", 3, "App.Hot.Popular", "Hot/Popular.cs"),
                heuristic_uses_member("Cold/Guess.cs", 4, "App.Hot.Popular", "Hot/Popular.cs"),
                heuristic_uses_member("Hot/Guess.cs", 5, "App.Hot.Popular", "Hot/Popular.cs"),
                uses_type("Cold/Far.cs", 6, "App.Hot.Popular", "Hot/Popular.cs"),
                uses_type("Hot/AlsoNear.cs", 7, "App.Hot.Popular", "Hot/Popular.cs"),
            ],
        );
        let root = temp_repo_root("inbound-rank");
        write_manifest_fixture(&root, &["Hot/Popular.cs", "Hot/Near.cs", "Hot/Guess.cs", "Hot/AlsoNear.cs", "Cold/Guess.cs", "Cold/Far.cs"]);
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Popular", false, DEFAULT_CAP, 3, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };

        fn files(t: &Table<InboundRow>) -> Vec<&str> {
            t.rows.iter().map(|r| r.file.as_str()).collect()
        }
        assert_eq!(files(&model.inbound.uses_type), vec!["Hot/AlsoNear.cs", "Cold/Far.cs"]);
        assert_eq!(files(&model.inbound.uses_member), vec!["Hot/Near.cs"]);
        assert_eq!(model.inbound.uses_member.total, 3);
        assert_eq!(model.inbound.uses_member.dropped, 2, "both guesses lost the budget to the facts");
        assert_eq!(model.inbound.uses_type.dropped, 0);
    }

    // --- 8d: one trimmed source line per shown hit ---

    #[test]
    fn build_refs_model_shown_hit_carries_its_trimmed_source_line_and_a_missing_file_carries_none() {
        let graph = make_graph(
            vec![def("App.Hot.Popular", "Popular", "App.Hot", "class", "Hot/Popular.cs", 1)],
            vec![
                uses_type("Hot/Real.cs", 2, "App.Hot.Popular", "Hot/Popular.cs"),
                uses_type("Hot/Absent.cs", 2, "App.Hot.Popular", "Hot/Popular.cs"),
            ],
        );
        let root = temp_repo_root("source-line");
        write_manifest_fixture(&root, &["Hot/Popular.cs", "Hot/Real.cs", "Hot/Absent.cs"]);
        std::fs::create_dir_all(root.join("Hot")).expect("fixture dir");
        std::fs::write(root.join("Hot/Real.cs"), "class Real\n\t{\tpublic Popular P { get; set; }\t}\n").expect("fixture file");

        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Popular", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let row = |file: &str| model.inbound.uses_type.rows.iter().find(|r| r.file == file).expect("row").clone();
        assert_eq!(row("Hot/Real.cs").source, "{ public Popular P { get; set; } }", "tabs collapse to single spaces, the indent is trimmed off");
        assert_eq!(row("Hot/Absent.cs").source, "", "a file that is not on disk yields no line, never a partial one");
    }

    // --- --out mirrors of 8c/8d above, over the four outbound kinds ---

    #[test]
    fn build_refs_model_one_outbound_cap_ranked_resolved_then_same_project_imports_foreign() {
        // A same-project resolved uses-type, an always-foreign imports edge, a
        // foreign resolved inherits edge and a same-project heuristic
        // uses-member, cap 2. Imports is never a guess so it beats both the
        // foreign inherits row and the heuristic row; among the two foreign,
        // non-heuristic rows (imports line 1, inherits line 6) the file/line
        // tiebreak decides, since project ties them.
        let graph = make_graph(
            vec![
                def("App.Hot.Consumer", "Consumer", "App.Hot", "class", "Hot/Consumer.cs", 1),
                def("App.Hot.Local", "Local", "App.Hot", "class", "Hot/Local.cs", 1),
                def("App.Cold.Far", "Far", "App.Cold", "class", "Cold/Far.cs", 1),
            ],
            vec![
                uses_type("Hot/Consumer.cs", 5, "App.Hot.Local", "Hot/Local.cs"),
                imports("Hot/Consumer.cs", 1, "System"),
                inherits("Hot/Consumer.cs", 6, "App.Cold.Far", "Cold/Far.cs"),
                heuristic_uses_member("Hot/Consumer.cs", 7, "App.Hot.Local", "Hot/Local.cs"),
            ],
        );
        let root = temp_repo_root("outbound-rank");
        write_manifest_fixture(&root, &["Hot/Consumer.cs", "Hot/Local.cs", "Cold/Far.cs"]);
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let ob = model.outbound.as_ref().expect("built with out=true");

        assert_eq!(ob.uses_type.rows.len(), 1, "the same-project resolved edge spends the shared budget first");
        assert_eq!(ob.uses_type.rows[0].line, 5);
        assert_eq!(ob.imports.rows.len(), 1, "imports is never a guess, so it beats both the foreign inherits and the heuristic row");
        assert_eq!(ob.imports.rows[0].line, 1);
        assert_eq!(ob.inherits.rows.len(), 0, "the foreign inherits edge lost the shared budget");
        assert_eq!(ob.inherits.dropped, 1);
        assert_eq!(ob.uses_member.rows.len(), 0, "the heuristic guess lost the budget too, project notwithstanding");
        assert_eq!(ob.uses_member.dropped, 1);
        assert_eq!(ob.imports.dropped, 0);
        assert_eq!(ob.uses_type.dropped, 0);
    }

    #[test]
    fn build_refs_model_shown_outbound_hit_carries_its_trimmed_source_line_and_a_missing_file_carries_none() {
        let graph = make_graph(
            vec![
                def_also("App.Hot.Consumer", "Consumer", "App.Hot", "class", "Hot/Consumer.cs", 1, vec![("Hot/Consumer.Extra.cs", 1)]),
                def("App.Hot.Local", "Local", "App.Hot", "class", "Hot/Local.cs", 1),
            ],
            vec![
                uses_type("Hot/Consumer.cs", 2, "App.Hot.Local", "Hot/Local.cs"),
                uses_type("Hot/Consumer.Extra.cs", 3, "App.Hot.Local", "Hot/Local.cs"),
            ],
        );
        let root = temp_repo_root("outbound-source-line");
        write_manifest_fixture(&root, &["Hot/Consumer.cs", "Hot/Consumer.Extra.cs", "Hot/Local.cs"]);
        std::fs::create_dir_all(root.join("Hot")).expect("fixture dir");
        std::fs::write(root.join("Hot/Consumer.cs"), "x\n\t{\tpublic Local L { get; set; }\t}\n").expect("fixture file");
        // Hot/Consumer.Extra.cs is deliberately never written to disk.

        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let rows = &model.outbound.as_ref().expect("built with out=true").uses_type.rows;
        let row = |line: usize| rows.iter().find(|r| r.line == line).expect("row").clone();
        assert_eq!(row(2).source, "{ public Local L { get; set; } }", "tabs collapse to single spaces, the indent is trimmed off");
        assert_eq!(row(3).source, "", "a file that is not on disk yields no line, never a partial one");
    }

    #[test]
    fn build_refs_model_all_out_lifts_the_outbound_cap() {
        let edges: Vec<graph::Edge> = (0..5).map(|i| imports("Hot/Consumer.cs", i + 1, &format!("Ns{i}"))).collect();
        let graph = make_graph(vec![def("App.Hot.Consumer", "Consumer", "App.Hot", "class", "Hot/Consumer.cs", 1)], edges);
        let root = temp_repo_root("outbound-all");
        write_manifest_fixture(&root, &["Hot/Consumer.cs"]);
        let index = load_graph_index(&graph, &root);

        let capped = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let capped_ob = capped.outbound.as_ref().expect("built with out=true");
        assert_eq!(capped_ob.imports.rows.len(), 2, "must respect the outbound cap");
        assert_eq!(capped_ob.imports.dropped, 3);

        let all = match build_refs_model(&index, "Consumer", true, DEFAULT_CAP, INBOUND_CAP, 2, true) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let all_ob = all.outbound.as_ref().expect("built with out=true");
        assert_eq!(all_ob.imports.rows.len(), 5, "--all lifts the outbound cap entirely");
        assert_eq!(all_ob.imports.dropped, 0);
    }

    // `--all` (`all_out`) must lift the INBOUND cap, not just the outbound one:
    // without it a caller reaching for `--all` on a truncated inbound table got
    // nothing back for it. Five distinct referring files against a cap of 2
    // proves the file-loss shape, not just a row-count-under-cap shape.
    #[test]
    fn build_refs_model_all_out_now_lifts_the_inbound_cap_too() {
        let files = ["Cold/ConsumerA.cs", "Cold/ConsumerB.cs", "Cold/ConsumerC.cs", "Cold/ConsumerD.cs", "Cold/ConsumerE.cs"];
        let edges: Vec<graph::Edge> = files.iter().map(|f| uses_type(f, 1, "App.Hot.Widget", "Hot/Widget.cs")).collect();
        let graph = make_graph(vec![def("App.Hot.Widget", "Widget", "App.Hot", "class", "Hot/Widget.cs", 1)], edges);
        let root = temp_repo_root("inbound-all");
        let mut manifest_files: Vec<&str> = vec!["Hot/Widget.cs"];
        manifest_files.extend_from_slice(&files);
        write_manifest_fixture(&root, &manifest_files);
        let index = load_graph_index(&graph, &root);

        let capped = match build_refs_model(&index, "Widget", false, DEFAULT_CAP, 2, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(capped.inbound.uses_type.rows.len(), 2, "must respect the inbound cap");
        assert_eq!(capped.inbound.uses_type.dropped, 3, "the other 3 referring files are lost without --all");

        let all = match build_refs_model(&index, "Widget", false, DEFAULT_CAP, 2, OUTBOUND_CAP, true) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(all.inbound.uses_type.rows.len(), 5, "--all lifts the inbound cap too, not just the outbound one");
        assert_eq!(all.inbound.uses_type.dropped, 0);
    }

    // --- 9: build_impact_model hop limit ---

    #[test]
    fn build_impact_model_hop_limit_honored() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);

        let one_hop = match build_impact_model(&index, "IWidget", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let mut files: Vec<&str> = one_hop.rows.iter().map(|r| r.file.as_str()).collect();
        files.sort();
        assert_eq!(files, vec!["Consumers/Holder.cs", "Widgets/Impl/OtherImpl.cs", "Widgets/Impl/WidgetImpl.cs"]);
        assert!(!one_hop.rows.iter().any(|r| r.file == "Consumers/TwoHop.cs"), "TwoHop.cs is 2 hops away and must not appear at hops=1");

        let two_hop = match build_impact_model(&index, "IWidget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let two_hop_row = two_hop.rows.iter().find(|r| r.file == "Consumers/TwoHop.cs").expect("TwoHop.cs must appear at hops=2");
        assert_eq!(two_hop_row.hop, 2);
        assert_eq!(two_hop_row.top_symbols, vec!["WidgetImpl".to_string()], "reached via WidgetImpl, not IWidget directly");
    }

    // --- 10: file-path seed ---

    #[test]
    fn build_impact_model_accepts_a_file_path_seeding_every_def_in_it() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Widgets/IWidget.cs", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.kind, SeedKind::File);
        assert_eq!(model.seed_files, vec!["Widgets/IWidget.cs".to_string()]);
        let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
        files.sort();
        assert_eq!(files, vec!["Consumers/Holder.cs", "Widgets/Impl/OtherImpl.cs", "Widgets/Impl/WidgetImpl.cs"]);
    }

    // --- 11: unknown file path -> notfound, not treated as a symbol ---

    #[test]
    fn build_impact_model_unknown_file_path_is_reported_not_silently_a_symbol() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        match build_impact_model(&index, "Nowhere/Missing.cs", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::NotFound { kind } => assert_eq!(kind, SeedKind::File),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    // --- 12: ranking: 1-hop outranks 2-hop; nothing dropped below cap ---

    #[test]
    fn build_impact_model_ranking_orders_direct_ahead_of_indirect_never_drops_below_cap() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "IWidget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let score_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap().score;
        let hop_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap().hop;
        assert!(score_of("Widgets/Impl/WidgetImpl.cs") > score_of("Consumers/TwoHop.cs"), "1-hop dependent must outrank the 2-hop one");
        assert_eq!(hop_of("Consumers/TwoHop.cs"), 2);
        assert_eq!(model.dropped, 0);
        assert_eq!(model.total_affected, model.rows.len(), "nothing filtered beyond the (unhit) cap");
    }

    // --- 13: ranking determinism across repeated runs ---

    #[test]
    fn build_impact_model_ranking_deterministic_across_repeated_runs() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let a: Vec<String> = match build_impact_model(&index, "IWidget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m.rows.into_iter().map(|r| r.file).collect(),
            other => panic!("expected Resolved, got {other:?}"),
        };
        let b: Vec<String> = match build_impact_model(&index, "IWidget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m.rows.into_iter().map(|r| r.file).collect(),
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(a, b);
    }

    // --- the interface hop ---

    fn ctor_di_to(from_file: &str, from_line: usize, iface: &str, resolution: &str, to: &str) -> graph::Edge {
        graph::Edge::CtorDi {
            from_file: from_file.into(),
            from_line,
            iface: iface.into(),
            resolution: resolution.into(),
            args: None,
            to: Some(to.into()),
            candidates: vec![],
        }
    }
    fn ctor_di_no_to(from_file: &str, from_line: usize, iface: &str, resolution: &str) -> graph::Edge {
        graph::Edge::CtorDi {
            from_file: from_file.into(),
            from_line,
            iface: iface.into(),
            resolution: resolution.into(),
            args: None,
            to: None,
            candidates: vec![],
        }
    }

    const IFACE_HOP_MANIFEST_FILES: &[&str] = &[
        "Pay/IPaymentGateway.cs",
        "Pay/StripeGateway.cs",
        "Pay/OrderService.cs",
        "Pay/RefundService.cs",
        "Pay/GatewayFactory.cs",
        "Pay/GatewayHolder.cs",
        "Pay/AuditLogger.cs",
    ];

    /// `StripeGateway` is `IPaymentGateway`'s SOLE implementor. `OrderService`/
    /// `RefundService` ctor-inject the interface (never naming the class) --
    /// each also carries the COMPANION plain `uses-type` ref always emitted
    /// alongside a ctor-param ref, at the identical from_file/from_line,
    /// resolving to the INTERFACE (dedup-proving). `GatewayFactory` names
    /// `StripeGateway` directly (a plain direct-name hit). `GatewayHolder`
    /// references `IPaymentGateway` directly but NOT through a constructor (a
    /// property type, distinct file/line from every ctor-di site). `AuditLogger`
    /// carries an unrelated, unresolvable `ILogger` ctor-di edge (`infra`, no
    /// `to`) that must never widen anything.
    fn iface_hop_fixture_graph() -> graph::Graph {
        make_graph(
            vec![
                def("App.Pay.IPaymentGateway", "IPaymentGateway", "App.Pay", "interface", "Pay/IPaymentGateway.cs", 3),
                def("App.Pay.StripeGateway", "StripeGateway", "App.Pay", "class", "Pay/StripeGateway.cs", 3),
                def("App.Pay.OrderService", "OrderService", "App.Pay", "class", "Pay/OrderService.cs", 3),
                def("App.Pay.RefundService", "RefundService", "App.Pay", "class", "Pay/RefundService.cs", 3),
                def("App.Pay.GatewayFactory", "GatewayFactory", "App.Pay", "class", "Pay/GatewayFactory.cs", 3),
                def("App.Pay.GatewayHolder", "GatewayHolder", "App.Pay", "class", "Pay/GatewayHolder.cs", 3),
                def("App.Pay.AuditLogger", "AuditLogger", "App.Pay", "class", "Pay/AuditLogger.cs", 3),
            ],
            vec![
                inherits("Pay/StripeGateway.cs", 3, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                ctor_di_to("Pay/OrderService.cs", 5, "IPaymentGateway", "plain", "App.Pay.StripeGateway"),
                uses_type("Pay/OrderService.cs", 5, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                ctor_di_to("Pay/RefundService.cs", 5, "IPaymentGateway", "plain", "App.Pay.StripeGateway"),
                uses_type("Pay/RefundService.cs", 5, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                uses_type("Pay/GatewayFactory.cs", 6, "App.Pay.StripeGateway", "Pay/StripeGateway.cs"),
                uses_type("Pay/GatewayHolder.cs", 4, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                ctor_di_no_to("Pay/AuditLogger.cs", 5, "ILogger", "infra"),
            ],
        )
    }

    fn iface_hop_fixture_root() -> PathBuf {
        let root = temp_repo_root("iface-hop");
        write_manifest_fixture(&root, IFACE_HOP_MANIFEST_FILES);
        root
    }

    #[test]
    fn build_impact_model_widens_through_ctor_injected_interface_consumers_and_direct_interface_references() {
        let graph = iface_hop_fixture_graph();
        let root = iface_hop_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "StripeGateway", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
        assert!(files.contains(&"Pay/GatewayFactory.cs"), "direct-name reference must still be reached: {files:?}");
        assert!(files.contains(&"Pay/OrderService.cs"), "ctor-injected consumer must be reached: {files:?}");
        assert!(files.contains(&"Pay/RefundService.cs"), "ctor-injected consumer must be reached: {files:?}");
        assert!(files.contains(&"Pay/GatewayHolder.cs"), "direct interface-name reference must be reached: {files:?}");
        assert!(!files.contains(&"Pay/AuditLogger.cs"), "an unrelated infra ctor-di edge must never widen: {files:?}");

        let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
        assert_eq!(row_of("Pay/OrderService.cs").hop, 1, "the interface hop counts as ONE hop, same as a direct reference");
        assert_eq!(row_of("Pay/OrderService.cs").iface_via, vec!["IPaymentGateway (ctor-di)".to_string()]);
        assert_eq!(row_of("Pay/RefundService.cs").iface_via, vec!["IPaymentGateway (ctor-di)".to_string()]);
        assert_eq!(row_of("Pay/GatewayHolder.cs").iface_via, vec!["IPaymentGateway".to_string()]);
        assert!(row_of("Pay/GatewayFactory.cs").iface_via.is_empty(), "a plain direct-name hit carries no iface_via label");
        // The companion plain `uses-type` ref (same from_file/from_line as the
        // ctor-di edge) must be deduped away, not double-counted.
        assert_eq!(row_of("Pay/OrderService.cs").via_count, 1, "the ctor-di hit and its companion ref are ONE hit, not two");
    }

    #[test]
    fn build_impact_model_no_iface_restores_the_pre_ds_0050_radius_byte_for_byte() {
        let graph = iface_hop_fixture_graph();
        let root = iface_hop_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "StripeGateway", 1, DEFAULT_CAP, false, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
        assert_eq!(files, vec!["Pay/GatewayFactory.cs"], "only the direct-name reference survives with --no-iface");
        assert!(model.rows.iter().all(|r| r.iface_via.is_empty()), "no row may carry an iface_via label with --no-iface");
    }

    /// A second implementor with the SAME bare interface name in a different
    /// namespace must never contribute its consumers to the first's radius --
    /// the widen is def-id-matched (via the resolver's own confirmed `to`),
    /// never name-matched. Also covers the truly ambiguous ctor-di shape
    /// (two tied implementors, `to: None`): it must widen neither.
    #[test]
    fn build_impact_model_never_widens_through_a_same_named_interface_elsewhere_or_an_ambiguous_ctor_di_edge() {
        let graph = make_graph(
            vec![
                def("App.Pay.IPaymentGateway", "IPaymentGateway", "App.Pay", "interface", "Pay/IPaymentGateway.cs", 3),
                def("App.Pay.StripeGateway", "StripeGateway", "App.Pay", "class", "Pay/StripeGateway.cs", 3),
                def("Other.Billing.IPaymentGateway", "IPaymentGateway", "Other.Billing", "interface", "Billing/IPaymentGateway.cs", 3),
                def("Other.Billing.LegacyGateway", "LegacyGateway", "Other.Billing", "class", "Billing/LegacyGateway.cs", 3),
                def("App.Pay.UnrelatedConsumer", "UnrelatedConsumer", "App.Pay", "class", "Pay/UnrelatedConsumer.cs", 3),
            ],
            vec![
                inherits("Pay/StripeGateway.cs", 3, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                inherits("Billing/LegacyGateway.cs", 3, "Other.Billing.IPaymentGateway", "Billing/IPaymentGateway.cs"),
                // Names the OTHER namespace's IPaymentGateway -- must never
                // reach StripeGateway just because the bare name matches.
                ctor_di_to("Pay/UnrelatedConsumer.cs", 5, "IPaymentGateway", "plain", "Other.Billing.LegacyGateway"),
                // A tied ('ambiguous') ctor-di edge naming StripeGateway's
                // OWN interface -- no `to`, so it must not widen either.
                ctor_di_no_to("Pay/UnrelatedConsumer.cs", 9, "IPaymentGateway", "ambiguous"),
            ],
        );
        let root = temp_repo_root("iface-hop-collision");
        write_manifest_fixture(
            &root,
            &["Pay/IPaymentGateway.cs", "Pay/StripeGateway.cs", "Billing/IPaymentGateway.cs", "Billing/LegacyGateway.cs", "Pay/UnrelatedConsumer.cs"],
        );
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "StripeGateway", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert!(model.rows.is_empty(), "a same-named interface elsewhere, and an ambiguous ctor-di edge, must never widen: {:?}", model.rows);
    }

    // --- the broad-interface fan-in brake ---

    const BROAD_IFACE_MANIFEST_FILES: &[&str] = &[
        "Widgets/IWidgetRepository.cs",
        "Widgets/IWidgetClock.cs",
        "Widgets/WidgetRepository.cs",
        "Widgets/GadgetService.cs",
        "Widgets/WidgetConsumer00.cs",
        "Widgets/WidgetConsumer01.cs",
        "Widgets/WidgetConsumer02.cs",
        "Widgets/WidgetConsumer03.cs",
        "Widgets/WidgetConsumer04.cs",
        "Widgets/WidgetConsumer05.cs",
        "Widgets/WidgetConsumer06.cs",
        "Widgets/WidgetConsumer07.cs",
        "Widgets/WidgetConsumer08.cs",
        "Widgets/ClockConsumer0.cs",
        "Widgets/ClockConsumer1.cs",
    ];

    /// `WidgetRepository` is the SOLE implementor of two contracts:
    /// `IWidgetRepository`, ctor-injected by 9 distinct constructors (one over
    /// the default threshold of 8), and `IWidgetClock`, injected by 2. Both are
    /// ordinary, well-named application interfaces -- nothing about their names
    /// or namespaces marks the first as plumbing, which is exactly why the
    /// name-pattern `infra` class never catches this shape. `GadgetService`
    /// names the concrete class directly.
    fn broad_iface_fixture_graph() -> graph::Graph {
        make_graph(
            vec![
                def("App.Widgets.IWidgetRepository", "IWidgetRepository", "App.Widgets", "interface", "Widgets/IWidgetRepository.cs", 3),
                def("App.Widgets.IWidgetClock", "IWidgetClock", "App.Widgets", "interface", "Widgets/IWidgetClock.cs", 3),
                def("App.Widgets.WidgetRepository", "WidgetRepository", "App.Widgets", "class", "Widgets/WidgetRepository.cs", 3),
                def("App.Widgets.GadgetService", "GadgetService", "App.Widgets", "class", "Widgets/GadgetService.cs", 3),
                def("App.Widgets.WidgetConsumer00", "WidgetConsumer00", "App.Widgets", "class", "Widgets/WidgetConsumer00.cs", 3),
                def("App.Widgets.WidgetConsumer01", "WidgetConsumer01", "App.Widgets", "class", "Widgets/WidgetConsumer01.cs", 3),
                def("App.Widgets.WidgetConsumer02", "WidgetConsumer02", "App.Widgets", "class", "Widgets/WidgetConsumer02.cs", 3),
                def("App.Widgets.WidgetConsumer03", "WidgetConsumer03", "App.Widgets", "class", "Widgets/WidgetConsumer03.cs", 3),
                def("App.Widgets.WidgetConsumer04", "WidgetConsumer04", "App.Widgets", "class", "Widgets/WidgetConsumer04.cs", 3),
                def("App.Widgets.WidgetConsumer05", "WidgetConsumer05", "App.Widgets", "class", "Widgets/WidgetConsumer05.cs", 3),
                def("App.Widgets.WidgetConsumer06", "WidgetConsumer06", "App.Widgets", "class", "Widgets/WidgetConsumer06.cs", 3),
                def("App.Widgets.WidgetConsumer07", "WidgetConsumer07", "App.Widgets", "class", "Widgets/WidgetConsumer07.cs", 3),
                def("App.Widgets.WidgetConsumer08", "WidgetConsumer08", "App.Widgets", "class", "Widgets/WidgetConsumer08.cs", 3),
                def("App.Widgets.ClockConsumer0", "ClockConsumer0", "App.Widgets", "class", "Widgets/ClockConsumer0.cs", 3),
                def("App.Widgets.ClockConsumer1", "ClockConsumer1", "App.Widgets", "class", "Widgets/ClockConsumer1.cs", 3),
            ],
            vec![
                inherits("Widgets/WidgetRepository.cs", 3, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                inherits("Widgets/WidgetRepository.cs", 3, "App.Widgets.IWidgetClock", "Widgets/IWidgetClock.cs"),
                uses_type("Widgets/GadgetService.cs", 6, "App.Widgets.WidgetRepository", "Widgets/WidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer00.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer00.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer01.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer01.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer02.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer02.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer03.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer03.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer04.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer04.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer05.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer05.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer06.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer06.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer07.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer07.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/WidgetConsumer08.cs", 5, "IWidgetRepository", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/WidgetConsumer08.cs", 5, "App.Widgets.IWidgetRepository", "Widgets/IWidgetRepository.cs"),
                ctor_di_to("Widgets/ClockConsumer0.cs", 5, "IWidgetClock", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/ClockConsumer0.cs", 5, "App.Widgets.IWidgetClock", "Widgets/IWidgetClock.cs"),
                ctor_di_to("Widgets/ClockConsumer1.cs", 5, "IWidgetClock", "plain", "App.Widgets.WidgetRepository"),
                uses_type("Widgets/ClockConsumer1.cs", 5, "App.Widgets.IWidgetClock", "Widgets/IWidgetClock.cs"),
            ],
        )
    }

    fn broad_iface_fixture_root() -> PathBuf {
        let root = temp_repo_root("broad-iface");
        write_manifest_fixture(&root, BROAD_IFACE_MANIFEST_FILES);
        root
    }

    #[test]
    fn build_impact_model_brakes_a_broad_interface_by_fan_in_while_a_narrow_one_on_the_same_class_still_hops() {
        let graph = broad_iface_fixture_graph();
        let root = broad_iface_fixture_root();
        let index = load_graph_index(&graph, &root);
        assert_eq!(index.ctor_di_fanin.get("IWidgetRepository"), Some(&9), "fan-in counts distinct constructor sites");
        assert_eq!(index.ctor_di_fanin.get("IWidgetClock"), Some(&2));

        let model = match build_impact_model(&index, "WidgetRepository", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
        files.sort();
        assert_eq!(
            files,
            vec!["Widgets/ClockConsumer0.cs", "Widgets/ClockConsumer1.cs", "Widgets/GadgetService.cs"],
            "the broad contract is braked on BOTH widening paths; the narrow one and the direct-name edge are untouched"
        );
        assert_eq!(
            model.braked,
            vec![BrakedIface { iface: "IWidgetRepository".to_string(), fanin: 9 }],
            "the narrowing is reported, never silent"
        );
    }

    #[test]
    fn build_impact_model_iface_max_fanin_zero_disables_the_brake_and_restores_the_ds_0050_radius() {
        let graph = broad_iface_fixture_graph();
        let root = broad_iface_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "WidgetRepository", 1, DEFAULT_CAP, true, 0, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.rows.len(), 12, "every ctor-injected consumer of both contracts, plus the direct-name reference");
        assert!(model.braked.is_empty(), "a brake that never fired reports nothing");

        // A threshold BELOW the narrow contract's own fan-in brakes it too,
        // widest first in the report -- the brake is a number, not a name list.
        let tight = match build_impact_model(&index, "WidgetRepository", 1, DEFAULT_CAP, true, 1, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(tight.rows.iter().map(|r| r.file.as_str()).collect::<Vec<_>>(), vec!["Widgets/GadgetService.cs"]);
        assert_eq!(
            tight.braked,
            vec![
                BrakedIface { iface: "IWidgetRepository".to_string(), fanin: 9 },
                BrakedIface { iface: "IWidgetClock".to_string(), fanin: 2 },
            ]
        );
    }

    #[test]
    fn build_impact_model_no_iface_keeps_its_ds_0050_meaning_no_hop_at_all_and_no_brake_report() {
        let graph = broad_iface_fixture_graph();
        let root = broad_iface_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "WidgetRepository", 1, DEFAULT_CAP, false, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            model.rows.iter().map(|r| r.file.as_str()).collect::<Vec<_>>(),
            vec!["Widgets/GadgetService.cs"],
            "the hop is off entirely, so the narrow contract does not widen either"
        );
        assert!(model.braked.is_empty(), "nothing was braked because nothing was attempted");
    }

    // --- the hub-file brake ---

    /// Two hub shapes reaching the same seed, each with its own consumers so
    /// the two brakes can be told apart: `Api/Startup.cs` is a hub by NAME (an
    /// entry point, whatever its in-degree), `Core/Shared.cs` only by
    /// IN-DEGREE. `Core/Plain.cs` is neither and must keep expanding.
    /// `Core/S5.cs`'s edge into `Core/Shared.cs` is a heuristic guess, so the
    /// in-degree it contributes is the proof that the index spans both edge
    /// kinds.
    fn hub_fixture_graph() -> graph::Graph {
        let mut defs = vec![
            def("App.Core.Widget", "Widget", "App.Core", "class", "Core/Widget.cs", 3),
            def("App.Api.Startup", "Startup", "App.Api", "class", "Api/Startup.cs", 3),
            def("App.Core.Shared", "Shared", "App.Core", "class", "Core/Shared.cs", 3),
            def("App.Core.Plain", "Plain", "App.Core", "class", "Core/Plain.cs", 3),
            def("App.Core.S5", "S5", "App.Core", "class", "Core/S5.cs", 3),
            def("App.Core.PlainUser", "PlainUser", "App.Core", "class", "Core/PlainUser.cs", 3),
        ];
        let mut edges = vec![
            uses_type("Api/Startup.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
            uses_type("Core/Shared.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
            uses_type("Core/Plain.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
            heuristic_uses_type("Core/S5.cs", 4, "App.Core.Shared", "Core/Shared.cs"),
            uses_type("Core/PlainUser.cs", 4, "App.Core.Plain", "Core/Plain.cs"),
        ];
        for n in ["A", "B", "C", "D"] {
            defs.push(def(&format!("App.Api.{n}"), n, "App.Api", "class", &format!("Api/{n}.cs"), 3));
            edges.push(uses_type(&format!("Api/{n}.cs"), 4, "App.Api.Startup", "Api/Startup.cs"));
        }
        for n in ["S1", "S2", "S3", "S4"] {
            defs.push(def(&format!("App.Core.{n}"), n, "App.Core", "class", &format!("Core/{n}.cs"), 3));
            edges.push(uses_type(&format!("Core/{n}.cs"), 4, "App.Core.Shared", "Core/Shared.cs"));
        }
        make_graph(defs, edges)
    }

    fn hub_fixture_root() -> PathBuf {
        let root = temp_repo_root("hub-file");
        let mut files: Vec<String> = vec![
            "Core/Widget.cs".into(),
            "Api/Startup.cs".into(),
            "Core/Shared.cs".into(),
            "Core/Plain.cs".into(),
            "Core/PlainUser.cs".into(),
            "Core/S5.cs".into(),
        ];
        for n in ["A", "B", "C", "D"] {
            files.push(format!("Api/{n}.cs"));
        }
        for n in ["S1", "S2", "S3", "S4"] {
            files.push(format!("Core/{n}.cs"));
        }
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        write_manifest_fixture(&root, &refs);
        root
    }

    fn hub_files_of(model: &ImpactModel) -> Vec<String> {
        let mut files: Vec<String> = model.rows.iter().map(|r| r.file.clone()).collect();
        files.sort();
        files
    }

    #[test]
    fn load_graph_index_hub_indegree_counts_distinct_referring_files_across_both_edge_kinds() {
        let graph = hub_fixture_graph();
        let root = hub_fixture_root();
        let index = load_graph_index(&graph, &root);
        assert_eq!(index.hub_indegree.get("Api/Startup.cs").copied(), Some(4));
        assert_eq!(
            index.hub_indegree.get("Core/Shared.cs").copied(),
            Some(5),
            "the heuristic referrer counts too -- a hub is reached by either kind"
        );
        assert_eq!(index.hub_indegree.get("Core/Plain.cs").copied(), Some(1));
        assert!(index.hub_indegree.get("Api/A.cs").is_none(), "a file nothing references carries no entry at all");
    }

    #[test]
    fn build_impact_model_a_hub_file_is_recorded_classed_infra_and_never_expanded_through() {
        let graph = hub_fixture_graph();
        let root = hub_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Widget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            hub_files_of(&model),
            vec![
                "Api/Startup.cs",
                "Core/Plain.cs",
                "Core/PlainUser.cs",
                "Core/S1.cs",
                "Core/S2.cs",
                "Core/S3.cs",
                "Core/S4.cs",
                "Core/S5.cs",
                "Core/Shared.cs"
            ],
            "the entry point is reached but its own four consumers are not"
        );
        let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
        assert!(row_of("Api/Startup.cs").infra, "the row says why the walk stopped there");
        assert!(!row_of("Core/Shared.cs").infra, "an ordinary file carries no class key at all");
        assert_eq!(model.braked_files, vec![BrakedFile { file: "Api/Startup.cs".to_string(), indegree: 4 }]);
        assert!(model.braked.is_empty(), "no interface was braked here");
    }

    #[test]
    fn build_impact_model_hub_max_indegree_brakes_a_file_no_name_pattern_matches_and_zero_disables_that_half_only() {
        let graph = hub_fixture_graph();
        let root = hub_fixture_root();
        let index = load_graph_index(&graph, &root);
        let tight = match build_impact_model(&index, "Widget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, 5) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            hub_files_of(&tight),
            vec!["Api/Startup.cs", "Core/Plain.cs", "Core/PlainUser.cs", "Core/Shared.cs"],
            "the in-degree-5 file stops expanding too"
        );
        assert_eq!(
            tight.braked_files,
            vec![
                BrakedFile { file: "Core/Shared.cs".to_string(), indegree: 5 },
                BrakedFile { file: "Api/Startup.cs".to_string(), indegree: 4 },
            ],
            "widest-first, then by path"
        );

        let off = match build_impact_model(&index, "Widget", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, 0) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            off.braked_files,
            vec![BrakedFile { file: "Api/Startup.cs".to_string(), indegree: 4 }],
            "0 disables the threshold; the name-pattern classification is not a threshold and stays on"
        );
        assert!(hub_files_of(&off).contains(&"Core/S1.cs".to_string()), "the in-degree hub widens again");
        assert!(
            !hub_files_of(&off).contains(&"Api/A.cs".to_string()),
            "an entry point is still an entry point at --hub-max-indegree 0"
        );
    }

    #[test]
    fn build_impact_model_a_hub_reached_on_the_last_hop_is_never_reported_as_braked() {
        let graph = hub_fixture_graph();
        let root = hub_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Widget", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(hub_files_of(&model), vec!["Api/Startup.cs", "Core/Plain.cs", "Core/Shared.cs"]);
        assert!(
            model.rows.iter().find(|r| r.file == "Api/Startup.cs").unwrap().infra,
            "the classification is a fact about the file, not about whether the walk had another hop left"
        );
        assert!(model.braked_files.is_empty(), "no hop remained, so no widening was refused");
    }

    #[test]
    fn is_infra_file_matches_the_four_shapes_and_nothing_that_merely_resembles_them() {
        for f in [
            "src/Program.cs",
            "Startup.cs",
            "a/CatalogServiceExtensions.cs",
            "a/FooServiceCollectionExtensions.cs",
            "a/JobQueueRegistration.cs",
            "a/DependencyResolution/Wire.cs",
            "a/CompositionRootTests.cs",
            "a/GroupControllerTestsBase.cs",
            "a/ControllerTestBase.cs",
            "a/BaseFixture.cs",
        ] {
            assert!(is_infra_file(f), "{f} must classify as infra");
        }
        for f in [
            "src/ProgramManager.cs",
            "src/StartupRunner.cs",
            "src/Registrations.cs",
            "src/DependencyResolutionHelper.cs",
            "src/CompositionRoot.Extra.cs",
            "src/Widget.cs",
        ] {
            assert!(!is_infra_file(f), "{f} must NOT classify as infra");
        }
    }

    // --- the per-kind referencing line on an impact row ---

    const FROM_LINES_MANIFEST_FILES: &[&str] =
        &["Pay/IPaymentGateway.cs", "Pay/StripeGateway.cs", "Pay/Mixed.cs", "Pay/GuessOnly.cs"];

    /// One consumer file reached by ALL FOUR kinds the walk distinguishes, each
    /// at its own line: two direct `uses-type` refs (lines 5 and 12 -- the
    /// lower one must win), a `ctor-di` edge (line 7) plus its companion plain
    /// ref at the identical site (deduped away, never a second kind), a direct
    /// interface-name ref (line 20), and a heuristic guess (line 30).
    fn from_lines_fixture_graph() -> graph::Graph {
        make_graph(
            vec![
                def("App.Pay.IPaymentGateway", "IPaymentGateway", "App.Pay", "interface", "Pay/IPaymentGateway.cs", 3),
                def("App.Pay.StripeGateway", "StripeGateway", "App.Pay", "class", "Pay/StripeGateway.cs", 3),
                def("App.Pay.Mixed", "Mixed", "App.Pay", "class", "Pay/Mixed.cs", 3),
                def("App.Pay.GuessOnly", "GuessOnly", "App.Pay", "class", "Pay/GuessOnly.cs", 3),
            ],
            vec![
                inherits("Pay/StripeGateway.cs", 3, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                uses_type("Pay/Mixed.cs", 12, "App.Pay.StripeGateway", "Pay/StripeGateway.cs"),
                uses_type("Pay/Mixed.cs", 5, "App.Pay.StripeGateway", "Pay/StripeGateway.cs"),
                ctor_di_to("Pay/Mixed.cs", 7, "IPaymentGateway", "plain", "App.Pay.StripeGateway"),
                uses_type("Pay/Mixed.cs", 7, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                uses_type("Pay/Mixed.cs", 20, "App.Pay.IPaymentGateway", "Pay/IPaymentGateway.cs"),
                heuristic_uses_member("Pay/Mixed.cs", 30, "App.Pay.StripeGateway", "Pay/StripeGateway.cs"),
                heuristic_uses_member("Pay/GuessOnly.cs", 9, "App.Pay.StripeGateway", "Pay/StripeGateway.cs"),
            ],
        )
    }

    fn from_lines_fixture_root() -> PathBuf {
        let root = temp_repo_root("from-lines");
        write_manifest_fixture(&root, FROM_LINES_MANIFEST_FILES);
        root
    }

    #[test]
    fn build_impact_model_names_one_referencing_line_per_edge_kind_lowest_line_per_kind() {
        let graph = from_lines_fixture_graph();
        let root = from_lines_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "StripeGateway", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let row_of = |file: &str| model.rows.iter().find(|r| r.file == file).unwrap();
        assert_eq!(
            row_of("Pay/Mixed.cs").from_lines,
            vec![("direct", 5), ("ctor-di", 7), ("heuristic", 30), ("iface", 20)],
            "key order is the walk's own kind declaration order, never a map iteration"
        );
        assert_eq!(row_of("Pay/Mixed.cs").via_count, 4, "the companion ref at the ctor-di site is still deduped, not a fifth hit");
        assert_eq!(row_of("Pay/GuessOnly.cs").from_lines, vec![("heuristic", 9)]);
    }

    #[test]
    fn build_impact_model_no_iface_drops_the_two_interface_hop_kinds_and_keeps_the_rest() {
        let graph = from_lines_fixture_graph();
        let root = from_lines_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "StripeGateway", 1, DEFAULT_CAP, false, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let row = model.rows.iter().find(|r| r.file == "Pay/Mixed.cs").unwrap();
        assert_eq!(
            row.from_lines,
            vec![("direct", 5), ("heuristic", 30)],
            "no hop was attempted, so neither interface kind may claim a line"
        );
    }

    #[test]
    fn build_impact_model_an_ambiguous_only_hit_still_names_its_line_under_the_direct_kind() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "One/Config.cs", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let row = model.rows.iter().find(|r| r.file == "Three/Consumer.cs").unwrap();
        assert_eq!(row.ambiguous_count, 1);
        assert_eq!(
            row.from_lines,
            vec![("direct", 4)],
            "refs' own tie-break: an ambiguous site is used only when no resolved one exists"
        );
    }

    #[test]
    fn build_impact_model_a_row_no_kind_could_attribute_a_line_to_carries_no_from_lines_at_all() {
        let graph = make_graph(
            vec![
                def("App.A.Seed", "Seed", "App.A", "class", "A/Seed.cs", 1),
                def("App.A.User", "User", "App.A", "class", "A/User.cs", 1),
            ],
            vec![uses_type("A/User.cs", 0, "App.A.Seed", "A/Seed.cs")],
        );
        let root = temp_repo_root("from-lines-empty");
        write_manifest_fixture(&root, &["A/Seed.cs", "A/User.cs"]);
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Seed", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.rows.iter().map(|r| r.file.as_str()).collect::<Vec<_>>(), vec!["A/User.cs"]);
        assert!(model.rows[0].from_lines.is_empty(), "a line-less edge adds no key, exactly like every other conditional field");
    }

    // --- 14: enum-member resolve_symbol by id and by unique simple name ---

    #[test]
    fn resolve_symbol_finds_enum_member_by_id_and_unique_simple_name() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        assert_eq!(resolve_symbol(&index, "App.Enums.PostType.Question"), Resolution::Resolved("App.Enums.PostType.Question".into()));
        assert_eq!(resolve_symbol(&index, "Question"), Resolution::Resolved("App.Enums.PostType.Question".into()));
    }

    // --- the Enum.Member tail and the member-count split ---

    #[test]
    fn resolve_symbol_accepts_a_dotted_tail_of_a_def_id_and_refuses_a_shared_one() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        assert_eq!(
            resolve_symbol(&index, "PostType.Question"),
            Resolution::Resolved("App.Enums.PostType.Question".into()),
            "the spelling a caller reaches for -- never the namespace-qualified id the graph keys it under"
        );
        assert_eq!(
            resolve_symbol(&index, "Enums.PostType"),
            Resolution::Resolved("App.Enums.PostType".into()),
            "the tail rule is not enum-specific: any dotted suffix of exactly one def id resolves"
        );
        assert_eq!(resolve_symbol(&index, "PostType.Missing"), Resolution::NotFound);

        let two = make_graph(
            vec![
                def("App.One.Mode", "Mode", "App.One", "enum", "One/Mode.cs", 1),
                def("App.One.Mode.Fast", "Fast", "App.One", "enum-member", "One/Mode.cs", 2),
                def("App.Two.Mode", "Mode", "App.Two", "enum", "Two/Mode.cs", 1),
                def("App.Two.Mode.Fast", "Fast", "App.Two", "enum-member", "Two/Mode.cs", 2),
            ],
            vec![],
        );
        let two_root = temp_repo_root("enum-tail-ambiguous");
        write_manifest_fixture(&two_root, &["One/Mode.cs", "Two/Mode.cs"]);
        let two_index = load_graph_index(&two, &two_root);
        assert_eq!(
            resolve_symbol(&two_index, "Mode.Fast"),
            Resolution::Ambiguous(vec!["App.One.Mode.Fast".into(), "App.Two.Mode.Fast".into()])
        );
    }

    #[test]
    fn build_refs_model_on_an_enum_appends_member_refs_in_declaration_order() {
        let graph = make_graph(
            vec![
                def("App.Flags.Toggles", "Toggles", "App.Flags", "enum", "Flags/Toggles.cs", 3),
                def("App.Flags.Toggles.EnableX", "EnableX", "App.Flags", "enum-member", "Flags/Toggles.cs", 5),
                def("App.Flags.Toggles.EnableY", "EnableY", "App.Flags", "enum-member", "Flags/Toggles.cs", 6),
                def("App.Flags.Toggles.EnableZ", "EnableZ", "App.Flags", "enum-member", "Flags/Toggles.cs", 7),
                def("App.Run.Runner", "Runner", "App.Run", "class", "Run/Runner.cs", 3),
            ],
            vec![
                uses_type("Run/Runner.cs", 5, "App.Flags.Toggles", "Flags/Toggles.cs"),
                uses_member("Run/Runner.cs", 6, "App.Flags.Toggles.EnableY", "Flags/Toggles.cs"),
                uses_member("Run/Runner.cs", 7, "App.Flags.Toggles.EnableX", "Flags/Toggles.cs"),
                uses_member("Run/Runner.cs", 8, "App.Flags.Toggles.EnableX", "Flags/Toggles.cs"),
            ],
        );
        let root = temp_repo_root("enum-member-refs");
        write_manifest_fixture(&root, &["Flags/Toggles.cs", "Run/Runner.cs"]);
        let index = load_graph_index(&graph, &root);

        let model = match build_refs_model(&index, "Toggles", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            model.member_refs,
            Some(MemberRefs {
                total: 3,
                member_count: 2,
                members: vec![
                    MemberRefEntry { name: "EnableX".into(), count: 2 },
                    MemberRefEntry { name: "EnableY".into(), count: 1 },
                ],
                dropped: 0,
            }),
            "declaration order, never count order, and a member nothing references is left out entirely"
        );
        assert_eq!(model.inbound.uses_member.total, 3, "the existing union is unchanged by the split");

        let other = match build_refs_model(&index, "Runner", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(other.member_refs, None, "nothing but an enum carries the field");
    }

    #[test]
    fn build_impact_model_on_an_enum_file_reaches_files_that_use_only_its_members() {
        let graph = make_graph(
            vec![
                def("App.Flags.Toggles", "Toggles", "App.Flags", "enum", "Flags/Toggles.cs", 3),
                def("App.Flags.Toggles.EnableX", "EnableX", "App.Flags", "enum-member", "Flags/Toggles.cs", 5),
                def("App.Run.Runner", "Runner", "App.Run", "class", "Run/Runner.cs", 3),
            ],
            vec![uses_member("Run/Runner.cs", 6, "App.Flags.Toggles.EnableX", "Flags/Toggles.cs")],
        );
        let root = temp_repo_root("enum-member-impact");
        write_manifest_fixture(&root, &["Flags/Toggles.cs", "Run/Runner.cs"]);
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Flags/Toggles.cs", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(
            model.rows.iter().map(|r| (r.file.as_str(), r.top_symbols.clone())).collect::<Vec<_>>(),
            vec![("Run/Runner.cs", vec!["EnableX".to_string()])],
            "the member def is a def of the seed FILE, so a member-only consumer is still in the blast radius"
        );
    }

    // --- 15: build_refs_model on an enum member ---

    #[test]
    fn build_refs_model_on_enum_member_def_site_plus_inbound_uses_member() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "Question", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.kind, "enum-member");
        assert_eq!(model.sites, vec![DefSite { file: "Enums/PostType.cs".into(), line: 6 }]);
        assert_eq!(model.inbound.uses_member.total, 1);
        assert_eq!(model.inbound.uses_member.rows[0].file, "Consumers/Reader.cs");
        assert_eq!(model.inbound.uses_member.rows[0].line, 8);
    }

    // --- 16: build_refs_model on the enum itself: members' inbound unions in ---

    #[test]
    fn build_refs_model_on_enum_itself_unions_members_inbound_uses_member() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_refs_model(&index, "PostType", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.kind, "enum");
        assert_eq!(model.inbound.uses_member.total, 1, "member-level access must surface on the enum query");
        assert_eq!(model.inbound.uses_member.rows[0].file, "Consumers/Reader.cs");
        assert_eq!(model.inbound.uses_member.rows[0].line, 8);
    }

    // --- 17: two enums, each with a same-named member -> ambiguous ---

    #[test]
    fn two_enums_with_same_named_member_resolve_ambiguous_both_sites_surfaced() {
        let graph = make_graph(
            vec![
                def("App.One.StatusEnum", "StatusEnum", "App.One", "enum", "One/StatusEnum.cs", 1),
                def("App.One.StatusEnum.Changed", "Changed", "App.One", "enum-member", "One/StatusEnum.cs", 2),
                def("App.Two.OtherEnum", "OtherEnum", "App.Two", "enum", "Two/OtherEnum.cs", 1),
                def("App.Two.OtherEnum.Changed", "Changed", "App.Two", "enum-member", "Two/OtherEnum.cs", 2),
            ],
            vec![],
        );
        let root = temp_repo_root("two-enums-changed");
        write_manifest_fixture(&root, &["One/StatusEnum.cs", "Two/OtherEnum.cs"]);
        let index = load_graph_index(&graph, &root);

        match resolve_symbol(&index, "Changed") {
            Resolution::Ambiguous(mut ids) => {
                ids.sort();
                assert_eq!(ids, vec!["App.One.StatusEnum.Changed".to_string(), "App.Two.OtherEnum.Changed".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }

        match build_refs_model(&index, "Changed", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Ambiguous(mut ids) => {
                ids.sort();
                assert_eq!(ids, vec!["App.One.StatusEnum.Changed".to_string(), "App.Two.OtherEnum.Changed".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    // --- 18: uses-member edge counts toward 1-hop blast radius ---

    #[test]
    fn build_impact_model_uses_member_edge_counts_toward_one_hop_blast_radius() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Question", 1, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        assert_eq!(model.rows.iter().map(|r| r.file.clone()).collect::<Vec<_>>(), vec!["Consumers/Reader.cs".to_string()]);
        assert_eq!(model.rows[0].hop, 1);
    }

    // --- 19: uses-member edge propagates a second hop through the ordinary type-ref graph ---

    #[test]
    fn build_impact_model_uses_member_edge_propagates_second_hop() {
        let graph = enum_fixture_graph();
        let root = enum_fixture_root();
        let index = load_graph_index(&graph, &root);
        let model = match build_impact_model(&index, "Question", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected Resolved, got {other:?}"),
        };
        let mut files: Vec<&str> = model.rows.iter().map(|r| r.file.as_str()).collect();
        files.sort();
        assert_eq!(files, vec!["Consumers/Reader.cs", "Consumers/TwoHop.cs"]);
        assert_eq!(model.rows.iter().find(|r| r.file == "Consumers/TwoHop.cs").unwrap().hop, 2);
    }

    // --- 20: impact_walk + personalized_page_rank -- finite, non-negative, never NaN ---

    #[test]
    fn impact_walk_and_ppr_never_negative_or_nan() {
        let graph = base_fixture_graph();
        let root = base_fixture_root();
        let index = load_graph_index(&graph, &root);
        let walk = impact_walk(&index, &["App.Widgets.IWidget".to_string()], 2, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE);
        let mut nodes: SeqSet<String> = SeqSet::new();
        for f in walk.seed_files.iter() {
            nodes.insert(f.clone());
        }
        for f in walk.visited.keys() {
            nodes.insert(f.clone());
        }
        let seeds: Vec<String> = walk.seed_files.iter().cloned().collect();
        let rank = personalized_page_rank(&nodes.into_vec(), &walk.fwd_adj, &seeds, DEFAULT_DAMPING, DEFAULT_ITERATIONS);
        for v in rank.values() {
            assert!(v.is_finite() && *v >= 0.0, "every rank must be a finite, non-negative number, got {v}");
        }
    }

    // --- extra: looks_like_file_path pinned directly (small and worth
    // documenting explicitly, incl. the qualified-id quirk). ---

    #[test]
    fn looks_like_file_path_matches_js_regex_semantics() {
        assert!(looks_like_file_path("Widgets/IWidget.cs"));
        assert!(looks_like_file_path("IWidget.cs"));
        assert!(!looks_like_file_path("IWidget"));
        assert!(looks_like_file_path("App.Widgets.IWidget"), "qualified id with alnum trailing segment matches the regex, same as JS");
        assert!(!looks_like_file_path("Foo."), "trailing dot with nothing after it does not match (empty extension)");
        assert!(!looks_like_file_path("Foo!"), "no dot-then-alnum-to-end anywhere");
    }

    // ========================================================================
    // The CLI contract for heuristic edges. The shapes pinned here (row order,
    // the shared cap, which edges seed a hop, the count line) live in this
    // module plus render.rs, so these tests drive THOSE directly -- same
    // fixtures, same expected literals, no process spawn.
    //
    // A hand-written graph rather than a `devscout map` of C# sources on
    // purpose: driving these from source would mean building a fixture whose
    // resolution happens to produce 51 inbound edges. The resolver's own tests
    // own the question of WHICH edges get tagged; these own what the
    // query+render layers do once they are.
    // ========================================================================

    fn widget_def() -> graph::Def {
        graph::Def {
            id: "App.Core.Widget".into(),
            name: "Widget".into(),
            namespace: "App.Core".into(),
            kind: "class".into(),
            file: "Core/Widget.cs".into(),
            line: 3,
            methods: vec!["Render".to_string()],
            test_methods: vec![],
            also_in: vec![],
            end_line: 0,
        }
    }

    /// A manifest listing every def file and every edge's from_file, so
    /// `manifest_gap` stays 0 and the rendered bytes carry no trailing gap line.
    fn stage4_root(defs: &[graph::Def], edges: &[graph::Edge], label: &str) -> PathBuf {
        let root = temp_repo_root(label);
        let mut files: Vec<String> = Vec::new();
        for d in defs {
            if !files.contains(&d.file) {
                files.push(d.file.clone());
            }
        }
        for e in edges {
            let f = edge_loc(e).0.to_string();
            if !files.contains(&f) {
                files.push(f);
            }
        }
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        write_manifest_fixture(&root, &refs);
        root
    }

    #[test]
    fn stage4_cli_refs_lists_every_precise_row_before_any_heuristic_row_and_suffixes_only_the_guesses() {
        // Alphabetically Guess.cs sorts BEFORE Precise.cs, so a single
        // by-location sort over the union would interleave them. The split is
        // what puts the facts first, not the sort.
        let defs = vec![widget_def()];
        let edges = vec![
            uses_member("Consumers/Precise.cs", 10, "App.Core.Widget", "Core/Widget.cs"),
            heuristic_uses_member("Consumers/Guess.cs", 7, "App.Core.Widget", "Core/Widget.cs"),
        ];
        let root = stage4_root(&defs, &edges, "stage4-order");
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        let model = match build_refs_model(&index, "Widget", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };

        let out = crate::render::render_refs_text(&model);
        let lines: Vec<&str> = out.lines().collect();
        let start = lines.iter().position(|l| *l == "  uses-member (2):").expect("a uses-member block counting both rows");
        assert_eq!(
            &lines[start + 1..start + 3],
            ["    Consumers/Precise.cs:10  uses-member", "    Consumers/Guess.cs:7  uses-member (heuristic)"]
        );

        // --compact marks the same row with the one-character form.
        let compact = crate::render::render_refs_compact(&model);
        assert!(compact.contains("in:uses-member (2):\n  Consumers/Precise.cs:10\n  Consumers/Guess.cs:7h"), "{compact}");
    }

    #[test]
    fn stage4_cli_refs_precise_rows_filling_the_cap_leave_no_room_for_heuristic_rows() {
        let defs = vec![widget_def()];
        let mut edges: Vec<graph::Edge> = (0..31)
            .map(|i| uses_member(&format!("Consumers/C{i:02}.cs"), 4, "App.Core.Widget", "Core/Widget.cs"))
            .collect();
        edges.push(heuristic_uses_member("Consumers/Zzz.cs", 9, "App.Core.Widget", "Core/Widget.cs"));
        let root = stage4_root(&defs, &edges, "stage4-cap");
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        let model = match build_refs_model(&index, "Widget", true, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) {
            RefsResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };

        let out = crate::render::render_refs_text(&model);
        assert!(out.contains("  uses-member (32, 2 dropped):"), "the cap is shared: 32 rows, 30 shown, 2 dropped\n{out}");
        assert!(out.contains("\n  +2 more\n"), "one trailer carries the exact count the call did not return\n{out}");
        assert!(!out.contains("(heuristic)"), "precise rows have priority -- a full cap shows zero guesses");
        assert_eq!(out.lines().filter(|l| l.starts_with("    Consumers/")).count(), 30);
    }

    fn stage4_impact_defs() -> Vec<graph::Def> {
        vec![
            widget_def(),
            def("App.Direct.Direct", "Direct", "App.Direct", "class", "Consumers/Direct.cs", 3),
            def("App.Guessed.Guessed", "Guessed", "App.Guessed", "class", "Consumers/Guessed.cs", 3),
        ]
    }

    #[test]
    fn stage4_cli_impact_declares_heuristic_reached_files_beside_the_affected_count_only_when_there_are_some() {
        let precise = uses_type("Consumers/Direct.cs", 4, "App.Core.Widget", "Core/Widget.cs");

        let defs = stage4_impact_defs();
        let edges = vec![precise.clone(), heuristic_uses_member("Consumers/Guessed.cs", 8, "App.Core.Widget", "Core/Widget.cs")];
        let root = stage4_root(&defs, &edges, "stage4-impact-with");
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        let model = match build_impact_model(&index, "Core/Widget.cs", DEFAULT_HOPS, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };
        let out = crate::render::render_impact_text("Core/Widget.cs", &model);
        assert!(out.contains("affected files: 1 (+1 heuristic)  shown: 2  dropped: 0"), "{out}");
        let lines: Vec<&str> = out.lines().collect();
        let header = lines.iter().position(|l| *l == "file  hops  via  top-symbols").expect("row header present");
        assert_eq!(
            &lines[header + 1..header + 3],
            ["Consumers/Direct.cs  1  1  Widget", "Consumers/Guessed.cs  1  1  Widget (heuristic)"],
            "heuristic-reached files are listed after every precise one, never ranked among them"
        );
        assert!(
            crate::render::render_impact_compact("Core/Widget.cs", &model).contains("Consumers/Guessed.cs via=1h"),
            "compact reports the guess count, never `via=0`"
        );

        // The same query on a graph with no heuristic edges must render the
        // count line byte-for-byte as it did before this stage -- no empty
        // parenthetical anywhere.
        let defs = stage4_impact_defs();
        let edges = vec![precise];
        let root = stage4_root(&defs, &edges, "stage4-impact-without");
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        let model = match build_impact_model(&index, "Core/Widget.cs", DEFAULT_HOPS, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
            ImpactResult::Resolved(m) => m,
            other => panic!("expected a resolved model, got {other:?}"),
        };
        let out = crate::render::render_impact_text("Core/Widget.cs", &model);
        assert!(out.contains("affected files: 1  shown: 1  dropped: 0"), "{out}");
        assert!(!out.contains("heuristic"), "{out}");
        assert!(!crate::render::render_impact_compact("Core/Widget.cs", &model).contains("heuristic"));
    }

    #[test]
    fn stage4_cli_impact_stops_at_a_heuristic_edge_instead_of_walking_through_it_to_a_second_hop() {
        // Core/Widget.cs <- Mid/Middle.cs <- Far/Far.cs. The first link is the
        // one under test; the second is always precise, so whether Far/Far.cs
        // shows up depends purely on whether the walk was allowed to continue
        // through link 1.
        let impact_out = |heuristic: bool, label: &str| {
            let defs = vec![
                widget_def(),
                def("App.Mid.Middle", "Middle", "App.Mid", "class", "Mid/Middle.cs", 3),
                def("App.Far.Far", "Far", "App.Far", "class", "Far/Far.cs", 3),
            ];
            let first = if heuristic {
                heuristic_uses_member("Mid/Middle.cs", 8, "App.Core.Widget", "Core/Widget.cs")
            } else {
                uses_member("Mid/Middle.cs", 8, "App.Core.Widget", "Core/Widget.cs")
            };
            let edges = vec![first, uses_type("Far/Far.cs", 4, "App.Mid.Middle", "Mid/Middle.cs")];
            let root = stage4_root(&defs, &edges, label);
            let g = make_graph(defs, edges);
            let index = load_graph_index(&g, &root);
            let model = match build_impact_model(&index, "Core/Widget.cs", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) {
                ImpactResult::Resolved(m) => m,
                other => panic!("expected a resolved model, got {other:?}"),
            };
            crate::render::render_impact_text("Core/Widget.cs", &model)
        };

        let control = impact_out(false, "stage4-walk-control");
        assert!(control.contains("Far/Far.cs"), "control: with a precise first link the walk reaches the second hop\n{control}");
        assert!(control.contains("affected files: 2  "), "{control}");

        let guessed = impact_out(true, "stage4-walk-guess");
        assert!(guessed.contains("Mid/Middle.cs  1  1  Widget (heuristic)"), "the guessed file itself is still reported\n{guessed}");
        assert!(
            !guessed.contains("Far/Far.cs"),
            "a guess may reach a file and must never become the premise of the next hop -- compounding guesses is how blast radius turns into fiction\n{guessed}"
        );
        assert!(guessed.contains("affected files: 0 (+1 heuristic)  shown: 1  dropped: 0"), "{guessed}");
    }

    /// The heuristic adjacency really is SEPARATE: a graph whose only
    /// uses-type edge is tagged leaves the precise inbound table empty, so no
    /// consumer that never asked for guesses can see one.
    #[test]
    fn stage4_heuristic_edges_never_enter_the_precise_adjacency() {
        let defs = vec![widget_def()];
        let edges = vec![heuristic_uses_type("Consumers/Guess.cs", 7, "App.Core.Widget", "Core/Widget.cs")];
        let root = stage4_root(&defs, &edges, "stage4-adjacency");
        let g = make_graph(defs, edges);
        let index = load_graph_index(&g, &root);
        assert!(index.inbound.get("App.Core.Widget").is_none(), "the precise inbound table never sees a tagged edge");
        assert_eq!(
            index.heuristic_inbound.get("App.Core.Widget").map(|e| e.uses_type.len()),
            Some(1),
            "and the heuristic one holds it, keyed by the same def id"
        );
        assert!(index.outbound_by_file.get("Consumers/Guess.cs").is_none());
        assert_eq!(index.heuristic_outbound_by_file.get("Consumers/Guess.cs").map(|e| e.uses_type.len()), Some(1));
    }

    // ========================================================================
    // Test-coverage: test_defs_by_file, build_tests_model, impact tests_affected.
    // ========================================================================

    /// `def()` carrying a test-method list -- what makes its file a TEST file.
    fn test_def(id: &str, name: &str, file: &str, line: usize, test_methods: &[&str]) -> graph::Def {
        graph::Def {
            test_methods: test_methods.iter().map(|s| s.to_string()).collect(),
            ..def(id, name, "App.Orders.Tests", "class", file, line)
        }
    }

    const TESTS_MANIFEST_FILES: &[&str] = &[
        "src/OrderService.cs",
        "src/Untested.cs",
        "tests/OrderServiceTests.cs",
        "tests/Fakes.cs",
        "tests/Partial.cs",
        "tests/Partial.Extra.cs",
    ];

    /// One production type referenced twice from a real test file, once from a
    /// non-test neighbour, and once by a GUESS from a partial test class's
    /// second declaring file -- the four cases the model has to tell apart.
    fn tests_fixture_graph() -> graph::Graph {
        let mut partial = test_def("App.Orders.Tests.PartialTests", "PartialTests", "tests/Partial.cs", 3, &["Scales"]);
        partial.also_in = vec![graph::AlsoIn { file: "tests/Partial.Extra.cs".into(), line: 3 }];
        make_graph(
            vec![
                def("App.Orders.OrderService", "OrderService", "App.Orders", "class", "src/OrderService.cs", 3),
                def("App.Orders.Untested", "Untested", "App.Orders", "class", "src/Untested.cs", 3),
                test_def("App.Orders.Tests.OrderServiceTests", "OrderServiceTests", "tests/OrderServiceTests.cs", 5, &["Totals"]),
                def("App.Orders.Tests.Fakes", "Fakes", "App.Orders.Tests", "class", "tests/Fakes.cs", 3),
                partial,
            ],
            vec![
                uses_type("tests/OrderServiceTests.cs", 11, "App.Orders.OrderService", "src/OrderService.cs"),
                uses_type("tests/OrderServiceTests.cs", 10, "App.Orders.OrderService", "src/OrderService.cs"),
                uses_type("tests/OrderServiceTests.cs", 10, "App.Orders.OrderService", "src/OrderService.cs"),
                uses_type("tests/Fakes.cs", 7, "App.Orders.OrderService", "src/OrderService.cs"),
                heuristic_uses_member("tests/Partial.Extra.cs", 9, "App.Orders.OrderService", "src/OrderService.cs"),
            ],
        )
    }

    fn tests_fixture_root() -> PathBuf {
        let root = temp_repo_root("tests-model");
        write_manifest_fixture(&root, TESTS_MANIFEST_FILES);
        root
    }

    fn resolved_tests(model: TestsResult) -> TestsModel {
        match model {
            TestsResult::Resolved(m) => m,
            other => panic!("expected a resolved tests model, got {other:?}"),
        }
    }

    #[test]
    fn test_defs_by_file_registers_every_declaring_site_and_never_a_file_without_a_test_def() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());
        assert!(index.test_defs_by_file.contains_key("tests/OrderServiceTests.cs"));
        assert!(
            index.test_defs_by_file.contains_key("tests/Partial.Extra.cs"),
            "a partial test class registers its second declaring file too"
        );
        assert!(!index.test_defs_by_file.contains_key("tests/Fakes.cs"), "a file is a test file only because of the attribute");
        assert!(!index.test_defs_by_file.contains_key("src/OrderService.cs"));
    }

    #[test]
    fn build_tests_model_names_the_test_file_its_test_defs_and_every_referencing_line() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());
        let m = resolved_tests(build_tests_model(&index, "OrderService"));

        assert_eq!(m.symbol, "App.Orders.OrderService");
        assert_eq!(m.def_files, vec!["src/OrderService.cs".to_string()]);
        assert_eq!(m.test_file_count, 1, "the non-test neighbour is not a test file");
        assert_eq!(m.ref_count, 3, "lines keep duplicates -- refCount is the reference count, not the distinct-line count");
        assert_eq!(m.rows.len(), 2, "one precise row, then the heuristic one");

        let precise = &m.rows[0];
        assert_eq!(precise.file, "tests/OrderServiceTests.cs");
        assert_eq!(precise.test_defs, vec!["App.Orders.Tests.OrderServiceTests".to_string()]);
        assert_eq!(precise.lines, vec![10, 10, 11], "ascending, duplicates kept");
        assert_eq!(precise.ref_count, 3);
        assert!(!precise.heuristic);
    }

    #[test]
    fn build_tests_model_puts_heuristic_rows_after_every_precise_one_and_counts_them_separately() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());
        let m = resolved_tests(build_tests_model(&index, "OrderService"));

        let guessed = &m.rows[1];
        assert!(guessed.heuristic, "a guess never sits inside the list of facts");
        assert_eq!(guessed.file, "tests/Partial.Extra.cs");
        assert_eq!(guessed.test_defs, vec!["App.Orders.Tests.PartialTests".to_string()]);
        assert_eq!(guessed.lines, vec![9]);
        assert_eq!(m.heuristic_file_count, 1);
        assert_eq!(m.heuristic_ref_count, 1);
        assert_eq!(m.test_file_count, 1, "files= and refs= stay PRECISE-only");
    }

    #[test]
    fn build_tests_model_on_a_symbol_no_test_references_resolves_with_no_rows() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());
        let m = resolved_tests(build_tests_model(&index, "Untested"));
        assert_eq!(m.symbol, "App.Orders.Untested");
        assert!(m.rows.is_empty());
        assert_eq!((m.test_file_count, m.ref_count, m.heuristic_file_count, m.heuristic_ref_count), (0, 0, 0, 0));
    }

    #[test]
    fn build_tests_model_uses_the_same_resolve_symbol_ladder_refs_does() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());
        assert_eq!(build_tests_model(&index, "NoSuchSymbol"), TestsResult::NotFound);
        assert_eq!(
            resolved_tests(build_tests_model(&index, "orderservice")).symbol,
            "App.Orders.OrderService",
            "case-insensitive unique name is the ladder's last rung, same as refs"
        );

        let ambiguous_graph = base_fixture_graph();
        let ambiguous_index = load_graph_index(&ambiguous_graph, &base_fixture_root());
        assert_eq!(
            build_tests_model(&ambiguous_index, "Config"),
            TestsResult::Ambiguous(vec!["App.One.Config".to_string(), "App.Two.Config".to_string()])
        );
    }

    #[test]
    fn build_impact_model_counts_precisely_affected_test_files_only() {
        let g = tests_fixture_graph();
        let index = load_graph_index(&g, &tests_fixture_root());

        let ImpactResult::Resolved(covered) = build_impact_model(&index, "src/OrderService.cs", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) else {
            panic!("file seed resolves");
        };
        assert_eq!(covered.tests_affected, 1, "the guessed test file is not coverage; the non-test neighbour is not a test file");

        let ImpactResult::Resolved(untouched) = build_impact_model(&index, "src/Untested.cs", 2, DEFAULT_CAP, true, DEFAULT_IFACE_MAX_FANIN, DEFAULT_HUB_MAX_INDEGREE) else {
            panic!("file seed resolves");
        };
        assert_eq!(untouched.tests_affected, 0, "zero is the interesting answer -- a blast radius reaching no test file at all");
    }
    // --- a bare member name, resolved by verifying edges at their line ---

    fn name_row(name: &str, kind: &str, file: &str, line: usize, owner: &str) -> graph::GraphName {
        graph::GraphName { name: name.into(), kind: kind.into(), file: file.into(), line, owner: owner.into() }
    }

    /// `Ledger` carries three inbound member edges (two of them one line
    /// apart), and only one line names each of `Post`/`PostEx`. `Approve` is
    /// declared on BOTH `Ledger` and `Journal`, each with one edge whose line
    /// names it -- the ambiguity case, where verification survives on more than
    /// one declaring type.
    fn member_fixture() -> (graph::Graph, PathBuf) {
        let mut g = make_graph(
            vec![
                def("App.Books.Ledger", "Ledger", "App.Books", "class", "Books/Ledger.cs", 3),
                def("App.Books.Journal", "Journal", "App.Books", "class", "Books/Journal.cs", 3),
                def("App.Books.Consumer", "Consumer", "App.Books", "class", "Books/Consumer.cs", 1),
            ],
            vec![
                uses_member("Books/Consumer.cs", 1, "App.Books.Ledger", "Books/Ledger.cs"),
                uses_member("Books/Consumer.cs", 2, "App.Books.Ledger", "Books/Ledger.cs"),
                uses_member("Books/Consumer.cs", 3, "App.Books.Ledger", "Books/Ledger.cs"),
                uses_member("Books/Consumer.cs", 4, "App.Books.Journal", "Books/Journal.cs"),
            ],
        );
        g.names = vec![
            name_row("Ledger", "class", "Books/Ledger.cs", 3, ""),
            name_row("Post", "method", "Books/Ledger.cs", 5, "App.Books.Ledger"),
            name_row("PostEx", "method", "Books/Ledger.cs", 7, "App.Books.Ledger"),
            name_row("Reconcile", "method", "Books/Ledger.cs", 9, "App.Books.Ledger"),
            name_row("Approve", "method", "Books/Ledger.cs", 11, "App.Books.Ledger"),
            name_row("Journal", "class", "Books/Journal.cs", 3, ""),
            name_row("Approve", "method", "Books/Journal.cs", 5, "App.Books.Journal"),
        ];
        let root = temp_repo_root("bare-member");
        write_manifest_fixture(&root, &["Books/Ledger.cs", "Books/Journal.cs", "Books/Consumer.cs"]);
        fs::create_dir_all(root.join("Books")).expect("fixture dir");
        fs::write(
            root.join("Books/Consumer.cs"),
            "Ledger.Post(1);\nLedger.PostEx(2);\nLedger.Approve(3);\nJournal.Approve(4);\n",
        )
        .expect("fixture file");
        (g, root)
    }

    fn member_models(index: &GraphIndex, query: &str, inbound_cap: usize) -> Vec<RefsModel> {
        match build_refs_model(index, query, false, DEFAULT_CAP, inbound_cap, OUTBOUND_CAP, false) {
            RefsResult::Members(models) => models,
            other => panic!("expected Members, got {other:?}"),
        }
    }

    #[test]
    fn line_has_token_matches_whole_tokens_only() {
        assert!(line_has_token("Ledger.Post(1);", "Post"));
        assert!(line_has_token("Post", "Post"));
        assert!(!line_has_token("Ledger.PostEx(2);", "Post"));
        assert!(!line_has_token("RePost(2);", "Post"));
        assert!(!line_has_token("Post_x();", "Post"));
        assert!(!line_has_token("x1Post();", "Post"));
        // Every code point outside ASCII is a boundary: this reads the
        // neighbouring UTF-8 byte, which is never an ASCII word character.
        assert!(line_has_token("\u{2026}Post\u{2026}", "Post"));
        assert!(!line_has_token("", "Post"));
        assert!(!line_has_token("Post", ""));
    }

    #[test]
    fn build_refs_model_bare_member_keeps_only_the_edges_whose_line_names_it() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);
        let models = member_models(&index, "PostEx", INBOUND_CAP);

        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.id, "App.Books.Ledger.PostEx");
        assert_eq!(m.kind, "member");
        assert_eq!(m.sites, vec![DefSite { file: "Books/Ledger.cs".into(), line: 7 }]);
        assert_eq!(m.inbound.uses_member.total, 1, "the type has three inbound member edges; one names PostEx");
        assert_eq!(
            m.inbound.uses_member.rows,
            vec![InboundRow { file: "Books/Consumer.cs".into(), line: 2, heuristic: false, source: "Ledger.PostEx(2);".into() }]
        );
        assert_eq!(m.inbound.inherits.total, 0);
        assert!(m.outbound.is_none(), "a member answer never carries the outbound tables");
    }

    #[test]
    fn build_refs_model_bare_member_refuses_a_longer_identifier_that_starts_with_the_query() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);
        let models = member_models(&index, "Post", INBOUND_CAP);
        let ledger = models.iter().find(|m| m.id == "App.Books.Ledger.Post").expect("Ledger declares Post");

        assert_eq!(
            ledger.inbound.uses_member.rows.iter().map(|r| r.line).collect::<Vec<_>>(),
            vec![1],
            "line 2 is `Ledger.PostEx(2);` -- a substring hit, not a token hit"
        );
    }

    // `Approve` is declared on Ledger AND Journal, and both survive edge-line
    // verification (unlike the fixture's own `Post`, now single-owner): the
    // answer is the ambiguous candidate list, in name-index order, never a
    // `Members` block per type -- the same house rule of never guessing between
    // candidates that an ambiguous TYPE name already answers with.
    #[test]
    fn build_refs_model_bare_member_verified_on_several_types_answers_ambiguous_not_members() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);
        let model = build_refs_model(&index, "Approve", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false);

        assert_eq!(
            model,
            RefsResult::Ambiguous(vec!["App.Books.Ledger".to_string(), "App.Books.Journal".to_string()])
        );
    }

    #[test]
    fn build_refs_model_bare_member_ambiguous_across_types_answers_the_same_regardless_of_inbound_cap() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);
        let model = build_refs_model(&index, "Approve", false, DEFAULT_CAP, 1, OUTBOUND_CAP, false);

        assert_eq!(
            model,
            RefsResult::Ambiguous(vec!["App.Books.Ledger".to_string(), "App.Books.Journal".to_string()])
        );
    }

    #[test]
    fn build_refs_model_bare_member_with_no_verified_edge_stays_not_found() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);

        assert_eq!(build_refs_model(&index, "Reconcile", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false), RefsResult::NotFound);
        assert_eq!(build_refs_model(&index, "NoSuchMemberAnywhere", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false), RefsResult::NotFound);
    }

    #[test]
    fn build_refs_model_prefers_a_type_over_a_member_of_the_same_name() {
        let (g, root) = member_fixture();
        let index = load_graph_index(&g, &root);

        let RefsResult::Resolved(model) = build_refs_model(&index, "Ledger", false, DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP, false) else {
            panic!("Ledger is a type and must resolve as one");
        };
        assert_eq!(model.id, "App.Books.Ledger");
    }

    #[test]
    fn read_excludes_references_inside_the_target_declaration_span() {
        let root = temp_repo_root("read-self-inbound");
        fs::create_dir_all(root.join("Core")).unwrap();
        fs::create_dir_all(root.join("Consumers")).unwrap();
        fs::write(root.join("Core/Widget.cs"), "namespace App;\npublic class Widget\n{\n    Widget Again() => new Widget();\n}\n").unwrap();
        fs::write(root.join("Consumers/Reader.cs"), "namespace App;\npublic class Reader { Widget Value; }\n").unwrap();
        write_manifest_fixture(&root, &["Core/Widget.cs", "Consumers/Reader.cs"]);
        let mut target = def("App.Widget", "Widget", "App", "class", "Core/Widget.cs", 2);
        target.end_line = 5;
        let graph = make_graph(vec![target], vec![
            uses_type("Core/Widget.cs", 4, "App.Widget", "Core/Widget.cs"),
            uses_type("Consumers/Reader.cs", 2, "App.Widget", "Core/Widget.cs"),
        ]);
        let index = load_graph_index(&graph, &root);
        let ReadResult::Resolved(model) = build_read_model(&index, "Widget") else { panic!("Widget must resolve") };
        assert_eq!(model.refs.inbound.uses_type.total, 1);
        assert_eq!(model.refs.inbound.uses_type.rows[0].file, "Consumers/Reader.cs");
    }

    // --- the per-file first-declaration line find's manifest-pool block reads ---

    fn graph_name(name: &str, kind: &str, file: &str, line: usize) -> graph::GraphName {
        graph::GraphName { name: name.to_string(), kind: kind.to_string(), file: file.to_string(), line, owner: String::new() }
    }

    #[test]
    fn first_decl_line_by_file_keeps_the_minimum_line_per_file_across_the_whole_name_index_not_just_the_last_one_seen() {
        let mut graph = make_graph(vec![], vec![]);
        graph.names = vec![
            graph_name("Widget", "class", "a.cs", 5),
            graph_name("Render", "method", "a.cs", 12),
            graph_name("Widget", "class", "a.cs", 1),
            graph_name("Other", "class", "b.cs", 8),
        ];
        let mut rows: Vec<(String, usize)> = first_decl_line_by_file(&graph).into_iter().collect();
        rows.sort();
        assert_eq!(rows, vec![("a.cs".to_string(), 1), ("b.cs".to_string(), 8)]);
    }

    #[test]
    fn first_decl_line_by_file_on_a_graph_with_no_names_index_is_empty() {
        assert!(first_decl_line_by_file(&make_graph(vec![], vec![])).is_empty());
    }
}
