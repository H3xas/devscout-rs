// The resolution ladder, including ambiguous marking: `build_def_index`,
// `resolve_ref`, `collect_global_usings`, `capped_candidates`, `resolve_graph`.
// Pure: no file I/O, no tree-sitter -- the only I/O this module performs is the
// single `git rev-parse HEAD` shell-out inside `resolve_graph`, delegated to
// `manifest::git_head`. Artifact load/save and the fragments cache live in
// `graph.rs`.
//
// Ladder rules (see `resolve_ref`'s doc comment for the exact order):
//   0. Type ALIASES (`using Foo = Some.Ns.Bar;` and `global` counterpart)
//      short-circuit before the ladder for bare (non-dotted) references --
//      never ambiguous, never falls through.
//   1. Exact qualified name, tried at every ENCLOSING namespace prefix,
//      innermost first, only for dotted references.
//   2. File's usings (local ∪ every `global using`) + simple name, each
//      using name itself tried at every enclosing-namespace prefix.
//   3. The reference site's namespace and every ancestor of it, innermost
//      first (the ancestor-namespace rule -- a walk, like step 1).
//   4. Globally unique simple name.
//   A step that finds exactly one candidate resolves; two or more STOPS
//   there as ambiguous, never falling through looking for a tiebreaker.
//
// The enum-member asymmetry (the single most load-bearing invariant here):
// enum members ARE keyed in `qualified_name_to_def` (reachable by exact id,
// e.g. for uses-member resolution) but are EXCLUDED from `simple_name_to_defs`
// (the pool step 2/4's bare-name lookups draw from) -- see `build_def_index`.
// Losing this exclusion doesn't change any TYPE resolution that was already
// unambiguous via using/namespace/alias, but it does turn every type whose
// simple name collides with some unrelated enum's member name into a false
// ambiguous (or worse, a step-4 resolution picking the wrong one) purely
// because that enum happens to exist somewhere in the same build. The test
// `enum_member_does_not_collide_with_a_same_named_class_via_global_uniqueness`
// below is a regression trap for exactly this: it fails loudly (ambiguous where
// it should resolve cleanly) if the exclusion is ever dropped.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::graph::{
    AlsoIn, Candidate, Def, Edge, EdgesByKind, FragExtensionMethod, FragFact, FragRef, FragUsing,
    Fragment, Graph, GraphName, OrderedMap, Percent1, Stats,
};
use crate::manifest;

const AMBIGUOUS_CAP: usize = 5;

// The two caps the scored heuristic tier lives inside. The uniqueness cap is a
// REFUSAL threshold: a member name carried by more than this many defs
// graph-wide is too common to guess from, so the tier emits nothing at all
// rather than a wide fan of maybes. The emit cap bounds how many of the
// surviving candidates a single ref may name.
const SCORED_UNIQUENESS_CAP: usize = 3;
const SCORED_EMIT_CAP: usize = 3;

// ---------------------------------------------------------------------------
// Def index.
// ---------------------------------------------------------------------------

/// `Vec<Def>` in first-insertion order (partial-class duplicates land in
/// `also_in`, never as a second Vec entry) plus two lookup indexes over it:
/// `qualified_name_to_def` (id -> index, every def incl. enum members) and
/// `simple_name_to_defs` (bare name -> indexes, EVERY def EXCEPT enum members
/// -- the asymmetry described in the module header).
pub struct DefIndex {
    /// The defs value.
    pub defs: Vec<Def>,
    /// Parallel to `defs` (same indexes), NOT fields on `Def`: a def's
    /// property/field lists are RESOLUTION inputs only. No graph.json reader
    /// consumes them (graph-query walks EDGES), so serializing them would
    /// multiply the def table's size on disk and in every `scout refs`/`scout
    /// impact` read for nothing. Keeping them off `Def` keeps the on-disk def
    /// rows to exactly the serialized fields.
    pub member_lists: Vec<MemberLists>,
    /// The qualified name to def value.
    pub qualified_name_to_def: HashMap<String, usize>,
    /// The qualified name and arity to def value.
    pub qualified_name_and_arity_to_def: HashMap<(String, usize), usize>,
    /// The simple name to defs value.
    pub simple_name_to_defs: HashMap<String, Vec<usize>>,
    /// The extension tier's own lookup: "<method_name> <this_type>" (one ASCII
    /// space) -> every candidate declaring that PAIR, as (def index, entry).
    ///
    /// The key was the (name, this_type, arity) TRIPLE until the range
    /// amendment. Arity moved out of the key and into a per-entry range filter
    /// because an exact arity is not the language's rule:
    /// `Send(this T t, X a, Y b = null)` accepts two OR three arguments, and
    /// keying on 3 alone made a call with two arguments miss it -- which in turn
    /// made a DIFFERENT class's exact-arity-2 `Send` look like the only
    /// candidate and win an edge it had no right to -- the false-uniqueness
    /// shape.
    ///
    /// Build order is def order -- files in fragment-map order, defs in source
    /// order, extension entries in declaration order -- so a bucket holding two
    /// candidates holds them in the same order on every run. The tier refuses on
    /// a 2-candidate bucket anyway; the order is pinned because it is what a
    /// later scored tier would read.
    pub extension_index: HashMap<String, Vec<ExtCandidate>>,
    /// Member name -> every def that vouches for a member of that name, the
    /// pool the scored tier's UNIQUENESS FALLBACK draws from when a ref's
    /// qualifier resolves to nothing at all (`x.Tally()` where the extractor
    /// could vouch for no type for `x`). Built AFTER the main def loop, not
    /// during it, so a partial class contributes its accumulated member set once
    /// instead of appearing in a bucket per declaring file.
    ///
    /// Insertion order is def order (fragment order, then source order), and
    /// within one def the names are collected methods -> properties -> fields ->
    /// extension-method names, first insertion winning. This order decides which
    /// candidates a `<= SCORED_UNIQUENESS_CAP` pool holds.
    pub member_name_to_defs: HashMap<String, Vec<usize>>,
}

/// One bucket slot: which def declares the entry, and the entry itself (the
/// range + generic-argument facts the tier filters on).
pub struct ExtCandidate {
    /// The def idx value.
    pub def_idx: usize,
    /// The entry value.
    pub entry: FragExtensionMethod,
}

/// The non-method halves of a def's member surface. `methods` stays on `Def`
/// (it IS serialized).
#[derive(Default)]
pub struct MemberLists {
    /// The properties value.
    pub properties: Vec<String>,
    /// The fields value.
    pub fields: Vec<String>,
    /// Kept here for the same reason as `properties`/`fields`: a resolution
    /// input, never a serialized def field. Holds the (name, this_type,
    /// arity_min, arity_max) QUADRUPLES this def has already contributed, so a
    /// partial class re-declaring one of its own entries in a second file can
    /// never push the def into its own extension bucket twice and make itself
    /// look like two candidates.
    pub extension_methods: Vec<(String, String, usize, i64)>,
    /// DIRECT base-type names, unioned across a partial class's declarations,
    /// resolved LAZILY by the veto walk.
    pub bases: Vec<String>,
    /// The declaring type's own type-parameter names (empty for a non-generic
    /// declaration). A resolution input, like `bases`: the ctor-DI resolver's
    /// "is this def itself an open-generic implementation" signal. First
    /// declaration wins, like `namespace`/`kind` -- not unioned across a partial
    /// class the way `bases` is.
    pub type_params: Vec<String>,
    /// Per base name that carried a type-argument list, that list's generic-arg
    /// descriptors relative to `type_params`. First declaration wins, same as
    /// `type_params`.
    pub base_generic_args: Vec<(String, Vec<String>)>,
    /// Method name -> declared return type NAME, the fact that turns a
    /// `var x = Q.M(...)` local into an ordinary receiver. A partial class's
    /// later parts contribute the names the first part did not answer; a name it
    /// already answered keeps its answer.
    pub method_returns: OrderedMap<String>,
    /// Property name -> declared type fact, the second half of a property hop.
    /// Merged across a partial class exactly like `method_returns`.
    pub property_types: OrderedMap<FragFact>,
}

fn build_def_index(fragments_by_file: &[(String, Fragment)]) -> DefIndex {
    let mut defs: Vec<Def> = Vec::new();
    let mut member_lists: Vec<MemberLists> = Vec::new();
    let mut qualified_name_to_def: HashMap<String, usize> = HashMap::new();
    let mut qualified_name_and_arity_to_def: HashMap<(String, usize), usize> = HashMap::new();
    let mut simple_name_to_defs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut extension_index: HashMap<String, Vec<ExtCandidate>> = HashMap::new();

    // Dedupe on the def's OWN quadruple list FIRST, then push into the bucket --
    // the guard is what keeps one def out of the same bucket twice.
    fn add_extension_method(
        member_lists: &mut [MemberLists],
        extension_index: &mut HashMap<String, Vec<ExtCandidate>>,
        idx: usize,
        entry: &FragExtensionMethod,
    ) {
        if member_lists[idx]
            .extension_methods
            .iter()
            .any(|(n, t, lo, hi)| {
                *n == entry.name
                    && *t == entry.this_type
                    && *lo == entry.arity_min
                    && *hi == entry.arity_max
            })
        {
            return;
        }
        member_lists[idx].extension_methods.push((
            entry.name.clone(),
            entry.this_type.clone(),
            entry.arity_min,
            entry.arity_max,
        ));
        extension_index
            .entry(format!("{} {}", entry.name, entry.this_type))
            .or_default()
            .push(ExtCandidate {
                def_idx: idx,
                entry: entry.clone(),
            });
    }

    for (file, frag) in fragments_by_file {
        for d in &frag.defs {
            let def_key = (d.id.clone(), d.type_params.len());
            match qualified_name_and_arity_to_def.get(&def_key) {
                None => {
                    let idx = defs.len();
                    defs.push(Def {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        namespace: d.namespace.clone(),
                        kind: d.kind.clone(),
                        file: file.clone(),
                        line: d.line,
                        methods: d.methods.clone(),
                        test_methods: d.test_methods.clone(),
                        also_in: Vec::new(),
                        end_line: d.end_line,
                    });
                    member_lists.push(MemberLists {
                        properties: d.properties.clone(),
                        fields: d.fields.clone(),
                        extension_methods: Vec::new(),
                        bases: d.bases.clone(),
                        type_params: d.type_params.clone(),
                        base_generic_args: d
                            .base_generic_args
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                        method_returns: d.method_returns.clone(),
                        property_types: d.property_types.clone(),
                    });
                    for e in &d.extension_methods {
                        add_extension_method(&mut member_lists, &mut extension_index, idx, e);
                    }
                    qualified_name_to_def.entry(d.id.clone()).or_insert(idx);
                    qualified_name_and_arity_to_def.insert(def_key, idx);
                    // Enum members are reachable by exact id (below, via
                    // qualified_name_to_def) but deliberately excluded from
                    // simple_name_to_defs -- see module header.
                    if d.kind != "enum-member" {
                        simple_name_to_defs
                            .entry(d.name.clone())
                            .or_default()
                            .push(idx);
                    }
                }
                Some(&idx) => {
                    // A partial class contributes its own members to the same
                    // record; first declaring file wins the order, later ones
                    // append what is new.
                    defs[idx].also_in.push(AlsoIn {
                        file: file.clone(),
                        line: d.line,
                    });
                    for m in &d.methods {
                        if !defs[idx].methods.contains(m) {
                            defs[idx].methods.push(m.clone());
                        }
                    }
                    // Test-coverage stage -- unioned exactly like `methods`
                    // above it, so a partial test class split across two files
                    // is one def declaring the union of both parts' tests.
                    for t in &d.test_methods {
                        if !defs[idx].test_methods.contains(t) {
                            defs[idx].test_methods.push(t.clone());
                        }
                    }
                    for p in &d.properties {
                        if !member_lists[idx].properties.contains(p) {
                            member_lists[idx].properties.push(p.clone());
                        }
                    }
                    for f in &d.fields {
                        if !member_lists[idx].fields.contains(f) {
                            member_lists[idx].fields.push(f.clone());
                        }
                    }
                    for e in &d.extension_methods {
                        add_extension_method(&mut member_lists, &mut extension_index, idx, e);
                    }
                    for b in &d.bases {
                        if !member_lists[idx].bases.contains(b) {
                            member_lists[idx].bases.push(b.clone());
                        }
                    }
                    // A partial class's later parts contribute the member types
                    // the first part did not declare; a name the first part
                    // already answered keeps its answer, the same
                    // first-declaration-wins rule the extractor applies within
                    // one declaration.
                    for (name, returns) in d.method_returns.iter() {
                        if member_lists[idx].method_returns.get(name).is_none() {
                            member_lists[idx]
                                .method_returns
                                .insert(name.clone(), returns.clone());
                        }
                    }
                    for (name, fact) in d.property_types.iter() {
                        if member_lists[idx].property_types.get(name).is_none() {
                            member_lists[idx]
                                .property_types
                                .insert(name.clone(), fact.clone());
                        }
                    }
                }
            }
        }
    }

    // See `DefIndex::member_name_to_defs`. A second pass over the ALREADY-MERGED
    // def records, so a partial class spread over three files lands in each of
    // its member-name buckets exactly once.
    let mut member_name_to_defs: HashMap<String, Vec<usize>> = HashMap::new();
    for idx in 0..defs.len() {
        let mut names: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for m in &defs[idx].methods {
            if seen.insert(m.as_str()) {
                names.push(m.as_str());
            }
        }
        for p in &member_lists[idx].properties {
            if seen.insert(p.as_str()) {
                names.push(p.as_str());
            }
        }
        for f in &member_lists[idx].fields {
            if seen.insert(f.as_str()) {
                names.push(f.as_str());
            }
        }
        for (name, ..) in &member_lists[idx].extension_methods {
            if seen.insert(name.as_str()) {
                names.push(name.as_str());
            }
        }
        for n in names {
            member_name_to_defs
                .entry(n.to_string())
                .or_default()
                .push(idx);
        }
    }

    DefIndex {
        defs,
        member_lists,
        qualified_name_to_def,
        qualified_name_and_arity_to_def,
        simple_name_to_defs,
        extension_index,
        member_name_to_defs,
    }
}

// A synthetic bare type reference, for re-running the ladder on a name the
// RESOLVER derived rather than one it read off a ref: a base type, a receiver's
// recorded type, a property's declared type, a callee's owner. Every field
// except `name`/`outer_types` is what a bare, non-generic, non-member reference
// carries, so a probe can never re-enter a member tier by accident.
fn name_probe(name: String, namespace: &str, outer_types: Vec<String>) -> FragRef {
    FragRef {
        kind: "uses-type".to_string(),
        name,
        qualified: None,
        member: None,
        line: 0,
        namespace: Some(namespace.to_string()),
        type_arg_count: None,
        generic: false,
        receiver_type: None,
        arg_count: None,
        receiver_args: None,
        outer_types,
        args: None,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
    }
}

// The def's member lists, unioned: methods ∪ properties ∪ fields, which is what
// lets a static PROPERTY access (MessageUrn.Prefix) and a const/static FIELD
// access earn an edge on the same evidence a static method call already did.
fn declares_member(index: &DefIndex, idx: usize, member: Option<&str>) -> bool {
    let Some(member) = member else { return false };
    index.defs[idx].methods.iter().any(|m| m == member)
        || index.member_lists[idx]
            .properties
            .iter()
            .any(|p| p == member)
        || index.member_lists[idx].fields.iter().any(|f| f == member)
}

// The membership test the SCORED tier uses: `declares_member` widened by the
// extension-method names the def declares. A static class holding
// `Render(this Widget w)` never "declares Render" in the instance sense
// `declares_member` means, but it is exactly the def a `something.Render()`
// guess should be allowed to name, so the scored tier counts it. Deliberately
// NOT used by any precise tier: widening `declares_member` itself would let
// tiers (a)/(e) emit a PRECISE edge on an extension name with none of tier
// (f)'s arity, generic-unification or admission filters applied.
fn member_vouched(index: &DefIndex, idx: usize, member: Option<&str>) -> bool {
    if declares_member(index, idx, member) {
        return true;
    }
    let Some(member) = member else { return false };
    index.member_lists[idx]
        .extension_methods
        .iter()
        .any(|(name, ..)| name == member)
}

// The whole scoring function, deterministic by construction and with no tie
// left to chance: same namespace as the ref site beats a namespace the file
// merely imports, which beats anything else. The global namespace is `""` on
// BOTH sides here (the extractor records an empty string, never a missing key,
// and `resolve_graph` folds a ref's absent namespace to `""` the same way), so
// it matches itself and correctly scores 3.
fn score_candidate(def_namespace: &str, ref_namespace: &str, usings: &HashSet<String>) -> u8 {
    if def_namespace == ref_namespace {
        return 3;
    }
    if usings.contains(def_namespace) {
        return 2;
    }
    1
}

/// One file's using/alias context.
struct FileContext {
    usings: HashSet<String>,
    aliases: HashMap<String, String>,
}

// Every file's own context (local ∪ every `global using`, with a local alias
// shadowing a same-named global one), built once instead of once per ref. The
// main loop needs it for the file it is walking; the instance-member veto needs
// it for a DIFFERENT file -- the one that declares the base type it is resolving
// -- which is why it is a map rather than two locals.
fn build_file_contexts(
    fragments_by_file: &[(String, Fragment)],
    global_usings: &HashSet<String>,
    global_aliases: &HashMap<String, String>,
) -> HashMap<String, FileContext> {
    let mut contexts = HashMap::new();
    for (file, frag) in fragments_by_file {
        let mut usings = global_usings.clone();
        let mut aliases = global_aliases.clone();
        for u in &frag.usings {
            match u {
                FragUsing::Alias { alias, target, .. } => {
                    aliases.insert(alias.clone(), target.clone());
                }
                FragUsing::Plain { text, .. } => {
                    usings.insert(text.clone());
                }
            }
        }
        contexts.insert(file.clone(), FileContext { usings, aliases });
    }
    contexts
}

// C#'s actual lookup rule: an INSTANCE member always beats an extension method,
// and "instance member" means anything the receiver's type declares ANYWHERE in
// its inheritance chain, not just on the type itself.
//
// True when `start` or any def in its transitive base closure declares
// `member`. Each def's DIRECT base names come from the extraction-time `bases`
// fact (the same base list the `inherits` refs are read from, reduced to base
// identifiers) and are resolved LAZILY here -- through the ordinary ladder, in
// the DECLARING file's own using/alias context and the def's own namespace,
// because a base name means what it meant where it was written.
//
// Cycle-guarded by def index (C# forbids inheritance cycles, but a fragment
// cache assembled from mid-edit sources can present one, and an infinite loop in
// the resolver is not an acceptable failure mode). Only in-graph defs are
// walked: an external base -- a BCL type, a NuGet type -- cannot be inspected,
// so a member it declares cannot veto. That is the documented bound, and it is
// the same one tier (e) already lives with.
fn inherited_member_declared(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: Option<&str>,
) -> bool {
    let mut seen: HashSet<usize> = HashSet::from([start]);
    let mut stack: Vec<usize> = vec![start];
    while let Some(cur) = stack.pop() {
        if declares_member(index, cur, member) {
            return true;
        }
        let Some(ctx) = file_contexts.get(&index.defs[cur].file) else {
            continue;
        };
        let ns = index.defs[cur].namespace.clone();
        for base in &index.member_lists[cur].bases {
            // The base-closure probe carries no stack: it walks BASE types,
            // not the lexical chain.
            let probe = name_probe(base.clone(), &ns, Vec::new());
            if let Resolution::Resolved(bidx, _) =
                resolve_ref(&probe, &ctx.usings, &ns, index, &ctx.aliases)
            {
                if seen.insert(bidx) {
                    stack.push(bidx);
                }
            }
        }
    }
    false
}

// The call's argument count against the entry's declared RANGE. `arity_max ==
// -1` is the `params` sentinel: unbounded above.
fn arity_accepts(entry: &crate::graph::FragExtensionMethod, arg_count: usize) -> bool {
    entry.arity_min <= arg_count && (entry.arity_max == -1 || (arg_count as i64) <= entry.arity_max)
}

// The this-parameter's top-level type arguments against the receiver's. Both
// sides absent (neither type is generic) is the base-name match. Exactly one
// side absent is a genuine generic/non-generic mismatch and never binds.
// Otherwise the lists unify position by position, where "*" -- a type parameter
// neither side can resolve to a concrete type -- matches anything.
fn generic_args_unify(
    this_args: Option<&Vec<String>>,
    receiver_args: Option<&Vec<String>>,
) -> bool {
    match (this_args, receiver_args) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == "*" || y == "*" || x == y)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Global usings/aliases.
// ---------------------------------------------------------------------------

fn collect_global_usings(
    fragments_by_file: &[(String, Fragment)],
) -> (HashSet<String>, HashMap<String, String>) {
    let mut global_usings: HashSet<String> = HashSet::new();
    let mut global_aliases: HashMap<String, String> = HashMap::new();
    for (_, frag) in fragments_by_file {
        for u in &frag.usings {
            match u {
                FragUsing::Alias {
                    alias,
                    target,
                    global,
                } => {
                    if *global {
                        // First global alias for a given name wins -- NOT
                        // last-wins. `entry(..).or_insert(..)` only writes on a
                        // vacant slot.
                        global_aliases
                            .entry(alias.clone())
                            .or_insert_with(|| target.clone());
                    }
                }
                FragUsing::Plain { text, global } => {
                    if *global {
                        global_usings.insert(text.clone());
                    }
                }
            }
        }
    }
    (global_usings, global_aliases)
}

