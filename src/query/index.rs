use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::graph;
use crate::manifest;

use super::seq::{push_ordered_unique, SeqSet};

// ============================================================================
// GraphIndex -- the built query index.
// ============================================================================

#[derive(Debug, Clone, Default)]
/// Represents `InboundEntry`.
pub struct InboundEntry {
    /// The inherits value.
    pub inherits: Vec<usize>,
    /// The uses type value.
    pub uses_type: Vec<usize>,
    /// The uses member value.
    pub uses_member: Vec<usize>,
}

/// The value shape of both `heuristic_inbound` and `heuristic_outbound_by_file`
/// (`inherits`/`uses-type`/`uses-member` -- note no `imports`: an imports edge
/// names a namespace string, never a def, so no tier can guess one).
/// Structurally identical to [`InboundEntry`], and deliberately the same type.
pub type HeuristicEntry = InboundEntry;

#[derive(Debug, Clone, Default)]
/// Represents `OutboundEntry`.
pub struct OutboundEntry {
    /// The inherits value.
    pub inherits: Vec<usize>,
    /// The uses type value.
    pub uses_type: Vec<usize>,
    /// The uses member value.
    pub uses_member: Vec<usize>,
    /// The imports value.
    pub imports: Vec<usize>,
}

/// The built query index over a `Graph`. `by_id`/`by_file`/`by_simple_name`/
/// `by_lower_name` store `graph.defs` indices rather than cloned `Def`s, so a
/// def is never copied into every bucket it is reachable from.
pub struct GraphIndex<'g> {
    /// The graph value.
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
    /// The repo's `.csproj` project model, rebuilt from `graph.units` --
    /// `None` when the graph carries none, which is every repo that declares
    /// no project and every graph written before `units` existed. Owned
    /// rather than borrowed: `ProjectModel` holds the derived directory map
    /// and reference closure, neither of which is persisted.
    pub project: Option<crate::project::ProjectModel>,
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

    /// Whether `file` counts as a TEST file for `tests`/`impact`: either an
    /// attribute-vouched def is declared in it (`test_defs_by_file`), or the
    /// project model places it inside a unit marked `test`. The second vouch
    /// fires even for a file that declares no attributed test method at all --
    /// a harness or fixture file living in a test project is still part of
    /// what a symbol's tests touch. Fails open exactly like the rest of the
    /// project model: with no model at all, or a file no discovered project
    /// owns, this half answers `false` and the attribute vouch alone decides.
    pub fn is_test_file(&self, file: &str) -> bool {
        if self.test_defs_by_file.contains_key(file) {
            return true;
        }
        let Some(project) = &self.project else {
            return false;
        };
        project
            .unit_of_file(file)
            .is_some_and(|u| project.units[u].test)
    }
}

fn note_file(flagged: &mut HashSet<String>, manifest_paths: Option<&HashSet<String>>, file: &str) {
    if let Some(paths) = manifest_paths {
        if !file.is_empty() && !paths.contains(file) {
            flagged.insert(file.to_string());
        }
    }
}

/// What the caller wants left OUT of the index it is asking for.
///
/// The one knob is `include_guesses`, and it is spent HERE rather than at each
/// render site on purpose: a filter applied while the adjacency is built cannot
/// be forgotten by the next consumer of that adjacency, which is the same
/// reasoning that keeps heuristic edges in their own buckets in the first place
/// (see [`GraphIndex::heuristic_inbound`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexOptions {
    /// Whether a scored-tier guess joins the heuristic adjacency at all.
    /// `false` (`--no-guess`) admits ONLY [`graph::HeuristicTier::Ext`] edges
    /// there: extension-method lookup is one of C#'s own rules, run over a
    /// recorded bucket and unverifiable only against an out-of-graph receiver,
    /// whereas the scored tier picked by name. An edge carrying no tier at all
    /// -- a graph written before schema 2 -- is treated as the weaker of the
    /// two and drops with the guesses.
    pub include_guesses: bool,
}

impl Default for IndexOptions {
    /// Guesses are IN by default: every caller that does not ask asked for the
    /// whole index, and a default that quietly narrowed the answer would change
    /// what `refs` means without anyone typing a flag.
    fn default() -> Self {
        IndexOptions {
            include_guesses: true,
        }
    }
}

/// Build the query index, phase two of the two-phase build (see module
/// header). `graph` is what the caller got back from `graph::read_graph(root)`;
/// `root` is used here only for the manifest join.
pub fn load_graph_index<'g>(graph: &'g graph::Graph, root: &Path) -> GraphIndex<'g> {
    load_graph_index_with(graph, root, IndexOptions::default())
}

