use std::collections::{HashMap, HashSet};

use crate::graph;

use super::index::{def_files, implemented_interfaces, GraphIndex};
use super::infra::is_infra_file;
use super::member::{self, MemberCandidate, MemberSeedResolution};
use super::rank::{personalized_page_rank, DEFAULT_DAMPING, DEFAULT_ITERATIONS};
use super::refs_tables::{cap_rows, edge_loc, row_tier};
use super::seq::{SeqMap, SeqSet};
use super::symbol::{resolve_symbol, Resolution};

/// Default number of hops for the impact walk.
pub const DEFAULT_HOPS: u32 = 2;

/// Default fan-in brake: an interface injected into more than this many
/// distinct constructors is treated as infrastructure for widening purposes,
/// whatever it is called. Chosen from the constructor-injection in-degree
/// histogram of a large corpus, where 151 of 155 injected contracts (97.4%)
/// sit at or below 8 and nothing sits between 8 and the four estate-wide ones
/// (9, 11, 12, 21). `0` disables the brake (`--iface-max-fanin 0`).
pub const DEFAULT_IFACE_MAX_FANIN: usize = 8;
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
    /// The direct value.
    pub direct: usize,
    /// The direct amb value.
    pub direct_amb: usize,
    /// The ctor di value.
    pub ctor_di: usize,
    /// The heuristic value.
    pub heuristic: usize,
    /// The iface value.
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
    // How many of `heuristic_count` came from the EXTENSION tier. Counted
    // rather than flagged so the walk keeps one shape for both tiers, and one
    // is all the row needs to call itself an extension (see `row_tier`).
    ext_count: u32,
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
    /// The hop value.
    pub hop: u32,
    /// The via count value.
    pub via_count: u32,
    /// The ambiguous count value.
    pub ambiguous_count: u32,
    /// The heuristic count value.
    pub heuristic_count: u32,
    /// How many of `heuristic_count` came from the extension tier.
    pub ext_count: u32,
    /// The symbols value.
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
    /// The iface value.
    pub iface: String,
    /// The fanin value.
    pub fanin: usize,
}