// ---------------------------------------------------------------------------
// The ladder itself.
// ---------------------------------------------------------------------------

/// The `via` field on a resolved return -- names the ladder step that answered.
/// Only the uses-member emission tiers consume it (an exact-qualified
/// resolution is type-certain in a way a bare-name fallthrough is not); nothing
/// in graph.json carries it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Via {
    Alias,
    Nested,
    Qualified,
    Usings,
    Namespace,
    Global,
}

enum Resolution {
    Resolved(usize, Via),
    Ambiguous(Vec<usize>),
    External,
}

fn type_candidate(index: &DefIndex, name: &str, arity: Option<usize>) -> Option<usize> {
    match arity {
        Some(n) => index
            .qualified_name_and_arity_to_def
            .get(&(name.to_string(), n))
            .copied(),
        None => index.qualified_name_to_def.get(name).copied(),
    }
}

// Resolve one ref (a type reference OR a uses-member qualifier -- same shape,
// same ladder) against the current file's using/alias context. `ns` is
// `ref.namespace` with `None` folded to `""`: an EMPTY namespace is treated the
// same as absent for both the step-1 prefix walk and the step-3 same-namespace
// check, so folding `None` to `""` up front avoids re-deriving that check at
// every call site.
fn resolve_ref(
    ref_: &FragRef,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
) -> Resolution {
    // Every enclosing-namespace prefix of the reference site, innermost first
    // and ending with the empty prefix (the name as literally written).
    // Shared by steps 1, 2 and 3, all three of which walk it.
    let segments: Vec<&str> = if ns.is_empty() {
        Vec::new()
    } else {
        ns.split('.').collect()
    };
    let prefixes: Vec<String> = (0..=segments.len())
        .rev()
        .map(|i| segments[..i].join("."))
        .collect();

    // Step 0: alias short-circuit, bare names only.
    if ref_.qualified.is_none() {
        if let Some(alias_target) = aliases.get(&ref_.name) {
            return match type_candidate(index, alias_target, ref_.type_arg_count) {
                Some(idx) => Resolution::Resolved(idx, Via::Alias),
                None => Resolution::External,
            };
        }
        // Step 0b: the enclosing TYPE chain, longest prefix first (innermost
        // out). A nested def id is its chain joined with "+" onto the ref's own
        // namespace, so this is one exact id lookup per level and can never
        // produce two candidates. A ref with no stack -- every namespace-level
        // ref, every fragment cached without a type stack -- skips it.
        for i in (1..=ref_.outer_types.len()).rev() {
            let stack = ref_.outer_types[..i].join("+");
            let candidate = if ns.is_empty() {
                format!("{stack}+{}", ref_.name)
            } else {
                format!("{ns}.{stack}+{}", ref_.name)
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                return Resolution::Resolved(idx, Via::Nested);
            }
        }
    }

    // Step 1: exact qualified name, walking enclosing namespaces innermost
    // first, only for dotted references.
    if let Some(qualified) = &ref_.qualified {
        for prefix in &prefixes {
            let candidate = if prefix.is_empty() {
                qualified.clone()
            } else {
                format!("{prefix}.{qualified}")
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                return Resolution::Resolved(idx, Via::Qualified);
            }
        }
    }

    // Step 2: file's usings (already the union of local + global by the time
    // this is called) + simple name, the using's OWN name walked over every
    // enclosing-namespace prefix -- `using Configuration;` inside
    // `namespace A.B.C` reaches `A.Configuration.T`. One directive contributes
    // at most ONE candidate: its innermost reading wins, exactly like step 1's
    // first-match-wins walk, and only then does the 1-vs-many rule run across
    // directives. Dedup by def id -- two different using texts landing on the
    // same def counts once.
    let mut using_matches: Vec<usize> = Vec::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for u in usings {
        for prefix in &prefixes {
            let candidate = if prefix.is_empty() {
                format!("{u}.{}", ref_.name)
            } else {
                format!("{prefix}.{u}.{}", ref_.name)
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                let id = index.defs[idx].id.as_str();
                if seen_ids.insert(id) {
                    using_matches.push(idx);
                }
                break;
            }
        }
    }
    if using_matches.len() == 1 {
        return Resolution::Resolved(using_matches[0], Via::Usings);
    }
    if using_matches.len() >= 2 {
        return Resolution::Ambiguous(using_matches);
    }

    // Step 3: the reference site's namespace AND every ancestor of it,
    // innermost first -- the same walk step 1 runs (`T` inside `A.B.C`
    // reaches `A.B.T` and `A.T`, not only `A.B.C.T`), which is C#'s
    // ancestor-namespace rule.
    for prefix in &prefixes {
        let candidate = if prefix.is_empty() {
            ref_.name.clone()
        } else {
            format!("{prefix}.{}", ref_.name)
        };
        if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
            return Resolution::Resolved(idx, Via::Namespace);
        }
    }

    // Step 4: globally unique simple name. Enum members are excluded from
    // this pool (see build_def_index) -- a member named e.g. "Active"
    // sharing a simple name with an unrelated class must not turn that
    // class's previously-unambiguous references ambiguous.
    let matches: Vec<usize> = index
        .simple_name_to_defs
        .get(&ref_.name)
        .into_iter()
        .flatten()
        .copied()
        .filter(|idx| {
            ref_.type_arg_count
                .map_or(true, |n| index.member_lists[*idx].type_params.len() == n)
        })
        .collect();
    match matches.as_slice() {
        [idx] => Resolution::Resolved(*idx, Via::Global),
        [_, _, ..] => Resolution::Ambiguous(matches),
        _ => Resolution::External,
    }
}

// ---------------------------------------------------------------------------
// Ambiguous-candidate capping.
// ---------------------------------------------------------------------------