/// [`load_graph_index`] with the caller's own [`IndexOptions`]. Everything is
/// built the same way either way; `opts` only decides which heuristic edges earn
/// a place in the heuristic adjacency.
pub fn load_graph_index_with<'g>(
    graph: &'g graph::Graph,
    root: &Path,
    opts: IndexOptions,
) -> GraphIndex<'g> {
    let manifest_value = match manifest::read_manifest(root) {
        Ok(v) => v,
        Err(_) => None, // corrupt manifest.json: fail open, see module header
    };
    let manifest_present = manifest_value.is_some();
    let manifest_paths: Option<HashSet<String>> = manifest_value.as_ref().and_then(|m| {
        m.get("entries")
            .and_then(|e| e.as_object())
            .map(|entries| entries.iter().map(|(k, _)| k.clone()).collect())
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
                test_defs_by_file
                    .entry(file.to_string())
                    .or_default()
                    .push(i);
            }
        }
        by_simple_name.entry(d.name.clone()).or_default().push(i);
        by_lower_name
            .entry(d.name.to_lowercase())
            .or_default()
            .push(i);
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

    // Whether a heuristic edge earns a place in the heuristic adjacency at all.
    // Under `--no-guess` only the extension tier does; every other guess is
    // dropped from the index, so no consumer downstream has to remember to ask.
    // It is asked ONLY of an edge already known to be heuristic -- a precise
    // edge carries no tier and this would wrongly refuse it.
    let admits =
        |e: &graph::Edge| opts.include_guesses || e.tier() == Some(graph::HeuristicTier::Ext);

    for (i, e) in graph.edges.iter().enumerate() {
        match e {
            graph::Edge::Imports { from_file, .. } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                outbound_by_file
                    .entry(from_file.clone())
                    .or_default()
                    .imports
                    .push(i);
            }
            graph::Edge::Ambiguous {
                from_file,
                candidates,
                ..
            } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                ambiguous_by_file
                    .entry(from_file.clone())
                    .or_default()
                    .push(i);
                for c in candidates {
                    ambiguous_by_candidate
                        .entry(c.id.clone())
                        .or_default()
                        .push(i);
                }
            }
            graph::Edge::Inherits {
                from_file,
                to,
                to_file,
                heuristic,
                ..
            } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file
                        .entry(to_file.clone())
                        .or_default()
                        .insert(from_file.clone());
                }
                if *heuristic {
                    // A guess `--no-guess` refuses leaves the index with no
                    // bucket at all: it was already counted for the hub brake
                    // above, and it is not a precise edge, so there is nowhere
                    // else for it to go.
                    if admits(e) {
                        heuristic_outbound_by_file
                            .entry(from_file.clone())
                            .or_default()
                            .inherits
                            .push(i);
                        heuristic_inbound
                            .entry(to.clone())
                            .or_default()
                            .inherits
                            .push(i);
                    }
                } else {
                    outbound_by_file
                        .entry(from_file.clone())
                        .or_default()
                        .inherits
                        .push(i);
                    inbound.entry(to.clone()).or_default().inherits.push(i);
                }
            }
            graph::Edge::UsesType {
                from_file,
                to,
                to_file,
                heuristic,
                ..
            } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file
                        .entry(to_file.clone())
                        .or_default()
                        .insert(from_file.clone());
                }
                if *heuristic {
                    // Same rule as the `inherits` arm above.
                    if admits(e) {
                        heuristic_outbound_by_file
                            .entry(from_file.clone())
                            .or_default()
                            .uses_type
                            .push(i);
                        heuristic_inbound
                            .entry(to.clone())
                            .or_default()
                            .uses_type
                            .push(i);
                    }
                } else {
                    outbound_by_file
                        .entry(from_file.clone())
                        .or_default()
                        .uses_type
                        .push(i);
                    inbound.entry(to.clone()).or_default().uses_type.push(i);
                }
            }
            graph::Edge::UsesMember {
                from_file,
                to,
                to_file,
                heuristic,
                ..
            } => {
                note_file(&mut flagged_files, manifest_paths_ref, from_file);
                note_file(&mut flagged_files, manifest_paths_ref, to_file);
                // Counted BEFORE the heuristic split below: the hub brake spans
                // both kinds on purpose, because a hub file is reached through
                // whichever of them the extractor happened to resolve.
                if !to_file.is_empty() && !from_file.is_empty() && to_file != from_file {
                    hub_referrers_by_file
                        .entry(to_file.clone())
                        .or_default()
                        .insert(from_file.clone());
                }
                if *heuristic {
                    // The one kind that actually carries a tier, and so the one
                    // kind `--no-guess` can keep anything of: an extension-tier
                    // edge survives here, a scored one does not.
                    if admits(e) {
                        heuristic_outbound_by_file
                            .entry(from_file.clone())
                            .or_default()
                            .uses_member
                            .push(i);
                        heuristic_inbound
                            .entry(to.clone())
                            .or_default()
                            .uses_member
                            .push(i);
                    }
                } else {
                    outbound_by_file
                        .entry(from_file.clone())
                        .or_default()
                        .uses_member
                        .push(i);
                    inbound.entry(to.clone()).or_default().uses_member.push(i);
                }
            }
            // 'ctor-di' is deliberately NOT one of the kinds `refs`/`impact`
            // render -- that kind list is fixed to
            // inherits/uses-type/uses-member -- so it earns no `inbound`/
            // `outbound_by_file` entry. It DOES earn its own reverse index,
            // keyed by the resolved implementor, when it carries one.
            graph::Edge::CtorDi {
                from_file,
                from_line,
                iface,
                to,
                ..
            } => {
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
        project: if graph.units.is_empty() {
            None
        } else {
            Some(crate::project::ProjectModel::from_units(
                crate::project::units_from_graph(&graph.units),
            ))
        },
        ctor_di_fanin: ctor_di_sites_by_iface
            .into_iter()
            .map(|(name, sites)| (name, sites.len()))
            .collect(),
        hub_indegree: hub_referrers_by_file
            .into_iter()
            .map(|(file, refs)| (file, refs.len()))
            .collect(),
    }
}