/// A hub file the walk refused to widen THROUGH, and how many distinct other
/// files reference it. Kept in its own vec (separate from [`BrakedIface`]) so
/// that type, its tests and its JSON bytes are untouched when the file brake
/// never fires. Sorted widest-first then by path, the same total order, for the
/// same reason.
#[derive(Debug, Clone, PartialEq)]
pub struct BrakedFile {
    /// The file value.
    pub file: String,
    /// The indegree value.
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
    fwd_adj
        .entry(from.to_string())
        .or_default()
        .insert(to.to_string());
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
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "one breadth-first walk with both widening brakes inline; the brakes only work if they see the same frontier the walk itself sees"
)]
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
                        let graph::Edge::CtorDi {
                            from_file,
                            from_line,
                            iface: iface_name,
                            ..
                        } = &index.graph.edges[ei]
                        else {
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
                    let Some(iface_inb) = index.inbound.get(&iface_id) else {
                        continue;
                    };
                    let iface_name = index
                        .def(&iface_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| iface_id.clone());
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
                        if index.graph.edges[ei].tier() == Some(graph::HeuristicTier::Ext) {
                            h.ext_count += 1;
                        }
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
                        ext_count: h.ext_count,
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
    let mut braked: Vec<BrakedIface> = braked
        .into_iter()
        .map(|(iface, fanin)| BrakedIface { iface, fanin })
        .collect();
    braked.sort_by(|a, b| b.fanin.cmp(&a.fanin).then_with(|| a.iface.cmp(&b.iface)));
    let mut braked_files: Vec<BrakedFile> = braked_files
        .into_iter()
        .map(|(file, indegree)| BrakedFile { file, indegree })
        .collect();
    braked_files.sort_by(|a, b| {
        b.indegree
            .cmp(&a.indegree)
            .then_with(|| a.file.cmp(&b.file))
    });

    ImpactWalkResult {
        visited,
        fwd_adj,
        seed_files,
        braked,
        braked_files,
    }
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
// ============================================================================
// resolve_impact_seed + build_impact_model.
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents `SeedKind`.
pub enum SeedKind {
    /// Represents `File`.
    File,
    /// Represents `Symbol`.
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
/// Represents `SeedResolution`.
pub enum SeedResolution {
    /// The value value.
    Resolved {
        /// The resolved seed kind.
        kind: SeedKind,
        /// The resolved definition identifiers.
        ids: Vec<String>,
    },
    /// The value value.
    Ambiguous {
        /// The seed kind.
        kind: SeedKind,
        /// The candidate definition identifiers.
        ids: Vec<String>,
    },
    /// The value value.
    NotFound {
        /// The inferred seed kind.
        kind: SeedKind,
    },
    /// A member seed whose name more than one type declares.
    MemberAmbiguous(Vec<MemberCandidate>),
}

// The member ladder, shared by both branches below: a member seed answers as
// its unique declaring type; `not_found_kind` is the caller's own miss kind.
fn member_fallback(index: &GraphIndex, arg: &str, not_found_kind: SeedKind) -> SeedResolution {
    match member::resolve_member_seed(index, arg) {
        MemberSeedResolution::Resolved(id) => SeedResolution::Resolved {
            kind: SeedKind::Symbol,
            ids: vec![id],
        },
        MemberSeedResolution::Ambiguous(candidates) => SeedResolution::MemberAmbiguous(candidates),
        MemberSeedResolution::NotFound => SeedResolution::NotFound {
            kind: not_found_kind,
        },
    }
}

/// Resolve an impact-walk seed to a file's defs or a single symbol. The type
/// ladder ([`resolve_symbol`]) runs first; a member seed, tried only once it
/// has missed, answers AS its unique declaring type -- `impact` has no
/// member-shaped answer of its own. A dotted member seed looks like a file
/// path under [`looks_like_file_path`], so it is tried on a file-path MISS
/// instead: that branch never resolved a type before, so nothing changes.
pub fn resolve_impact_seed(index: &GraphIndex, arg: &str) -> SeedResolution {
    if looks_like_file_path(arg) {
        return match index.by_file.get(arg) {
            Some(ids) if !ids.is_empty() => SeedResolution::Resolved {
                kind: SeedKind::File,
                ids: ids
                    .iter()
                    .map(|&i| index.graph.defs[i].id.clone())
                    .collect(),
            },
            _ => member_fallback(index, arg, SeedKind::File),
        };
    }
    match resolve_symbol(index, arg) {
        Resolution::Resolved(id) => SeedResolution::Resolved {
            kind: SeedKind::Symbol,
            ids: vec![id],
        },
        Resolution::Ambiguous(ids) => SeedResolution::Ambiguous {
            kind: SeedKind::Symbol,
            ids,
        },
        Resolution::NotFound => member_fallback(index, arg, SeedKind::Symbol),
    }
}

/// One row of an impact model: a file in the blast radius and its metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactRow {
    /// The file value.
    pub file: String,
    /// The hop value.
    pub hop: u32,
    /// The via count value.
    pub via_count: u32,
    /// The ambiguous count value.
    pub ambiguous_count: u32,
    /// The top symbols value.
    pub top_symbols: Vec<String>,
    /// The top symbols more value.
    pub top_symbols_more: usize,
    /// The score value.
    pub score: f64,
    /// "heuristic" is a property of how the file was REACHED, not of the edges
    /// that reached it: one precise or ambiguous hit makes the file an ordinary
    /// affected file no matter how many guesses also point at it. Only a file
    /// reached EXCLUSIVELY by guesses is flagged. On a precise row neither this
    /// nor `heuristic` is emitted in `--json`.
    pub heuristic_count: u32,
    /// The heuristic value.
    pub heuristic: bool,
    /// Which heuristic tier REACHED this file. Folded the same way `heuristic`
    /// itself is -- over every guess that landed here -- so a file one
    /// extension edge and ten name guesses all point at is an extension row.
    /// Absent from `--json` on a precise row, like `heuristic`.
    pub tier: Option<graph::HeuristicTier>,
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
    /// The kind value.
    pub kind: SeedKind,
    /// The seed files value.
    pub seed_files: Vec<String>,
    /// The hops value.
    pub hops: u32,
    /// The total affected value.
    pub total_affected: usize,
    /// The rows value.
    pub rows: Vec<ImpactRow>,
    /// The dropped value.
    pub dropped: usize,
    /// The manifest gap value.
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
    /// Represents `Resolved`.
    Resolved(ImpactModel),
    /// The value value.
    Ambiguous {
        /// The seed kind.
        kind: SeedKind,
        /// The candidate definition identifiers.
        ids: Vec<String>,
    },
    /// The value value.
    NotFound {
        /// The inferred seed kind.
        kind: SeedKind,
    },
    /// The seed named a member declared by more than one type.
    MemberAmbiguous(Vec<MemberCandidate>),
}

// Assembles a row's per-kind representative lines. The resolved-over-ambiguous
// tie-break decides the `direct` kind: a resolved site outranks an ambiguous
// one, and the ambiguous line is used only when the resolved half never fired.
fn from_lines_of(lines: &KindLines) -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    let direct = if lines.direct != 0 {
        lines.direct
    } else {
        lines.direct_amb
    };
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
        SeedResolution::MemberAmbiguous(candidates) => {
            return ImpactResult::MemberAmbiguous(candidates)
        }
    };

    let walk = impact_walk(
        index,
        &seed_ids,
        hops,
        iface,
        iface_max_fanin,
        hub_max_indegree,
    );

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

    let rank = personalized_page_rank(
        &nodes,
        &walk.fwd_adj,
        &seeds,
        DEFAULT_DAMPING,
        DEFAULT_ITERATIONS,
    );

    let mut rows: Vec<ImpactRow> = walk
        .visited
        .iter()
        .map(|(file, h)| {
            let names: Vec<String> = h
                .symbols
                .iter()
                .map(|id| {
                    index
                        .def(id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
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
                tier: row_tier(heuristic, h.ext_count > 0),
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
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.hop.cmp(&b.hop))
            .then_with(|| a.file.cmp(&b.file))
    });

    let heuristic_affected = rows.iter().filter(|r| r.heuristic).count();
    let total_affected = rows.len() - heuristic_affected;
    let tests_affected = rows
        .iter()
        .filter(|r| !r.heuristic && index.is_test_file(&r.file))
        .count();
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