// Sorted by id, capped at `AMBIGUOUS_CAP`, using plain Unicode-codepoint
// `str::cmp` rather than locale-aware collation. For every id this extractor can
// produce (C# namespace/type names -- letters, digits, underscore, `.`, `+`)
// codepoint order and locale order coincide in the overwhelming common case
// (PascalCase-leading identifiers, the C# naming convention this ladder's own
// fixtures and every def id observed so far follow). A pathological mix of
// leading-case or comparing `+` against a letter at the exact divergence point
// could reorder (never change the SET of) candidates within the cap -- flagged,
// not solved (no ICU collation available without a new dependency).
fn capped_candidates(index: &DefIndex, mut candidate_indices: Vec<usize>) -> Vec<Candidate> {
    candidate_indices.sort_by(|&a, &b| index.defs[a].id.cmp(&index.defs[b].id));
    candidate_indices
        .into_iter()
        .take(AMBIGUOUS_CAP)
        .map(|i| Candidate {
            id: index.defs[i].id.clone(),
            file: index.defs[i].file.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Constructor-parameter DI resolution.
// ---------------------------------------------------------------------------

// Reverse index over EVERY def's `bases`: bare base name -> every def index
// whose bases array names it. This is the raw candidate pool a ctor-param ref's
// interface name is matched against; `resolve_ctor_param` confirms each
// candidate the same way `inherited_member_declared` confirms a base name -- by
// re-resolving it through the ladder in the CANDIDATE's own file context --
// before trusting the bare-name match, so a same-named-but-unrelated interface
// elsewhere in the corpus can never masquerade as an implementor.
fn build_implementor_index(index: &DefIndex) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, lists) in index.member_lists.iter().enumerate() {
        for base_name in &lists.bases {
            map.entry(base_name.clone()).or_default().push(idx);
        }
    }
    map
}

// The "namespace prefix is System./Microsoft." half of the infra-leaf
// classification. A heuristic, not a semantic check: scout parses no project
// file and does no package-reference resolution, so this reads the reference
// SITE's own usings (file-local ∪ global) rather than the unresolved type's
// true origin -- a documented imprecision.
fn is_infra_namespace(name: &str) -> bool {
    name == "System"
        || name == "Microsoft"
        || name.starts_with("System.")
        || name.starts_with("Microsoft.")
}

/// Outcome of resolving one 'ctor-param' ref, as a Rust enum -- the caller
/// turns this into the `Edge::CtorDi` variant's fields.
enum CtorDiResolution {
    /// A single non-generic implementor of a non-generic interface.
    Plain(usize),
    /// A single non-generic implementor whose own base-list entry names the
    /// SAME closed arguments the ctor param does.
    Closed(usize),
    /// A single implementor that is itself generic and passes the
    /// interface's type argument straight through. Preferred only when no
    /// closed implementor exists.
    OpenGeneric(usize),
    /// Two or more implementors tie at the same precedence tier -- never a
    /// guess.
    Ambiguous(Vec<usize>),
    /// The interface itself does not resolve in the corpus AND the reference
    /// site's usings name a BCL/Microsoft namespace: a framework leaf, not a
    /// corpus gap.
    Infra,
    /// Every other case: the interface resolves in-corpus but nothing
    /// implements it, or it resolves to nothing and no using suggests a
    /// framework origin. Still EMITTED by the caller (never silently
    /// dropped, unlike the general ladder's `unresolved_external` count).
    Unresolved,
}

fn resolve_ctor_param(
    ref_: &FragRef,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
    implementors_by_base_name: &HashMap<String, Vec<usize>>,
) -> CtorDiResolution {
    match resolve_ref(ref_, usings, ns, index, aliases) {
        Resolution::Ambiguous(candidate_indices) => CtorDiResolution::Ambiguous(candidate_indices),
        Resolution::External => {
            if usings.iter().any(|u| is_infra_namespace(u)) {
                CtorDiResolution::Infra
            } else {
                CtorDiResolution::Unresolved
            }
        }
        Resolution::Resolved(iface_idx, _) => {
            let base_name = &ref_.name;
            let raw_candidates = implementors_by_base_name
                .get(base_name)
                .cloned()
                .unwrap_or_default();
            let mut closed_or_plain: Vec<usize> = Vec::new();
            let mut open_generic: Vec<usize> = Vec::new();
            for cand in raw_candidates {
                if cand == iface_idx {
                    continue;
                }
                let cand_def = &index.defs[cand];
                let Some(cand_ctx) = file_contexts.get(&cand_def.file) else {
                    continue;
                };
                // Re-resolve the SAME base name through the CANDIDATE's own
                // file context -- exactly inherited_member_declared's own
                // pattern.
                let probe = name_probe(base_name.clone(), &cand_def.namespace, Vec::new());
                let base_res = resolve_ref(
                    &probe,
                    &cand_ctx.usings,
                    &cand_def.namespace,
                    index,
                    &cand_ctx.aliases,
                );
                let Resolution::Resolved(resolved_iface, _) = base_res else {
                    continue;
                };
                if resolved_iface != iface_idx {
                    continue;
                }
                let cand_args = index.member_lists[cand]
                    .base_generic_args
                    .iter()
                    .find(|(k, _)| k == base_name)
                    .map(|(_, v)| v);
                if !generic_args_unify(cand_args, ref_.args.as_ref()) {
                    continue;
                }
                if cand_args
                    .map(|a| a.iter().any(|x| x == "*"))
                    .unwrap_or(false)
                {
                    open_generic.push(cand);
                } else {
                    closed_or_plain.push(cand);
                }
            }
            if closed_or_plain.len() == 1 {
                return if ref_.args.is_some() {
                    CtorDiResolution::Closed(closed_or_plain[0])
                } else {
                    CtorDiResolution::Plain(closed_or_plain[0])
                };
            }
            if closed_or_plain.len() > 1 {
                return CtorDiResolution::Ambiguous(closed_or_plain);
            }
            if open_generic.len() == 1 {
                return CtorDiResolution::OpenGeneric(open_generic[0]);
            }
            if open_generic.len() > 1 {
                return CtorDiResolution::Ambiguous(open_generic);
            }
            CtorDiResolution::Unresolved
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level orchestration.
// ---------------------------------------------------------------------------

fn type_edge(kind: &str, file: &str, line: usize, target: &Def) -> Edge {
    let from_file = file.to_string();
    let to = target.id.clone();
    let to_file = target.file.clone();
    match kind {
        "inherits" => Edge::Inherits {
            from_file,
            from_line: line,
            to,
            to_file,
            heuristic: false,
        },
        _ => Edge::UsesType {
            from_file,
            from_line: line,
            to,
            to_file,
            heuristic: false,
        },
    }
}

// The identity a heuristic edge is deduped on: everything its serialized form
// carries. `None` for a precise edge, which is never a dedup subject. Field
// order matches the edge's own, so two edges share a key exactly when they
// serialize to the same bytes.
fn heuristic_edge_key(e: &Edge) -> Option<String> {
    let (kind, from_file, from_line, to, to_file) = match e {
        Edge::Inherits {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("inherits", from_file, from_line, to, to_file),
        Edge::UsesType {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("uses-type", from_file, from_line, to, to_file),
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("uses-member", from_file, from_line, to, to_file),
        _ => return None,
    };
    Some(format!("{kind} {from_file} {from_line} {to} {to_file}"))
}

/// Resolve C# fragments into a graph. Pure: `fragments_by_file` is
/// file-walk-ordered `(rel, Fragment)` pairs. No file I/O beyond the single
/// `git rev-parse HEAD` shell-out (`manifest::git_head`) that fills
/// `built_at_head` -- cheap enough to re-run on every `devscout map` whose C#
/// set changed, and unit-testable without a parser (see this module's tests,
/// which build `Fragment` values by hand).
pub fn resolve_graph(root: &Path, fragments_by_file: &[(String, Fragment)]) -> Graph {
    resolve_graph_with_ts(root, fragments_by_file, &[])
}

/// The same resolve, with the TS/TSX half alongside. The caller passes the two
/// halves already split (it reads the tag off each cache entry --
/// `graph::AnyFragment`), and each half goes to its own resolver.
///
/// The TS contribution is a SUFFIX and never an interleave: def order, edge
/// order and the stats block a C#-only repo produces are untouched, and a
/// reader diffing two graphs sees the TS rows appended after every C# row. The
/// four TS edge kinds join `edges_by_kind` and `ts` joins `stats` ONLY when the
/// repo has a TS fragment at all.
pub fn resolve_graph_with_ts(
    root: &Path,
    fragments_by_file: &[(String, Fragment)],
    ts_fragments_by_file: &[(String, crate::extract::TsFragment)],
) -> Graph {
    let index = build_def_index(fragments_by_file);
    let (global_usings, global_aliases) = collect_global_usings(fragments_by_file);
    let file_contexts = build_file_contexts(fragments_by_file, &global_usings, &global_aliases);
    // Built once, not per-ref: every ctor-param ref's implementor lookup shares
    // this one reverse index.
    let implementors_by_base_name = build_implementor_index(&index);

    let mut edges: Vec<Edge> = Vec::new();
    let mut edges_by_kind = EdgesByKind::default();
    let mut ambiguous_count: usize = 0;
    let mut unresolved_external: usize = 0;
    // Heuristic edges are counted HERE and nowhere else. `edges_by_kind` stays a
    // count of PRECISE edges only, so a consumer reading
    // `edges_by_kind['uses-member']` never has a guess folded into a fact. The
    // heuristic total is reported separately in the stats object.
    let mut heuristic_edge_count: usize = 0;

    for (file, frag) in fragments_by_file {
        // Local alias shadows a same-named global one -- see
        // build_file_contexts, which builds every file's context once up front
        // so the veto walk can read a DIFFERENT file's context too.
        let FileContext { usings, aliases } = &file_contexts[file];

        for r in &frag.refs {
            if r.kind == "imports" {
                edges.push(Edge::Imports {
                    from_file: file.clone(),
                    from_line: r.line,
                    target: r.name.clone(),
                });
                edges_by_kind.imports += 1;
                continue;
            }

            let ns = r.namespace.as_deref().unwrap_or("");

            if r.kind == "uses-member" {
                // Resolve the qualifier through the SAME ladder as a type
                // ref, then only act when it lands on exactly one candidate
                // that clears an emission tier. Enums emit unconditionally
                // (the member def is the target when it exists). Non-enum types
                // emit only on syntactic type-certainty, because a bare
                // qualifier that resolves to a type can still be an instance
                // property or local sharing the type's name:
                //   (a) the member is in the def's recorded member lists --
                //       methods, properties and fields
                //       (`MessageUrn.ForType(...)`, `MessageUrn.Prefix` -- an
                //       instance property named MessageUrn would not carry
                //       either), or
                //   (b) the qualifier carried a type-argument list (syntax no
                //       local/field/property can carry), or
                //   (c) the qualifier was dotted AND answered at the
                //       exact-qualified ladder step.
                // Everything else (ambiguous, external, or a non-enum
                // resolution with no certainty signal) is dropped silently
                // and deliberately NOT counted in ambiguous_count/
                // unresolved_external: almost every member access in a file
                // becomes a uses-member candidate (locals, properties, BCL
                // calls), and counting the misses would swamp the
                // type-ref-quality stats with noise ("never guess").
                //
                // The outcome is bound whole rather than pattern-matched
                // inline: the scored tier reads its STATUS (ambiguous vs.
                // nothing-at-all) to decide which candidate pool it may draw
                // from, and re-walking the ladder there would be a second
                // resolution of the same name in the same file context.
                let result = resolve_ref(r, usings, ns, &index, aliases);
                let mut emitted = false;
                if let Resolution::Resolved(idx, via) = &result {
                    let (idx, via) = (*idx, *via);
                    if index.defs[idx].kind == "enum" {
                        let member_key = format!(
                            "{}.{}",
                            index.defs[idx].id,
                            r.member.as_deref().unwrap_or("")
                        );
                        let (to, to_file) = match index.qualified_name_to_def.get(&member_key) {
                            Some(&mi) => (index.defs[mi].id.clone(), index.defs[mi].file.clone()),
                            None => (index.defs[idx].id.clone(), index.defs[idx].file.clone()),
                        };
                        edges.push(Edge::UsesMember {
                            from_file: file.clone(),
                            from_line: r.line,
                            to,
                            to_file,
                            heuristic: false,
                        });
                        edges_by_kind.uses_member += 1;
                        emitted = true;
                    } else {
                        // generic counts only for BARE qualifiers: a
                        // flattened chain inherits the flag from its inner
                        // segment while ladder steps 2-4 resolve by the
                        // chain's TAIL name, which can name-match an
                        // unrelated type. Dotted
                        // chains earn their edge via the member lists or the
                        // exact-qualified step instead.
                        if declares_member(&index, idx, r.member.as_deref())
                            || (r.generic && r.qualified.is_none())
                            || (r.qualified.is_some() && via == Via::Qualified)
                        {
                            edges.push(Edge::UsesMember {
                                from_file: file.clone(),
                                from_line: r.line,
                                to: index.defs[idx].id.clone(),
                                to_file: index.defs[idx].file.clone(),
                                heuristic: false,
                            });
                            edges_by_kind.uses_member += 1;
                            emitted = true;
                        }
                    }
                }
                // Tier (e): the qualifier is an INSTANCE the extractor has
                // exactly one local type fact for (`_repo.Save()` where the
                // file declares `private IRepo _repo;`). The recorded type name
                // goes through the same ladder from the same file context -- it
                // is a bare identifier, so alias/usings/namespace/global all
                // apply -- and the member must be declared by the def it lands
                // on. Anything less (type unresolved, type ambiguous, member not
                // declared) is no edge; the extension tier may claim it, the
                // scored tier may tag it. Tried only after tiers (a)-(c) have
                // declined, so no ref can earn two edges.
                //
                // The resolution itself is hoisted into `receiver_def` so tier
                // (f)'s instance-member veto can reuse it instead of walking the
                // ladder a second time for the same name in the same file
                // context. Tier (e) still requires the member on the EXACT
                // receiver def, with no inheritance widening. The closure is a
                // negative signal only.
                //
                // The FULL outcome is kept too, not just the def: the scored
                // tier reads its status (ambiguous vs. nothing-at-all) for any
                // ref carrying a receiver fact, since that fact IS what the
                // qualifier's type is and outranks resolving the qualifier
                // identifier itself.
                let mut receiver_def: Option<usize> = None;
                let mut receiver_result: Option<Resolution> = None;
                // A `var x = Q.M(...)` local carries the CALL, not a type: the
                // extractor cannot know what `M` returns, and the def that can
                // is in another file. Resolving the callee's owner through the
                // same ladder and reading its recorded return type is what turns
                // the call into an ordinary receiver fact; from here on every
                // tier treats it as one. An owner that resolves to nothing or to
                // several candidates, and an owner with no recorded return for
                // that name, both yield no fact -- the local stays
                // taken-but-unknown, which is the answer the extractor already
                // gave.
                let mut receiver_type_name = r.receiver_type.clone();
                if receiver_type_name.is_none() {
                    if let (Some(owner), Some(member)) =
                        (&r.receiver_call_owner, &r.receiver_call_member)
                    {
                        let probe = name_probe(owner.clone(), ns, r.outer_types.clone());
                        if let Resolution::Resolved(oidx, _) =
                            resolve_ref(&probe, usings, ns, &index, aliases)
                        {
                            receiver_type_name =
                                index.member_lists[oidx].method_returns.get(member).cloned();
                        }
                    }
                }
                if !emitted {
                    if let Some(receiver_type) = &receiver_type_name {
                        let probe = name_probe(receiver_type.clone(), ns, r.outer_types.clone());
                        let rr = resolve_ref(&probe, usings, ns, &index, aliases);
                        if let Resolution::Resolved(ridx, _) = &rr {
                            let ridx = *ridx;
                            receiver_def = Some(ridx);
                            if declares_member(&index, ridx, r.member.as_deref()) {
                                edges.push(Edge::UsesMember {
                                    from_file: file.clone(),
                                    from_line: r.line,
                                    to: index.defs[ridx].id.clone(),
                                    to_file: index.defs[ridx].file.clone(),
                                    heuristic: false,
                                });
                                edges_by_kind.uses_member += 1;
                                // Tier (e) RECORDS its claim: the extension
                                // tier below reads `emitted`, and that is
                                // exactly what implements C#'s shadowing
                                // rule (see that tier's note).
                                emitted = true;
                            }
                        }
                        receiver_result = Some(rr);
                    }
                }
                // Tier (e2): the qualifier is a two-segment chain
                // whose head the extractor could type (`_widget.Config.Reload()`
                // where the file declares `private Widget _widget;`). The head
                // type goes through the same ladder tier (e) uses, its def's
                // recorded property types answer what the SECOND segment is,
                // and that answer goes through the ladder again -- so the hop is
                // the field/local hop run twice, with a def fact where the file
                // had no declaration to read.
                //
                // Every step must land on exactly one def and the member must be
                // declared by the type the property is declared as. A head that
                // resolves to nothing or to several, a property with no recorded
                // type (a predefined one records none), a property type that
                // resolves to nothing, and a member the property's type does not
                // declare all end the hop with no edge, exactly as tier (e) ends
                // on the same failures.
                //
                // The hop is deliberately precise-tier only: it does not feed
                // the extension tier's lookup key or the scored tier's pool,
                // both of which read `receiver_type_name`, which this tier never
                // sets. The property type is a fact about the property's
                // DECLARATION, and a guess built on top of a second hop is a
                // guess about a guess.
                if !emitted {
                    if let Some(owner) = &r.receiver_property_owner {
                        let probe = name_probe(owner.clone(), ns, r.outer_types.clone());
                        if let Resolution::Resolved(oidx, _) =
                            resolve_ref(&probe, usings, ns, &index, aliases)
                        {
                            if let Some(fact) = index.member_lists[oidx].property_types.get(&r.name)
                            {
                                let hop =
                                    name_probe(fact.type_name.clone(), ns, r.outer_types.clone());
                                if let Resolution::Resolved(hidx, _) =
                                    resolve_ref(&hop, usings, ns, &index, aliases)
                                {
                                    if declares_member(&index, hidx, r.member.as_deref()) {
                                        edges.push(Edge::UsesMember {
                                            from_file: file.clone(),
                                            from_line: r.line,
                                            to: index.defs[hidx].id.clone(),
                                            to_file: index.defs[hidx].file.clone(),
                                            heuristic: false,
                                        });
                                        edges_by_kind.uses_member += 1;
                                        emitted = true;
                                    }
                                }
                            }
                        }
                    }
                }
                // Tier (f): extension methods, by C#'s own lookup rule.
                //
                // This tier emits HEURISTIC edges (bucket, arity range, generic
                // unification, admission, veto, one-distinct-class rule); the
                // emitted edge gains `heuristic: true` and does not count toward
                // edges_by_kind. The reason is a STRUCTURAL bound found by the
                // corpus audit, not a loose filter: the instance-member veto can
                // only inspect in-graph types, and the receivers that matter
                // most in real code (BCL types, NuGet types, anything outside
                // the mapped scope) hide every member they declare. When such a
                // receiver's own type declares the member, C# binds the instance
                // member and this tier's edge is simply wrong -- and no no-build
                // veto can see it. That is unfixable without a compile, so the
                // honest move is to keep the edge and tag it as a guess rather
                // than delete a tier that is right far more often than not.
                //
                // Reached only when every earlier tier declined --
                // including tier (e), whose `emitted` flag is what makes the C#
                // SHADOWING rule fall out of tier order for free: when the
                // receiver's own type declares the member, tier (e) has already
                // claimed the ref and this tier never runs, exactly as the
                // compiler prefers an instance member over any extension
                // method.
                //
                // ONLY refs carrying a receiver fact qualify -- one the
                // extractor recorded, or one the call hop just produced --
                // and by construction rather than by a check: an extension
                // method is callable in instance-call syntax only, so a static
                // or namespace qualifier ("Utils.Helper()") must never reach
                // this tier, and such a ref carries no receiver fact, so the
                // lookup key cannot even be formed.
                //
                // Admission is the LANGUAGE's rule, not a proximity heuristic: a
                // candidate counts only when its declaring static class's
                // namespace is imported by this file (local or global using) or
                // IS this file's namespace. Exactly one admitted candidate
                // emits. Zero or two-or-more emit nothing and are NOT counted as
                // ambiguous -- the same silence every other uses-member miss
                // keeps, since counting them would swamp the type-ref-quality
                // stats (the scored tier may tag them instead).
                //
                // The ref must also carry an `argCount`, which is both an arity
                // test and a SHAPE test. A property read (`t.P`) records no
                // argCount at extraction, so it cannot form a key and never
                // enters this tier at all -- an extension method is only ever
                // reachable through call syntax. A call records one, and it has
                // to fall inside the candidate's declared [arityMin, arityMax]
                // range.
                //
                // Four filters, in this order. Every one of them can only ever
                // REMOVE a candidate, and the tier emits only on exactly one
                // survivor:
                //   1. the bucket -- exact (member name, thisType) pair;
                //   2. arity range -- arityMin <= argCount <= arityMax, where
                //      an arityMax of -1 (a trailing `params` array) is
                //      unbounded above;
                //   3. generic unification -- the this-parameter's top-level
                //      type arguments against the receiver's, with "*" (either
                //      side's own type parameters) matching anything, and a
                //      generic-vs-non-generic pairing never matching at all;
                //   4. admission -- the declaring static class's namespace is
                //      imported by this file (local or global using) or IS this
                //      file's namespace.
                // Candidates are counted as DISTINCT DECLARING CLASSES, not as
                // entries: an edge names the class, so two overloads of one
                // class both accepting this call agree on the answer and are
                // not an ambiguity. Two different classes are.
                //
                // On top of the filters, the instance-member VETO: if the
                // receiver resolves in-graph and the member is declared
                // anywhere in its inheritance closure, C# binds the instance
                // member and this tier must not claim the ref at all. Tier (e)
                // already implements the exact-type half of that rule by
                // claiming the ref first; the closure walk is what extends it
                // to inherited and interface-declared members, which tier (e)
                // deliberately does not widen to (it would start EMITTING edges
                // to the wrong def -- the base declares the member, the derived
                // type is what the code names).
                //
                // Three documented bounds, each with a pinning negative test:
                //   - thisType is matched by EXACT name. No base-class walk, no
                //     interface widening on the POSITIVE side: `this
                //     IEnumerable<T>` does not claim a receiver typed List,
                //     `this BaseWidget` does not claim one typed Widget.
                //   - the namespace test is exact too: a static class in an
                //     ENCLOSING namespace of the ref site (App.Ext visible from
                //     App.Ext.Deep) is not admitted, though real C# would.
                //     Narrower than the language, never wider.
                //   - the veto can only see IN-GRAPH types. An external
                //     receiver, or an external base of an in-graph receiver,
                //     hides whatever members it declares, so no veto is
                //     possible there.
                if !emitted {
                    if let (Some(receiver_type), Some(member), Some(arg_count)) =
                        (&receiver_type_name, r.member.as_deref(), r.arg_count)
                    {
                        let key = format!("{member} {receiver_type}");
                        let candidates: &[ExtCandidate] = index
                            .extension_index
                            .get(&key)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]);
                        let mut distinct: Vec<usize> = Vec::new();
                        for c in candidates {
                            if !arity_accepts(&c.entry, arg_count) {
                                continue;
                            }
                            if !generic_args_unify(
                                c.entry.this_args.as_ref(),
                                r.receiver_args.as_ref(),
                            ) {
                                continue;
                            }
                            let def_ns = &index.defs[c.def_idx].namespace;
                            if !usings.contains(def_ns) && def_ns != ns {
                                continue;
                            }
                            if !distinct.contains(&c.def_idx) {
                                distinct.push(c.def_idx);
                            }
                        }
                        let vetoed = match receiver_def {
                            Some(ridx) => inherited_member_declared(
                                &index,
                                &file_contexts,
                                ridx,
                                r.member.as_deref(),
                            ),
                            None => false,
                        };
                        if distinct.len() == 1 && !vetoed {
                            let didx = distinct[0];
                            edges.push(Edge::UsesMember {
                                from_file: file.clone(),
                                from_line: r.line,
                                to: index.defs[didx].id.clone(),
                                to_file: index.defs[didx].file.clone(),
                                heuristic: true,
                            });
                            heuristic_edge_count += 1;
                            emitted = true;
                        }
                    }
                }
                // The SCORED tier, the last one, and the only one that may name
                // more than one def for a single ref. It
                // runs on refs no precise tier and not even tier (f) could
                // claim, and it never invents a candidate: the pool is always
                // something already recorded.
                //
                // Two mutually exclusive pools, chosen by what the ladder said
                // about the qualifier (for a bare/static qualifier) or about
                // the receiver's recorded type (whenever the ref carries a
                // receiverType fact -- that fact IS what the qualifier's type
                // is, so it outranks resolving the qualifier identifier
                // itself):
                //   - AMBIGUOUS: the ladder found several same-named defs and
                //     refused to pick. Those exact candidates, filtered to the
                //     ones that vouch for the member. This is the strong case
                //     -- the real answer is provably in the pool, only the
                //     choice is unknown.
                //   - NOTHING AT ALL (external/unresolved): fall back to
                //     member-name uniqueness -- every def graph-wide vouching
                //     for that member name, and ONLY when there are at most
                //     SCORED_UNIQUENESS_CAP of them. Past that threshold the
                //     name is common vocabulary (`Add`, `Name`, `Value`) and a
                //     guess carries no information, so the tier refuses
                //     outright rather than emitting its top three.
                // A qualifier that RESOLVED is deliberately in neither pool:
                // the resolution is a fact, the precise tiers already had their
                // chance at it, and a heuristic edge there would be a second
                // answer contradicting a known one. That is what keeps
                // "precise refs never get heuristic duplicates" true by
                // construction rather than by a later filter.
                //
                // Scoring is `score_candidate`; ties break on def id, ordinal
                // (the same `str::cmp` substitution `capped_candidates`
                // documents -- codepoint and locale order coincide for every
                // C# def id this extractor can produce). At most
                // SCORED_EMIT_CAP edges leave here, in scored order.
                //
                // A nested type (`Outer+Nested`) is not nameable from another
                // file without naming its outer type, so a guess landing on
                // one from outside its own file is unreachable by
                // construction. Same-file candidates stay -- inside the
                // declaring file the short name is real. The refusal happens
                // on the way OUT, after the cap: a refused guess gives up its
                // slot rather than promoting a weaker one into it.
                if !emitted {
                    let source: &Resolution = if receiver_type_name.is_some() {
                        receiver_result.as_ref().expect(
                            "a receiverType ref always resolves its receiver before this tier: tier (e) runs whenever !emitted",
                        )
                    } else {
                        &result
                    };
                    let pool: Option<Vec<usize>> = match source {
                        Resolution::Ambiguous(candidates) => Some(
                            candidates
                                .iter()
                                .copied()
                                .filter(|&d| member_vouched(&index, d, r.member.as_deref()))
                                .collect(),
                        ),
                        Resolution::External => {
                            let named: Vec<usize> = match r
                                .member
                                .as_deref()
                                .and_then(|m| index.member_name_to_defs.get(m))
                            {
                                Some(list) => list.clone(),
                                None => Vec::new(),
                            };
                            if named.len() <= SCORED_UNIQUENESS_CAP {
                                Some(named)
                            } else {
                                None
                            }
                        }
                        Resolution::Resolved(..) => None,
                    };
                    if let Some(pool) = pool {
                        let mut scored: Vec<(usize, u8)> = pool
                            .into_iter()
                            .map(|d| (d, score_candidate(&index.defs[d].namespace, ns, usings)))
                            .collect();
                        scored.sort_by(|a, b| {
                            b.1.cmp(&a.1)
                                .then_with(|| index.defs[a.0].id.cmp(&index.defs[b.0].id))
                        });
                        for (d, _) in scored.into_iter().take(SCORED_EMIT_CAP).filter(|&(d, _)| {
                            !index.defs[d].id.contains('+') || index.defs[d].file == *file
                        }) {
                            edges.push(Edge::UsesMember {
                                from_file: file.clone(),
                                from_line: r.line,
                                to: index.defs[d].id.clone(),
                                to_file: index.defs[d].file.clone(),
                                heuristic: true,
                            });
                            heuristic_edge_count += 1;
                        }
                    }
                }
                continue;
            }

            // A 'ctor-param' ref never falls through to the generic
            // type-reference ladder below: that ladder resolves a bare name to
            // the INTERFACE's own def (which is what the plain 'uses-type' edge
            // for the same parameter already does), never to an IMPLEMENTATION.
            // This branch is the DI-specific resolution instead, and it always
            // emits (never silently drops, unlike unresolved_external below).
            if r.kind == "ctor-param" {
                let classification = resolve_ctor_param(
                    r,
                    usings,
                    ns,
                    &index,
                    aliases,
                    &file_contexts,
                    &implementors_by_base_name,
                );
                let (resolution, to, candidates): (&str, Option<String>, Vec<Candidate>) =
                    match classification {
                        CtorDiResolution::Plain(i) => {
                            ("plain", Some(index.defs[i].id.clone()), Vec::new())
                        }
                        CtorDiResolution::Closed(i) => {
                            ("closed", Some(index.defs[i].id.clone()), Vec::new())
                        }
                        CtorDiResolution::OpenGeneric(i) => {
                            ("open-generic", Some(index.defs[i].id.clone()), Vec::new())
                        }
                        CtorDiResolution::Ambiguous(idxs) => {
                            ("ambiguous", None, capped_candidates(&index, idxs))
                        }
                        CtorDiResolution::Infra => ("infra", None, Vec::new()),
                        CtorDiResolution::Unresolved => ("unresolved", None, Vec::new()),
                    };
                edges.push(Edge::CtorDi {
                    from_file: file.clone(),
                    from_line: r.line,
                    iface: r.name.clone(),
                    resolution: resolution.to_string(),
                    args: r.args.clone(),
                    to,
                    candidates,
                });
                edges_by_kind.ctor_di += 1;
                continue;
            }

            match resolve_ref(r, usings, ns, &index, aliases) {
                Resolution::Resolved(idx, _) => {
                    edges.push(type_edge(&r.kind, file, r.line, &index.defs[idx]));
                    match r.kind.as_str() {
                        "inherits" => edges_by_kind.inherits += 1,
                        "uses-type" => edges_by_kind.uses_type += 1,
                        _ => {}
                    }
                }
                Resolution::Ambiguous(candidate_indices) => {
                    let candidate_count = candidate_indices.len();
                    edges.push(Edge::Ambiguous {
                        origin: r.kind.clone(),
                        from_file: file.clone(),
                        from_line: r.line,
                        raw: r.name.clone(),
                        candidates: capped_candidates(&index, candidate_indices),
                        candidate_count,
                    });
                    ambiguous_count += 1;
                }
                Resolution::External => {
                    unresolved_external += 1;
                }
            }
        }
    }

    // The full name index. Every name the mapped set declares, with the file
    // and line it is declared on: one entry per fragment def (its own `line`,
    // so `find` and `refs` point a caller at the same site), then that file's
    // member and markup names in source order. Types come off the FRAGMENT defs
    // rather than the merged rows, so a partial class contributes each declaring
    // site instead of only the first. Build order is fragment-map order, the
    // same order the edge loop above walks -- these bytes must be emitted in
    // that order or the artifacts diverge.
    //
    // A MARKUP def is the one def that contributes no row here. Its declaration
    // is already in the index, one entry earlier, as the `markup-class` name the
    // same scan emitted from the same `x:Class` on the same line -- under the
    // FULLY QUALIFIED spelling markup writes it in, which is strictly more than
    // a bare-name row would carry. Emitting both would put two rows on one
    // declaration and change what every existing `find` over a markup repo
    // returns.
    let mut names: Vec<GraphName> = Vec::new();
    for (file, frag) in fragments_by_file {
        if !crate::markup::is_markup(file) {
            for d in &frag.defs {
                names.push(GraphName {
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                    file: file.clone(),
                    line: d.line,
                    owner: String::new(),
                });
            }
        }
        for n in &frag.names {
            names.push(GraphName {
                name: n.name.clone(),
                kind: n.kind.clone(),
                file: file.clone(),
                line: n.line,
                owner: n.owner.clone(),
            });
        }
    }

    // Heuristic-side dedup, single pass, first occurrence wins. Independent
    // guess tiers (and repeated windows over one chain) can name the same
    // (kind, from_file, from_line, to, to_file) more than once; a second
    // byte-identical guess carries no information a reader can act on, so it
    // is dropped and its count with it. PRECISE edges are untouched: a
    // repeated precise edge is a repeated FACT about the source (two
    // references on one line), and collapsing it would silently lose a real
    // occurrence. `Vec::retain` keeps relative order, so the surviving first
    // occurrence sits exactly where it did.
    let mut seen_heuristic: HashSet<String> = HashSet::new();
    edges.retain(|e| match heuristic_edge_key(e) {
        None => true,
        Some(key) => {
            if seen_heuristic.insert(key) {
                true
            } else {
                heuristic_edge_count -= 1;
                false
            }
        }
    });

    let type_ref_attempts = edges_by_kind.inherits + edges_by_kind.uses_type + ambiguous_count;

    let mut graph = Graph {
        schema_version: 1,
        built_at_head: manifest::git_head(root),
        stats: Stats {
            def_count: index.defs.len(),
            file_count: fragments_by_file.len() + ts_fragments_by_file.len(),
            edges_by_kind,
            ambiguous_count,
            ambiguous_pct: Percent1::from_ratio(ambiguous_count, type_ref_attempts),
            unresolved_external_count: unresolved_external,
            heuristic_edge_count,
            // Appended LAST and always written, like the heuristic-edge counter
            // above it. Counts merged DEF ROWS, not fragment entries, so a
            // partial test class split across two files is one test def, not
            // two.
            test_def_count: index
                .defs
                .iter()
                .filter(|d| !d.test_methods.is_empty())
                .count(),
            ts: None,
        },
        defs: index.defs,
        edges,
        names,
    };
    if !ts_fragments_by_file.is_empty() {
        let alias = crate::tsgraph::read_ts_path_aliases(root);
        let ts = crate::tsgraph::resolve_ts_graph(ts_fragments_by_file, &alias);
        graph.defs.extend(ts.defs);
        graph.edges.extend(ts.edges);
        graph.stats.def_count = graph.defs.len();
        graph.stats.edges_by_kind.import = Some(ts.edges_by_kind.import);
        graph.stats.edges_by_kind.call = Some(ts.edges_by_kind.call);
        graph.stats.edges_by_kind.jsx_use = Some(ts.edges_by_kind.jsx_use);
        graph.stats.edges_by_kind.dispatch = Some(ts.edges_by_kind.dispatch);
        graph.stats.ts = Some(ts.stats);
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::FragDef;

    /// Not a real git repo -- `resolve_graph`'s single I/O call (`git_head`)
    /// fails closed to `None` here, the same as a non-repo shell-out failure.
    fn no_git_root() -> std::path::PathBuf {
        std::env::temp_dir().join("scout-resolve-test-not-a-repo")
    }

    fn def(id: &str, name: &str, ns: &str, kind: &str) -> FragDef {
        FragDef {
            id: id.into(),
            name: name.into(),
            namespace: ns.into(),
            kind: kind.into(),
            line: 1,
            methods: vec![],
            properties: vec![],
            fields: vec![],
            method_returns: crate::graph::OrderedMap::new(),
            extension_methods: vec![],
            bases: vec![],
            type_params: vec![],
            base_generic_args: crate::graph::OrderedMap::new(),
            test_methods: vec![],
            property_types: crate::graph::OrderedMap::new(),
            end_line: 0,
        }
    }

    /// `def()` for a static class declaring extension methods -- the only input
    /// tier (f) reads besides the def's own namespace. The tuples are (name,
    /// this_type, arity_min, arity_max).
    fn ext_def(id: &str, name: &str, ns: &str, extensions: &[(&str, &str, usize, i64)]) -> FragDef {
        FragDef {
            extension_methods: extensions
                .iter()
                .map(|(n, t, lo, hi)| FragExtensionMethod {
                    name: (*n).to_string(),
                    this_type: (*t).to_string(),
                    arity_min: *lo,
                    arity_max: *hi,
                    this_args: None,
                })
                .collect(),
            ..def(id, name, ns, "class")
        }
    }

    /// `def()` with member lists filled in -- the resolver reads
    /// `methods`/`properties`/`fields` as one union (tier (a)), so every tier
    /// test that turns on WHICH list a member lives in builds its def here.
    fn def_with(
        id: &str,
        name: &str,
        ns: &str,
        kind: &str,
        methods: &[&str],
        properties: &[&str],
        fields: &[&str],
    ) -> FragDef {
        FragDef {
            methods: methods.iter().map(|s| s.to_string()).collect(),
            properties: properties.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
            ..def(id, name, ns, kind)
        }
    }

    fn type_ref(kind: &str, name: &str, qualified: Option<&str>, ns: &str) -> FragRef {
        FragRef {
            kind: kind.into(),
            name: name.into(),
            qualified: qualified.map(String::from),
            member: None,
            line: 1,
            namespace: Some(ns.into()),
            type_arg_count: Some(0),
            generic: false,
            receiver_type: None,
            arg_count: None,
            receiver_args: None,
            outer_types: Vec::new(),
            args: None,
            receiver_property_owner: None,
            receiver_call_owner: None,
            receiver_call_member: None,
        }
    }

    fn member_ref(name: &str, qualified: Option<&str>, member: &str, ns: &str) -> FragRef {
        FragRef {
            kind: "uses-member".into(),
            name: name.into(),
            qualified: qualified.map(String::from),
            member: Some(member.into()),
            line: 1,
            namespace: Some(ns.into()),
            type_arg_count: None,
            generic: false,
            receiver_type: None,
            arg_count: None,
            receiver_args: None,
            outer_types: Vec::new(),
            args: None,
            receiver_property_owner: None,
            receiver_call_owner: None,
            receiver_call_member: None,
        }
    }

    /// A bare-qualifier member ref carrying the receiver fact the extractor
    /// would have recorded for it. `arg_count` is the CALL shape: `Some(n)` for
    /// `x.M(<n args>)`, `None` for a property read.
    fn receiver_ref(
        name: &str,
        member: &str,
        ns: &str,
        receiver_type: &str,
        arg_count: Option<usize>,
    ) -> FragRef {
        FragRef {
            receiver_type: Some(receiver_type.into()),
            arg_count,
            ..member_ref(name, None, member, ns)
        }
    }

    fn frag(defs: Vec<FragDef>, usings: Vec<FragUsing>, refs: Vec<FragRef>) -> Fragment {
        Fragment {
            defs,
            usings,
            refs,
            names: Vec::new(),
        }
    }

    fn find_edge<'a>(g: &'a Graph, want: impl Fn(&Edge) -> bool) -> Option<&'a Edge> {
        g.edges.iter().find(|e| want(e))
    }

    // --- built_at_head threading (manifest::git_head unit-tested there;
    // this is the integration check that resolve_graph actually calls it
    // and plumbs the result into the right field) --------------------------

    #[test]
    fn resolve_graph_threads_the_real_head_hash_through_built_at_head() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("scout-resolve-head-test-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap()
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("a.txt"), "hi").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "one"]);
        let expected = manifest::git_head(&dir).expect("git_head must see the commit just made");

        let g = resolve_graph(&dir, &[]);
        assert_eq!(g.built_at_head, Some(expected));
    }

    #[test]
    fn resolve_graph_built_at_head_is_none_outside_any_repo() {
        let g = resolve_graph(&no_git_root(), &[]);
        assert_eq!(g.built_at_head, None);
    }

    // --- alias short-circuit --------------------------------------------

    #[test]
    fn alias_wins_over_an_otherwise_ambiguous_simple_name() {
        // Two "Money" classes plus an alias pinning "Cash" to one of them.
        // A bare reference to the ALIAS NAME must resolve cleanly even
        // though "Money" itself would be globally ambiguous.
        let files = vec![
            (
                "A/Money.cs".to_string(),
                frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
            ),
            (
                "B/Money.cs".to_string(),
                frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
            ),
            (
                "C/Wallet.cs".to_string(),
                frag(
                    vec![def("C.Wallet", "Wallet", "C", "class")],
                    vec![FragUsing::Alias {
                        alias: "Cash".into(),
                        target: "A.Money".into(),
                        global: false,
                    }],
                    vec![type_ref("uses-type", "Cash", None, "C")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge =
            find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present");
        match edge {
            Edge::UsesType { to, .. } => assert_eq!(to, "A.Money"),
            _ => unreachable!(),
        }
        assert_eq!(g.stats.ambiguous_count, 0);
    }

    #[test]
    fn local_alias_shadows_a_same_named_global_alias() {
        let files = vec![
            (
                "A/Money.cs".to_string(),
                frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
            ),
            (
                "B/Money.cs".to_string(),
                frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
            ),
            (
                "Globals.cs".to_string(),
                frag(
                    vec![],
                    vec![FragUsing::Alias {
                        alias: "Cash".into(),
                        target: "A.Money".into(),
                        global: true,
                    }],
                    vec![],
                ),
            ),
            (
                "C/Wallet.cs".to_string(),
                frag(
                    vec![def("C.Wallet", "Wallet", "C", "class")],
                    vec![FragUsing::Alias {
                        alias: "Cash".into(),
                        target: "B.Money".into(),
                        global: false,
                    }],
                    vec![type_ref("uses-type", "Cash", None, "C")],
                ),
            ),
            // A file with NO local override sees the global alias.
            (
                "D/Ledger.cs".to_string(),
                frag(
                    vec![def("D.Ledger", "Ledger", "D", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Cash", None, "D")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let shadowed = g
            .edges
            .iter()
            .find(|e| matches!(e, Edge::UsesType { from_file, .. } if from_file == "C/Wallet.cs"))
            .expect("shadowed edge present");
        let global = g
            .edges
            .iter()
            .find(|e| matches!(e, Edge::UsesType { from_file, .. } if from_file == "D/Ledger.cs"))
            .expect("global edge present");
        match (shadowed, global) {
            (
                Edge::UsesType {
                    to: shadowed_to, ..
                },
                Edge::UsesType { to: global_to, .. },
            ) => {
                assert_eq!(
                    shadowed_to, "B.Money",
                    "local alias must win over the global one"
                );
                assert_eq!(
                    global_to, "A.Money",
                    "no local override -- global alias applies"
                );
            }
            _ => unreachable!(),
        }
    }

    // --- ambiguous marking (never guess) ---------------------------------

    #[test]
    fn ambiguous_via_using_step_stops_before_reaching_global_uniqueness() {
        let files = vec![
            (
                "A/Money.cs".to_string(),
                frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
            ),
            (
                "B/Money.cs".to_string(),
                frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
            ),
            (
                "C/Statement.cs".to_string(),
                frag(
                    vec![def("C.Statement", "Statement", "C", "class")],
                    vec![
                        FragUsing::Plain {
                            text: "A".into(),
                            global: false,
                        },
                        FragUsing::Plain {
                            text: "B".into(),
                            global: false,
                        },
                    ],
                    vec![type_ref("uses-type", "Money", None, "C")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.ambiguous_count, 1);
        let edge = find_edge(&g, |e| matches!(e, Edge::Ambiguous { .. })).unwrap();
        match edge {
            Edge::Ambiguous {
                candidate_count,
                candidates,
                raw,
                ..
            } => {
                assert_eq!(*candidate_count, 2);
                assert_eq!(raw, "Money");
                assert_eq!(
                    candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                    vec!["A.Money", "B.Money"]
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn ambiguous_via_global_uniqueness_step_when_no_usings_apply() {
        let files = vec![
            (
                "A/Money.cs".to_string(),
                frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
            ),
            (
                "B/Money.cs".to_string(),
                frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
            ),
            (
                "C/Report.cs".to_string(),
                frag(
                    vec![def("C.Report", "Report", "C", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Money", None, "C")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.ambiguous_count, 1);
        assert_eq!(g.stats.unresolved_external_count, 0);
    }

    #[test]
    fn ambiguous_candidates_are_capped_at_five_sorted_by_id() {
        let mut files = Vec::new();
        for letter in ["E", "D", "C", "B", "A", "F", "G"] {
            files.push((
                format!("{letter}/Widget.cs"),
                frag(
                    vec![def(&format!("{letter}.Widget"), "Widget", letter, "class")],
                    vec![],
                    vec![],
                ),
            ));
        }
        files.push((
            "Z/Probe.cs".to_string(),
            frag(
                vec![def("Z.Probe", "Probe", "Z", "class")],
                vec![],
                vec![type_ref("uses-type", "Widget", None, "Z")],
            ),
        ));
        let g = resolve_graph(&no_git_root(), &files);
        let edge = g
            .edges
            .iter()
            .find(|e| matches!(e, Edge::Ambiguous { .. }))
            .unwrap();
        match edge {
            Edge::Ambiguous {
                candidate_count,
                candidates,
                ..
            } => {
                assert_eq!(*candidate_count, 7);
                assert_eq!(candidates.len(), 5, "capped at AMBIGUOUS_CAP");
                let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
                let mut sorted = ids.clone();
                sorted.sort();
                assert_eq!(ids, sorted, "candidates must be sorted by id");
            }
            _ => unreachable!(),
        }
    }

    // --- enum-member asymmetry (the load-bearing case) --------------------

    #[test]
    fn enum_member_id_is_reachable_via_qualified_name_to_def() {
        let files = vec![(
            "A/Status.cs".to_string(),
            frag(
                vec![
                    def("A.Status", "Status", "A", "enum"),
                    def("A.Status.Active", "Active", "A", "enum-member"),
                ],
                vec![],
                vec![],
            ),
        )];
        let index = build_def_index(&files);
        assert!(index.qualified_name_to_def.contains_key("A.Status.Active"));
    }

    #[test]
    fn enum_member_does_not_collide_with_a_same_named_class_via_global_uniqueness() {
        // Regression trap: if enum members were NOT excluded from
        // simple_name_to_defs, "Active" would have two candidates (the
        // class AND the enum member) and this reference would incorrectly
        // come back ambiguous instead of resolving to the class.
        let files = vec![
            (
                "A/Status.cs".to_string(),
                frag(
                    vec![
                        def("A.Status", "Status", "A", "enum"),
                        def("A.Status.Active", "Active", "A", "enum-member"),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "B/Active.cs".to_string(),
                frag(
                    vec![def("B.Active", "Active", "B", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "C/Toggle.cs".to_string(),
                frag(
                    vec![def("C.Toggle", "Toggle", "C", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Active", None, "C")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesType { .. }))
            .expect("must resolve cleanly, not go ambiguous");
        match edge {
            Edge::UsesType { to, .. } => assert_eq!(to, "B.Active"),
            _ => unreachable!(),
        }
        assert_eq!(g.stats.ambiguous_count, 0);
    }

    #[test]
    fn uses_member_edge_resolves_only_when_qualifier_is_an_enum() {
        let files = vec![
            (
                "A/Status.cs".to_string(),
                frag(
                    vec![
                        def("A.Status", "Status", "A", "enum"),
                        def("A.Status.Active", "Active", "A", "enum-member"),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "B/Bundle.cs".to_string(),
                frag(
                    vec![def("B.Bundle", "Bundle", "B", "class")],
                    vec![],
                    vec![member_ref("Status", None, "Active", "A")], // same namespace as the enum
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("uses-member edge present");
        match edge {
            Edge::UsesMember { to, to_file, .. } => {
                assert_eq!(to, "A.Status.Active");
                assert_eq!(to_file, "A/Status.cs");
            }
            _ => unreachable!(),
        }
        assert_eq!(g.stats.edges_by_kind.uses_member, 1);
    }

    #[test]
    fn uses_member_on_a_non_enum_qualifier_is_dropped_silently() {
        let files = vec![
            (
                "A/Constants.cs".to_string(),
                frag(
                    vec![def("A.Constants", "Constants", "A", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "B/View.cs".to_string(),
                frag(
                    vec![def("B.View", "View", "B", "class")],
                    vec![FragUsing::Plain {
                        text: "A".into(),
                        global: false,
                    }],
                    vec![member_ref("Constants", None, "MaxRetries", "B")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(g
            .edges
            .iter()
            .all(|e| !matches!(e, Edge::UsesMember { .. })));
        // Silently dropped -- not counted as ambiguous or external.
        assert_eq!(g.stats.ambiguous_count, 0);
        assert_eq!(g.stats.unresolved_external_count, 0);
    }

    // --- multi-part (qualified) member-access qualifiers ---

    #[test]
    fn qualified_multipart_member_qualifier_resolves_via_exact_fqn_ladder_step() {
        let files = vec![
            (
                "Enums/MyEnum.cs".to_string(),
                frag(
                    vec![
                        def("Some.Namespace.MyEnum", "MyEnum", "Some.Namespace", "enum"),
                        def(
                            "Some.Namespace.MyEnum.Member",
                            "Member",
                            "Some.Namespace",
                            "enum-member",
                        ),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Reader.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.Reader",
                        "Reader",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![member_ref(
                        "MyEnum",
                        Some("Some.Namespace.MyEnum"),
                        "Member",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("uses-member edge present");
        match edge {
            Edge::UsesMember { to, to_file, .. } => {
                assert_eq!(to, "Some.Namespace.MyEnum.Member");
                assert_eq!(to_file, "Enums/MyEnum.cs");
            }
            _ => unreachable!(),
        }
        assert_eq!(g.stats.edges_by_kind.uses_member, 1);
    }

    #[test]
    fn namespace_alias_qualified_member_ref_resolves_only_via_global_uniqueness_not_a_genuine_alias_walk(
    ) {
        // Resolution-ladder subtlety: step 0 (the alias short-circuit) only
        // ever fires for a BARE, non-dotted ref. "Ns.MyEnum" is dotted the
        // moment it has 2+ segments, so "Ns" is never looked up in the alias
        // map -- this resolves purely because "MyEnum" happens to be
        // globally unique (step 4), not genuine alias resolution.
        let files = vec![
            (
                "Enums/MyEnum.cs".to_string(),
                frag(
                    vec![
                        def("Some.Namespace.MyEnum", "MyEnum", "Some.Namespace", "enum"),
                        def(
                            "Some.Namespace.MyEnum.Member",
                            "Member",
                            "Some.Namespace",
                            "enum-member",
                        ),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/AliasNsUser.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.AliasNsUser",
                        "AliasNsUser",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Alias {
                        alias: "Ns".into(),
                        target: "Some.Namespace".into(),
                        global: false,
                    }],
                    vec![member_ref(
                        "MyEnum",
                        Some("Ns.MyEnum"),
                        "Member",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("resolves via step-4 global uniqueness of MyEnum, not the Ns alias");
        match edge {
            Edge::UsesMember { to, .. } => assert_eq!(to, "Some.Namespace.MyEnum.Member"),
            _ => unreachable!(),
        }
    }

    // --- non-enum emission tiers ---

    #[test]
    fn dotted_exact_qualified_member_access_to_a_static_class_emits_uses_member_edge() {
        let files = vec![
            (
                "Other/Utils.cs".to_string(),
                frag(
                    vec![def("App.Other.Utils", "Utils", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UsesNonEnum.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UsesNonEnum",
                        "UsesNonEnum",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![member_ref(
                        "Utils",
                        Some("App.Other.Utils"),
                        "MaxRetries",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("exact-qualified static access emits");
        match edge {
            Edge::UsesMember { to, .. } => assert_eq!(
                to, "App.Other.Utils",
                "targets the type def -- member defs exist only for enums"
            ),
            _ => unreachable!(),
        }
        assert_eq!(g.stats.edges_by_kind.uses_member, 1);
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "uses-member misses must never be reported as ambiguous"
        );
        assert_eq!(g.stats.unresolved_external_count, 0);
    }

    #[test]
    fn bare_qualifier_to_a_class_emits_only_when_the_member_is_a_recorded_method() {
        let urn = def_with(
            "App.Other.MessageUrn",
            "MessageUrn",
            "App.Other",
            "class",
            &["ForType"],
            &[],
            &[],
        );
        let files = vec![
            (
                "Other/MessageUrn.cs".to_string(),
                frag(vec![urn], vec![], vec![]),
            ),
            (
                "Consumers/CallsStatic.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.CallsStatic",
                        "CallsStatic",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![
                        member_ref("MessageUrn", None, "ForType", "App.Consumers"),
                        member_ref("MessageUrn", None, "SomeUnknownField", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 1,
            "the method call emits; the unknown-member access does not (could be a same-named property)"
        );
        match find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).unwrap() {
            Edge::UsesMember { to, .. } => assert_eq!(to, "App.Other.MessageUrn"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn generic_qualifier_emits_because_type_argument_syntax_cannot_be_a_local_or_property() {
        let mut generic_ref = member_ref("TypeCache", None, "Cached", "App.Consumers");
        generic_ref.generic = true;
        let files = vec![
            (
                "Other/TypeCache.cs".to_string(),
                frag(
                    vec![def(
                        "App.Other.TypeCache",
                        "TypeCache",
                        "App.Other",
                        "class",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UsesCache.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UsesCache",
                        "UsesCache",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![generic_ref],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 1,
            "a generic qualifier is type-certain even for a property member"
        );
        match find_edge(&g, |e| matches!(e, Edge::UsesMember { .. })).unwrap() {
            Edge::UsesMember { to, .. } => assert_eq!(to, "App.Other.TypeCache"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn dotted_chain_with_inherited_generic_flag_does_not_emit_via_tail_name_match() {
        // "EqualityComparer<TSaga>.Default.GetHashCode(...)" shape: the
        // flattened qualifier "EqualityComparer.Default" carries generic=true
        // from its inner segment; its TAIL name "Default" happens to match a
        // real type. Gate-audit regression: no edge.
        let mut chain_ref = member_ref(
            "Default",
            Some("EqualityComparer.Default"),
            "GetHashCode",
            "App.Consumers",
        );
        chain_ref.generic = true;
        let files = vec![
            (
                "Other/Default.cs".to_string(),
                frag(
                    vec![def("App.Other.Default", "Default", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Chain.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.Chain",
                        "Chain",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![chain_ref],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            g.edges
                .iter()
                .all(|e| !matches!(e, Edge::UsesMember { .. })),
            "chain-tail name match must not emit"
        );
    }

    #[test]
    fn bare_non_method_member_on_a_non_enum_qualifier_is_still_dropped_silently() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def("App.Other.Widget", "Widget", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/PropLike.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.PropLike",
                        "PropLike",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![member_ref("Widget", None, "Name", "App.Consumers")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            g.edges
                .iter()
                .all(|e| !matches!(e, Edge::UsesMember { .. })),
            "no certainty signal -> no edge (could be an instance property named Widget)"
        );
        assert_eq!(g.stats.ambiguous_count, 0);
        assert_eq!(g.stats.unresolved_external_count, 0);
    }

    #[test]
    fn nested_enum_dotted_qualifier_resolves_via_global_uniqueness_not_the_plus_joined_id() {
        // The "+"-joined nested-type id ("App.Widgets.Outer+Inner") never
        // matches the literal dotted source text ("Outer.Inner") at ladder
        // step 1 -- same as an ordinary nested TYPE reference, this only
        // resolves via step 4 (globally unique simple name "Inner").
        let files = vec![
            (
                "Enums/Container.cs".to_string(),
                frag(
                    vec![
                        def("App.Widgets.Outer", "Outer", "App.Widgets", "class"),
                        def("App.Widgets.Outer+Inner", "Inner", "App.Widgets", "enum"),
                        def(
                            "App.Widgets.Outer+Inner.On",
                            "On",
                            "App.Widgets",
                            "enum-member",
                        ),
                    ],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/NestedUser.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.NestedUser",
                        "NestedUser",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![member_ref(
                        "Inner",
                        Some("Outer.Inner"),
                        "On",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("uses-member edge present");
        match edge {
            Edge::UsesMember { to, to_file, .. } => {
                assert_eq!(to, "App.Widgets.Outer+Inner.On");
                assert_eq!(to_file, "Enums/Container.cs");
            }
            _ => unreachable!(),
        }
    }

    // --- declaration_expression -> uses-type ref, same ladder ---

    #[test]
    fn declaration_expression_type_ref_resolves_through_the_normal_uses_type_ladder() {
        let files = vec![
            (
                "Enums/PostType.cs".to_string(),
                frag(
                    vec![def("App.Enums.PostType", "PostType", "App.Enums", "enum")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/OutUser.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.OutUser",
                        "OutUser",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Enums".into(),
                        global: false,
                    }],
                    vec![type_ref("uses-type", "PostType", None, "App.Consumers")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge =
            find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present");
        match edge {
            Edge::UsesType { to, .. } => assert_eq!(to, "App.Enums.PostType"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn declaration_expression_type_ref_with_ambiguous_simple_name_is_marked_ambiguous() {
        let files = vec![
            (
                "A/Status.cs".to_string(),
                frag(
                    vec![def("A.Status", "Status", "A", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "B/Status.cs".to_string(),
                frag(
                    vec![def("B.Status", "Status", "B", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/AmbiguousOutUser.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.AmbiguousOutUser",
                        "AmbiguousOutUser",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![type_ref("uses-type", "Status", None, "App.Consumers")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge =
            find_edge(&g, |e| matches!(e, Edge::Ambiguous { .. })).expect("ambiguous edge present");
        match edge {
            Edge::Ambiguous {
                origin,
                raw,
                candidate_count,
                ..
            } => {
                assert_eq!(origin, "uses-type");
                assert_eq!(raw, "Status");
                assert_eq!(*candidate_count, 2);
            }
            _ => unreachable!(),
        }
    }

    // --- qualified (dotted) resolution: enclosing-namespace walk ----------

    #[test]
    fn qualified_reference_resolves_at_an_outer_enclosing_namespace_prefix() {
        // From within `Fixtures.Billing` (2 segments), a reference to
        // `Common.IIdentifiable` must find `Fixtures.Common.IIdentifiable`
        // by trying prefix "Fixtures" (outer), after "Fixtures.Billing"
        // (innermost) fails -- exercises the walk, not just a literal or
        // innermost-only match.
        let files = vec![
            (
                "Common/IIdentifiable.cs".to_string(),
                frag(
                    vec![def(
                        "Fixtures.Common.IIdentifiable",
                        "IIdentifiable",
                        "Fixtures.Common",
                        "interface",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Billing/Invoice.cs".to_string(),
                frag(
                    vec![def(
                        "Fixtures.Billing.Invoice",
                        "Invoice",
                        "Fixtures.Billing",
                        "class",
                    )],
                    vec![],
                    vec![type_ref(
                        "inherits",
                        "IIdentifiable",
                        Some("Common.IIdentifiable"),
                        "Fixtures.Billing",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::Inherits { .. }))
            .expect("resolved via namespace walk");
        match edge {
            Edge::Inherits { to, .. } => assert_eq!(to, "Fixtures.Common.IIdentifiable"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn qualified_reference_with_no_matching_prefix_and_no_literal_match_is_external() {
        let files = vec![(
            "A/Probe.cs".to_string(),
            frag(
                vec![def("A.Probe", "Probe", "A", "class")],
                vec![],
                vec![type_ref("uses-type", "Y", Some("X.Y"), "A")],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.unresolved_external_count, 1);
    }

    // --- namespace-proximity (step 3, exact match, not a walk) ------------

    #[test]
    fn same_namespace_reference_resolves_without_any_using() {
        let files = vec![
            (
                "A/Widget.cs".to_string(),
                frag(
                    vec![def("A.Widget", "Widget", "A", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "A/Holder.cs".to_string(),
                frag(
                    vec![def("A.Holder", "Holder", "A", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Widget", None, "A")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.ambiguous_count, 0);
        assert_eq!(g.stats.unresolved_external_count, 0);
        assert_eq!(g.stats.edges_by_kind.uses_type, 1);
    }

    #[test]
    fn empty_namespace_is_treated_as_absent_not_as_a_matchable_prefix() {
        // A file-scope (no enclosing namespace) reference to another
        // file-scope type must resolve via step 4 (global uniqueness), NOT
        // step 3 -- an empty-string ns must behave as absent, not as a
        // matchable prefix.
        let files = vec![
            (
                "Root.cs".to_string(),
                frag(vec![def("Anchor", "Anchor", "", "class")], vec![], vec![]),
            ),
            (
                "Probe.cs".to_string(),
                frag(
                    vec![def("Probe", "Probe", "", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Anchor", None, "")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.ambiguous_count, 0);
        assert_eq!(g.stats.edges_by_kind.uses_type, 1);
    }

    // --- global using -----------------------------------------------------

    #[test]
    fn global_using_resolves_a_bare_name_from_an_unrelated_namespace() {
        let files = vec![
            (
                "Catalog/Status.cs".to_string(),
                frag(
                    vec![def("Catalog.Status", "Status", "Catalog", "enum")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Globals.cs".to_string(),
                frag(
                    vec![],
                    vec![FragUsing::Plain {
                        text: "Catalog".into(),
                        global: true,
                    }],
                    vec![],
                ),
            ),
            (
                "Ops/View.cs".to_string(),
                frag(
                    vec![def("Ops.View", "View", "Ops", "class")],
                    vec![],
                    vec![type_ref("uses-type", "Status", None, "Ops")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesType { .. }))
            .expect("resolved via global using");
        match edge {
            Edge::UsesType { to, .. } => assert_eq!(to, "Catalog.Status"),
            _ => unreachable!(),
        }
    }

    // --- tier (a) widened: static property / field ------------------------

    // The uses-member edge set is split in two. Almost every assertion in this
    // module is about the PRECISE half, so the two default accessors below
    // filter to it and the `heuristic_*` counterparts are what a heuristic-tier
    // test reaches for -- rather than filtering `heuristic` inline forty times.
    fn member_edge_targets(g: &Graph) -> Vec<&str> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    to,
                    heuristic: false,
                    ..
                } => Some(to.as_str()),
                _ => None,
            })
            .collect()
    }

    fn heuristic_member_edge_targets(g: &Graph) -> Vec<&str> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    to,
                    heuristic: true,
                    ..
                } => Some(to.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn stage2_tier_a_static_property_access_on_a_bare_qualifier_now_emits() {
        // The MessageUrn.Prefix shape: same namespace as the def, so the
        // qualifier answers at the namespace ladder step -- no using, no
        // type-argument list, no dotted qualifier. Such a bare qualifier
        // carries no certainty signal on its own.
        let files = vec![
            (
                "Other/MessageUrn.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Consumers.MessageUrn",
                        "MessageUrn",
                        "App.Consumers",
                        "class",
                        &[],
                        &["Prefix"],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UsesProperty.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UsesProperty",
                        "UsesProperty",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![
                        member_ref("MessageUrn", None, "Prefix", "App.Consumers"),
                        member_ref("MessageUrn", None, "NotDeclared", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edge_targets(&g),
            vec!["App.Consumers.MessageUrn"],
            "the declared property emits; the undeclared member still does not"
        );
    }

    #[test]
    fn stage2_tier_a_const_field_access_on_a_bare_qualifier_emits() {
        let files = vec![
            (
                "Other/Limits.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Limits",
                        "Limits",
                        "App.Other",
                        "class",
                        &[],
                        &[],
                        &["MaxRetries"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UsesField.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UsesField",
                        "UsesField",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![member_ref("Limits", None, "MaxRetries", "App.Consumers")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(member_edge_targets(&g), vec!["App.Other.Limits"]);
    }

    #[test]
    fn stage2_tier_a_partial_class_contributes_its_own_properties_and_fields() {
        let files = vec![
            (
                "Other/Config.Part1.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Config",
                        "Config",
                        "App.Other",
                        "class",
                        &[],
                        &[],
                        &["Retries"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Config.Part2.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Config",
                        "Config",
                        "App.Other",
                        "class",
                        &[],
                        &["Name"],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UsesBoth.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UsesBoth",
                        "UsesBoth",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![
                        member_ref("Config", None, "Retries", "App.Consumers"),
                        member_ref("Config", None, "Name", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edge_targets(&g),
            vec!["App.Other.Config", "App.Other.Config"],
            "both halves of the partial class vouch for their own member"
        );
    }

    // --- tier (e): instance receivers ---

    #[test]
    fn stage2_tier_e_a_declared_local_receiver_resolves_through_the_ladder() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/LocalReceiver.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.LocalReceiver",
                        "LocalReceiver",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![receiver_ref(
                        "w",
                        "Render",
                        "App.Consumers",
                        "Widget",
                        Some(0),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("uses-member edge present");
        match edge {
            Edge::UsesMember { to, to_file, .. } => {
                assert_eq!(to, "App.Other.Widget");
                assert_eq!(to_file, "Other/Widget.cs");
            }
            _ => unreachable!(),
        }
        assert_eq!(g.stats.edges_by_kind.uses_member, 1);
    }

    #[test]
    fn stage2_tier_e_a_receiver_whose_member_lives_in_the_property_list_also_emits() {
        // Tier (e) reuses the SAME widened membership test as tier (a) --
        // methods ∪ properties ∪ fields, not methods alone.
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &[],
                        &["Name"],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/PropReceiver.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.PropReceiver",
                        "PropReceiver",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![receiver_ref("w", "Name", "App.Consumers", "Widget", None)],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
    }

    #[test]
    fn stage2_tier_e_a_receiver_type_reached_only_through_a_type_alias_resolves_too() {
        let files = vec![
            (
                "One/Item.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.One.Item",
                        "Item",
                        "App.One",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Two/Item.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Two.Item",
                        "Item",
                        "App.Two",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/AliasReceiver.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.AliasReceiver",
                        "AliasReceiver",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Alias {
                        alias: "AliasedItem".into(),
                        target: "App.Two.Item".into(),
                        global: false,
                    }],
                    vec![receiver_ref(
                        "item",
                        "Go",
                        "App.Consumers",
                        "AliasedItem",
                        Some(0),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edge_targets(&g),
            vec!["App.Two.Item"],
            "the alias pins the receiver type even though the simple name \"Item\" is ambiguous"
        );
    }

    #[test]
    fn stage2_tier_e_a_receiver_whose_type_does_not_declare_the_member_earns_no_edge() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/UnknownMember.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.UnknownMember",
                        "UnknownMember",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![receiver_ref(
                        "w",
                        "Explode",
                        "App.Consumers",
                        "Widget",
                        Some(0),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edge_targets(&g).is_empty(),
            "stage 3/4 territory, not an edge"
        );
    }

    #[test]
    fn stage2_tier_e_an_ambiguous_receiver_type_earns_no_edge_and_no_ambiguous_noise() {
        let files = vec![
            (
                "One/Handler.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.One.Handler",
                        "Handler",
                        "App.One",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Two/Handler.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Two.Handler",
                        "Handler",
                        "App.Two",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/AmbiguousReceiver.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.AmbiguousReceiver",
                        "AmbiguousReceiver",
                        "App.Consumers",
                        "class",
                    )],
                    vec![],
                    vec![receiver_ref("h", "Go", "App.Consumers", "Handler", Some(0))],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edge_targets(&g).is_empty(),
            "never pick a winner between two same-named receiver types"
        );
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "a uses-member miss is still never reported as ambiguous"
        );
        assert_eq!(g.stats.unresolved_external_count, 0);
        // Refusing to PICK is not the same as having nothing to say. Both
        // candidates declare Go, so both are named as guesses -- the
        // strong scored case, where the right answer is provably one of the
        // two. Neither is same-namespace and the file has no usings, so both
        // score 1 and the def id breaks the tie.
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.One.Handler", "App.Two.Handler"]
        );
        assert_eq!(g.stats.heuristic_edge_count, 2);
    }

    #[test]
    fn stage2_tier_e_an_unresolvable_receiver_type_earns_no_edge() {
        let files = vec![(
            "Consumers/ExternalReceiver.cs".to_string(),
            frag(
                vec![def(
                    "App.Consumers.ExternalReceiver",
                    "ExternalReceiver",
                    "App.Consumers",
                    "class",
                )],
                vec![],
                vec![receiver_ref(
                    "s",
                    "Trim",
                    "App.Consumers",
                    "StringBuilder",
                    Some(0),
                )],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(member_edge_targets(&g).is_empty());
        assert_eq!(
            g.stats.unresolved_external_count, 0,
            "a uses-member miss is never counted as external either"
        );
    }

    #[test]
    fn stage2_tier_e_a_ref_with_no_fact_at_all_is_untouched_by_the_new_tier() {
        // The extraction-side negatives (predefined-type receiver, conflicting
        // duplicate locals, var-from-call) all arrive here as the SAME thing:
        // a member ref with no receiverType. One resolver-side pin covers the
        // whole family -- their extraction-side halves are pinned in
        // extract.rs's own stage2b tests.
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/NoFact.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.NoFact",
                        "NoFact",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![member_ref("widget", None, "Render", "App.Consumers")],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edge_targets(&g).is_empty(),
            "no fact was recorded, so there is nothing to resolve"
        );
        // With no fact of any kind the qualifier `widget` resolves to nothing
        // at all, which is the only door into the scored
        // tier's uniqueness fallback -- and Widget is the one def graph-wide
        // declaring `Render`, so it is named as a GUESS. Pinned deliberately:
        // this is the weakest evidence the resolver acts on, it is exactly why
        // the fallback is capped and tagged rather than emitted as fact, and it
        // must never leak into the precise set above.
        assert_eq!(heuristic_member_edge_targets(&g), vec!["App.Other.Widget"]);
    }

    #[test]
    fn stage2_tier_e_shadowing_is_settled_at_extraction_so_the_edge_follows_the_recorded_fact() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Gadget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Gadget",
                        "Gadget",
                        "App.Other",
                        "class",
                        &["Go"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/ShadowReceiver.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.ShadowReceiver",
                        "ShadowReceiver",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    // The parameter shadows the same-named field, so the
                    // extractor recorded Gadget (see extract.rs's own test).
                    vec![receiver_ref(
                        "handler",
                        "Go",
                        "App.Consumers",
                        "Gadget",
                        Some(0),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edge_targets(&g),
            vec!["App.Other.Gadget"],
            "the innermost declaration wins"
        );
    }

    #[test]
    fn stage2_tier_e_never_adds_a_second_edge_for_a_ref_an_earlier_tier_already_claimed() {
        // `public void Run(Widget Widget) => Widget.Render();` -- the
        // qualifier resolves as a TYPE (tier (a)) AND carries a receiver fact.
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/OneEdge.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.OneEdge",
                        "OneEdge",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![receiver_ref(
                        "Widget",
                        "Render",
                        "App.Consumers",
                        "Widget",
                        Some(0),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 1,
            "still exactly one edge"
        );
        assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
    }

    #[test]
    fn stage2_tier_e_never_fires_for_a_dotted_chain_tail_because_it_carries_no_fact() {
        // Chain-tail regression, resolver half: "w.Inner.Tail()" flattens to a
        // DOTTED qualifier, which the extractor's bare-only guard refuses a
        // fact for. Even though Widget declares Tail, no edge is earned here.
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Tail"],
                        &["Inner"],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Chain.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.Chain",
                        "Chain",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![
                        receiver_ref("w", "Inner", "App.Consumers", "Widget", None),
                        member_ref("Inner", Some("w.Inner"), "Tail", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edge_targets(&g),
            vec!["App.Other.Widget"],
            "only the head access earns a PRECISE edge"
        );
        // The tail's dotted qualifier ("w.Inner") resolves to nothing at all,
        // so the uniqueness fallback reaches it and -- Widget
        // being the only def declaring `Tail` -- names Widget as a GUESS. That
        // is the opposite of the bug this test pins: the tail may never inherit
        // the head's fact and emit a PRECISE edge, but it is allowed to be
        // guessed at by name, tagged, from far weaker evidence.
        assert_eq!(heuristic_member_edge_targets(&g), vec!["App.Other.Widget"]);
    }

    // --- tier (e) end-to-end: real C# through extract -> resolve ---
    //
    // The tier tests above hand-build fragments, which pins the RESOLVER in
    // isolation but takes the extractor's word for what it records. These four
    // run real fixtures through this crate's own extractor, so a fact that never
    // gets recorded (or gets recorded on the wrong line) fails here rather than
    // passing vacuously.

    fn fragments_for(files: &[(&str, &str)]) -> Vec<(String, Fragment)> {
        files
            .iter()
            .map(|(rel, src)| {
                (
                    (*rel).to_string(),
                    crate::graph::fragment_from_extraction(&crate::extract::extract(src)),
                )
            })
            .collect()
    }

    fn member_edges_from<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, usize)> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    from_line,
                    to,
                    heuristic: false,
                    ..
                } if from_file == from => Some((to.as_str(), *from_line)),
                _ => None,
            })
            .collect()
    }

    /// The guessed half of the same file's uses-member edges, in emission order
    /// (which for the scored tier IS scored order). Resolved TYPE-reference
    /// targets out of one file, in edge order -- the ladder-walk tests assert on
    /// these the way the member tests assert on `member_edges_from`.
    fn type_edge_targets_from<'a>(g: &'a Graph, from: &str) -> Vec<&'a str> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesType { from_file, to, .. } if from_file == from => Some(to.as_str()),
                _ => None,
            })
            .collect()
    }

    fn heuristic_member_edges_from<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, usize)> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    from_line,
                    to,
                    heuristic: true,
                    ..
                } if from_file == from => Some((to.as_str(), *from_line)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn stage2_end_to_end_static_property_access_emits_and_an_undeclared_member_does_not() {
        let files = fragments_for(&[
            (
                "Other/MessageUrn.cs",
                "namespace App.Consumers { public static class MessageUrn { public static string Prefix { get; } } }",
            ),
            (
                "Consumers/UsesProperty.cs",
                "\nnamespace App.Consumers;\n\npublic class UsesProperty\n{\n  public object Get() => MessageUrn.Prefix;\n  public object Miss() => MessageUrn.NotDeclared;\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/UsesProperty.cs"),
            vec![("App.Consumers.MessageUrn", 6)],
            "the declared property emits at its own line; the undeclared member does not"
        );
    }

    #[test]
    fn stage2_end_to_end_a_declared_local_receiver_earns_an_edge_at_the_access_line() {
        let files = fragments_for(&[
            ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
            (
                "Consumers/LocalReceiver.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class LocalReceiver\n{\n  public void Run()\n  {\n    Widget w = new Widget();\n    w.Render();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/LocalReceiver.cs"),
            vec![("App.Other.Widget", 11)]
        );
    }

    #[test]
    fn stage2_end_to_end_a_class_field_receiver_earns_an_edge_the_ctor_injection_shape() {
        let files = fragments_for(&[
            ("Other/IRepo.cs", "namespace App.Other { public interface IRepo { void Save(); } }"),
            (
                "Consumers/FieldReceiver.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class FieldReceiver\n{\n  private readonly IRepo _repo;\n\n  public FieldReceiver(IRepo repo) { _repo = repo; }\n\n  public void Run() { _repo.Save(); }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/FieldReceiver.cs"),
            vec![("App.Other.IRepo", 12)],
            "the field access in Run, not the constructor assignment"
        );
    }

    #[test]
    fn stage2_end_to_end_a_chain_tail_earns_no_edge_from_a_fact_it_did_not_inherit() {
        // Widget declares BOTH Inner and Tail, so if the flattened tail
        // ("w.Inner") had inherited the head's receiverType it would have
        // produced a second, wrong edge. Stage-1 chain-tail regression class.
        let files = fragments_for(&[
            (
                "Other/Widget.cs",
                "namespace App.Other { public class Widget { public object Inner { get; } public void Tail() { } } }",
            ),
            (
                "Consumers/Chain.cs",
                "using App.Other;\n\nnamespace App.Consumers;\n\npublic class Chain\n{\n  public void Run()\n  {\n    Widget w = new Widget();\n    w.Inner.Tail();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Chain.cs"),
            vec![("App.Other.Widget", 10)],
            "only the head access (\"w.Inner\") earns a PRECISE edge, and it is the head's line, not the tail's"
        );
        // End-to-end half of the same split: the tail is guessed at by
        // member-name uniqueness, tagged, on the same line.
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Chain.cs"),
            vec![("App.Other.Widget", 10)],
            "exactly one guess -- the tail; the head already has its fact and is never second-guessed"
        );
    }

    // --- tier (f): extension methods ---
    //
    // Real fixtures run through this crate's own extractor, so an extension fact
    // that never gets recorded fails here rather than passing vacuously.

    const WIDGET_SRC: (&str, &str) = (
        "Other/Widget.cs",
        "namespace App.Other { public class Widget { } }",
    );
    const WIDGET_EXTENSIONS_SRC: (&str, &str) = (
        "Ext/WidgetExtensions.cs",
        "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
    );

    #[test]
    fn stage3_tier_f_an_extension_call_resolves_to_the_static_class_when_its_namespace_is_imported()
    {
        let files = fragments_for(&[
            WIDGET_SRC,
            WIDGET_EXTENSIONS_SRC,
            (
                "Consumers/UsesExtension.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class UsesExtension\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/UsesExtension.cs"),
            vec![("App.Ext.WidgetExtensions", 9)],
            "Widget does not declare Render -- only the extension tier can claim this call, and the edge targets the DECLARING static class"
        );
        let edge = find_edge(&g, |e| matches!(e, Edge::UsesMember { .. }))
            .expect("uses-member edge present");
        match edge {
            // Tier (f) is a HEURISTIC tier: it emits exactly this one edge, and
            // the edge declares itself a guess, because the instance-member veto
            // that would disprove it cannot see members of an out-of-graph
            // receiver and never will without a build.
            Edge::UsesMember {
                to_file, heuristic, ..
            } => {
                assert_eq!(to_file, "Ext/WidgetExtensions.cs");
                assert!(*heuristic, "tier (f) emits heuristic edges");
            }
            _ => unreachable!(),
        }
        assert_eq!(
            serde_json::to_string(edge).unwrap(),
            r#"{"kind":"uses-member","from_file":"Consumers/UsesExtension.cs","from_line":9,"to":"App.Ext.WidgetExtensions","to_file":"Ext/WidgetExtensions.cs","heuristic":true}"#,
            "heuristic is appended LAST -- the exact field order Node's own byte assertion pins"
        );
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 0,
            "edges_by_kind counts PRECISE edges only, so a heuristic tier cannot inflate it"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 1,
            "guesses are counted in their own stat instead"
        );
    }

    #[test]
    fn stage3_tier_f_an_extension_class_in_the_refs_own_namespace_is_admitted_with_no_using_at_all()
    {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Consumers/WidgetExtensions.cs",
                "namespace App.Consumers { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
            ),
            (
                "Consumers/SameNamespace.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class SameNamespace\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/SameNamespace.cs"),
            vec![("App.Consumers.WidgetExtensions", 8)]
        );
        assert!(
            member_edges_from(&g, "Consumers/SameNamespace.cs").is_empty(),
            "admission by own-namespace is still tier (f), so still a guess"
        );
    }

    #[test]
    fn stage3_tier_f_an_extension_class_whose_namespace_is_not_imported_earns_no_edge() {
        // Deliberately no `using App.Ext;` -- in real C# this file would not
        // compile, and the resolver must not paper over that with a name match.
        let files = fragments_for(&[
            WIDGET_SRC,
            WIDGET_EXTENSIONS_SRC,
            (
                "Consumers/NoUsing.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class NoUsing\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/NoUsing.cs").is_empty(),
            "visibility is the admission rule -- an unimported extension class is not a candidate"
        );
        assert!(
            !g.edges
                .iter()
                .any(|e| matches!(e, Edge::Ambiguous { origin, .. } if origin == "uses-member")),
            "a declined extension lookup is still never ambiguous noise"
        );
    }

    #[test]
    fn stage3_tier_f_two_admitted_candidates_earn_no_edge_and_no_ambiguous_increment() {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "ExtA/AExtensions.cs",
                "namespace App.ExtA { public static class AExtensions { public static void Render(this Widget w) { } } }",
            ),
            (
                "ExtB/BExtensions.cs",
                "namespace App.ExtB { public static class BExtensions { public static void Render(this Widget w) { } } }",
            ),
            (
                "Consumers/TwoCandidates.cs",
                "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class TwoCandidates\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/TwoCandidates.cs").is_empty(),
            "never pick a winner between two visible extension classes"
        );
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "a refused extension lookup does not touch the ambiguous stats"
        );
    }

    #[test]
    fn stage3_tier_f_an_extension_whose_this_type_differs_from_the_receiver_type_earns_no_edge() {
        let files = fragments_for(&[
            WIDGET_SRC,
            ("Other/Gadget.cs", "namespace App.Other { public class Gadget { } }"),
            (
                "Ext/GadgetExtensions.cs",
                "namespace App.Ext { public static class GadgetExtensions { public static void Render(this Gadget g) { } } }",
            ),
            (
                "Consumers/WrongReceiver.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class WrongReceiver\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/WrongReceiver.cs").is_empty(),
            "the method name matches but the this-type does not -- no edge"
        );
    }

    #[test]
    fn stage3_tier_f_an_instance_member_shadows_a_visible_extension_of_the_same_name() {
        // MUTATION-CRITICAL: this is what tier (e)'s `emitted = true` buys.
        // Drop that assignment and BOTH tiers claim the ref, producing two
        // edges -- the count assertion below is the one that catches it.
        let files = fragments_for(&[
            ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
            WIDGET_EXTENSIONS_SRC,
            (
                "Consumers/Shadowed.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Shadowed\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Shadowed.cs"),
            vec![("App.Other.Widget", 9)],
            "exactly one edge, and C#'s shadowing rule falls out of tier order: the instance member wins"
        );
        // Tier (e) is precise -- only tier (f) emits heuristic edges. And the
        // ref is claimed, so the scored tier never runs on it either: one ref,
        // one answer.
        assert!(heuristic_member_edges_from(&g, "Consumers/Shadowed.cs").is_empty());
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 1,
            "a precise edge still counts in edges_by_kind"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage3_tier_f_a_ref_with_no_receiver_type_never_enters_the_extension_tier() {
        let files = fragments_for(&[
            WIDGET_SRC,
            WIDGET_EXTENSIONS_SRC,
            (
                "Consumers/NoReceiverFact.cs",
                // A TYPE-name qualifier resolves to App.Other.Widget through
                // the ladder, but extension methods are instance-call syntax
                // only, so "Widget.Render()" must never be claimed here. And a
                // receiver the extractor refused to vouch for (var + a call)
                // carries no receiverType, so no lookup key exists at all.
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class NoReceiverFact\n{\n  public void Static() => Widget.Render();\n\n  public void Unknown()\n  {\n    var w = Compute();\n    w.Render();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/NoReceiverFact.cs").is_empty(),
            "neither shape carries a receiver fact, so neither can reach the extension tier"
        );
        // The scored tier draws the line between the two shapes tier (f)
        // treated alike. `Widget.Render()` RESOLVED -- a resolved qualifier is a fact
        // the precise tiers already judged, so the scored tier refuses to
        // second-guess it and emits nothing. `w.Render()` resolved to nothing
        // at all, which is the only door into the uniqueness fallback, and the
        // extension class is a candidate there because the fallback counts
        // extension-method names too (`member_vouched`, not `declares_member`).
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/NoReceiverFact.cs"),
            vec![("App.Ext.WidgetExtensions", 14)]
        );
    }

    #[test]
    fn stage3_tier_f_bound_this_type_matching_is_exact_so_a_base_class_param_never_claims_a_derived_receiver(
    ) {
        let files = fragments_for(&[
            ("Other/BaseWidget.cs", "namespace App.Other { public class BaseWidget { } }"),
            ("Other/Widget.cs", "namespace App.Other { public class Widget : BaseWidget { } }"),
            (
                "Ext/BaseExtensions.cs",
                "namespace App.Ext { public static class BaseExtensions { public static void Render(this BaseWidget b) { } } }",
            ),
            (
                "Consumers/Derived.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Derived\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/Derived.cs").is_empty(),
            "documented limitation: no inheritance walking and no interface widening -- real C# WOULD bind this, the resolver stays narrower rather than guessing"
        );
    }

    // --- tighten amendment: arity is part of the match ---
    //
    // Real fixtures run through this crate's own extractor, so an arity or
    // arg_count that never gets recorded fails here rather than passing
    // vacuously.

    #[test]
    fn stage3_tighten_regression_a_three_argument_call_never_binds_to_a_one_parameter_extension() {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Ext/WidgetExtensions.cs",
                "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
            ),
            (
                "Consumers/ArityMismatch.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class ArityMismatch\n{\n  public void Wrong(Widget w) => w.Render(1, 2, 3);\n  public void Right(Widget w) => w.Render(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/ArityMismatch.cs"),
            vec![("App.Ext.WidgetExtensions", 10)],
            "corpus audit round 1 found 3/20 wrong edges of exactly this shape: an arity-blind index let a 3-argument call (line 9) claim a 1-parameter extension, stealing the edge from the real instance method. Only the arity-MATCHED call on line 10 survives"
        );
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "the refused arity mismatch is silent, like every other uses-member miss"
        );
    }

    #[test]
    fn stage3_tighten_a_property_read_never_enters_the_extension_tier() {
        // A 0-arity extension is exactly what an argCount-blind tier would have
        // matched a property read against, since a property read has no
        // arguments to disagree about. It carries no argCount AT ALL, which is
        // the actual gate: an extension method is reachable through call syntax
        // only.
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Ext/SlugExtensions.cs",
                "namespace App.Ext { public static class SlugExtensions { public static string Slug(this Widget w) => \"s\"; } }",
            ),
            (
                "Consumers/PropertyRead.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class PropertyRead\n{\n  public string Read(Widget w) => w.Slug;\n}\n",
            ),
        ]);
        let consumer = &files
            .iter()
            .find(|(rel, _)| rel == "Consumers/PropertyRead.cs")
            .expect("consumer fragment")
            .1;
        let r = consumer
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Slug"))
            .expect("Slug ref present");
        assert_eq!(
            r.receiver_type.as_deref(),
            Some("Widget"),
            "the receiver fact still fires -- it is the argCount that is absent"
        );
        assert_eq!(
            r.arg_count, None,
            "a property read is not an invocation, so it records no argCount"
        );

        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/PropertyRead.cs").is_empty(),
            "no argCount, no key, no candidate lookup at all"
        );
    }

    #[test]
    fn stage3_range_an_optional_parameter_makes_the_entry_a_range_and_every_count_inside_it_binds()
    {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Ext/OptExtensions.cs",
                "namespace App.Ext { public static class OptExtensions { public static void Render(this Widget w, int depth, string label = null) { } } }",
            ),
            (
                "Consumers/OptionalRange.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class OptionalRange\n{\n  public void One(Widget w) => w.Render(1);\n  public void Two(Widget w) => w.Render(1, \"a\");\n  public void Three(Widget w) => w.Render(1, \"a\", 3);\n}\n",
            ),
        ]);
        let ext = &files
            .iter()
            .find(|(rel, _)| rel == "Ext/OptExtensions.cs")
            .expect("ext fragment")
            .1;
        let d = ext
            .defs
            .iter()
            .find(|d| d.id == "App.Ext.OptExtensions")
            .expect("OptExtensions def present");
        assert_eq!(
            d.extension_methods
                .iter()
                .map(|e| (
                    e.name.as_str(),
                    e.this_type.as_str(),
                    e.arity_min,
                    e.arity_max
                ))
                .collect::<Vec<_>>(),
            vec![("Render", "Widget", 1, 2)],
            "arityMin skips the defaulted parameter; arityMax still counts every DECLARED one"
        );

        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/OptionalRange.cs"),
            vec![("App.Ext.OptExtensions", 9), ("App.Ext.OptExtensions", 10)],
            "one argument and two both fall inside [1, 2]; three falls outside and earns nothing"
        );
    }

    #[test]
    fn stage3_tier_f_a_partial_static_class_declaring_the_same_quadruple_twice_stays_one_candidate()
    {
        // The Rust index stores def INDEXES in each bucket, so the per-def
        // dedup has to happen BEFORE the push -- otherwise a partial class
        // re-declaring the same (name, thisType, arityMin, arityMax) in a second
        // file would fill its own bucket twice and the one-candidate gate would
        // refuse a call that has exactly one real candidate.
        let files = vec![
            (
                "Ext/Widget.cs".to_string(),
                frag(
                    vec![def("App.Other.Widget", "Widget", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Ext/Part1.cs".to_string(),
                frag(
                    vec![ext_def(
                        "App.Ext.Helpers",
                        "Helpers",
                        "App.Ext",
                        &[("Render", "Widget", 0, 0)],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Ext/Part2.cs".to_string(),
                frag(
                    vec![ext_def(
                        "App.Ext.Helpers",
                        "Helpers",
                        "App.Ext",
                        &[("Render", "Widget", 0, 0), ("Poke", "Widget", 0, 0)],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/PartialExt.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.PartialExt",
                        "PartialExt",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Ext".into(),
                        global: false,
                    }],
                    // Distinct LINES: the two calls guess the same static class,
                    // and the heuristic-side dedup collapses byte-identical
                    // guesses -- which a shared synthetic line would make these,
                    // hiding the second candidate this test exists to see.
                    vec![
                        receiver_ref("w", "Render", "App.Consumers", "Widget", Some(0)),
                        FragRef {
                            line: 2,
                            ..receiver_ref("w", "Poke", "App.Consumers", "Widget", Some(0))
                        },
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.Ext.Helpers", "App.Ext.Helpers"],
            "the duplicate pair is deduped, and the second file's NEW pair still registers"
        );
    }

    // --- second tighten: instance-member veto, arity range,
    // --- generic argument unification ---
    //
    // Real fixtures run through this crate's own extractor, so a range, a base
    // name, or a type-argument descriptor that never gets recorded fails here
    // rather than passing vacuously.

    #[test]
    fn stage3_range_regression_an_exact_arity_class_no_longer_looks_unique_next_to_a_range_class() {
        let files = fragments_for(&[
            ("Other/Bus.cs", "namespace App.Other { public class Bus { } }"),
            // Exactly two parameters -- the shape the arity-keyed index used to
            // hand the edge to, because the range class below was keyed under
            // arity 3 and could not be found at argCount 2 at all.
            (
                "ExtA/WrongSend.cs",
                "namespace App.ExtA { public static class WrongSend { public static void Send(this Bus b, object m, int retries) { } } }",
            ),
            (
                "ExtB/RightSend.cs",
                "namespace App.ExtB { public static class RightSend { public static void Send(this Bus b, object m, string topic, int retries = 0) { } } }",
            ),
            (
                "Consumers/TwoSends.cs",
                "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class TwoSends\n{\n  public void Run(Bus b) => b.Send(1, 2);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/TwoSends.cs").is_empty(),
            "both classes accept two arguments once the range is honoured, so the tier sees TWO candidates and refuses -- the arity-keyed index saw one and picked the wrong class"
        );
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "honest ambiguity here is still silence, not an ambiguous edge"
        );
    }

    #[test]
    fn stage3_range_a_params_array_records_arity_max_minus_one_and_accepts_any_count() {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Ext/ParamsExtensions.cs",
                "namespace App.Ext { public static class ParamsExtensions { public static void All(this Widget w, params int[] xs) { } } }",
            ),
            (
                "Consumers/Spread.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Spread\n{\n  public void None(Widget w) => w.All();\n  public void Five(Widget w) => w.All(1, 2, 3, 4, 5);\n}\n",
            ),
        ]);
        let ext = &files
            .iter()
            .find(|(rel, _)| rel == "Ext/ParamsExtensions.cs")
            .expect("ext fragment")
            .1;
        let d = ext
            .defs
            .iter()
            .find(|d| d.id == "App.Ext.ParamsExtensions")
            .expect("ParamsExtensions def present");
        assert_eq!(
            d.extension_methods
                .iter()
                .map(|e| (e.arity_min, e.arity_max))
                .collect::<Vec<_>>(),
            vec![(0, -1)],
            "a params array is optional AND unbounded: nothing forces it, nothing caps it"
        );

        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Spread.cs"),
            vec![
                ("App.Ext.ParamsExtensions", 9),
                ("App.Ext.ParamsExtensions", 10)
            ],
            "zero arguments and five both bind to the same unbounded entry"
        );
    }

    #[test]
    fn stage3_range_an_unbounded_params_entry_alongside_a_second_visible_class_still_drops() {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "ExtA/ParamsExtensions.cs",
                "namespace App.ExtA { public static class ParamsExtensions { public static void All(this Widget w, params int[] xs) { } } }",
            ),
            (
                "ExtB/ExactExtensions.cs",
                "namespace App.ExtB { public static class ExactExtensions { public static void All(this Widget w, int a, int b) { } } }",
            ),
            (
                "Consumers/SpreadTwo.cs",
                "\nusing App.Other;\nusing App.ExtA;\nusing App.ExtB;\n\nnamespace App.Consumers;\n\npublic class SpreadTwo\n{\n  public void Two(Widget w) => w.All(1, 2);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/SpreadTwo.cs").is_empty(),
            "an unbounded range never wins a tie -- two candidates is two candidates"
        );
    }

    #[test]
    fn stage3_veto_a_member_declared_by_the_receivers_interface_beats_a_matching_visible_extension()
    {
        let files = fragments_for(&[
            ("Other/IWidget.cs", "namespace App.Other { public interface IWidget { void Render(int depth); } }"),
            ("Other/Widget.cs", "namespace App.Other { public class Widget : IWidget { } }"),
            (
                "Ext/WidgetExtensions.cs",
                "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
            ),
            (
                "Consumers/Vetoed.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Vetoed\n{\n  public void Run(Widget w) => w.Render(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/Vetoed.cs").is_empty(),
            "C# binds the instance member the interface declares; the extension is unreachable, so the tier must not claim the ref"
        );
        // Tier (e) must NOT have widened either: it emits only on the exact
        // receiver def, and Widget itself declares nothing.
        assert_eq!(g.stats.ambiguous_count, 0);
    }

    #[test]
    fn stage3_veto_control_the_same_shape_with_the_member_absent_still_earns_its_extension_edge() {
        let files = fragments_for(&[
            ("Other/IWidget.cs", "namespace App.Other { public interface IWidget { void Measure(int depth); } }"),
            ("Other/Widget.cs", "namespace App.Other { public class Widget : IWidget { } }"),
            (
                "Ext/WidgetExtensions.cs",
                "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w, int depth) { } } }",
            ),
            (
                "Consumers/NotVetoed.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class NotVetoed\n{\n  public void Run(Widget w) => w.Render(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/NotVetoed.cs"),
            vec![("App.Ext.WidgetExtensions", 9)],
            "the closure declares Measure, not Render -- nothing vetoes"
        );
    }

    #[test]
    fn stage3_veto_the_closure_is_transitive_so_a_member_on_the_base_of_the_base_still_vetoes() {
        let files = fragments_for(&[
            ("Other/Root.cs", "namespace App.Other { public class Root { public void Render(int depth) { } } }"),
            ("Other/Middle.cs", "namespace App.Other { public class Middle : Root { } }"),
            ("Other/Leaf.cs", "namespace App.Other { public class Leaf : Middle { } }"),
            (
                "Ext/LeafExtensions.cs",
                "namespace App.Ext { public static class LeafExtensions { public static void Render(this Leaf l, int depth) { } } }",
            ),
            (
                "Consumers/DeepVeto.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class DeepVeto\n{\n  public void Run(Leaf l) => l.Render(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/DeepVeto.cs").is_empty(),
            "two hops up the chain is still an instance member"
        );
    }

    #[test]
    fn stage3_veto_a_cycle_in_the_base_closure_terminates_instead_of_hanging() {
        // Not legal C#, but a fragments cache assembled from mid-edit sources
        // can present exactly this, and an unbounded walk is not an acceptable
        // failure mode.
        let files = fragments_for(&[
            ("Other/A.cs", "namespace App.Other { public class A : B { } }"),
            ("Other/B.cs", "namespace App.Other { public class B : A { } }"),
            (
                "Ext/AExtensions.cs",
                "namespace App.Ext { public static class AExtensions { public static void Render(this A a, int depth) { } } }",
            ),
            (
                "Consumers/Cyclic.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Cyclic\n{\n  public void Run(A a) => a.Render(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Cyclic.cs"),
            vec![("App.Ext.AExtensions", 9)],
            "the walk terminates and, finding no Render in the cycle, lets the extension edge stand"
        );
    }

    #[test]
    fn stage3_veto_bound_an_external_receiver_type_can_never_be_vetoed() {
        // No definition of `HttpClient` anywhere in the graph -- the receiver
        // resolves to nothing, so no closure exists to inspect.
        let files = fragments_for(&[
            (
                "Ext/HttpExtensions.cs",
                "namespace App.Ext { public static class HttpExtensions { public static void Ping(this HttpClient c, int n) { } } }",
            ),
            (
                "Consumers/External.cs",
                "\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class External\n{\n  public void Run(HttpClient c) => c.Ping(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/External.cs"),
            vec![("App.Ext.HttpExtensions", 8)],
            "documented bound: an out-of-graph receiver hides whatever members it declares"
        );
    }

    #[test]
    fn stage3_generic_concrete_this_args_must_match_the_receivers_concrete_args() {
        let files = fragments_for(&[
            (
                "Other/Types.cs",
                "namespace App.Other\n{\n  public class IDictionary<TKey, TValue> { }\n  public class IMessageDeserializer { }\n}\n",
            ),
            (
                "Ext/DictExtensions.cs",
                "namespace App.Ext { public static class DictExtensions { public static void TryGetValue(this IDictionary<string, object> d, int k) { } } }",
            ),
            (
                "Consumers/WrongArgs.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class WrongArgs\n{\n  public void Run(IDictionary<string, IMessageDeserializer> d) => d.TryGetValue(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/WrongArgs.cs").is_empty(),
            "the base name and arity both match -- only the type ARGUMENTS disagree, which is exactly the wrong edge the corpus audit found"
        );
    }

    #[test]
    fn stage3_generic_exactly_matching_concrete_this_args_earn_the_edge() {
        let files = fragments_for(&[
            ("Other/Types.cs", "namespace App.Other { public class IDictionary<TKey, TValue> { } }"),
            (
                "Ext/DictExtensions.cs",
                "namespace App.Ext { public static class DictExtensions { public static void TryGetValue(this IDictionary<string, object> d, int k) { } } }",
            ),
            (
                "Consumers/RightArgs.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class RightArgs\n{\n  public void Run(IDictionary<string, object> d) => d.TryGetValue(1);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/RightArgs.cs"),
            vec![("App.Ext.DictExtensions", 9)]
        );
    }

    #[test]
    fn stage3_generic_a_wildcard_this_arg_unifies_with_an_unbound_method_type_parameter() {
        let files = fragments_for(&[
            (
                "Other/Types.cs",
                "namespace App.Other\n{\n  public class EventPipelineBinder<TSaga, TData> { }\n  public class FutureState { }\n}\n",
            ),
            (
                "Ext/BinderExtensions.cs",
                "namespace App.Ext\n{\n  public static class BinderExtensions\n  {\n    public static void Then<TSaga, TData>(this EventPipelineBinder<TSaga, TData> b, int a) { }\n  }\n}\n",
            ),
            (
                "Consumers/Wildcards.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Wildcards\n{\n  public void Run<T>(EventPipelineBinder<FutureState, T> b) => b.Then(1);\n}\n",
            ),
        ]);
        let ext = &files
            .iter()
            .find(|(rel, _)| rel == "Ext/BinderExtensions.cs")
            .expect("ext fragment")
            .1;
        let d = ext
            .defs
            .iter()
            .find(|d| d.id == "App.Ext.BinderExtensions")
            .expect("BinderExtensions def present");
        assert_eq!(
            d.extension_methods[0].this_args,
            Some(vec!["*".to_string(), "*".to_string()]),
            "the extension's own type parameters are wildcards"
        );
        let consumer = &files
            .iter()
            .find(|(rel, _)| rel == "Consumers/Wildcards.cs")
            .expect("consumer fragment")
            .1;
        let r = consumer
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Then"))
            .expect("Then ref present");
        assert_eq!(
            r.receiver_args,
            Some(vec!["FutureState".to_string(), "*".to_string()]),
            "the enclosing method's own type parameter is a wildcard on the receiver side too"
        );

        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Wildcards.cs"),
            vec![("App.Ext.BinderExtensions", 9)]
        );
    }

    #[test]
    fn stage3_generic_a_non_generic_receiver_never_binds_a_generic_this_parameter() {
        let files = fragments_for(&[
            ("Other/Types.cs", "namespace App.Other\n{\n  public class Box<T> { }\n  public class Widget { }\n}\n"),
            (
                "Ext/BoxExtensions.cs",
                "namespace App.Ext { public static class BoxExtensions { public static void Open(this Box<Widget> b) { } } }",
            ),
            (
                "Consumers/Bare.cs",
                "\nusing App.Other;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Bare\n{\n  public void Run(Box b) => b.Open();\n}\n",
            ),
        ]);
        let consumer = &files
            .iter()
            .find(|(rel, _)| rel == "Consumers/Bare.cs")
            .expect("consumer fragment")
            .1;
        let r = consumer
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Open"))
            .expect("Open ref present");
        assert_eq!(r.receiver_type.as_deref(), Some("Box"));
        assert_eq!(
            r.receiver_args, None,
            "a non-generic declared type records no args at all"
        );

        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/Bare.cs").is_empty(),
            "generic on one side and not the other is a mismatch, never a wildcard"
        );
    }

    // --- the scored heuristic tier ---
    //
    // Real fixtures run through this crate's own extractor, so a fact that never
    // gets recorded fails here rather than passing vacuously.

    #[test]
    fn stage4_scored_an_ambiguous_qualifier_names_every_member_declaring_candidate_and_only_those()
    {
        let files = fragments_for(&[
            ("One/Config.cs", "namespace App.One { public class Config { public void Load() { } } }"),
            ("Two/Config.cs", "namespace App.Two { public class Config { public void Load() { } } }"),
            // Same simple name, so it IS one of the ambiguous candidates the
            // ladder hands over -- but it declares nothing called Load, so the
            // member filter drops it. The pool is never "everything the ladder
            // was confused by".
            ("Three/Config.cs", "namespace App.Three { public class Config { public void Save() { } } }"),
            (
                "Consumers/AmbiguousQualifier.cs",
                "\nnamespace App.Consumers;\n\npublic class AmbiguousQualifier\n{\n  public void Run() => Config.Load();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/AmbiguousQualifier.cs").is_empty(),
            "the precise tiers still refuse to pick"
        );
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/AmbiguousQualifier.cs"),
            vec![("App.One.Config", 6), ("App.Two.Config", 6)],
            "both member-declaring candidates, ordered by def id since nothing separates their scores"
        );
    }

    #[test]
    fn stage4_scored_same_namespace_beats_usings_visible_beats_global_and_that_is_the_emitted_order(
    ) {
        let files = fragments_for(&[
            ("Consumers/LocalStore.cs", "namespace App.Consumers { public class LocalStore { public void Persist() { } } }"),
            ("Imported/ImportedStore.cs", "namespace App.Imported { public class ImportedStore { public void Persist() { } } }"),
            ("Far/FarStore.cs", "namespace App.Far { public class FarStore { public void Persist() { } } }"),
            (
                "Consumers/Caller.cs",
                "\nusing App.Imported;\n\nnamespace App.Consumers;\n\npublic class Caller\n{\n  public void Run()\n  {\n    var s = Build();\n    s.Persist();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.Consumers.LocalStore", "App.Imported.ImportedStore", "App.Far.FarStore"],
            "score 3 then 2 then 1 -- NOT def-id order, which would have put App.Consumers, App.Far, App.Imported"
        );
    }

    #[test]
    fn stage4_scored_the_uniqueness_fallback_emits_at_two_member_declaring_defs() {
        let files = fragments_for(&[
            ("A/Counter.cs", "namespace App.A { public class Counter { public void Tally() { } } }"),
            ("B/Ledger.cs", "namespace App.B { public class Ledger { public void Tally() { } } }"),
            (
                "Consumers/Unknown.cs",
                "\nnamespace App.Consumers;\n\npublic class Unknown\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.A.Counter", "App.B.Ledger"]
        );
    }

    #[test]
    fn stage4_scored_a_member_name_carried_by_four_defs_is_too_common_to_guess_from() {
        let files = fragments_for(&[
            ("A/Counter.cs", "namespace App.A { public class Counter { public void Tally() { } } }"),
            ("B/Ledger.cs", "namespace App.B { public class Ledger { public void Tally() { } } }"),
            ("C/Register.cs", "namespace App.C { public class Register { public void Tally() { } } }"),
            ("D/Book.cs", "namespace App.D { public class Book { public void Tally() { } } }"),
            (
                "Consumers/TooCommon.cs",
                "\nnamespace App.Consumers;\n\npublic class TooCommon\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            heuristic_member_edges_from(&g, "Consumers/TooCommon.cs").is_empty(),
            "the refusal is total, not a top-three slice: past the threshold the name carries no information at all"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage4_scored_an_ambiguous_pool_larger_than_the_emit_cap_yields_exactly_three() {
        let files = fragments_for(&[
            ("A/Repo.cs", "namespace App.A { public class Repo { public void Save() { } } }"),
            ("B/Repo.cs", "namespace App.B { public class Repo { public void Save() { } } }"),
            ("C/Repo.cs", "namespace App.C { public class Repo { public void Save() { } } }"),
            ("D/Repo.cs", "namespace App.D { public class Repo { public void Save() { } } }"),
            ("E/Repo.cs", "namespace App.E { public class Repo { public void Save() { } } }"),
            ("Consumers/Many.cs", "\nnamespace App.Consumers;\n\npublic class Many\n{\n  public void Run() => Repo.Save();\n}\n"),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let guesses = heuristic_member_edges_from(&g, "Consumers/Many.cs");
        assert_eq!(
            guesses.len(),
            3,
            "the emit cap binds on the AMBIGUOUS pool, which has no size threshold of its own"
        );
        assert_eq!(
            guesses.iter().map(|(to, _)| *to).collect::<Vec<_>>(),
            vec!["App.A.Repo", "App.B.Repo", "App.C.Repo"],
            "all five score 1 at the global ladder step, so the def-id tiebreak alone decides which three survive"
        );
    }

    #[test]
    fn stage4_scored_a_ref_a_precise_tier_already_answered_never_gets_a_heuristic_duplicate() {
        let files = fragments_for(&[
            // Two more defs declaring Render, so the uniqueness fallback WOULD
            // have a pool to draw from if it were ever reached for this ref.
            ("Other/Widget.cs", "namespace App.Other { public class Widget { public void Render() { } } }"),
            ("Other/Gadget.cs", "namespace App.Other { public class Gadget { public void Render() { } } }"),
            (
                "Consumers/Precise.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class Precise\n{\n  private Widget _widget;\n\n  public void Run() => _widget.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Precise.cs"),
            vec![("App.Other.Widget", 10)],
            "one ref, one answer -- a fact is never restated as a guess"
        );
        assert!(heuristic_member_edges_from(&g, "Consumers/Precise.cs").is_empty());
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage4_scored_a_qualifier_that_resolved_but_vouched_for_nothing_is_left_alone() {
        let files = fragments_for(&[
            // Widget resolves uniquely and simply does not declare Render. That
            // is a KNOWN answer ("not here"), not an unknown one, so the scored
            // tier -- which only ever reads AMBIGUOUS or nothing-at-all
            // outcomes -- must not fire, even though Gadget would be a tidy
            // single-candidate guess.
            ("Other/Widget.cs", "namespace App.Other { public class Widget { } }"),
            ("Other/Gadget.cs", "namespace App.Other { public class Gadget { public void Render() { } } }"),
            (
                "Consumers/ResolvedMiss.cs",
                "\nusing App.Other;\n\nnamespace App.Consumers;\n\npublic class ResolvedMiss\n{\n  public void Run() => Widget.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(member_edges_from(&g, "Consumers/ResolvedMiss.cs").is_empty());
        assert!(heuristic_member_edges_from(&g, "Consumers/ResolvedMiss.cs").is_empty());
    }

    // The byte-identity fixture: a fixed set of sources whose resolved edge and
    // stats bytes are pinned exactly.
    const BYTE_IDENTITY_FIXTURE: &[(&str, &str)] = &[
        ("Core/Status.cs", "\nnamespace App.Core;\n\npublic enum Status\n{\n  Active,\n  Idle\n}\n"),
        ("Core/IWidget.cs", "namespace App.Core { public interface IWidget { void Render(); } }"),
        (
            "Core/Widget.cs",
            "\nusing App.Core;\n\nnamespace App.Core;\n\npublic class Widget : IWidget\n{\n  public string Name { get; set; }\n\n  public void Render() { }\n}\n",
        ),
        ("Alpha/Config.cs", "namespace App.Alpha { public class Config { public void Load() { } } }"),
        ("Beta/Config.cs", "namespace App.Beta { public class Config { public void Load() { } } }"),
        (
            "Consumers/Consumer.cs",
            "\nusing App.Core;\nusing App.Alpha;\nusing App.Beta;\n\nnamespace App.Consumers;\n\npublic class Consumer\n{\n  private Widget _widget;\n\n  public void Run()\n  {\n    _widget.Render();\n    var s = Status.Active;\n    Config.Load();\n    var c = Compute();\n    c.Tally();\n  }\n}\n",
        ),
        ("Solo/Counter.cs", "namespace App.Solo { public class Counter { public void Tally() { } } }"),
    ];

    // The precise-only bytes of that fixture's edge array (the heuristic edges
    // dropped). A literal on purpose: a golden recomputed by the code under test
    // proves nothing.
    const PRE_STAGE4_EDGE_ROWS: &[&str] = &[
        r#"{"kind":"imports","from_file":"Core/Widget.cs","from_line":2,"target":"App.Core"}"#,
        r#"{"kind":"inherits","from_file":"Core/Widget.cs","from_line":6,"to":"App.Core.IWidget","to_file":"Core/IWidget.cs"}"#,
        r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":2,"target":"App.Core"}"#,
        r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":3,"target":"App.Alpha"}"#,
        r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":4,"target":"App.Beta"}"#,
        r#"{"kind":"uses-type","from_file":"Consumers/Consumer.cs","from_line":10,"to":"App.Core.Widget","to_file":"Core/Widget.cs"}"#,
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":14,"to":"App.Core.Widget","to_file":"Core/Widget.cs"}"#,
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":15,"to":"App.Core.Status.Active","to_file":"Core/Status.cs"}"#,
    ];

    #[test]
    fn stage4_byte_identity_dropping_the_heuristic_edges_reproduces_the_pre_stage4_edge_array() {
        let files = fragments_for(BYTE_IDENTITY_FIXTURE);
        let g = resolve_graph(&no_git_root(), &files);

        let precise: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| {
                !matches!(
                    e,
                    Edge::Inherits {
                        heuristic: true,
                        ..
                    } | Edge::UsesType {
                        heuristic: true,
                        ..
                    } | Edge::UsesMember {
                        heuristic: true,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(
            serde_json::to_string(&precise).unwrap(),
            format!("[{}]", PRE_STAGE4_EDGE_ROWS.join(",")),
            "stage 4 is emission-only: it may ADD tagged edges and may never move, drop or re-key a precise one"
        );

        // And the addition really happened -- otherwise the assertion above
        // would pass just as well on a scored tier that emits nothing at all.
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Consumer.cs"),
            vec![
                ("App.Alpha.Config", 16),
                ("App.Beta.Config", 16),
                ("App.Solo.Counter", 18)
            ]
        );
    }

    #[test]
    fn stage4_stats_heuristic_edge_count_is_appended_last_and_edges_by_kind_never_counts_a_guess() {
        let files = fragments_for(BYTE_IDENTITY_FIXTURE);
        let g = resolve_graph(&no_git_root(), &files);

        // Whole-object bytes rather than a key list: this pins the ORDER the
        // serialized `stats` keys appear in, and the values with them.
        assert_eq!(
            serde_json::to_string(&g.stats).unwrap(),
            r#"{"def_count":9,"file_count":7,"edges_by_kind":{"inherits":1,"uses-type":1,"imports":4,"uses-member":2,"ctor-di":0},"ambiguous_count":0,"ambiguous_pct":0,"unresolved_external_count":0,"heuristic_edge_count":3,"test_def_count":0}"#,
            "test_def_count is appended LAST -- the stats key order the Node reference pins"
        );
        assert_eq!(g.stats.heuristic_edge_count, 3);
        assert_eq!(
            g.stats.edges_by_kind.uses_member, 2,
            "the two precise member edges only -- three heuristic ones landed in the same array and moved this number by zero"
        );
        assert_eq!(g.stats.edges_by_kind.inherits, 1);
        assert_eq!(g.stats.edges_by_kind.uses_type, 1);
        assert_eq!(g.stats.edges_by_kind.imports, 4);
        assert_eq!(
            g.stats.ambiguous_count, 0,
            "ambiguous_count semantics are untouched by this stage"
        );
    }

    // --- partial classes: also_in accumulation + method union -------------

    #[test]
    fn partial_class_across_files_merges_into_also_in_with_method_union() {
        let files = vec![
            (
                "A/Product.cs".to_string(),
                frag(
                    vec![FragDef {
                        line: 3,
                        ..def_with(
                            "A.Product",
                            "Product",
                            "A",
                            "class",
                            &["Describe"],
                            &["Name"],
                            &["_cache"],
                        )
                    }],
                    vec![],
                    vec![],
                ),
            ),
            (
                "A/Product.Extra.cs".to_string(),
                frag(
                    vec![FragDef {
                        line: 3,
                        ..def_with(
                            "A.Product",
                            "Product",
                            "A",
                            "class",
                            &["Refresh"],
                            &["Sku"],
                            &["_extra"],
                        )
                    }],
                    vec![],
                    vec![],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.defs.len(), 1, "one def entry, not two");
        let d = &g.defs[0];
        assert_eq!(
            d.file, "A/Product.cs",
            "first-insertion file wins as the primary site"
        );
        assert_eq!(
            d.methods,
            vec!["Describe".to_string(), "Refresh".to_string()],
            "methods union in encounter order"
        );
        assert_eq!(
            d.also_in,
            vec![AlsoIn {
                file: "A/Product.Extra.cs".into(),
                line: 3
            }]
        );
    }

    // --- resolution ladder: enclosing-namespace walks at steps 2 and 3 ------

    #[test]
    fn ladder_step2_a_using_directive_is_itself_read_against_the_enclosing_namespaces() {
        let files = fragments_for(&[
            ("A/Configuration/Setting.cs", "namespace A.Configuration { public class Setting { } }"),
            // The collision partner: without it a bare "Setting" would resolve
            // at step 4 (globally unique simple name) and this test would pass
            // on a ladder that never walked anything. With it, step 4 can only
            // report ambiguous.
            ("Other/Setting.cs", "namespace Other { public class Setting { } }"),
            ("A/B/C/Holder.cs", "\nusing Configuration;\n\nnamespace A.B.C;\n\npublic class Holder\n{\n  private Setting _setting;\n}\n"),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            type_edge_targets_from(&g, "A/B/C/Holder.cs"),
            vec!["A.Configuration.Setting"],
            "`using Configuration;` inside A.B.C reaches A.Configuration.Setting"
        );
        assert!(
            !g.edges.iter().any(
                |e| matches!(e, Edge::Ambiguous { from_file, .. } if from_file == "A/B/C/Holder.cs")
            ),
            "step 2 answers, so the ladder never reaches the ambiguous step-4 pool"
        );
    }

    #[test]
    fn ladder_step3_the_ancestor_namespace_rule_walks_every_enclosing_namespace() {
        let files = fragments_for(&[
            ("A/Shared.cs", "namespace A { public class Shared { } }"),
            // Same role as above -- makes step 4 ambiguous, so only a step-3
            // walk can produce a resolved edge here.
            (
                "Other/Shared.cs",
                "namespace Other { public class Shared { } }",
            ),
            (
                "A/B/C/Deep.cs",
                "\nnamespace A.B.C;\n\npublic class Deep\n{\n  private Shared _shared;\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            type_edge_targets_from(&g, "A/B/C/Deep.cs"),
            vec!["A.Shared"],
            "no usings at all -- the ancestor-namespace walk is the only step that can answer"
        );
        assert!(
            !g.edges.iter().any(
                |e| matches!(e, Edge::Ambiguous { from_file, .. } if from_file == "A/B/C/Deep.cs")
            ),
            "a walked step-3 hit resolves and never falls through to the ambiguous step-4 pool"
        );
    }

    #[test]
    fn stage4_scored_a_nested_type_candidate_is_refused_from_outside_its_own_file_and_kept_inside_it(
    ) {
        let files = fragments_for(&[
            // Cross-file nested candidate: unreachable from Holder.cs without
            // naming Remote first, so a guess landing on it could never be what
            // the code said.
            (
                "Far/Remote.cs",
                "\nnamespace App.Far;\n\npublic class Remote\n{\n  public class Inner\n  {\n    public void Tally() { }\n  }\n}\n",
            ),
            // Same-file nested candidate + the ref itself. `mystery` is a
            // var-from-call local, so no receiver fact and no qualifier
            // resolution at all -- the only door into the scored tier's
            // uniqueness fallback.
            (
                "Nested/Holder.cs",
                "\nnamespace App.Nested;\n\npublic class Outer\n{\n  public class Nested\n  {\n    public void Tally() { }\n  }\n\n  public string Probe()\n  {\n    var mystery = Fetch();\n    mystery.Tally();\n    return \"x\";\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.Nested.Outer+Nested"],
            "the same-file nested candidate is nameable and stays; the cross-file one is refused"
        );
    }

    #[test]
    fn heuristic_side_dedup_collapses_byte_identical_guesses_and_keeps_an_identical_precise_pair() {
        let files = fragments_for(&[
            ("Widgets/Widget.cs", "namespace App.Widgets { public class Widget { } }"),
            (
                "Ext/Helpers.cs",
                "\nnamespace App.Ext;\n\npublic static class Helpers\n{\n  public static string Slug(this Widget widget)\n  {\n    return \"s\";\n  }\n\n  public static string Tag(this Widget widget)\n  {\n    return \"t\";\n  }\n}\n",
            ),
            // Two DIFFERENT extension calls on ONE line, both naming the same
            // declaring static class: two guesses that serialize to the same
            // bytes.
            (
                "Ops/Caller.cs",
                "\nusing App.Ext;\nusing App.Widgets;\n\nnamespace App.Ops;\n\npublic class Caller\n{\n  public string Run()\n  {\n    Widget widget = new Widget();\n    return widget.Tag() + widget.Slug();\n  }\n}\n",
            ),
            ("Enums/Mode.cs", "namespace App.Enums { public enum Mode { On, Off } }"),
            // The precise counterpart: the same enum member read twice on one
            // line. Two identical PRECISE edges are two real occurrences in the
            // source, and dropping either would lose a fact -- so both survive.
            (
                "Ops/Twice.cs",
                "\nusing App.Enums;\n\nnamespace App.Ops;\n\npublic class Twice\n{\n  public bool Both(Mode a, Mode b)\n  {\n    return a == Mode.On && b == Mode.On;\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Ops/Caller.cs"),
            vec![("App.Ext.Helpers", 12)],
            "the second byte-identical guess is dropped, the first kept"
        );
        assert_eq!(
            member_edges_from(&g, "Ops/Twice.cs"),
            vec![("App.Enums.Mode.On", 10), ("App.Enums.Mode.On", 10)],
            "the precise side is untouched: two identical rows are two occurrences, not a duplicate"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 1,
            "the dropped guess leaves the counter too"
        );
    }

    // --- test coverage: test_methods on the merged row + the counter ---

    /// `def()` carrying a test-method list -- the one member fact that reaches
    /// graph.json's def rows.
    fn test_def(id: &str, name: &str, ns: &str, test_methods: &[&str]) -> FragDef {
        FragDef {
            test_methods: test_methods.iter().map(|s| s.to_string()).collect(),
            ..def(id, name, ns, "class")
        }
    }

    #[test]
    fn partial_test_class_unions_its_test_methods_across_both_declaring_files() {
        let files = vec![
            (
                "Tests/WidgetTests.Part1.cs".to_string(),
                frag(
                    vec![test_def(
                        "App.Tests.WidgetTests",
                        "WidgetTests",
                        "App.Tests",
                        &["Renders"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Tests/WidgetTests.Part2.cs".to_string(),
                frag(
                    vec![test_def(
                        "App.Tests.WidgetTests",
                        "WidgetTests",
                        "App.Tests",
                        &["Renders", "Scales"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.defs.len(), 1);
        assert_eq!(
            g.defs[0].test_methods,
            vec!["Renders".to_string(), "Scales".to_string()],
            "union across parts, deduped, first-seen order"
        );
        assert_eq!(
            serde_json::to_string(&g.defs[0]).unwrap(),
            r#"{"id":"App.Tests.WidgetTests","name":"WidgetTests","namespace":"App.Tests","kind":"class","file":"Tests/WidgetTests.Part1.cs","line":1,"methods":[],"testMethods":["Renders","Scales"],"also_in":[{"file":"Tests/WidgetTests.Part2.cs","line":1}]}"#,
            "the graph ROW keeps testMethods -- between methods and also_in"
        );
    }

    #[test]
    fn test_def_count_counts_merged_def_rows_not_fragment_entries() {
        let files = vec![
            (
                "Tests/WidgetTests.Part1.cs".to_string(),
                frag(
                    vec![test_def(
                        "App.Tests.WidgetTests",
                        "WidgetTests",
                        "App.Tests",
                        &["Renders"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Tests/WidgetTests.Part2.cs".to_string(),
                frag(
                    vec![test_def(
                        "App.Tests.WidgetTests",
                        "WidgetTests",
                        "App.Tests",
                        &["Scales"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Tests/CartTests.cs".to_string(),
                frag(
                    vec![test_def(
                        "App.Tests.CartTests",
                        "CartTests",
                        "App.Tests",
                        &["Places"],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Src/Widget.cs".to_string(),
                frag(
                    vec![def("App.Src.Widget", "Widget", "App.Src", "class")],
                    vec![],
                    vec![],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            g.stats.test_def_count, 2,
            "the partial class counts once across its two fragments; the production class not at all"
        );
    }

    // --- imports: always recorded, never resolved --------------------------

    #[test]
    fn imports_edge_is_recorded_regardless_of_whether_the_target_is_known() {
        let files = vec![(
            "A/Widget.cs".to_string(),
            frag(
                vec![],
                vec![],
                vec![FragRef {
                    kind: "imports".into(),
                    name: "System.Text".into(),
                    qualified: None,
                    member: None,
                    line: 1,
                    namespace: None,
                    type_arg_count: None,
                    generic: false,
                    receiver_type: None,
                    arg_count: None,
                    receiver_args: None,
                    outer_types: Vec::new(),
                    args: None,
                    receiver_property_owner: None,
                    receiver_call_owner: None,
                    receiver_call_member: None,
                }],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(g.stats.edges_by_kind.imports, 1);
        assert_eq!(
            g.stats.unresolved_external_count, 0,
            "imports never counts toward unresolved"
        );
    }

    // --- stats math ---------------------------------------------------------

    #[test]
    fn ambiguous_pct_only_counts_type_ref_attempts_not_uses_member_or_imports() {
        let files = vec![
            (
                "A/Money.cs".to_string(),
                frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
            ),
            (
                "B/Money.cs".to_string(),
                frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
            ),
            (
                "C/Mixed.cs".to_string(),
                frag(
                    vec![def("C.Mixed", "Mixed", "C", "class")],
                    vec![],
                    vec![
                        type_ref("uses-type", "Money", None, "C"), // ambiguous
                        FragRef {
                            kind: "imports".into(),
                            name: "System".into(),
                            qualified: None,
                            member: None,
                            line: 2,
                            namespace: None,
                            type_arg_count: None,
                            generic: false,
                            receiver_type: None,
                            arg_count: None,
                            receiver_args: None,
                            outer_types: Vec::new(),
                            args: None,
                            receiver_property_owner: None,
                            receiver_call_owner: None,
                            receiver_call_member: None,
                        },
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        // type_ref_attempts = inherits(0) + uses-type(0) + ambiguous(1) = 1
        assert_eq!(g.stats.ambiguous_pct, Percent1::from_ratio(1, 1));
        assert_eq!(
            serde_json::to_string(&g.stats.ambiguous_pct).unwrap(),
            "100"
        );
    }

    // --- the enclosing-type step ---

    /// A bare type ref carrying the enclosing-type stack the extractor would
    /// have recorded for it.
    fn nested_ref(kind: &str, name: &str, ns: &str, outer: &[&str]) -> FragRef {
        FragRef {
            outer_types: outer.iter().map(|s| (*s).to_string()).collect(),
            ..type_ref(kind, name, None, ns)
        }
    }

    #[test]
    fn v8_nested_step_resolves_a_type_declared_in_the_enclosing_type() {
        let files = vec![(
            "Core/Types.cs".to_string(),
            frag(
                vec![
                    def("App.Core.Outer", "Outer", "App.Core", "class"),
                    def("App.Core.Outer+Nested", "Nested", "App.Core", "class"),
                    def("App.Core.Other+Nested", "Nested", "App.Core", "class"),
                ],
                vec![],
                vec![nested_ref("uses-type", "Nested", "App.Core", &["Outer"])],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present")
        {
            Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Outer+Nested"),
            _ => unreachable!(),
        }
        // Two same-named nested defs: without the stack this was ambiguous.
        assert_eq!(g.stats.ambiguous_count, 0);
    }

    #[test]
    fn v8_nested_step_beats_the_namespace_and_usings_steps() {
        let files = vec![
            (
                "Other/Beta.cs".to_string(),
                frag(
                    vec![def("App.Other.Beta", "Beta", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Core/Types.cs".to_string(),
                frag(
                    vec![
                        def("App.Core.Alpha", "Alpha", "App.Core", "class"),
                        def("App.Core.Outer+Alpha", "Alpha", "App.Core", "class"),
                        def("App.Core.Outer+Beta", "Beta", "App.Core", "class"),
                    ],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![
                        nested_ref("uses-type", "Alpha", "App.Core", &["Outer"]),
                        nested_ref("uses-type", "Beta", "App.Core", &["Outer"]),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let targets: Vec<&str> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesType { to, .. } => Some(to.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(targets, vec!["App.Core.Outer+Alpha", "App.Core.Outer+Beta"]);
    }

    #[test]
    fn v8_alias_still_short_circuits_above_the_nested_step() {
        // C# puts type scope above a using-alias; devscout keeps the alias first
        // by construction -- a documented deviation, pinned here.
        let files = vec![
            (
                "Other/Gamma.cs".to_string(),
                frag(
                    vec![def("App.Other.Gamma", "Gamma", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Core/Types.cs".to_string(),
                frag(
                    vec![def("App.Core.Outer+Gamma", "Gamma", "App.Core", "class")],
                    vec![FragUsing::Alias {
                        alias: "Gamma".into(),
                        target: "App.Other.Gamma".into(),
                        global: false,
                    }],
                    vec![nested_ref("uses-type", "Gamma", "App.Core", &["Outer"])],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present")
        {
            Edge::UsesType { to, .. } => assert_eq!(to, "App.Other.Gamma"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn v8_the_innermost_enclosing_type_wins_over_an_outer_one() {
        let files = vec![(
            "Core/Nest.cs".to_string(),
            frag(
                vec![
                    def("App.Core.Outer+Target", "Target", "App.Core", "class"),
                    def("App.Core.Outer+Inner+Target", "Target", "App.Core", "class"),
                ],
                vec![],
                vec![nested_ref(
                    "uses-type",
                    "Target",
                    "App.Core",
                    &["Outer", "Inner"],
                )],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present")
        {
            Edge::UsesType { to, .. } => assert_eq!(to, "App.Core.Outer+Inner+Target"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn v8_a_dotted_nested_ref_never_enters_the_nested_step() {
        // BOUNDS: "." is not "+", and the ref text alone cannot say which was
        // meant, so a dotted "Outer.Nested" stays on the qualified ladder and
        // falls through to the global step -- ambiguous here, by design.
        let files = vec![(
            "Core/Types.cs".to_string(),
            frag(
                vec![
                    def("App.Core.Outer+Nested", "Nested", "App.Core", "class"),
                    def("App.Core.Other+Nested", "Nested", "App.Core", "class"),
                ],
                vec![],
                vec![FragRef {
                    outer_types: vec!["Outer".into()],
                    ..type_ref("uses-type", "Nested", Some("Outer.Nested"), "App.Core")
                }],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).is_none());
        assert_eq!(g.stats.ambiguous_count, 1);
    }

    #[test]
    fn v8_an_outer_types_naming_no_nested_id_falls_through_unchanged() {
        let files = vec![
            (
                "Other/Marker.cs".to_string(),
                frag(
                    vec![def("App.Other.Marker", "Marker", "App.Other", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Core/Types.cs".to_string(),
                frag(
                    vec![def("App.Core.Outer", "Outer", "App.Core", "class")],
                    vec![FragUsing::Plain {
                        text: "App.Other".into(),
                        global: false,
                    }],
                    vec![nested_ref("uses-type", "Marker", "App.Core", &["Outer"])],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        match find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present")
        {
            Edge::UsesType { to, .. } => assert_eq!(to, "App.Other.Marker"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn v8_tier_e_receiver_probe_carries_the_refs_outer_types() {
        // The whole defect: without the stack on the SYNTHETIC probe the
        // member access resolves against two same-named nested types and can
        // only ever be a guess.
        let files = vec![(
            "Core/Types.cs".to_string(),
            frag(
                vec![
                    def("App.Core.Outer", "Outer", "App.Core", "class"),
                    def_with(
                        "App.Core.Outer+Nested",
                        "Nested",
                        "App.Core",
                        "class",
                        &["Run"],
                        &[],
                        &[],
                    ),
                    def_with(
                        "App.Core.Other+Nested",
                        "Nested",
                        "App.Core",
                        "class",
                        &["Run"],
                        &[],
                        &[],
                    ),
                ],
                vec![],
                vec![FragRef {
                    outer_types: vec!["Outer".into()],
                    ..receiver_ref("_n", "Run", "App.Core", "Nested", Some(0))
                }],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        let precise: Vec<&str> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    to,
                    heuristic: false,
                    ..
                } => Some(to.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(precise, vec!["App.Core.Outer+Nested"]);
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    // --- constructor-parameter DI resolution ---
    //
    // These tests cover the RESOLVED 'ctor-di' edge `resolve_graph` produces
    // from the extraction-layer facts (a def's type_params/base_generic_args and
    // the 'ctor-param' ref itself).

    fn def_with_bases_and_generics(
        id: &str,
        name: &str,
        ns: &str,
        bases: &[&str],
        type_params: &[&str],
        base_generic_args: &[(&str, &[&str])],
    ) -> FragDef {
        let mut bga = crate::graph::OrderedMap::new();
        for (k, v) in base_generic_args {
            bga.insert((*k).to_string(), v.iter().map(|s| s.to_string()).collect());
        }
        FragDef {
            bases: bases.iter().map(|s| s.to_string()).collect(),
            type_params: type_params.iter().map(|s| s.to_string()).collect(),
            base_generic_args: bga,
            ..def(id, name, ns, "class")
        }
    }

    fn ctor_param_ref(name: &str, ns: &str, args: Option<Vec<String>>) -> FragRef {
        FragRef {
            args,
            ..type_ref("ctor-param", name, None, ns)
        }
    }

    fn ctor_di_edges<'a>(g: &'a Graph, iface: &str) -> Vec<&'a Edge> {
        g.edges
            .iter()
            .filter(|e| matches!(e, Edge::CtorDi { iface: i, .. } if i == iface))
            .collect()
    }

    #[test]
    fn ctor_di_a_closed_generic_ctor_param_resolves_to_the_open_generic_implementation_that_passes_its_type_argument_through(
    ) {
        let files = vec![
            (
                "Di/IRepository.cs".to_string(),
                frag(
                    vec![def(
                        "App.Di.IRepository",
                        "IRepository",
                        "App.Di",
                        "interface",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/MongoRepository.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.MongoRepository",
                        "MongoRepository",
                        "App.Di",
                        &["IRepository"],
                        &["T"],
                        &[("IRepository", &["*"])],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/User.cs".to_string(),
                frag(
                    vec![def("App.Di.User", "User", "App.Di", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/Controller.cs".to_string(),
                frag(
                    vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                    vec![],
                    vec![ctor_param_ref(
                        "IRepository",
                        "App.Di",
                        Some(vec!["User".to_string()]),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "IRepository");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi {
                resolution,
                args,
                to,
                candidates,
                ..
            } => {
                assert_eq!(resolution, "open-generic");
                assert_eq!(args.as_deref(), Some(&["User".to_string()][..]));
                assert_eq!(to.as_deref(), Some("App.Di.MongoRepository"));
                assert!(candidates.is_empty());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctor_di_a_plain_non_generic_ctor_param_resolves_to_its_sole_implementor() {
        let files = vec![
            (
                "Di/IFooService.cs".to_string(),
                frag(
                    vec![def(
                        "App.Di.IFooService",
                        "IFooService",
                        "App.Di",
                        "interface",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/FooService.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.FooService",
                        "FooService",
                        "App.Di",
                        &["IFooService"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/Controller.cs".to_string(),
                frag(
                    vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                    vec![],
                    vec![ctor_param_ref("IFooService", "App.Di", None)],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "IFooService");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi {
                resolution,
                args,
                to,
                ..
            } => {
                assert_eq!(resolution, "plain");
                assert_eq!(
                    *args, None,
                    "a non-generic ctor param carries no args field at all"
                );
                assert_eq!(to.as_deref(), Some("App.Di.FooService"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctor_di_a_ctor_param_type_absent_from_the_corpus_is_classified_infra_when_the_file_imports_a_bcl_namespace(
    ) {
        let files = vec![(
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![FragUsing::Plain {
                    text: "Microsoft.Extensions.Logging".into(),
                    global: false,
                }],
                vec![ctor_param_ref(
                    "ILogger",
                    "App.Di",
                    Some(vec!["Controller".to_string()]),
                )],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "ILogger");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi {
                resolution,
                args,
                to,
                ..
            } => {
                assert_eq!(resolution, "infra");
                assert_eq!(args.as_deref(), Some(&["Controller".to_string()][..]));
                assert_eq!(*to, None);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctor_di_an_unresolvable_ctor_param_with_no_bcl_using_in_scope_is_unresolved_not_dropped() {
        let files = vec![(
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![],
                vec![ctor_param_ref("ISomeThirdPartyThing", "App.Di", None)],
            ),
        )];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "ISomeThirdPartyThing");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi { resolution, .. } => assert_eq!(resolution, "unresolved"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctor_di_two_implementors_tied_at_the_same_precedence_tier_are_ambiguous_never_guessed() {
        let files = vec![
            (
                "Di/IFooService.cs".to_string(),
                frag(
                    vec![def(
                        "App.Di.IFooService",
                        "IFooService",
                        "App.Di",
                        "interface",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/FooServiceA.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.FooServiceA",
                        "FooServiceA",
                        "App.Di",
                        &["IFooService"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/FooServiceB.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.FooServiceB",
                        "FooServiceB",
                        "App.Di",
                        &["IFooService"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/Controller.cs".to_string(),
                frag(
                    vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                    vec![],
                    vec![ctor_param_ref("IFooService", "App.Di", None)],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "IFooService");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi {
                resolution,
                candidates,
                ..
            } => {
                assert_eq!(resolution, "ambiguous");
                assert_eq!(
                    candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                    vec!["App.Di.FooServiceA", "App.Di.FooServiceB"]
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctor_di_a_closed_implementor_is_preferred_over_an_open_generic_one_when_both_exist() {
        let files = vec![
            (
                "Di/IRepository.cs".to_string(),
                frag(
                    vec![def(
                        "App.Di.IRepository",
                        "IRepository",
                        "App.Di",
                        "interface",
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/MongoRepository.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.MongoRepository",
                        "MongoRepository",
                        "App.Di",
                        &["IRepository"],
                        &["T"],
                        &[("IRepository", &["*"])],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/SpecificUserRepository.cs".to_string(),
                frag(
                    vec![def_with_bases_and_generics(
                        "App.Di.SpecificUserRepository",
                        "SpecificUserRepository",
                        "App.Di",
                        &["IRepository"],
                        &[],
                        &[("IRepository", &["User"])],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/User.cs".to_string(),
                frag(
                    vec![def("App.Di.User", "User", "App.Di", "class")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Di/Controller.cs".to_string(),
                frag(
                    vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                    vec![],
                    vec![ctor_param_ref(
                        "IRepository",
                        "App.Di",
                        Some(vec!["User".to_string()]),
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        let edges = ctor_di_edges(&g, "IRepository");
        assert_eq!(edges.len(), 1);
        match edges[0] {
            Edge::CtorDi { resolution, to, .. } => {
                assert_eq!(resolution, "closed", "the non-generic, exactly-matching implementor wins over the open-generic passthrough");
                assert_eq!(to.as_deref(), Some("App.Di.SpecificUserRepository"));
            }
            _ => unreachable!(),
        }
    }

    // --- the property hop and the call hop ---

    /// The two member->type maps the hops read, on top of `def_with`'s member
    /// lists: (method, return type) and (property, declared type).
    fn with_member_types(
        base: FragDef,
        method_returns: &[(&str, &str)],
        property_types: &[(&str, &str)],
    ) -> FragDef {
        let mut returns = OrderedMap::new();
        for (name, ty) in method_returns {
            returns.insert((*name).to_string(), (*ty).to_string());
        }
        let mut properties = OrderedMap::new();
        for (name, ty) in property_types {
            properties.insert(
                (*name).to_string(),
                FragFact {
                    type_name: (*ty).to_string(),
                    args: None,
                },
            );
        }
        FragDef {
            method_returns: returns,
            property_types: properties,
            ..base
        }
    }

    /// The TAIL window of a two-segment chain, carrying the head type the
    /// extractor recorded for it: `head.<property>.<member>()`.
    fn property_hop_ref(owner: &str, property: &str, member: &str, ns: &str) -> FragRef {
        FragRef {
            receiver_property_owner: Some(owner.into()),
            ..member_ref(property, Some(&format!("head.{property}")), member, ns)
        }
    }

    /// A bare-qualifier member ref whose qualifier is a
    /// `var x = Owner.Callee(...)` local.
    fn call_receiver_ref(name: &str, owner: &str, callee: &str, member: &str, ns: &str) -> FragRef {
        FragRef {
            receiver_call_owner: Some(owner.into()),
            receiver_call_member: Some(callee.into()),
            ..member_ref(name, None, member, ns)
        }
    }

    #[test]
    fn ds0012_property_hop_resolves_to_the_propertys_declared_type() {
        let files = vec![
            (
                "Other/Settings.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Settings",
                        "Settings",
                        "App.Other",
                        "class",
                        &["Reload"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![with_member_types(
                        def_with(
                            "App.Other.Widget",
                            "Widget",
                            "App.Other",
                            "class",
                            &[],
                            &["Config"],
                            &[],
                        ),
                        &[],
                        &[("Config", "Settings")],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Hop.cs".to_string(),
                frag(
                    vec![def("App.Consumers.Hop", "Hop", "App.Consumers", "class")],
                    vec![FragUsing::Plain {
                        text: "App.Other".to_string(),
                        global: false,
                    }],
                    vec![property_hop_ref(
                        "Widget",
                        "Config",
                        "Reload",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(member_edge_targets(&g), vec!["App.Other.Settings"]);
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "a precise hop leaves nothing for the scored tier"
        );
    }

    #[test]
    fn ds0012_property_hop_stops_on_an_unrecorded_property_a_missing_member_and_an_ambiguous_type()
    {
        let files = vec![
            (
                "Other/Settings.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Settings",
                        "Settings",
                        "App.Other",
                        "class",
                        &["Reload"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![with_member_types(
                        def_with(
                            "App.Other.Widget",
                            "Widget",
                            "App.Other",
                            "class",
                            &[],
                            &["Label", "Config", "Price"],
                            &[],
                        ),
                        &[],
                        // `Label` is declared `string`: a predefined type
                        // records no fact at all, so it is absent here.
                        &[("Config", "Settings"), ("Price", "Money")],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Money/A.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Money.A.Money",
                        "Money",
                        "App.Money.A",
                        "class",
                        &["Round"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Money/B.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Money.B.Money",
                        "Money",
                        "App.Money.B",
                        "class",
                        &["Round"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Stops.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.Stops",
                        "Stops",
                        "App.Consumers",
                        "class",
                    )],
                    vec![
                        FragUsing::Plain {
                            text: "App.Other".to_string(),
                            global: false,
                        },
                        FragUsing::Plain {
                            text: "App.Money.A".to_string(),
                            global: false,
                        },
                        FragUsing::Plain {
                            text: "App.Money.B".to_string(),
                            global: false,
                        },
                    ],
                    vec![
                        property_hop_ref("Widget", "Label", "Trim", "App.Consumers"),
                        property_hop_ref("Widget", "Config", "Missing", "App.Consumers"),
                        property_hop_ref("Widget", "Price", "Round", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edge_targets(&g).is_empty(),
            "no recorded type, no declared member, and an ambiguous type each end the hop"
        );
    }

    #[test]
    fn ds0010_var_from_invocation_resolves_through_the_callees_recorded_return_type() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Factory.cs".to_string(),
                frag(
                    vec![with_member_types(
                        def_with(
                            "App.Other.Factory",
                            "Factory",
                            "App.Other",
                            "class",
                            &["Make"],
                            &[],
                            &[],
                        ),
                        &[("Make", "Widget")],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/FromCall.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.FromCall",
                        "FromCall",
                        "App.Consumers",
                        "class",
                    )],
                    vec![FragUsing::Plain {
                        text: "App.Other".to_string(),
                        global: false,
                    }],
                    vec![call_receiver_ref(
                        "made",
                        "Factory",
                        "Make",
                        "Render",
                        "App.Consumers",
                    )],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(member_edge_targets(&g), vec!["App.Other.Widget"]);
    }

    #[test]
    fn ds0010_ambiguous_out_of_graph_and_return_less_callees_stay_taken_but_unknown() {
        let files = vec![
            (
                "Other/Widget.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Widget",
                        "Widget",
                        "App.Other",
                        "class",
                        &["Render"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "A/Factory.cs".to_string(),
                frag(
                    vec![with_member_types(
                        def_with(
                            "App.A.Factory",
                            "Factory",
                            "App.A",
                            "class",
                            &["Make"],
                            &[],
                            &[],
                        ),
                        &[("Make", "Widget")],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "B/Factory.cs".to_string(),
                frag(
                    vec![with_member_types(
                        def_with(
                            "App.B.Factory",
                            "Factory",
                            "App.B",
                            "class",
                            &["Make"],
                            &[],
                            &[],
                        ),
                        &[("Make", "Widget")],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Other/Silent.cs".to_string(),
                frag(
                    vec![def_with(
                        "App.Other.Silent",
                        "Silent",
                        "App.Other",
                        "class",
                        &["Make"],
                        &[],
                        &[],
                    )],
                    vec![],
                    vec![],
                ),
            ),
            (
                "Consumers/Unknowns.cs".to_string(),
                frag(
                    vec![def(
                        "App.Consumers.Unknowns",
                        "Unknowns",
                        "App.Consumers",
                        "class",
                    )],
                    vec![
                        FragUsing::Plain {
                            text: "App.Other".to_string(),
                            global: false,
                        },
                        FragUsing::Plain {
                            text: "App.A".to_string(),
                            global: false,
                        },
                        FragUsing::Plain {
                            text: "App.B".to_string(),
                            global: false,
                        },
                    ],
                    vec![
                        call_receiver_ref(
                            "ambiguous",
                            "Factory",
                            "Make",
                            "Render",
                            "App.Consumers",
                        ),
                        call_receiver_ref(
                            "external",
                            "ThirdParty",
                            "Make",
                            "Render",
                            "App.Consumers",
                        ),
                        // `Silent.Make` is declared but records no return type
                        // (a void method blocks its own name).
                        call_receiver_ref("silent", "Silent", "Make", "Render", "App.Consumers"),
                    ],
                ),
            ),
        ];
        let g = resolve_graph(&no_git_root(), &files);
        // An owner the ladder refuses to pick, an owner it never finds, and an
        // owner whose `Make` records no return type all leave the local exactly
        // as unknown as the extractor left it.
        assert!(member_edge_targets(&g).is_empty());
    }
}