// The interface def id(s) a class def's OWN file(s) declare an `inherits` edge
// to, restricted to a def of kind `"interface"` -- a plain base class is not
// part of the interface hop. Reuses `outbound_by_file`'s file-level union
// rather than attributing an edge to one specific def in a multi-type file (an
// accepted imprecision). Insertion order is `def_files(def_id)` order, then
// that file's own inherits-edge array order -- the first-seen order of the
// `via` labels a widened hit carries.
pub(super) fn implemented_interfaces(index: &GraphIndex, def_id: &str) -> Vec<String> {
    let mut seen: SeqSet<String> = SeqSet::new();
    for file in def_files(index, def_id) {
        let Some(o) = index.outbound_by_file.get(&file) else {
            continue;
        };
        for &ei in &o.inherits {
            let graph::Edge::Inherits { to, .. } = &index.graph.edges[ei] else {
                continue;
            };
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
// def_files / def_sites.
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
/// Represents `DefSite`.
pub struct DefSite {
    /// The file value.
    pub file: String,
    /// The line value.
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
        out.push(DefSite {
            file: d.file.clone(),
            line: d.line,
        });
        for also in &d.also_in {
            out.push(DefSite {
                file: also.file.clone(),
                line: also.line,
            });
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
    /// The inbound inherits value.
    pub inbound_inherits: Vec<usize>,
    /// The inbound uses type value.
    pub inbound_uses_type: Vec<usize>,
    /// The inbound uses member value.
    pub inbound_uses_member: Vec<usize>,
    /// The outbound inherits value.
    pub outbound_inherits: Vec<usize>,
    /// The outbound uses type value.
    pub outbound_uses_type: Vec<usize>,
    /// The outbound uses member value.
    pub outbound_uses_member: Vec<usize>,
    /// The outbound imports value.
    pub outbound_imports: Vec<usize>,
    /// The same two tables again over the HEURISTIC adjacency, built by the
    /// identical rules (enum members union into the enum's inbound, partial
    /// classes union outbound over every declaring file) so a heuristic row is
    /// never present or absent for a reason a precise row would not have been.
    pub heuristic_inbound_inherits: Vec<usize>,
    /// The heuristic inbound uses type value.
    pub heuristic_inbound_uses_type: Vec<usize>,
    /// The heuristic inbound uses member value.
    pub heuristic_inbound_uses_member: Vec<usize>,
    /// The heuristic outbound inherits value.
    pub heuristic_outbound_inherits: Vec<usize>,
    /// The heuristic outbound uses type value.
    pub heuristic_outbound_uses_type: Vec<usize>,
    /// The heuristic outbound uses member value.
    pub heuristic_outbound_uses_member: Vec<usize>,
    /// The ambiguous inbound value.
    pub ambiguous_inbound: Vec<usize>,
    /// The ambiguous outbound value.
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

    let ambiguous_inbound = index
        .ambiguous_by_candidate
        .get(def_id)
        .cloned()
        .unwrap_or_default();
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
