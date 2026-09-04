// The resolution ladder, including ambiguous marking: `build_def_index`,
// `resolve_ref`, `collect_global_usings_by_unit`, `capped_candidates`,
// `resolve_graph`.
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
    Fragment, Graph, GraphName, HeuristicByTier, HeuristicTier, OrderedMap, Percent1, Stats,
    GRAPH_SCHEMA_VERSION,
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
    /// Field name -> declared type fact, the field half of a bare-identifier
    /// receiver's type when the file that reads it carries no local/
    /// parameter/field fact of its own for the name (a sibling partial-class
    /// file's field, reached only through this merged table). Merged across
    /// a partial class exactly like `property_types`.
    pub field_types: OrderedMap<FragFact>,
    /// The file whose declaration supplied each `property_types` /
    /// `field_types` entry -- one key per key of the table beside it, filled
    /// in at merge time. Purely an in-memory bookkeeping table (nothing here
    /// is serialized), and the only record of WHERE a merged fact came from:
    /// a partial class's def carries the FIRST declaring file, which is not
    /// necessarily the file that declared any given member. The type name a
    /// fact holds is a bare identifier that only means anything under the
    /// `using` directives, aliases and namespace of the file that WROTE it,
    /// so `bare_receiver_field_or_property_type`'s caller resolves it in
    /// that file's context rather than the reading file's.
    pub property_type_files: OrderedMap<String>,
    /// The `field_types` half of `property_type_files`.
    pub field_type_files: OrderedMap<String>,
    /// Method name -> the generic-arg descriptors `method_returns` itself
    /// strips off (see `FragDef.method_return_args`), read ONLY by the
    /// awaited-call unwrap: an entry here exists exactly when the same name
    /// has a `method_returns` entry AND that return type carried a
    /// top-level type-argument list. Merged across a partial class exactly
    /// like `method_returns`.
    pub method_return_args: OrderedMap<Vec<String>>,
    /// Declared method names `methods` (on `Def`) does not carry -- see
    /// `FragDef.non_public_methods`. Merged across a partial class exactly
    /// like `properties`/`fields` (union, first-insertion order). Read ONLY
    /// by `declares_member_any_visibility`, itself read ONLY for
    /// hierarchy-internal receivers (`base.` and the `this.` shape's own
    /// base walk); every other caller of a "does this def declare the
    /// member" question keeps reading `Def.methods` alone.
    pub non_public_methods: Vec<String>,
    /// Method name -> the (min, max) argument-count ranges the overloads
    /// sharing that name accept -- see `FragDef.method_arities`. Merged
    /// across a partial class as a UNION of ranges per NAME, the way
    /// `Def.methods` unions the names themselves: every declaring part
    /// contributes the overloads it declares, duplicates dropped, because
    /// the parts of one partial class share a single overload set and a
    /// call some part's overload accepts is a call the type accepts. Read
    /// by `declares_member`/`declares_member_any_visibility` for a ref that
    /// carries an `argCount`.
    pub method_arities: OrderedMap<Vec<(usize, i64)>>,
}

fn build_def_index(fragments_by_file: &[(String, Fragment)]) -> DefIndex {
    let mut defs: Vec<Def> = Vec::new();
    let mut member_lists: Vec<MemberLists> = Vec::new();
    let mut qualified_name_to_def: HashMap<String, usize> = HashMap::new();
    let mut qualified_name_and_arity_to_def: HashMap<(String, usize), usize> = HashMap::new();
    let mut simple_name_to_defs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut extension_index: HashMap<String, Vec<ExtCandidate>> = HashMap::new();

    // One entry per key of `map`, all naming `file` -- the shape
    // `MemberLists::property_type_files`/`field_type_files` hold for a def's
    // FIRST declaration, where every fact came from the same file by
    // construction.
    fn keys_mapped_to_file<V>(map: &OrderedMap<V>, file: &str) -> OrderedMap<String> {
        let mut out = OrderedMap::new();
        for (name, _) in map.iter() {
            out.insert(name.clone(), file.to_string());
        }
        out
    }

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
                        field_types: d.field_types.clone(),
                        property_type_files: keys_mapped_to_file(&d.property_types, file),
                        field_type_files: keys_mapped_to_file(&d.field_types, file),
                        method_return_args: d.method_return_args.clone(),
                        non_public_methods: d.non_public_methods.clone(),
                        method_arities: d.method_arities.clone(),
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
                    for m in &d.non_public_methods {
                        if !member_lists[idx].non_public_methods.contains(m) {
                            member_lists[idx].non_public_methods.push(m.clone());
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
                    // The declaring FILE is recorded alongside each fact
                    // it accepts -- see `MemberLists::property_type_files`:
                    // a type name is only meaningful under the usings of
                    // the file that wrote it, and for a partial class that
                    // is not always the def's own first-declaring file.
                    for (name, fact) in d.property_types.iter() {
                        if member_lists[idx].property_types.get(name).is_none() {
                            member_lists[idx]
                                .property_types
                                .insert(name.clone(), fact.clone());
                            member_lists[idx]
                                .property_type_files
                                .insert(name.clone(), file.clone());
                        }
                    }
                    for (name, fact) in d.field_types.iter() {
                        if member_lists[idx].field_types.get(name).is_none() {
                            member_lists[idx]
                                .field_types
                                .insert(name.clone(), fact.clone());
                            member_lists[idx]
                                .field_type_files
                                .insert(name.clone(), file.clone());
                        }
                    }
                    for (name, args) in d.method_return_args.iter() {
                        if member_lists[idx].method_return_args.get(name).is_none() {
                            member_lists[idx]
                                .method_return_args
                                .insert(name.clone(), args.clone());
                        }
                    }
                    // A UNION per NAME, unlike `method_returns` above: a
                    // partial class's parts declare OVERLOADS of one name,
                    // not competing answers for it, so `void Run()` in one
                    // file and `void Run(int)` in another must both be
                    // admitted -- first-declaration-wins here would have
                    // hidden the sibling file's overload and made
                    // `method_arity_admits` refuse a call the type really
                    // accepts. Ranges already recorded are not repeated, so
                    // a part re-declaring an overload the merged set holds
                    // leaves the set unchanged.
                    for (name, ranges) in d.method_arities.iter() {
                        let mut merged = member_lists[idx]
                            .method_arities
                            .get(name)
                            .cloned()
                            .unwrap_or_default();
                        for range in ranges {
                            if !merged.contains(range) {
                                merged.push(*range);
                            }
                        }
                        member_lists[idx]
                            .method_arities
                            .insert(name.clone(), merged);
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
        receiver_base: false,
        receiver_awaited: false,
        receiver_local: false,
    }
}

// Whether SOME overload's own (min, max) range admits exactly `arg_count`
// arguments -- the OR every overload sharing a name contributes, since C#
// overload resolution picks whichever member of the set actually accepts the
// call. `max == -1` is the same unbounded-`params` sentinel
// `arity_accepts` (the extension-method counterpart) already reads. No entry
// for the name at all -- an arity fact `raw_method_arities` did not attach,
// or a fragment cached before this table existed (`serde(default)` reads it
// back empty) -- admits ANY count: an arity gate this resolver cannot answer
// must never silently NARROW what `declares_member` would otherwise have
// said, and must never turn a stale, un-remapped cache into a false miss.
fn method_arity_admits(index: &DefIndex, idx: usize, member: &str, arg_count: usize) -> bool {
    match index.member_lists[idx].method_arities.get(member) {
        Some(ranges) => ranges
            .iter()
            .any(|&(min, max)| min <= arg_count && (max == -1 || (arg_count as i64) <= max)),
        None => true,
    }
}

// The def's member lists, unioned: methods ∪ properties ∪ fields for a READ
// (`arg_count == None`), which is what lets a static PROPERTY access
// (MessageUrn.Prefix) and a const/static FIELD access earn an edge on the
// same evidence a static method call already did.
//
// A CALL (`arg_count == Some(n)`) is narrower on both axes (Unit A4 item 2):
// properties and fields never satisfy a call (the rule `member_vouched`'s own
// Call/Read split already enforces for the scored tier; this is where the
// PRECISE tier gains it too), and `methods` alone is not enough either -- the
// name must ALSO have an overload whose own arity range admits `n`
// (`method_arity_admits`), or this answers `false` exactly as it would for a
// name this def never declares at all. That is what lets tier (f)/the scored
// tier run when a same-named instance member exists but at the WRONG
// signature: the precise tier's own callers read `false` here as "nothing
// declared", never mark the ref `emitted`, and every later tier proceeds
// undisturbed.
fn declares_member(
    index: &DefIndex,
    idx: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
) -> bool {
    let Some(member) = member else { return false };
    match arg_count {
        Some(n) => {
            index.defs[idx].methods.iter().any(|m| m == member)
                && method_arity_admits(index, idx, member, n)
        }
        None => {
            index.defs[idx].methods.iter().any(|m| m == member)
                || index.member_lists[idx]
                    .properties
                    .iter()
                    .any(|p| p == member)
                || index.member_lists[idx].fields.iter().any(|f| f == member)
        }
    }
}

// `declares_member` widened by `non_public_methods` -- `properties`/`fields`
// already carry every accessibility with no filter of their own (see
// `DefRecord::properties`), so `methods` is the only list this widens, and
// (Unit A4 item 2) the SAME arity gate applies to a non-public method: a
// call whose `arg_count` no non-public overload admits is exactly as
// undeclared as one whose PUBLIC overloads all decline. Read ONLY where the
// SITE is inside the hierarchy the member lookup is walking:
// `base_member_declared` (a `base.` qualifier never considers anything but
// the enclosing type's own bases) and the typed-receiver precise tier's own
// base walk, and even there ONLY when the receiver is the enclosing type
// itself (the `this.` shape). Every other caller -- the scored tier's veto
// and its `member_vouched` pool filter, tier (f)'s instance-member veto, and
// an ordinary typed receiver's own base walk -- keeps asking `declares_member`
// unchanged, so a guess can never start vouching through a member C# would
// refuse it visibility to.
fn declares_member_any_visibility(
    index: &DefIndex,
    idx: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
) -> bool {
    if declares_member(index, idx, member, arg_count) {
        return true;
    }
    let Some(member) = member else { return false };
    if !index.member_lists[idx]
        .non_public_methods
        .iter()
        .any(|m| m == member)
    {
        return false;
    }
    match arg_count {
        Some(n) => method_arity_admits(index, idx, member, n),
        None => true,
    }
}

// The two shapes a member reference can take, read straight off the ref's own
// recorded fact: a CALL carries an `argCount` (the extractor only ever sets
// one on the function half of an `invocation_expression`, see
// `invocation_arg_count`), a READ carries none. C# will only ever bind a call
// to something invocable, so the shape is what tells `member_vouched` whether
// a property or field is even eligible to answer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemberShape {
    Call,
    Read,
}

fn member_shape(r: &FragRef) -> MemberShape {
    if r.arg_count.is_some() {
        MemberShape::Call
    } else {
        MemberShape::Read
    }
}

// The membership test the SCORED tier uses: `declares_member` widened by the
// extension-method names the def declares, and -- for a CALL -- narrowed
// first to the names that are actually invocable. A static class holding
// `Render(this Widget w)` never "declares Render" in the instance sense
// `declares_member` means, but it is exactly the def a `something.Render()`
// guess should be allowed to name, so the scored tier counts it for a call.
// A property or a field is never callable: `entity.Property(x => x.Id)`
// cannot bind to a property or field under any C# overload resolution, so a
// property/field-only def must not vouch for a ref shaped like a call, even
// though the very same def is fair game for a READ of that same member name
// (`entity.Property`). Deliberately NOT used by any precise tier: widening
// `declares_member` itself would let tiers (a)/(e) emit a PRECISE edge on an
// extension name with none of tier (f)'s arity, generic-unification or
// admission filters applied.
fn member_vouched(index: &DefIndex, idx: usize, member: Option<&str>, shape: MemberShape) -> bool {
    let Some(member) = member else { return false };
    let instance_vouches = match shape {
        MemberShape::Call => index.defs[idx].methods.iter().any(|m| m == member),
        MemberShape::Read => declares_member(index, idx, Some(member), None),
    };
    if instance_vouches {
        return true;
    }
    index.member_lists[idx]
        .extension_methods
        .iter()
        .any(|(name, ..)| name == member)
}

// The half of C#'s namespace visibility a `using` directive does NOT account
// for: a type declared in an ENCLOSING namespace of the reference site is in
// scope there with no import at all -- `App.Ext.LogExt` is nameable from
// inside `namespace App.Ext.Deep`, and no file has to say so.
//
// Tier (f) needs this because its admission test is the only place in this
// module that asks "is this def visible here" WITHOUT going through the
// ladder (which has walked enclosing namespaces since step 3). Until the
// project model arrived, the repo-wide `global using` pool papered over the
// gap -- one file anywhere in the repo importing the namespace made it
// visible everywhere, its own enclosing-namespace children included. Scoping
// global usings per project removed that accident and left the real rule
// missing, so here it is, stated.
//
// The global namespace is deliberately NOT treated as enclosing: a static
// class declared with no namespace at all would otherwise become a candidate
// at every ref site in the repo at once, which is a far wider change than the
// lexical rule this implements.
fn namespace_encloses(outer: &str, inner: &str) -> bool {
    inner
        .strip_prefix(outer)
        .is_some_and(|rest| rest.starts_with('.'))
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

// Every file's own context (local ∪ every `global using` IN SCOPE for it, with
// a local alias shadowing a same-named global one), built once instead of once
// per ref. The main loop needs it for the file it is walking; the
// instance-member veto needs it for a DIFFERENT file -- the one that declares
// the base type it is resolving -- which is why it is a map rather than two
// locals.
//
// "In scope" is a project-model question. A `global using` belongs to the
// COMPILATION that declares it and does not flow across a `ProjectReference`,
// so with a model in hand each file OWNED BY A UNIT is seeded from that unit's
// globals (`by_unit`) and sees nothing another project declared -- an owned
// unit that declared none seeds from nothing at all.
//
// A file NO unit owns is the separate case: there is no compilation to read
// boundaries from, so it falls open to the repo-wide pool, exactly as a resolve
// with no model at all does. That is the documented over-approximation this
// resolver has always used, and it is the only answer that does not silently
// strip a loose file of every global using in the tree.
fn build_file_contexts(
    fragments_by_file: &[(String, Fragment)],
    repo_wide: &GlobalUsings,
    by_unit: Option<UnitGlobals<'_>>,
) -> HashMap<String, FileContext> {
    let mut contexts = HashMap::new();
    // The seed for a file whose OWNING unit declared no `global using` at all:
    // built once here so the match below can hand back a reference with the
    // same lifetime as the real pools.
    let empty: GlobalUsings = (HashSet::new(), HashMap::new());
    for (file, frag) in fragments_by_file {
        let seed = match by_unit {
            Some((unit_of_file, by_unit)) => match unit_of_file.get(file).copied().flatten() {
                Some(u) => by_unit.get(&u).unwrap_or(&empty),
                None => repo_wide,
            },
            None => repo_wide,
        };
        let mut usings = seed.0.clone();
        let mut aliases = seed.1.clone();
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
// The same walk as `inheritance_walk_matches`, returning the matched def's
// OWN index instead of a bare bool -- the primitive both that function and
// the `receiver_base` bases-only lookup below are built on, so the walk
// algorithm (cycle guard, lazy per-def base resolution) lives in exactly one
// place.
fn inheritance_walk_find(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    mut matches: impl FnMut(usize) -> bool,
) -> Option<usize> {
    let mut seen: HashSet<usize> = HashSet::from([start]);
    let mut stack: Vec<usize> = vec![start];
    while let Some(cur) = stack.pop() {
        if matches(cur) {
            return Some(cur);
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
                resolve_ref(&probe, &ctx.usings, &ns, index, &ctx.aliases, file_contexts)
            {
                if seen.insert(bidx) {
                    stack.push(bidx);
                }
            }
        }
    }
    None
}

fn inheritance_walk_matches(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    matches: impl FnMut(usize) -> bool,
) -> bool {
    inheritance_walk_find(index, file_contexts, start, matches).is_some()
}

fn inherited_member_declared(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
) -> bool {
    inheritance_walk_matches(index, file_contexts, start, |idx| {
        declares_member(index, idx, member, arg_count)
    })
}

// The recursive worker `first_base_declaring` drives (Unit A4 item 1): `cur`
// is a def already known to be in-graph and, when `skip_interfaces`, already
// known not to be an interface. Checked by `declares` FIRST -- so a base
// that itself declares the member wins before its own bases are even looked
// at -- then, only if that misses, each of `cur`'s OWN in-graph bases in
// turn, EACH FULLY EXPLORED (this function calls itself) before the next
// sibling base is even resolved: true depth-first, declaration order, the
// first base string's entire subtree ahead of the second. Class bases are
// tried before any interface AT EVERY LEVEL (not just `cur`'s own direct
// bases -- every recursive call repeats the same split), and when
// `skip_interfaces` an interface base is dropped ENTIRELY, its own closure
// never walked either, so a class-typed receiver can never bind to an
// interface's member declaration at any depth -- not only among `start`'s
// direct bases, which is as far as the walk this replaces reached.
//
// `seen` is per BRANCH (see `first_base_declaring`'s own call site, which
// seeds a fresh set for each of `start`'s direct bases): a cycle within one
// direct base's own closure cannot re-enter that closure, but two SIBLING
// direct bases sharing a common ancestor each see it once, from their own
// branch -- exactly the guard the walk this replaces already gave. Each
// branch's set also holds `start` itself from the outset, so a hierarchy
// that names its own descendant (`class A : B`, `class B : A` -- invalid
// C#, but a shape this parser reads happily) can never walk back INTO the
// type the lookup started from and answer with it.
fn declares_in_base_closure(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    cur: usize,
    skip_interfaces: bool,
    seen: &mut HashSet<usize>,
    declares: &mut impl FnMut(&DefIndex, usize) -> bool,
) -> Option<usize> {
    if declares(index, cur) {
        return Some(cur);
    }
    let ctx = file_contexts.get(&index.defs[cur].file)?;
    let ns = index.defs[cur].namespace.clone();
    let mut classes: Vec<usize> = Vec::new();
    let mut interfaces: Vec<usize> = Vec::new();
    for base in &index.member_lists[cur].bases {
        let probe = name_probe(base.clone(), &ns, Vec::new());
        let Resolution::Resolved(bidx, _) =
            resolve_ref(&probe, &ctx.usings, &ns, index, &ctx.aliases, file_contexts)
        else {
            continue;
        };
        if !seen.insert(bidx) {
            continue;
        }
        if index.defs[bidx].kind == "interface" {
            if skip_interfaces {
                continue;
            }
            interfaces.push(bidx);
        } else {
            classes.push(bidx);
        }
    }
    for bidx in classes.into_iter().chain(interfaces) {
        if let Some(found) =
            declares_in_base_closure(index, file_contexts, bidx, skip_interfaces, seen, declares)
        {
            return Some(found);
        }
    }
    None
}

// The shared shape both `base_member_declared` and the typed-receiver
// precise tier's own base walk need: never `start` itself, only its OWN
// direct bases -- read off `start`'s `MemberLists.bases`, in DECLARATION
// order -- each fully explored (`declares_in_base_closure`) before the next
// sibling base is even resolved, so a member declared on the base of the
// base still resolves, and the FIRST base string in the source always wins
// over a later one when both would otherwise answer (Unit A4 item 1 --
// `inheritance_walk_find`'s LIFO stack, which this no longer uses, visited
// bases in REVERSE declaration order). Returns the first in-graph def,
// across that ordered search, for which `declares` answers true; `None`
// when `start` resolves to nothing in-graph, when it declares no in-graph
// base, or when no in-graph base's closure satisfies `declares` at all.
//
// `skip_interfaces` drops a base whose resolved def is itself an `interface`
// -- and never walks into its closure either -- at EVERY depth the walk
// reaches, not only among `start`'s own direct bases: `declares_in_base_closure`
// re-applies the same rule at every recursive level. This is
// `base_member_declared`'s own rule (a `base.` qualifier never names an
// interface member; an interface can only ever extend other interfaces, so
// skipping the whole base is equivalent to skipping its closure), and both
// of this function's current callers pass `true`.
fn first_base_declaring(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    skip_interfaces: bool,
    mut declares: impl FnMut(&DefIndex, usize) -> bool,
) -> Option<usize> {
    let ctx = file_contexts.get(&index.defs[start].file)?;
    let ns = index.defs[start].namespace.clone();
    let mut classes: Vec<usize> = Vec::new();
    let mut interfaces: Vec<usize> = Vec::new();
    for base in &index.member_lists[start].bases {
        let probe = name_probe(base.clone(), &ns, Vec::new());
        let Resolution::Resolved(bidx, _) =
            resolve_ref(&probe, &ctx.usings, &ns, index, &ctx.aliases, file_contexts)
        else {
            continue;
        };
        if index.defs[bidx].kind == "interface" {
            if skip_interfaces {
                continue;
            }
            interfaces.push(bidx);
        } else {
            classes.push(bidx);
        }
    }
    for bidx in classes.into_iter().chain(interfaces) {
        // Seeded with `start` as well as the branch's own root: this walk
        // answers "which BASE declares it", so the starting type is out of
        // bounds however a cyclic hierarchy leads back to it.
        let mut seen: HashSet<usize> = HashSet::from([start, bidx]);
        if let Some(found) = declares_in_base_closure(
            index,
            file_contexts,
            bidx,
            skip_interfaces,
            &mut seen,
            &mut declares,
        ) {
            return Some(found);
        }
    }
    None
}

// The `receiver_base == true` lookup (`base.M`): `first_base_declaring` with
// `skip_interfaces = true` (`base.` never names an interface member, at any
// depth) and `declares_member_any_visibility` (a `base.` site is, by
// construction, lexically inside the hierarchy it is walking, so a protected
// or internal member is exactly as reachable as a public one), arity-gated
// by the ref's own `arg_count` (Unit A4 item 2) exactly like the
// typed-receiver walk below. `None` is the ordinary external-receiver
// answer to the caller, never a candidate for a scored guess.
fn base_member_declared(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
) -> Option<usize> {
    first_base_declaring(index, file_contexts, start, true, |index, idx| {
        declares_member_any_visibility(index, idx, member, arg_count)
    })
}

// The typed-receiver precise tier's own base walk (Unit A3 item 4): when a
// resolved receiver def does not itself declare the member, the first def
// in its in-graph base closure that does is the precise target -- exactly
// the widening `base_member_declared` already does for `base.`, applied to
// an ORDINARY typed receiver, `skip_interfaces = true` for the same reason
// `base_member_declared` skips them, at every depth (Unit A4 item 1): an
// interface's own method declaration has no body of its own to be the
// target of an ordinary call (a C# 8+ default interface implementation is
// indistinguishable from an abstract one at this def's own record, so
// neither is treated as a precise bind target here) -- see
// `stage3_veto_a_member_declared_by_the_receivers_interface_beats_a_matching_visible_extension`,
// which pins exactly this: an interface-only ancestor must NOT earn a
// precise edge, only veto the extension tier (which reads the closure
// itself, not this function). `any_visibility` is the caller's own answer
// to "is this receiver the enclosing type itself" (the `this.` shape,
// `receiver_type == outer_types.last()`): `true` walks
// `declares_member_any_visibility`, `false` keeps the public-only
// `declares_member`, so a receiver typed by anything OTHER than the
// enclosing type can only ever bind to a member C# would let it see from
// outside. `arg_count` is the ref's own call-shape fact (Unit A4 item 2): a
// base that declares the name at the WRONG arity is skipped exactly like
// one that does not declare it at all.
fn typed_receiver_base_member(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
    any_visibility: bool,
) -> Option<usize> {
    first_base_declaring(index, file_contexts, start, true, |index, idx| {
        if any_visibility {
            declares_member_any_visibility(index, idx, member, arg_count)
        } else {
            declares_member(index, idx, member, arg_count)
        }
    })
}

// Unit A3 item 3: the extension bucket key tier (f) tries when the exact
// `"{member} {receiverType}"` key names no bucket at all. Walks the
// receiver's own nominal closure -- itself first, then its in-graph bases
// transitively, the same DFS `inheritance_walk_find` uses everywhere else --
// and at each visited def tries that def's own NAME as a key, then every RAW
// base string it declares (an external interface included, whether or not
// that name resolves in-graph: an extension's `thisType` is written against
// the interface's bare name, and a raw base string is exactly that name,
// unresolved or not). First key with an existing bucket wins and the walk
// stops; which CANDIDATE within that bucket is right is still decided by
// the caller's own unchanged arity/namespace/admission filters and veto --
// this function only ever widens which key is looked up, never which
// candidates a matched key returns.
//
// Unit A5 item 2: also returns the MATCHED node's own generic-argument
// picture, since a key widened onto a base or ancestor names a DIFFERENT
// type than the receiver -- the receiver's own type arguments (`r.receiver_
// args`) describe the receiver, not the matched node, and comparing the
// extension's `this`-parameter arguments against them is only correct on
// the exact-key path, never here:
//   - matched via one of `idx`'s own RAW base strings: `idx`'s own
//     `base_generic_args` entry for that exact base -- the arguments `idx`
//     declared THAT base with (`*` for a pass-through of `idx`'s own type
//     parameters), absent entirely when the base carries no type-argument
//     list at all (`raw_base_generic_args`'s own rule).
//   - matched via `idx`'s own bare NAME (no base list is involved -- `idx`
//     IS the matched node): a wildcard per `idx`'s own type parameter,
//     `None` when `idx` is not generic at all -- so a non-generic matched
//     node unifies with a non-generic `this` parameter regardless of what
//     the receiver's own arguments were.
fn extension_closure_key(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: &str,
) -> Option<(String, Option<Vec<String>>)> {
    let mut found: Option<(String, Option<Vec<String>>)> = None;
    inheritance_walk_find(index, file_contexts, start, |idx| {
        let own_key = format!("{member} {}", index.defs[idx].name);
        if index.extension_index.contains_key(&own_key) {
            let type_params = index.member_lists[idx].type_params.len();
            let args = if type_params == 0 {
                None
            } else {
                Some(vec!["*".to_string(); type_params])
            };
            found = Some((own_key, args));
            return true;
        }
        for base in &index.member_lists[idx].bases {
            let base_key = format!("{member} {base}");
            if index.extension_index.contains_key(&base_key) {
                let args = index.member_lists[idx]
                    .base_generic_args
                    .iter()
                    .find(|(k, _)| k == base)
                    .map(|(_, v)| v.clone());
                found = Some((base_key, args));
                return true;
            }
        }
        false
    });
    found
}

// `true` exactly for the `this.` shape (precision rule (a)): a `uses-member`
// ref whose recorded receiver type IS the innermost `outer_types` entry --
// the enclosing type itself, whether the qualifier was literally `this.` or
// an ordinary same-typed local/parameter/field (C#'s own private/protected
// access rule reaches every expression of the declaring type from within
// its own members, not only `this`). `false` whenever `receiver_type` is
// unset (nothing to compare) or names a different type.
fn is_this_shaped_receiver(r: &FragRef) -> bool {
    match (&r.receiver_type, r.outer_types.last()) {
        (Some(rt), Some(outer)) => rt == outer,
        _ => false,
    }
}

// One def's own field or property fact for `name`, field_types tried first
// -- the order the caller's doc comment names. Never widens to the def's
// bases; the walk that does is the caller's job.
//
// The declaring FILE rides along with the fact (see
// `MemberLists::property_type_files`), because a type NAME is only
// meaningful under that file's usings and aliases. A table built without
// the companion map -- every `MemberLists` a test assembles by hand -- falls
// back to the def's own first-declaring file, which is what a single-file
// def has anyway.
fn declared_field_or_property_type<'a>(
    index: &'a DefIndex,
    idx: usize,
    name: &str,
) -> Option<(&'a FragFact, &'a str)> {
    let lists = &index.member_lists[idx];
    let (fact, files) = match lists.field_types.get(name) {
        Some(fact) => (fact, &lists.field_type_files),
        None => (lists.property_types.get(name)?, &lists.property_type_files),
    };
    let file = files
        .get(name)
        .map_or(index.defs[idx].file.as_str(), String::as_str);
    Some((fact, file))
}

// A bare-identifier receiver's type as some def's field or property table
// answered it, with everything the answer needs to be read correctly: the
// def that declares the member (its namespace and nesting chain) and the
// FILE whose declaration wrote the type name down (its usings and aliases).
struct ReceiverFieldType {
    type_name: String,
    declaring_def: usize,
    declaring_file: String,
}

// The enclosing-type chain a ref written INSIDE `def`'s own body carries
// (`FragRef::outer_types`): every nesting level from the outermost in,
// ending with the def itself. A def id spells nesting exactly that way --
// the namespace, a dot, then the chain joined with "+", which is how
// `resolve_ref`'s step 0b rebuilds an id from a ref's chain -- so the chain
// is the id with its namespace prefix taken off. A namespace-level def
// yields a one-entry chain holding its own name.
fn def_outer_types(def: &Def) -> Vec<String> {
    let chain = if def.namespace.is_empty() {
        def.id.as_str()
    } else {
        def.id
            .strip_prefix(&format!("{}.", def.namespace))
            .unwrap_or(def.name.as_str())
    };
    chain.split('+').map(str::to_string).collect()
}

// The bare-identifier receiver lookup a ref with NO in-file fact at all
// falls back to: the innermost `outer_types` def's OWN merged field_types,
// then property_types (a partial class's sibling-file field/property, the
// current file cannot see for itself), then the same two tables on each
// in-graph base of that def, in DECLARATION order, each followed by its own
// inheritance walk -- exactly `base_member_declared`'s structure, except
// this lookup checks `start` itself FIRST (unlike `base.`, an ordinary bare
// identifier's own enclosing type is exactly where its fields live).
// `None` when `outer_types` is empty (no enclosing type, so no field/
// property table to consult), when the innermost entry does not resolve
// in-graph, or when neither table on `start` nor on any in-graph base
// answers for the name.
//
// The answer names the def and the file the fact came FROM, not the site
// that read it: the type name is a bare identifier, and the caller has to
// resolve it under the usings, aliases, namespace and nesting of the
// declaration that wrote it down.
fn bare_receiver_field_or_property_type(
    index: &DefIndex,
    ns: &str,
    r: &FragRef,
    usings: &HashSet<String>,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
) -> Option<ReceiverFieldType> {
    let innermost = r.outer_types.last()?;
    let probe = name_probe(innermost.clone(), ns, r.outer_types.clone());
    let Resolution::Resolved(start, _) =
        resolve_ref(&probe, usings, ns, index, aliases, file_contexts)
    else {
        return None;
    };
    if let Some((fact, declaring_file)) = declared_field_or_property_type(index, start, &r.name) {
        return Some(ReceiverFieldType {
            type_name: fact.type_name.clone(),
            declaring_def: start,
            declaring_file: declaring_file.to_string(),
        });
    }
    let ctx = file_contexts.get(&index.defs[start].file)?;
    let base_ns = index.defs[start].namespace.clone();
    for base in &index.member_lists[start].bases {
        let probe = name_probe(base.clone(), &base_ns, Vec::new());
        if let Resolution::Resolved(bidx, _) = resolve_ref(
            &probe,
            &ctx.usings,
            &base_ns,
            index,
            &ctx.aliases,
            file_contexts,
        ) {
            let mut found: Option<ReceiverFieldType> = None;
            inheritance_walk_find(index, file_contexts, bidx, |idx| {
                match declared_field_or_property_type(index, idx, &r.name) {
                    Some((fact, declaring_file)) => {
                        found = Some(ReceiverFieldType {
                            type_name: fact.type_name.clone(),
                            declaring_def: idx,
                            declaring_file: declaring_file.to_string(),
                        });
                        true
                    }
                    None => false,
                }
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

// The scored tier's own receiver test, and the mirror image of the veto above:
// that one asks whether an in-graph receiver ALREADY declares the member (so a
// guess would be wrong); this one asks whether a candidate is a type the
// receiver could even be, which is the question an EXTERNAL receiver leaves
// open. C# binds `x.M(...)` only to a member of a type `x` is assignable to, so
// a candidate the receiver type cannot reach is a disproved guess rather than a
// weak one.
//
// True when `start` IS `type_name`, when any def in its in-graph base closure
// is, or when any def in that closure lists `type_name` as a RAW base string.
// The last case carries the weight: a receiver typed by an external interface
// has no def to walk to, so the only evidence available is the base name the
// candidate wrote down. `bases` holds bare identifiers (a base written
// `System.IDisposable` is recorded as `IDisposable`) and a receiver fact's type
// name is bare the same way, so the two strings meet without either side being
// resolved.
//
// `args_known` says whether the receiver's type ARGUMENTS are known at all. A
// receiver read off a declaration carries both halves of the fact, so
// `ILogger<Worker>` must not accept a candidate whose base is the non-generic
// `ILogger`. A receiver inferred from a method's recorded RETURN type carries a
// name and nothing else, and refusing every generic implementation on the
// strength of an absence would be reading a fact the extractor never recorded --
// so that case compares names only.
fn nominally_assignable(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    type_name: &str,
    receiver_args: Option<&Vec<String>>,
    args_known: bool,
) -> bool {
    inheritance_walk_matches(index, file_contexts, start, |idx| {
        (index.defs[idx].name == type_name
            && (!args_known
                || index.member_lists[idx].type_params.len() == receiver_args.map_or(0, Vec::len)))
            || index.member_lists[idx].bases.iter().any(|b| {
                b == type_name
                    && (!args_known
                        || generic_args_unify(
                            index.member_lists[idx]
                                .base_generic_args
                                .iter()
                                .find(|(k, _)| k == b)
                                .map(|(_, v)| v),
                            receiver_args,
                        ))
            })
    })
}

/// Memo for `nominally_assignable`, one per resolve run. The walk is a
/// transitive base closure with a ladder resolution at every hop, and a corpus
/// asks the same `(candidate, receiver type)` question once per call site, so
/// the answer is cached rather than recomputed. `args_known` is part of the key
/// because it changes the answer for the same receiver name: an absent
/// type-argument list means "no arguments" when the fact is a declaration and
/// "unknown" when it is a return type.
type AssignabilityCache = HashMap<(usize, String, Option<Vec<String>>, bool), bool>;

fn nominally_assignable_cached(
    cache: &mut AssignabilityCache,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    type_name: &str,
    receiver_args: Option<&Vec<String>>,
    args_known: bool,
) -> bool {
    let key = (
        start,
        type_name.to_string(),
        receiver_args.cloned(),
        args_known,
    );
    if let Some(&answer) = cache.get(&key) {
        return answer;
    }
    let answer = nominally_assignable(
        index,
        file_contexts,
        start,
        type_name,
        receiver_args,
        args_known,
    );
    cache.insert(key, answer);
    answer
}

// The whole receiver rule for ONE scored candidate, applied only when the ref
// carries a receiver type that resolved to nothing in-graph. Two ways in, and a
// candidate needs just one of them:
//
//   - as an INSTANCE member: the candidate declares the member (per the ref's
//     shape) AND is nominally assignable to the receiver type.
//   - as an EXTENSION method: the candidate declares an extension of that
//     member name whose `this` parameter is the receiver type EXACTLY, type
//     arguments unified -- the same (member, thisType) key and the same
//     unification tier (f) uses. Tier (f) declined this ref for one of its own
//     reasons (most often the namespace test, which is narrower than the
//     language), and re-admitting the candidate HERE, as a guess, is the
//     honest answer: the this-parameter is direct evidence about this exact
//     receiver type, which is more than the uniqueness pool alone ever had.
//
// The two are OR-ed rather than tried in order because an extension method is
// also an ordinary public static method, so the static class holding it
// vouches through `methods` too -- requiring assignability of a candidate that
// merely LOOKS instance-vouched would refuse every extension there is.
//
// Nine parameters, deliberately: every one is a distinct fact about the ONE
// question asked here, and bundling them into a struct built per candidate
// would add an allocation and a second name for each field without making any
// caller shorter -- there is exactly one caller.
#[allow(clippy::too_many_arguments)]
fn receiver_admits_candidate(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    cache: &mut AssignabilityCache,
    candidate: usize,
    member: &str,
    shape: MemberShape,
    receiver_type: &str,
    receiver_args: Option<&Vec<String>>,
    args_known: bool,
) -> bool {
    let instance_vouches = match shape {
        MemberShape::Call => index.defs[candidate].methods.iter().any(|m| m == member),
        MemberShape::Read => declares_member(index, candidate, Some(member), None),
    };
    if instance_vouches
        && nominally_assignable_cached(
            cache,
            index,
            file_contexts,
            candidate,
            receiver_type,
            receiver_args,
            args_known,
        )
    {
        return true;
    }
    index
        .extension_index
        .get(&format!("{member} {receiver_type}"))
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .any(|c| {
            c.def_idx == candidate && generic_args_unify(c.entry.this_args.as_ref(), receiver_args)
        })
}

fn nested_candidate_visible_from_site(
    ref_: &FragRef,
    ns: &str,
    candidate: usize,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> bool {
    let Some((enclosing_id, _)) = index.defs[candidate].id.rsplit_once('+') else {
        return true;
    };
    let Some(&enclosing_candidate) = index.qualified_name_to_def.get(enclosing_id) else {
        return false;
    };

    for depth in (1..=ref_.outer_types.len()).rev() {
        let enclosing_site = ref_.outer_types[..depth].join("+");
        let enclosing_site = if ns.is_empty() {
            enclosing_site
        } else {
            format!("{ns}.{enclosing_site}")
        };
        if let Some(&site_idx) = index.qualified_name_to_def.get(&enclosing_site) {
            return inheritance_walk_matches(index, file_contexts, site_idx, |idx| {
                idx == enclosing_candidate
            });
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

/// The per-unit half of the `global using` picture, as `build_file_contexts`
/// takes it: which unit owns each file, and each unit's own pool.
type UnitGlobals<'a> = (
    &'a HashMap<String, Option<usize>>,
    &'a HashMap<usize, GlobalUsings>,
);

/// One pool of `global using` facts: the plain namespaces, and the aliases
/// keyed by alias name. Used both repo-wide and per project unit.
type GlobalUsings = (HashSet<String>, HashMap<String, String>);

// Every `global using` in the fragment set, collected twice over the same
// single pass: once repo-wide (what a resolve with no project model uses, and
// what a file no project owns falls back to) and once per owning unit (what a
// resolve WITH a model uses, because a global using is a per-compilation fact).
//
// A file whose `unit_of_file` entry is absent or `None` contributes to the
// repo-wide pool only: its globals are real, but there is no project to
// attribute them to, and inventing one would leak them into whichever project
// happened to be nearest.
fn collect_global_usings_by_unit(
    fragments_by_file: &[(String, Fragment)],
    unit_of_file: &HashMap<String, Option<usize>>,
) -> (GlobalUsings, HashMap<usize, GlobalUsings>) {
    let mut repo_wide: GlobalUsings = (HashSet::new(), HashMap::new());
    let mut by_unit: HashMap<usize, GlobalUsings> = HashMap::new();
    for (file, frag) in fragments_by_file {
        let unit = unit_of_file.get(file).copied().flatten();
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
                        // vacant slot. The per-unit pools apply the same rule
                        // within their own scope, so a unit's own first
                        // declaration wins there even if some other unit
                        // declared that alias earlier in file order.
                        repo_wide
                            .1
                            .entry(alias.clone())
                            .or_insert_with(|| target.clone());
                        if let Some(idx) = unit {
                            by_unit
                                .entry(idx)
                                .or_default()
                                .1
                                .entry(alias.clone())
                                .or_insert_with(|| target.clone());
                        }
                    }
                }
                FragUsing::Plain { text, global } => {
                    if *global {
                        repo_wide.0.insert(text.clone());
                        if let Some(idx) = unit {
                            by_unit.entry(idx).or_default().0.insert(text.clone());
                        }
                    }
                }
            }
        }
    }
    (repo_wide, by_unit)
}

// ---------------------------------------------------------------------------
// Admission: the project model's veto over the two HEURISTIC tiers.
// ---------------------------------------------------------------------------

/// The structural gate the two heuristic tiers consult before naming a def.
///
/// A heuristic tier guesses from a member NAME; the project model is the one
/// fact available here that can disprove such a guess without reading a single
/// line of the candidate's body -- the site's assembly could not reference the
/// candidate's assembly, so the call the guess describes could not compile,
/// whatever the name says.
///
/// Two refusals, both structural:
///   - REACHABILITY: the candidate's project is not on the transitive
///     `ProjectReference` closure of the site's project.
///   - TEST DIRECTION: the candidate's project is a test project and the
///     site's is not. Production code never calls into a test assembly, and
///     this half catches the fixture/helper classes that carry no test
///     attribute of their own and so are invisible to def-level test
///     detection.
///
/// Everything else FAILS OPEN, deliberately and in three places: no model at
/// all (a repo with no `.csproj`), a site file no project owns, and a
/// candidate file no project owns. Ownership here is path-based and knows
/// nothing about linked or globbed `Compile Include` items, so an ownership
/// answer this resolver could not compute must never delete an edge it would
/// otherwise have emitted.
///
/// Only the heuristic tiers consult it. The precise tiers resolve a type
/// first and emit on a FACT, and the ctor-DI resolver picks an implementor
/// from an interface the site demonstrably names -- neither is a guess the
/// model is entitled to overrule.
struct Admission<'m> {
    model: Option<&'m crate::project::ProjectModel>,
    /// `unit_of_def[i]` is the unit owning `index.defs[i]`'s declaring file,
    /// computed once per resolve rather than per candidate. Always `None`
    /// when there is no model.
    unit_of_def: Vec<Option<usize>>,
}

impl Admission<'_> {
    fn admits(&self, site_unit: Option<usize>, cand: usize) -> bool {
        let Some(model) = self.model else {
            return true;
        };
        let (Some(site), Some(cand)) = (site_unit, self.unit_of_def.get(cand).copied().flatten())
        else {
            return true;
        };
        model.reachable(site, cand) && !(model.units[cand].test && !model.units[site].test)
    }
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
    /// Several same-named defs the ladder refused to choose between, plus the
    /// step that pooled them -- only steps 2 and 4 can produce this, so the
    /// `Via` is always `Usings` or `Global`. It rides along so
    /// `narrow_by_reachability` can hand back a `Resolved` carrying the step
    /// that actually answered instead of inventing one: the uses-member
    /// emission tier reads that step (`via == Via::Qualified`) as one of its
    /// type-certainty signals, and a narrowed resolution must be judged by the
    /// same rule as any other.
    Ambiguous(Vec<usize>, Via),
    External,
}

/// The project model's answer to an ambiguity the ladder could not settle:
/// C# cannot name a type in an assembly this one does not reference, so such
/// a candidate was never really a candidate. Applied at the ladder's THREE
/// `Ambiguous` consumers rather than inside `resolve_ref`, because the ladder
/// is a pure name-resolution function that knows nothing about projects and
/// because two of its callers -- the base-closure probes and the ctor-DI
/// resolver -- must keep seeing the unnarrowed answer.
///
/// Every other resolution passes through untouched, and so does every
/// candidate when there is no model (`Admission::admits` then says yes to
/// everything), which is what keeps a csproj-less repo's graph byte-identical.
///
/// One survivor is a FACT, not a guess: the ambiguity was only ever the
/// ladder's refusal to choose, and the reference rule chose for it. Zero
/// survivors is an ordinary `External` -- the same answer the ladder gives for
/// a name it never found, which is exactly what a name whose every candidate
/// is out of reach IS. Two or more stay ambiguous on the FILTERED list, so the
/// reported candidates and `candidate_count` shrink together.
fn narrow_by_reachability(
    res: Resolution,
    site_unit: Option<usize>,
    admission: &Admission,
) -> Resolution {
    let Resolution::Ambiguous(candidates, via) = res else {
        return res;
    };
    let reachable: Vec<usize> = candidates
        .into_iter()
        .filter(|&c| admission.admits(site_unit, c))
        .collect();
    match reachable.as_slice() {
        [] => Resolution::External,
        [idx] => Resolution::Resolved(*idx, via),
        _ => Resolution::Ambiguous(reachable, via),
    }
}

/// A narrowed resolution plus the one bit narrowing would otherwise destroy:
/// whether an `External` means "the ladder never found this name" or "the
/// ladder found candidates and the project model put every one of them out of
/// reach".
///
/// The two are the same answer for a precise tier -- neither can produce an
/// edge -- but they are opposite answers for the scored tier. A name the
/// ladder never found may still be a member-name-uniqueness guess. A name
/// whose every candidate was narrowed away has already been ANSWERED: the
/// candidates were real, and the language rule says none of them is nameable
/// here. Falling through to the graph-wide uniqueness pool there would answer
/// a settled question with a stranger, so `narrowed_away` gets an empty pool
/// and emits nothing.
struct Narrowed {
    res: Resolution,
    narrowed_away: bool,
}

/// `narrow_by_reachability`, keeping the pre-narrowing shape as the flag
/// `Narrowed` documents. Used at the two `uses-member` consumers, whose
/// resolutions reach the scored tier; the plain type-reference consumer has no
/// heuristic tier behind it and calls `narrow_by_reachability` directly.
fn narrow_tracked(res: Resolution, site_unit: Option<usize>, admission: &Admission) -> Narrowed {
    let was_ambiguous = matches!(res, Resolution::Ambiguous(..));
    let res = narrow_by_reachability(res, site_unit, admission);
    Narrowed {
        narrowed_away: was_ambiguous && matches!(res, Resolution::External),
        res,
    }
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
    file_contexts: &HashMap<String, FileContext>,
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
        return Resolution::Ambiguous(using_matches, Via::Usings);
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
    // class's previously-unambiguous references ambiguous. Nested definitions
    // remain in the pool, but a bare reference can see one only when its
    // enclosing type inherits from the nested definition's enclosing type.
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
        .filter(|idx| {
            ref_.qualified.is_some()
                || nested_candidate_visible_from_site(ref_, ns, *idx, index, file_contexts)
        })
        .collect();
    match matches.as_slice() {
        [idx] => Resolution::Resolved(*idx, Via::Global),
        [_, _, ..] => Resolution::Ambiguous(matches, Via::Global),
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
    match resolve_ref(ref_, usings, ns, index, aliases, file_contexts) {
        Resolution::Ambiguous(candidate_indices, _) => {
            CtorDiResolution::Ambiguous(candidate_indices)
        }
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
                    file_contexts,
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
// serialize to the same bytes -- which is why `tier` and `member` join the
// key the moment they join the edge: two guesses that name DIFFERENT members
// of the same target on one line are two distinct facts now, and collapsing
// them would drop one.
fn heuristic_edge_key(e: &Edge) -> Option<String> {
    let (kind, from_file, from_line, to, to_file, tier, member) = match e {
        Edge::Inherits {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("inherits", from_file, from_line, to, to_file, None, None),
        Edge::UsesType {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("uses-type", from_file, from_line, to, to_file, None, None),
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
            tier,
            member,
        } => (
            "uses-member",
            from_file,
            from_line,
            to,
            to_file,
            *tier,
            member.as_deref(),
        ),
        _ => return None,
    };
    let tier = match tier {
        Some(HeuristicTier::Ext) => "ext",
        Some(HeuristicTier::Guess) => "guess",
        None => "-",
    };
    let member = member.unwrap_or("-");
    Some(format!(
        "{kind} {from_file} {from_line} {to} {to_file} {tier} {member}"
    ))
}

/// Resolve C# fragments into a graph. Pure: `fragments_by_file` is
/// file-walk-ordered `(rel, Fragment)` pairs. No file I/O beyond the single
/// `git rev-parse HEAD` shell-out (`manifest::git_head`) that fills
/// `built_at_head` -- cheap enough to re-run on every `devscout map` whose C#
/// set changed, and unit-testable without a parser (see this module's tests,
/// which build `Fragment` values by hand).
pub fn resolve_graph(root: &Path, fragments_by_file: &[(String, Fragment)]) -> Graph {
    resolve_graph_with_model(root, fragments_by_file, &[], None)
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
    resolve_graph_with_model(root, fragments_by_file, ts_fragments_by_file, None)
}

/// The same resolve again, now with the repo's `.csproj` project model
/// alongside -- `devscout map`'s entry point, and the only one that can
/// produce a graph carrying `units`. The other two wrap this one with `None`.
///
/// `model` is `None` for a repo that declares no `.csproj`, and a `None`
/// model must leave the resolve BYTE-IDENTICAL to what it was: `units` is
/// omitted when empty, so the whole artifact is unchanged for such a tree.
pub fn resolve_graph_with_model(
    root: &Path,
    fragments_by_file: &[(String, Fragment)],
    ts_fragments_by_file: &[(String, crate::extract::TsFragment)],
    model: Option<&crate::project::ProjectModel>,
) -> Graph {
    let index = build_def_index(fragments_by_file);
    // Ownership, resolved once for every fragment file and then once for every
    // def through its declaring file. `None` for every entry when there is no
    // model, which is what makes every model-dependent rule below a no-op for
    // a repo that declares no `.csproj`.
    let unit_of_file: HashMap<String, Option<usize>> = fragments_by_file
        .iter()
        .map(|(file, _)| (file.clone(), model.and_then(|m| m.unit_of_file(file))))
        .collect();
    let unit_of_def: Vec<Option<usize>> = index
        .defs
        .iter()
        .map(|d| unit_of_file.get(&d.file).copied().flatten())
        .collect();
    let admission = Admission { model, unit_of_def };
    let (repo_wide_globals, globals_by_unit) =
        collect_global_usings_by_unit(fragments_by_file, &unit_of_file);
    let file_contexts = build_file_contexts(
        fragments_by_file,
        &repo_wide_globals,
        model.map(|_| (&unit_of_file, &globals_by_unit)),
    );
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
    // The same total, split by emitting tier. Kept beside the total rather
    // than derived from the edge array afterwards so the dedup below can
    // decrement both in one place and neither can drift.
    let mut heuristic_by_tier = HeuristicByTier::default();
    // One memo for the whole run: the receiver rule below asks the same
    // "is this candidate assignable to this receiver type" question once per
    // call site, and the answer is a base-closure walk.
    let mut assignable_cache: AssignabilityCache = HashMap::new();

    for (file, frag) in fragments_by_file {
        // Local alias shadows a same-named global one -- see
        // build_file_contexts, which builds every file's context once up front
        // so the veto walk can read a DIFFERENT file's context too.
        let FileContext { usings, aliases } = &file_contexts[file];
        // The project this file belongs to, if any -- the left-hand side of
        // every admission question the two heuristic tiers ask below.
        let site_unit = unit_of_file.get(file).copied().flatten();

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
                let Narrowed {
                    res: result,
                    narrowed_away: result_narrowed_away,
                } = narrow_tracked(
                    resolve_ref(r, usings, ns, &index, aliases, &file_contexts),
                    site_unit,
                    &admission,
                );
                let mut emitted = false;
                // `base.M`: a receiver_base ref's `result` names the
                // ENCLOSING type (the same resolution a plain `this.M` ref
                // gets, from the same `receiver_type`/`name`), and this rule
                // exists precisely so that type is never consulted for the
                // member -- only its OWN bases, in declaration order, each
                // with its own in-graph inheritance walk (see
                // `base_member_declared`). `emitted` is forced `true`
                // regardless of whether a base declared the member, which is
                // what keeps every tier below (e, e2, f, scored) from ever
                // treating a `base.` ref as an ordinary receiver fact: falling
                // through to them would let the enclosing type's OWN
                // `receiver_type` reintroduce the exact self-edge this rule
                // forbids, or let the scored tier guess where the design
                // requires silent external.
                if r.receiver_base {
                    if let Resolution::Resolved(start, _) = &result {
                        if let Some(target) = base_member_declared(
                            &index,
                            &file_contexts,
                            *start,
                            r.member.as_deref(),
                            r.arg_count,
                        ) {
                            edges.push(Edge::uses_member(
                                file.clone(),
                                r.line,
                                index.defs[target].id.clone(),
                                index.defs[target].file.clone(),
                                r.member.clone(),
                                None,
                            ));
                            edges_by_kind.uses_member += 1;
                        }
                    }
                    emitted = true;
                } else if let Resolution::Resolved(idx, via) = &result {
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
                        edges.push(Edge::uses_member(
                            file.clone(),
                            r.line,
                            to,
                            to_file,
                            r.member.clone(),
                            None,
                        ));
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
                        //
                        // `this_shaped` is precision rule (a)'s guard: this
                        // resolution arm is where a `this.M` ref lands (its
                        // `name` IS the enclosing type, resolved through the
                        // ordinary type ladder like any other bare type
                        // name), so a member declared non-publicly on the
                        // enclosing type itself, or on one of its bases,
                        // must still bind precisely -- Unit A3 items 1 and
                        // 4. Every other typed-qualified access reaching
                        // this arm (`SomeType.Member`, an inherited STATIC
                        // member named through a derived type) keeps the
                        // public-only walk.
                        let this_shaped = is_this_shaped_receiver(r);
                        let declares_here = if this_shaped {
                            declares_member_any_visibility(
                                &index,
                                idx,
                                r.member.as_deref(),
                                r.arg_count,
                            )
                        } else {
                            declares_member(&index, idx, r.member.as_deref(), r.arg_count)
                        };
                        if declares_here
                            || (r.generic && r.qualified.is_none())
                            || (r.qualified.is_some() && via == Via::Qualified)
                        {
                            edges.push(Edge::uses_member(
                                file.clone(),
                                r.line,
                                index.defs[idx].id.clone(),
                                index.defs[idx].file.clone(),
                                r.member.clone(),
                                None,
                            ));
                            edges_by_kind.uses_member += 1;
                            emitted = true;
                        } else if let Some(target) = typed_receiver_base_member(
                            &index,
                            &file_contexts,
                            idx,
                            r.member.as_deref(),
                            r.arg_count,
                            this_shaped,
                        ) {
                            // Unit A3 item 4: `idx` itself does not declare
                            // the member (at the visibility this receiver
                            // may see) -- the first in-graph base that does
                            // is the precise target, exactly the widening
                            // `base_member_declared` already does for
                            // `base.`, applied here to a receiver whose OWN
                            // type resolved directly rather than through a
                            // `base.` qualifier.
                            edges.push(Edge::uses_member(
                                file.clone(),
                                r.line,
                                index.defs[target].id.clone(),
                                index.defs[target].file.clone(),
                                r.member.clone(),
                                None,
                            ));
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
                // context. Tier (e) tries the EXACT receiver def first and,
                // only when that def itself does not declare the member, its
                // in-graph base closure (Unit A3 item 4, mirroring the
                // widening `base_member_declared` already does for `base.`)
                // -- public visibility, unless the receiver IS the enclosing
                // type itself (`is_this_shaped_receiver`), which may also see
                // a non-public member per precision rule (a).
                //
                // The FULL outcome is kept too, not just the def: the scored
                // tier reads its status (ambiguous vs. nothing-at-all) for any
                // ref carrying a receiver fact, since that fact IS what the
                // qualifier's type is and outranks resolving the qualifier
                // identifier itself.
                let mut receiver_def: Option<usize> = None;
                let mut receiver_result: Option<Resolution> = None;
                let mut receiver_narrowed_away = false;
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
                // Set only by the bare-identifier field/property fallback
                // below, and only so the resolution of the name it produced
                // can happen in the DECLARING file's context rather than
                // this one's. Every other way `receiver_type_name` is
                // filled reads a name off this file's own ref, so it stays
                // `None` and the site's own context is used.
                let mut receiver_field: Option<ReceiverFieldType> = None;
                if receiver_type_name.is_none() {
                    if let (Some(owner), Some(member)) =
                        (&r.receiver_call_owner, &r.receiver_call_member)
                    {
                        let probe = name_probe(owner.clone(), ns, r.outer_types.clone());
                        if let Resolution::Resolved(oidx, _) =
                            resolve_ref(&probe, usings, ns, &index, aliases, &file_contexts)
                        {
                            let returns =
                                index.member_lists[oidx].method_returns.get(member).cloned();
                            // An AWAITED callee returning `Task<T>`/
                            // `ValueTask<T>` unwraps to `T` -- exactly ONE
                            // layer, read off the same one-level generic-arg
                            // capture every other base-identifier fact keeps
                            // beside its own bare name
                            // (`method_return_args`, never re-derived from
                            // source). An UNAWAITED call keeps the bare
                            // wrapper name unchanged (`var t = x.FetchAsync();
                            // t.Wait();` must stay typed `Task`, never
                            // `Order`), and a doubly-wrapped
                            // `Task<Task<Order>>` return unwraps to the INNER
                            // `Task`'s own bare name -- never twice -- because
                            // `method_return_args` itself only ever records
                            // one level of argument base identifiers. A
                            // type-parameter pass-through ("*") is refused,
                            // the same as every other wildcard generic-arg
                            // fact in this file: nothing at THIS call site
                            // knows what it is bound to.
                            receiver_type_name = match &returns {
                                Some(name)
                                    if r.receiver_awaited
                                        && (name == "Task" || name == "ValueTask") =>
                                {
                                    match index.member_lists[oidx].method_return_args.get(member) {
                                        Some(args) if args.len() == 1 && args[0] != "*" => {
                                            Some(args[0].clone())
                                        }
                                        _ => returns,
                                    }
                                }
                                _ => returns,
                            };
                        }
                    } else if r.qualified.is_none() && !r.generic && !r.receiver_local {
                        // `receiver_type` AND `receiver_call_owner` are both
                        // `None` here, which `push_member_ref` produces in
                        // two cases it cannot tell apart from ITS OWN two
                        // fields alone: no local/parameter/field fact for the
                        // name exists in this file at all, OR one exists but
                        // is a TAKEN-BUT-UNTYPED entry (an unresolved call, a
                        // predefined type, a conflicting re-declaration --
                        // see `receiver_fact_for`'s own doc comment).
                        // `r.receiver_local` is the signal that DOES tell
                        // the two apart: `true` whenever the enclosing
                        // MEMBER's own fact table holds ANY entry for the
                        // name (typed or not -- `Scope::has_local_fact`), so
                        // a same-named local or parameter ALWAYS shadows a
                        // field here, exactly like a TYPED one already does
                        // by leaving `receiver_type` set. Bare identifier
                        // only (`r.qualified.is_none() && !r.generic`): a
                        // dotted or generic qualifier is never a field or
                        // property name. Typed from the enclosing def's OWN
                        // field and property declarations, merged across
                        // every file that declares it (a sibling
                        // partial-class file), then the same two tables
                        // walked across each in-graph base of that def, in
                        // declaration order -- the field the CURRENT file
                        // cannot see for itself.
                        receiver_field = bare_receiver_field_or_property_type(
                            &index,
                            ns,
                            r,
                            usings,
                            aliases,
                            &file_contexts,
                        );
                        receiver_type_name = receiver_field.as_ref().map(|f| f.type_name.clone());
                    }
                }
                if !emitted {
                    if let Some(receiver_type) = &receiver_type_name {
                        // A field's declared type is a bare identifier that
                        // only means what the file that WROTE it meant: its
                        // usings, its aliases, its namespace, its nesting.
                        // A fact merged in from a sibling partial-class file
                        // or read off a base in another file therefore
                        // resolves in THAT file's context -- resolving it
                        // here would let a same-named type visible only from
                        // the reading file answer for a declaration that
                        // never saw it. The site's own admission filter
                        // still applies: the edge is emitted from here, so
                        // what this project may reference is still this
                        // project's question.
                        let declaring = receiver_field.as_ref().and_then(|f| {
                            file_contexts
                                .get(&f.declaring_file)
                                .map(|ctx| (f.declaring_def, ctx))
                        });
                        let (probe_usings, probe_ns, probe_aliases, probe_outer) = match declaring {
                            Some((didx, dctx)) => (
                                &dctx.usings,
                                index.defs[didx].namespace.as_str(),
                                &dctx.aliases,
                                def_outer_types(&index.defs[didx]),
                            ),
                            None => (usings, ns, aliases, r.outer_types.clone()),
                        };
                        let probe = name_probe(receiver_type.clone(), probe_ns, probe_outer);
                        let Narrowed {
                            res: rr,
                            narrowed_away,
                        } = narrow_tracked(
                            resolve_ref(
                                &probe,
                                probe_usings,
                                probe_ns,
                                &index,
                                probe_aliases,
                                &file_contexts,
                            ),
                            site_unit,
                            &admission,
                        );
                        receiver_narrowed_away = narrowed_away;
                        if let Resolution::Resolved(ridx, _) = &rr {
                            let ridx = *ridx;
                            receiver_def = Some(ridx);
                            // Unit A3 item 4: the receiver's OWN def may not
                            // declare the member while an in-graph base of
                            // it does -- `IS_THIS_SHAPED` decides only
                            // whether that base walk may see a non-public
                            // member (precision rule (a)), never whether it
                            // runs at all, so an ordinary field/local/
                            // parameter receiver widens to its bases exactly
                            // like the `this.` shape does, public visibility
                            // only.
                            let this_shaped = is_this_shaped_receiver(r);
                            let declares_here = if this_shaped {
                                declares_member_any_visibility(
                                    &index,
                                    ridx,
                                    r.member.as_deref(),
                                    r.arg_count,
                                )
                            } else {
                                declares_member(&index, ridx, r.member.as_deref(), r.arg_count)
                            };
                            let target = if declares_here {
                                Some(ridx)
                            } else {
                                typed_receiver_base_member(
                                    &index,
                                    &file_contexts,
                                    ridx,
                                    r.member.as_deref(),
                                    r.arg_count,
                                    this_shaped,
                                )
                            };
                            if let Some(target) = target {
                                edges.push(Edge::uses_member(
                                    file.clone(),
                                    r.line,
                                    index.defs[target].id.clone(),
                                    index.defs[target].file.clone(),
                                    r.member.clone(),
                                    None,
                                ));
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
                // Unit A5 item 1: a chain-tail ref (one carrying
                // `receiver_call_owner`/`receiver_call_member`, `a.B().C`'s
                // `.C`) resolves ONLY through the method-return hop above.
                // `receiver_type_name` is `None` here in every way that hop
                // can come up EMPTY -- the owner did not resolve, the owner
                // resolved ambiguously, or the callee has no recorded return
                // at all -- and for a chain-tail ref there is no OTHER fact
                // to fall back on: `r.name` is the invocation's own source
                // text (`"a.B()"`), which by construction never resolves as
                // a real def (`push_member_ref`'s doc comment), so `result`
                // is unconditionally `External` and unnarrowed. Left alone,
                // that is exactly the shape the scored tier's UNFILTERED
                // name-uniqueness fallback exists for -- every def
                // graph-wide vouching for the OUTER member name, with no
                // receiver to filter by, since the receiver-narrowing rule
                // below only ever runs when `receiver_type_name` is `Some`.
                // Forcing `emitted` the same way `receiver_base` does above
                // finishes the ref as external right here instead: silent,
                // never entering tier (f) (already gated on `Some`) and
                // never falling into that unfiltered pool.
                //
                // A hop that DID produce a name -- in-graph OR a name this
                // extractor cannot look inside (an external return type,
                // e.g. `ILogger`) -- leaves `receiver_type_name` `Some` and
                // this guard alone: tier (e) above may already have claimed
                // it (in-graph case), and otherwise the ref keeps walking
                // the ordinary typed-receiver path below (tier (f), and the
                // scored tier's own RECEIVER rule, `receiver_admits_
                // candidate`, which -- unlike this guard -- filters rather
                // than silences, and is what an external-but-named receiver
                // is supposed to get: `stage5_receiver_rule_a_call_hop_
                // receiver_with_unknown_args_compares_by_name_only` pins
                // exactly this case green).
                if !emitted
                    && r.receiver_call_owner.is_some()
                    && r.receiver_call_member.is_some()
                    && receiver_type_name.is_none()
                {
                    emitted = true;
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
                            resolve_ref(&probe, usings, ns, &index, aliases, &file_contexts)
                        {
                            if let Some(fact) = index.member_lists[oidx].property_types.get(&r.name)
                            {
                                let hop =
                                    name_probe(fact.type_name.clone(), ns, r.outer_types.clone());
                                if let Resolution::Resolved(hidx, _) =
                                    resolve_ref(&hop, usings, ns, &index, aliases, &file_contexts)
                                {
                                    if declares_member(
                                        &index,
                                        hidx,
                                        r.member.as_deref(),
                                        r.arg_count,
                                    ) {
                                        edges.push(Edge::uses_member(
                                            file.clone(),
                                            r.line,
                                            index.defs[hidx].id.clone(),
                                            index.defs[hidx].file.clone(),
                                            r.member.clone(),
                                            None,
                                        ));
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
                // namespace is imported by this file (local or global using),
                // IS this file's namespace, or encloses it -- and, when a
                // project model exists, only when that class's project is one
                // this site could reference. Exactly one admitted candidate
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
                // Five filters, in this order. Every one of them can only ever
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
                //   4. visibility -- the declaring static class's namespace is
                //      imported by this file (local or global using), IS this
                //      file's namespace, or ENCLOSES it;
                //   5. project admission -- when a project model exists, the
                //      declaring static class's project is one the ref site's
                //      project can reference (see `Admission`).
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
                // Three documented bounds, each with a pinning test:
                //   - thisType is matched by EXACT name. No base-class walk, no
                //     interface widening on the POSITIVE side: `this
                //     IEnumerable<T>` does not claim a receiver typed List,
                //     `this BaseWidget` does not claim one typed Widget.
                //   - the namespace test admits an ENCLOSING namespace of the
                //     ref site as well as an imported one (App.Ext is visible
                //     from App.Ext.Deep with no using at all, which is the
                //     language's own rule -- see `namespace_encloses`), but
                //     nothing wider: a SIBLING namespace still needs the
                //     import, and the global namespace does not enclose.
                //   - the veto can only see IN-GRAPH types. An external
                //     receiver, or an external base of an in-graph receiver,
                //     hides whatever members it declares, so no veto is
                //     possible there.
                if !emitted {
                    if let (Some(receiver_type), Some(member), Some(arg_count)) =
                        (&receiver_type_name, r.member.as_deref(), r.arg_count)
                    {
                        let exact_key = format!("{member} {receiver_type}");
                        // Unit A3 item 3: the exact key misses for an
                        // extension whose `this` parameter is a BASE of the
                        // receiver rather than the receiver's own exact
                        // type -- widen to the receiver's nominal closure
                        // only once the exact key itself names no bucket,
                        // and only when the receiver resolved in-graph
                        // (`receiver_def`, the same resolution tier (e)
                        // already computed). Applies to every typed
                        // receiver, `this.` included -- `receiver_def` is
                        // set identically for both.
                        //
                        // Unit A5 item 2: the widened key names a DIFFERENT
                        // type than the receiver (a base or an ancestor), so
                        // filter 3 below must not unify against the
                        // receiver's OWN type arguments once the key was
                        // widened -- `unify_args` is whichever picture is
                        // right for the key actually chosen: the receiver's
                        // own arguments, unchanged, on the exact-key path;
                        // the matched node's own arguments, from
                        // `extension_closure_key`, on the widened path.
                        let (key, unify_args): (String, Option<Vec<String>>) =
                            if index.extension_index.contains_key(&exact_key) {
                                (exact_key, r.receiver_args.clone())
                            } else {
                                match receiver_def.and_then(|ridx| {
                                    extension_closure_key(&index, &file_contexts, ridx, member)
                                }) {
                                    Some((widened_key, args)) => (widened_key, args),
                                    None => (exact_key, r.receiver_args.clone()),
                                }
                            };
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
                            if !generic_args_unify(c.entry.this_args.as_ref(), unify_args.as_ref())
                            {
                                continue;
                            }
                            let def_ns = &index.defs[c.def_idx].namespace;
                            if !usings.contains(def_ns)
                                && def_ns != ns
                                && !namespace_encloses(def_ns, ns)
                            {
                                continue;
                            }
                            // Filter 5, the project model's: a static class in
                            // an assembly this one cannot reference is not a
                            // candidate at all. It runs BEFORE the distinct
                            // count on purpose -- an unreachable duplicate that
                            // merely counted would silence the tier on a
                            // candidate that is otherwise the single right
                            // answer.
                            if !admission.admits(site_unit, c.def_idx) {
                                continue;
                            }
                            if !distinct.contains(&c.def_idx) {
                                distinct.push(c.def_idx);
                            }
                        }
                        // Unit A4 item 2: arity-gated exactly like the
                        // precise tier's own `declares_here` check -- a
                        // same-named instance member at an arity `arg_count`
                        // does not fall inside is not a veto, so this tier
                        // runs "exactly as for an undeclared member" for
                        // that name.
                        let vetoed = match receiver_def {
                            Some(ridx) => inherited_member_declared(
                                &index,
                                &file_contexts,
                                ridx,
                                r.member.as_deref(),
                                r.arg_count,
                            ),
                            None => false,
                        };
                        if distinct.len() == 1 && !vetoed {
                            let didx = distinct[0];
                            edges.push(Edge::uses_member(
                                file.clone(),
                                r.line,
                                index.defs[didx].id.clone(),
                                index.defs[didx].file.clone(),
                                r.member.clone(),
                                Some(HeuristicTier::Ext),
                            ));
                            heuristic_edge_count += 1;
                            heuristic_by_tier.ext += 1;
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
                // A qualifier NARROWED AWAY -- the ladder DID find candidates
                // and the project model put every one of them out of reach --
                // is a third case and gets an EMPTY pool. It reaches this tier
                // as an `External` like any other, but it is not an unanswered
                // name: the answer is "none of the real candidates is nameable
                // here", and reaching for a graph-wide stranger instead would
                // contradict the language rule that produced it. `narrowed_away`
                // is what tells the two apart (`Narrowed`).
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
                    // The same choice, for the flag that rides alongside the
                    // resolution the pool is drawn from.
                    let source_narrowed_away = if receiver_type_name.is_some() {
                        receiver_narrowed_away
                    } else {
                        result_narrowed_away
                    };
                    // The ref's own call shape, read once and reused by both
                    // pools below: a property or field never vouches for a
                    // ref shaped like a call, no matter which pool it came
                    // from.
                    let shape = member_shape(r);
                    let pool: Option<Vec<usize>> = match source {
                        Resolution::Ambiguous(candidates, _) => Some(
                            candidates
                                .iter()
                                .copied()
                                .filter(|&d| member_vouched(&index, d, r.member.as_deref(), shape))
                                .collect(),
                        ),
                        // Narrowed to nothing: answered, not unanswered.
                        Resolution::External if source_narrowed_away => Some(Vec::new()),
                        Resolution::External => {
                            let named: Vec<usize> = match r
                                .member
                                .as_deref()
                                .and_then(|m| index.member_name_to_defs.get(m))
                            {
                                Some(list) => list.clone(),
                                None => Vec::new(),
                            };
                            // The uniqueness CAP is measured on the raw,
                            // shape-blind bucket -- a member name common
                            // enough to refuse a guess stays refused
                            // regardless of how many of its declarers survive
                            // the shape filter below. Only once the ref is
                            // admitted at all does the shape rule get to
                            // narrow which of those declarers actually vouch.
                            if named.len() <= SCORED_UNIQUENESS_CAP {
                                Some(
                                    named
                                        .into_iter()
                                        .filter(|&d| {
                                            member_vouched(&index, d, r.member.as_deref(), shape)
                                        })
                                        .collect(),
                                )
                            } else {
                                None
                            }
                        }
                        Resolution::Resolved(..) => None,
                    };
                    // The RECEIVER rule, the pool's last filter and, like
                    // every other filter here, purely subtractive -- it can
                    // remove a candidate, never add one. It applies only where
                    // the ref carries a receiver type that resolved to nothing
                    // in-graph -- the shape that made the uniqueness pool a
                    // pool of same-named strangers. An AMBIGUOUS receiver
                    // (several in-graph candidates, none picked) is untouched:
                    // there the pool already IS the receiver's own candidate
                    // set, so assignability is not in question. A ref with no
                    // receiver fact at all is untouched too -- there is nothing
                    // to be assignable TO.
                    let pool = match (&receiver_type_name, source, r.member.as_deref()) {
                        (Some(receiver_type), Resolution::External, Some(member)) => {
                            pool.map(|candidates| {
                                candidates
                                    .into_iter()
                                    .filter(|&d| {
                                        receiver_admits_candidate(
                                            &index,
                                            &file_contexts,
                                            &mut assignable_cache,
                                            d,
                                            member,
                                            shape,
                                            receiver_type,
                                            r.receiver_args.as_ref(),
                                            r.receiver_type.is_some(),
                                        )
                                    })
                                    .collect()
                            })
                        }
                        _ => pool,
                    };
                    // The project model's filter, last and applying to BOTH
                    // pools: the uniqueness pool because a same-named stranger
                    // in an unreferenced assembly is exactly the guess it was
                    // built to make, and the ambiguous pool because the ladder
                    // pooled candidates by name too. Placed after the
                    // uniqueness cap so that cap keeps measuring the name's
                    // repo-wide commonness -- a name carried by five defs is
                    // common vocabulary whether or not this project can see
                    // four of them.
                    let pool = pool.map(|c| {
                        c.into_iter()
                            .filter(|&d| admission.admits(site_unit, d))
                            .collect::<Vec<usize>>()
                    });
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
                            edges.push(Edge::uses_member(
                                file.clone(),
                                r.line,
                                index.defs[d].id.clone(),
                                index.defs[d].file.clone(),
                                r.member.clone(),
                                Some(HeuristicTier::Guess),
                            ));
                            heuristic_edge_count += 1;
                            heuristic_by_tier.guess += 1;
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

            match narrow_by_reachability(
                resolve_ref(r, usings, ns, &index, aliases, &file_contexts),
                site_unit,
                &admission,
            ) {
                Resolution::Resolved(idx, _) => {
                    edges.push(type_edge(&r.kind, file, r.line, &index.defs[idx]));
                    match r.kind.as_str() {
                        "inherits" => edges_by_kind.inherits += 1,
                        "uses-type" => edges_by_kind.uses_type += 1,
                        _ => {}
                    }
                }
                Resolution::Ambiguous(candidate_indices, _) => {
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
                match e.tier() {
                    Some(HeuristicTier::Ext) => heuristic_by_tier.ext -= 1,
                    Some(HeuristicTier::Guess) => heuristic_by_tier.guess -= 1,
                    None => {}
                }
                false
            }
        }
    });

    let type_ref_attempts = edges_by_kind.inherits + edges_by_kind.uses_type + ambiguous_count;

    let mut graph = Graph {
        schema_version: GRAPH_SCHEMA_VERSION,
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
            // Appended after the test counter, always written, and summing to
            // `heuristic_edge_count` above: the two tiers are the whole
            // population of guesses.
            heuristic_by_tier,
            ts: None,
        },
        defs: index.defs,
        edges,
        names,
        // Appended LAST and empty without a model, which is what keeps a
        // csproj-less repo's graph.json byte-identical to what it was.
        units: model.map(crate::project::graph_units).unwrap_or_default(),
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
            field_types: crate::graph::OrderedMap::new(),
            method_return_args: crate::graph::OrderedMap::new(),
            non_public_methods: vec![],
            method_arities: crate::graph::OrderedMap::new(),
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
            receiver_base: false,
            receiver_awaited: false,
            receiver_local: false,
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
            receiver_base: false,
            receiver_awaited: false,
            receiver_local: false,
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

    /// The members named by one file's heuristic uses-member edges, in edge
    /// order -- the fact `heuristic_member_edges_from` above cannot show.
    fn heuristic_member_names_from<'a>(g: &'a Graph, from: &str) -> Vec<Option<&'a str>> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    heuristic: true,
                    member,
                    ..
                } if from_file == from => Some(member.as_deref()),
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
            r#"{"kind":"uses-member","from_file":"Consumers/UsesExtension.cs","from_line":9,"to":"App.Ext.WidgetExtensions","to_file":"Ext/WidgetExtensions.cs","heuristic":true,"tier":"ext","member":"Render"}"#,
            "heuristic, then tier, then member -- appended in that order after the shared prefix"
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

    // The positive half of the same rule, and the one thing tier (f)'s
    // namespace test learned in stage 6: an ENCLOSING namespace needs no
    // using directive, because in C# it is already in scope. Until global
    // usings became per-project this gap was invisible -- any `global using`
    // for the namespace, declared in any file anywhere in the repo, admitted
    // the class here by accident.
    #[test]
    fn stage6_tier_f_an_extension_class_in_an_enclosing_namespace_needs_no_using() {
        let files = fragments_for(&[
            WIDGET_SRC,
            (
                "Ext/Registration.cs",
                "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
            ),
            (
                "Ext/Deep/DeepRunner.cs",
                "\nusing App.Other;\n\nnamespace App.Ext.Deep;\n\npublic class DeepRunner\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
            (
                "Sibling/SiblingRunner.cs",
                "\nusing App.Other;\n\nnamespace App.Sibling;\n\npublic class SiblingRunner\n{\n  public void Run(Widget w) => w.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);

        assert_eq!(
            heuristic_member_edges_from(&g, "Ext/Deep/DeepRunner.cs"),
            vec![("App.Ext.WidgetExtensions", 8)],
            "App.Ext encloses App.Ext.Deep, so the extension class is in scope with no import"
        );
        assert_eq!(
            heuristic_member_tiers_from(&g, "Ext/Deep/DeepRunner.cs"),
            vec![Some(HeuristicTier::Ext)],
            "and tier (f) is what claims it -- not the scored tier's weaker second look"
        );
        assert!(
            heuristic_member_edges_from(&g, "Sibling/SiblingRunner.cs").is_empty(),
            "nothing wider than the lexical rule: a SIBLING namespace still needs the import"
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
        // Two hops up the chain is still an instance member -- Unit A3 item 4
        // is exactly this widening: Leaf itself declares nothing, but Root,
        // reached through Leaf's transitive in-graph base closure, does, so
        // the typed-receiver precise tier binds there instead of leaving the
        // extension tier's veto as the only visible effect.
        assert_eq!(
            member_edges_from(&g, "Consumers/DeepVeto.cs"),
            vec![("App.Other.Root", 9)],
            "the precise tier now walks the closure the veto always could see"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "the extension is still unreachable -- precise supersedes it, not joins it"
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

    // --- stage 5: the call-shape rule -------------------------------------
    //
    // A property or field is undeniable evidence for a READ of its own name,
    // but no evidence at all for a CALL of that name -- C# simply has no
    // overload-resolution path from `entity.Property(x => x.Id)` to a
    // property or a field. Letting one vouch for a call anyway is exactly the
    // false-positive shape a corpus audit surfaced: 41% of all heuristic
    // edges were a call landing on a property/field-only def.

    #[test]
    fn stage5_shape_rule_a_call_never_vouches_through_a_property_or_field_in_the_uniqueness_pool() {
        let files = fragments_for(&[
            (
                "Model/Customer.cs",
                "namespace App.Model { public class Customer { public string Property { get; set; } } }",
            ),
            (
                "Model/Order.cs",
                "namespace App.Model { public class Order { public int Property; } }",
            ),
            (
                "Consumers/CallShape.cs",
                "\nnamespace App.Consumers;\n\npublic class CallShape\n{\n  public void Run()\n  {\n    var e = Entity();\n    e.Property(x => x.Id);\n    var p = e.Property;\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/CallShape.cs"),
            vec![("App.Model.Customer", 10), ("App.Model.Order", 10)],
            "the call at line 9 names neither a property nor a field -- only the read at line 10, which both a property and a field vouch for, survives"
        );
    }

    #[test]
    fn stage5_shape_rule_filters_the_ambiguous_pool_the_same_way() {
        let files = fragments_for(&[
            (
                "One/Config.cs",
                "namespace App.One { public class Config { public void Load() { } } }",
            ),
            (
                "Two/Config.cs",
                "namespace App.Two { public class Config { public string Load { get; } } }",
            ),
            (
                "Consumers/AmbiguousCall.cs",
                "\nnamespace App.Consumers;\n\npublic class AmbiguousCall\n{\n  public void Run() => Config.Load();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/AmbiguousCall.cs").is_empty(),
            "the precise tiers still refuse to pick"
        );
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/AmbiguousCall.cs"),
            vec![("App.One.Config", 6)],
            "Config.Load() is a call -- App.Two.Config only ever declared Load as a property, so the shape rule drops it out of the ambiguous pool before scoring"
        );
    }

    #[test]
    fn stage5_shape_rule_a_method_or_extension_name_still_vouches_for_a_call() {
        let files = fragments_for(&[
            (
                "A/Counter.cs",
                "namespace App.A { public class Counter { public void Tally() { } } }",
            ),
            ("Other/Foo.cs", "namespace App.Other { public class Foo { } }"),
            (
                "Ext/FooExtensions.cs",
                "namespace App.Ext { public static class FooExtensions { public static void Tally(this Foo f) { } } }",
            ),
            (
                "Consumers/CallShapeOk.cs",
                "\nnamespace App.Consumers;\n\npublic class CallShapeOk\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["App.A.Counter", "App.Ext.FooExtensions"],
            "a method name and an extension-method name both still vouch for a call -- the shape rule only ever removes candidates, never adds one"
        );
    }

    // --- stage 5: the receiver-assignability rule --------------------------
    //
    // The scored tier's uniqueness pool is drawn by member NAME alone, so a
    // ref whose receiver is typed but EXTERNAL (`private ILogger _logger;`
    // where ILogger is a NuGet interface) used to name any in-graph class
    // carrying a method of that name -- a log adapter implementing an
    // unrelated interface, say. The receiver's type is a fact the extractor
    // already recorded, and C# will only bind that call to a member of a type
    // the receiver is assignable to, so a candidate the in-graph inheritance
    // closure cannot connect to the receiver type is not a weak guess, it is a
    // disproved one. The rule below refuses it.
    //
    // The connection is NOMINAL and deliberately shallow: a candidate answers
    // when it IS the receiver type, when a def in its in-graph base closure
    // is, or when any def in that closure merely NAMES the receiver type in
    // its raw base list -- the last case being the one that matters, since the
    // receiver type is usually external and so has no def to walk to.

    #[test]
    fn stage5_receiver_rule_an_external_receiver_refuses_a_candidate_not_assignable_to_it() {
        let files = fragments_for(&[
            (
                "Logging/DbUpLogAdapter.cs",
                "namespace App.Logging { public class DbUpLogAdapter : IUpgradeLog { public void LogInformation(string m) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
            "DbUpLogAdapter implements IUpgradeLog and nothing in its closure names ILogger -- the receiver's own type disproves the guess"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage5_receiver_rule_a_candidate_whose_base_closure_names_the_receiver_type_still_emits() {
        let files = fragments_for(&[
            (
                "Logging/FileLogger.cs",
                "namespace App.Logging { public class FileLogger : ILogger { public void LogInformation(string m) { } } }",
            ),
            (
                "Logging/Base.cs",
                "namespace App.Logging { public class Base : ILogger { } }",
            ),
            (
                "Logging/Derived.cs",
                "namespace App.Logging { public class Derived : Base { public void LogInformation(string m) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Logging.Derived", 7), ("App.Logging.FileLogger", 7)],
            "FileLogger names ILogger directly; Derived reaches it one in-graph hop up, through Base"
        );
    }

    #[test]
    fn stage5_receiver_rule_generic_arguments_must_unify_on_the_matched_base() {
        let files = fragments_for(&[
            (
                "Logging/Adapter.cs",
                "namespace App.Logging { public class Adapter : ILogger { public void LogInformation(string m) { } } }",
            ),
            (
                "Logging/Typed.cs",
                "namespace App.Logging { public class Typed : ILogger<Worker> { public void LogInformation(string m) { } } }",
            ),
            (
                "Logging/Open.cs",
                "namespace App.Logging { public class Open<T> : ILogger<T> { public void LogInformation(string m) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nnamespace App.Consumers;\n\npublic class Runner\n{\n  private ILogger<Worker> _logger;\n  public void Run() => _logger.LogInformation(\"x\");\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Logging.Open", 7), ("App.Logging.Typed", 7)],
            "the base NAME matching is not enough: the non-generic `: ILogger` never binds an ILogger<Worker> receiver, while a closed and an open implementation both do"
        );
    }

    #[test]
    fn stage5_receiver_rule_a_call_hop_receiver_with_unknown_args_compares_by_name_only() {
        let files = fragments_for(&[
            (
                "Logging/LoggerFactory.cs",
                "namespace App.Logging { public class LoggerFactory { public static ILogger Make() { return null; } } }",
            ),
            (
                "Logging/Typed.cs",
                "namespace App.Logging { public class Typed : ILogger<Worker> { public void LogInformation(string m) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Logging;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var l = LoggerFactory.Make();\n    l.LogInformation(\"x\");\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Logging.Typed", 11)],
            "a method's recorded RETURN type carries a name and no type arguments, so the rule compares names only rather than refusing every generic implementation"
        );
    }

    #[test]
    fn stage5_receiver_rule_an_extension_only_candidate_is_refused_after_tier_f_declined() {
        let files = fragments_for(&[
            (
                "Ext/LogExt.cs",
                "namespace App.Ext { public static class LogExt { public static void LogInformation(this IOtherLogger l, string m) { } } }",
            ),
            (
                "Registration/WidgetServiceExtensions.cs",
                "namespace App.Registration { public static class WidgetServiceExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
            ),
            (
                "Consumers/Startup.cs",
                "\nnamespace App.Consumers;\n\npublic class Startup\n{\n  public void Run(ILogger logger, IServiceCollection services)\n  {\n    logger.LogInformation(\"x\");\n    services.AddWidgets();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Startup.cs"),
            vec![("App.Registration.WidgetServiceExtensions", 9)],
            "LogExt extends IOtherLogger, not ILogger, so no this-type of its own answers the receiver and nothing else connects it -- while AddWidgets extends the receiver type exactly and only tier (f)'s namespace test (App.Registration is not imported here) kept it out"
        );
    }

    #[test]
    fn stage5_receiver_rule_an_in_graph_receiver_still_resolves_precisely() {
        let files = fragments_for(&[
            (
                "Widgets/Widget.cs",
                "namespace App.Widgets { public class Widget { public void Render() { } } }",
            ),
            (
                "Consumers/UsesWidget.cs",
                "\nusing App.Widgets;\n\nnamespace App.Consumers;\n\npublic class UsesWidget\n{\n  private Widget _widget;\n  public void Run() => _widget.Render();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/UsesWidget.cs"),
            vec![("App.Widgets.Widget", 9)],
            "an in-graph receiver never reaches the scored tier at all -- tier (e) answers it precisely"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    // The probe the design asked for: a base written with its namespace
    // (`class Handle : System.IDisposable`) is recorded as the bare identifier
    // `IDisposable`, and a receiver declared the same dotted way is recorded
    // bare too, so the two raw strings meet and the rule admits the candidate.
    // Both halves of that are extractor behaviour, which is why this runs real
    // sources rather than hand-built facts.
    #[test]
    fn stage5_receiver_rule_a_dotted_base_name_meets_a_dotted_receiver_type_by_bare_identifier() {
        let files = fragments_for(&[
            (
                "Io/Handle.cs",
                "namespace App.Io { public class Handle : System.IDisposable { public void Dispose() { } } }",
            ),
            (
                "Consumers/Closer.cs",
                "\nnamespace App.Consumers;\n\npublic class Closer\n{\n  public void Run(System.IDisposable d) => d.Dispose();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Closer.cs"),
            vec![("App.Io.Handle", 6)],
            "both sides reduce to the bare identifier IDisposable, so the raw base string answers the receiver type"
        );
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
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":14,"to":"App.Core.Widget","to_file":"Core/Widget.cs","member":"Render"}"#,
        r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":15,"to":"App.Core.Status.Active","to_file":"Core/Status.cs","member":"Active"}"#,
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

    // Stage 6 adds a project model the resolver may consult; a repo that
    // declares no `.csproj` has none, and for such a repo the WHOLE artifact
    // -- not just the edge array -- must serialize exactly as it did before
    // stage 6 existed. Whole-graph bytes rather than a spot check on `units`:
    // an omitted key is only half the guarantee, the other half is that
    // threading the model through moved nothing else.
    #[test]
    fn stage6_without_a_project_model_the_byte_identity_fixture_serializes_exactly_as_before() {
        let files = fragments_for(BYTE_IDENTITY_FIXTURE);
        let root = no_git_root();

        let legacy = serde_json::to_string(&resolve_graph(&root, &files)).unwrap();
        let modelled =
            serde_json::to_string(&resolve_graph_with_model(&root, &files, &[], None)).unwrap();
        assert_eq!(
            legacy, modelled,
            "a None model must leave the artifact byte-identical, key for key"
        );

        assert!(
            !legacy.contains(r#""units""#),
            "no `.csproj`, no `units` key -- it is omit-when-empty precisely so a \
             csproj-less repo's graph.json is unchanged: {legacy}"
        );
    }

    // --- stage 6: the admission gate on the two heuristic tiers -----------
    //
    // A heuristic tier guesses from NAMES; the project model is the one fact
    // that can disprove such a guess structurally -- a def the site's assembly
    // could not reference even if the name were right. The gate is a filter
    // like every other heuristic-tier rule: purely subtractive, and it fails
    // OPEN (a file or a def outside every project admits everything), because
    // an ownership answer this resolver cannot compute must never delete an
    // edge it would otherwise have emitted.

    /// One hand-built `Unit`: `id` is the repo-relative `.csproj` path, `dir`
    /// is derived from it exactly as discovery derives it, `name` is the file
    /// stem, and `refs` are the ids this project references DIRECTLY (the
    /// model closes over them).
    fn unit(id: &str, refs: &[&str], test: bool) -> crate::project::Unit {
        let (dir, file) = match id.rfind('/') {
            Some(i) => (&id[..i], &id[i + 1..]),
            None => ("", id),
        };
        crate::project::Unit {
            id: id.to_string(),
            name: file.trim_end_matches(".csproj").to_string(),
            dir: dir.to_string(),
            refs: refs.iter().map(|r| (*r).to_string()).collect(),
            test,
        }
    }

    fn model_of(units: Vec<crate::project::Unit>) -> crate::project::ProjectModel {
        crate::project::ProjectModel::from_units(units)
    }

    /// The tiers carried by one file's heuristic uses-member edges, in edge
    /// order -- what `heuristic_member_edges_from` cannot show.
    fn heuristic_member_tiers_from(g: &Graph, from: &str) -> Vec<Option<HeuristicTier>> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    heuristic: true,
                    tier,
                    ..
                } if from_file == from => Some(*tier),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn stage6_admission_a_scored_guess_never_names_a_def_in_a_project_the_site_cannot_reach() {
        // `Build()` resolves to nothing, so `q` has no recorded type: the ref
        // carries no receiver fact at all and lands in the scored tier's
        // uniqueness pool, where the only evidence is the member NAME. Two
        // projects declare `Enqueue`; only one of them is on the site's
        // reference closure.
        let files = fragments_for(&[
            (
                "src/Domain/Order.cs",
                "namespace Fixture.Domain { public class Order { public void Enqueue(string m) { } } }",
            ),
            (
                "src/Unreachable/Mailer.cs",
                "namespace Fixture.Unreachable { public class Mailer { public void Enqueue(string m) { } } }",
            ),
            (
                "src/App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var q = Build();\n    q.Enqueue(\"x\");\n  }\n}\n",
            ),
        ]);
        let root = no_git_root();

        assert_eq!(
            heuristic_member_edge_targets(&resolve_graph(&root, &files)),
            vec!["Fixture.Domain.Order", "Fixture.Unreachable.Mailer"],
            "without a model the tier has only the member name to go on, and both declarers are equally plausible"
        );

        let model = model_of(vec![
            unit("src/App/App.csproj", &["src/Domain/Domain.csproj"], false),
            unit("src/Domain/Domain.csproj", &[], false),
            unit("src/Unreachable/Unreachable.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
        assert_eq!(
            heuristic_member_edge_targets(&g),
            vec!["Fixture.Domain.Order"],
            "App references Domain and nothing references Unreachable -- Mailer.Enqueue is not a call App could ever have made"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 1,
            "the refused guess is dropped, not retagged"
        );
    }

    #[test]
    fn stage6_admission_a_non_test_site_never_names_a_def_in_a_test_project_even_when_it_has_no_test_methods(
    ) {
        // The fixture-class shape: a helper in a test project carrying no
        // `[Fact]`/`[Test]` attribute at all, so `test_def_count` cannot see
        // it and no attribute-based rule would refuse it. Reachability cannot
        // refuse it either -- this model deliberately lets the production
        // project reference the test one, so the ONLY thing standing between
        // the guess and the edge is the test-project half of the gate.
        let files = fragments_for(&[
            (
                "tests/App.Tests/AdapterFixture.cs",
                "namespace Fixture.App.Tests { public class AdapterFixture { public void Reset() { } } }",
            ),
            (
                "src/App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Reset();\n  }\n}\n",
            ),
        ]);
        let root = no_git_root();

        assert_eq!(
            heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
            vec![("Fixture.App.Tests.AdapterFixture", 9)],
            "without a model the guess stands -- nothing in the sources says AdapterFixture is test-only"
        );

        let model = model_of(vec![
            unit(
                "src/App/App.csproj",
                &["tests/App.Tests/App.Tests.csproj"],
                false,
            ),
            unit("tests/App.Tests/App.Tests.csproj", &[], true),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
        assert_eq!(
            g.stats.test_def_count, 0,
            "AdapterFixture declares no test method, so def-level test detection never marked it -- the UNIT is what makes it test-only"
        );
        assert!(
            heuristic_member_edges_from(&g, "src/App/Runner.cs").is_empty(),
            "production code calling into a test assembly is not a thing the build allows, whatever the name says"
        );
    }

    #[test]
    fn stage6_admission_a_test_site_may_name_a_def_in_a_referenced_test_utility_project() {
        // The other side of the same rule: test -> test is an ordinary
        // reference, so the gate must not turn "is a test project" into a
        // blanket refusal.
        let files = fragments_for(&[
            (
                "tests/Test.Utilities/FakeServer.cs",
                "namespace Fixture.Test.Utilities { public class FakeServer { public void Reset() { } } }",
            ),
            (
                "tests/App.Tests/WorkerTests.cs",
                "\nnamespace Fixture.App.Tests;\n\npublic class WorkerTests\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Reset();\n  }\n}\n",
            ),
        ]);
        let model = model_of(vec![
            unit(
                "tests/App.Tests/App.Tests.csproj",
                &["tests/Test.Utilities/Test.Utilities.csproj"],
                true,
            ),
            unit("tests/Test.Utilities/Test.Utilities.csproj", &[], true),
        ]);
        let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));
        assert_eq!(
            heuristic_member_edges_from(&g, "tests/App.Tests/WorkerTests.cs"),
            vec![("Fixture.Test.Utilities.FakeServer", 9)],
            "a test site reaching a referenced test-utility project is exactly what that project is for"
        );
    }

    #[test]
    fn stage6_admission_tier_f_ignores_an_unreachable_duplicate_and_emits_the_reachable_one() {
        // Tier (f) emits on exactly ONE distinct declaring class, so a second
        // same-named extension method in the same namespace silences it
        // entirely and the ref falls through to the scored tier, which names
        // both. The gate runs BEFORE that count, which is why an unreachable
        // duplicate stops being an ambiguity at all rather than merely losing
        // a race -- and the edge that comes back is the EXT one, not the pair
        // of guesses the fallthrough produced.
        let files = fragments_for(&[
            (
                "src/Ext.Adapters/ServiceCollectionExtensions.cs",
                "namespace Fixture.Registration { public static class ServiceCollectionExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
            ),
            (
                "src/Unreachable/UnreachableExtensions.cs",
                "namespace Fixture.Registration { public static class UnreachableExtensions { public static void AddWidgets(this IServiceCollection s) { } } }",
            ),
            (
                "src/App/Startup.cs",
                "\nusing Fixture.Registration;\n\nnamespace Fixture.App;\n\npublic class Startup\n{\n  public void Run(IServiceCollection s) => s.AddWidgets();\n}\n",
            ),
        ]);
        let root = no_git_root();

        let bare = resolve_graph(&root, &files);
        assert_eq!(
            heuristic_member_edges_from(&bare, "src/App/Startup.cs"),
            vec![
                ("Fixture.Registration.ServiceCollectionExtensions", 8),
                ("Fixture.Registration.UnreachableExtensions", 8)
            ],
            "without a model both static classes clear every tier-(f) filter, two distinct classes is an ambiguity, and the tier stays silent"
        );
        assert_eq!(
            heuristic_member_tiers_from(&bare, "src/App/Startup.cs"),
            vec![Some(HeuristicTier::Guess), Some(HeuristicTier::Guess)],
            "the two edges are the scored tier's, re-admitted by the receiver rule because each `this` parameter names the receiver type exactly"
        );

        let model = model_of(vec![
            unit(
                "src/App/App.csproj",
                &["src/Ext.Adapters/Ext.Adapters.csproj"],
                false,
            ),
            unit("src/Ext.Adapters/Ext.Adapters.csproj", &[], false),
            unit("src/Unreachable/Unreachable.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
        assert_eq!(
            heuristic_member_edges_from(&g, "src/App/Startup.cs"),
            vec![("Fixture.Registration.ServiceCollectionExtensions", 8)],
            "one admitted candidate is one distinct class, and tier (f) emits"
        );
        assert_eq!(
            heuristic_member_tiers_from(&g, "src/App/Startup.cs"),
            vec![Some(HeuristicTier::Ext)],
            "the edge is tier (f)'s, not the scored tier's second-guess"
        );
    }

    #[test]
    fn stage6_admission_a_file_outside_every_project_fails_open() {
        // Both directions of "unknown": a candidate whose file no project
        // owns, and a SITE whose file no project owns. Neither may lose an
        // edge -- the gate refuses only on a positive answer.
        let files = fragments_for(&[
            (
                "src/App/Widget.cs",
                "namespace Fixture.App { public class Widget { public void Ping() { } } }",
            ),
            (
                "tools/Helper.cs",
                "namespace Fixture.Tools { public class Helper { public void Pong() { } } }",
            ),
            (
                "src/App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    var h = Build();\n    h.Pong();\n  }\n}\n",
            ),
            (
                "tools/Script.cs",
                "\nnamespace Fixture.Tools;\n\npublic class Script\n{\n  public void Run()\n  {\n    var w = Build();\n    w.Ping();\n  }\n}\n",
            ),
        ]);
        let model = model_of(vec![unit("src/App/App.csproj", &[], false)]);
        let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

        assert_eq!(
            heuristic_member_edges_from(&g, "src/App/Runner.cs"),
            vec![("Fixture.Tools.Helper", 9)],
            "the candidate sits outside every project, so nothing can be proven about reaching it"
        );
        assert_eq!(
            heuristic_member_edges_from(&g, "tools/Script.cs"),
            vec![("Fixture.App.Widget", 9)],
            "the SITE sits outside every project -- same fail-open answer from the other side"
        );
    }

    // --- stage 6: `global using` is a per-PROJECT fact ---------------------
    //
    // A `global using` is scoped to the compilation that declares it and does
    // NOT flow across a ProjectReference. Without a model the resolver cannot
    // see project boundaries and pools every global using repo-wide (the
    // documented over-approximation); with one, each file is seeded from its
    // OWN project's globals only.

    // Two same-named `Config` classes, so the ladder's global-uniqueness step
    // cannot answer `Config` on its own and the `global using` is the ONLY
    // thing that can pick one -- which is what makes "who can see that global
    // using" observable at all.
    const SCOPED_GLOBAL_USING_FIXTURE: &[(&str, &str)] = &[
        (
            "src/Alpha/Config.cs",
            "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
        ),
        (
            "src/Beta/Config.cs",
            "namespace Fixture.Beta { public class Config { public void Load() { } } }",
        ),
        ("src/App/GlobalUsings.cs", "global using Fixture.Alpha;\n"),
        (
            "src/App/AppConsumer.cs",
            "\nnamespace Fixture.App;\n\npublic class AppConsumer\n{\n  public void Run() => Config.Load();\n}\n",
        ),
        (
            "src/Other/OtherConsumer.cs",
            "\nnamespace Fixture.Other;\n\npublic class OtherConsumer\n{\n  public void Run() => Config.Load();\n}\n",
        ),
    ];

    #[test]
    fn stage6_global_usings_are_scoped_to_the_declaring_unit_when_a_model_exists() {
        let files = fragments_for(SCOPED_GLOBAL_USING_FIXTURE);
        // Both consumers reference both Alpha and Beta, so admission has
        // nothing to say here: the only difference between the two files is
        // which project declared the `global using`.
        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/Beta/Beta.csproj", &[], false),
            unit(
                "src/App/App.csproj",
                &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
                false,
            ),
            unit(
                "src/Other/Other.csproj",
                &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
                false,
            ),
        ]);
        let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

        assert_eq!(
            member_edges_from(&g, "src/App/AppConsumer.cs"),
            vec![("Fixture.Alpha.Config", 6)],
            "the declaring project's own files still see its global using"
        );
        assert!(
            member_edges_from(&g, "src/Other/OtherConsumer.cs").is_empty(),
            "the other project never wrote that global using, so `Config` names nothing there"
        );
        assert_eq!(
            heuristic_member_edges_from(&g, "src/Other/OtherConsumer.cs"),
            vec![("Fixture.Alpha.Config", 6), ("Fixture.Beta.Config", 6)],
            "it degrades to the ordinary two-way ambiguity an unimported `Config` always is -- not to a precise edge borrowed from another project"
        );
    }

    #[test]
    fn stage6_global_usings_are_repo_wide_without_one() {
        let files = fragments_for(SCOPED_GLOBAL_USING_FIXTURE);
        let g = resolve_graph(&no_git_root(), &files);

        assert_eq!(
            member_edges_from(&g, "src/App/AppConsumer.cs"),
            vec![("Fixture.Alpha.Config", 6)]
        );
        assert_eq!(
            member_edges_from(&g, "src/Other/OtherConsumer.cs"),
            vec![("Fixture.Alpha.Config", 6)],
            "with no project boundaries to read, every global using is in scope everywhere -- the pre-stage-6 behaviour, unchanged"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "both qualifiers resolved precisely, so no ref ever reached a heuristic tier"
        );
    }

    #[test]
    fn stage6_global_usings_fall_open_to_the_repo_wide_pool_for_a_file_no_project_owns() {
        // A file under no project directory has no compilation whose global
        // usings could be read, so it is NOT an owned unit that happened to
        // declare none -- it is the no-model case in miniature, and it falls
        // open to the repo-wide pool. Seeding it from nothing instead would
        // strip a loose file of every global using in the tree and silently
        // demote a resolvable name to a guess.
        let mut files: Vec<(&str, &str)> = SCOPED_GLOBAL_USING_FIXTURE.to_vec();
        files.push((
            "Loose/LooseConsumer.cs",
            "\nnamespace Fixture.Loose;\n\npublic class LooseConsumer\n{\n  public void Run() => Config.Load();\n}\n",
        ));
        let files = fragments_for(&files);
        // Every unit lives under `src/`; `Loose/` is under none of them, so
        // `unit_of_file` answers `None` for the consumer and admission -- which
        // needs a site unit to filter anything -- has nothing to say either.
        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/Beta/Beta.csproj", &[], false),
            unit(
                "src/App/App.csproj",
                &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
                false,
            ),
            unit(
                "src/Other/Other.csproj",
                &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
                false,
            ),
        ]);
        let g = resolve_graph_with_model(&no_git_root(), &files, &[], Some(&model));

        assert_eq!(
            model.unit_of_file("Loose/LooseConsumer.cs"),
            None,
            "the fixture only means anything while this file is owned by no unit"
        );
        assert_eq!(
            member_edges_from(&g, "Loose/LooseConsumer.cs"),
            vec![("Fixture.Alpha.Config", 6)],
            "the App project's `global using Fixture.Alpha;` is in the repo-wide pool, and an unowned file draws from that pool"
        );
        assert!(
            heuristic_member_edges_from(&g, "Loose/LooseConsumer.cs").is_empty(),
            "the name resolved precisely, so no tier ever had a guess to make"
        );
        assert_eq!(
            member_edges_from(&g, "src/Other/OtherConsumer.cs"),
            Vec::new(),
            "a file an OWNED project holds still sees only its own unit's globals -- the fall-open is for unowned files alone"
        );
    }

    // --- stage 6: narrowing an AMBIGUOUS resolution by reachability -------
    //
    // The ladder pools same-named defs and refuses to pick; the project model
    // can settle some of those refusals with the language's own rule rather
    // than a guess -- a type in a project this one does not reference cannot
    // be named here at all, so it was never a candidate. The narrowing runs
    // OUTSIDE the ladder, at the three places that consume an `Ambiguous`,
    // which is why it can turn one into a PRECISE edge without any tier
    // learning about projects.

    // Two same-named `Config` classes in two different projects and one
    // consumer that names `Config` twice: once as a plain type reference (the
    // field declaration on line 6) and once as a uses-member qualifier
    // (`Config.Load()` on line 8). No using is in scope, so both refs are
    // answered at the ladder's global-simple-name step, where two candidates
    // is exactly an ambiguity -- so one resolve shows what the model does to
    // both consumers at once.
    const CROSS_PROJECT_AMBIGUITY_FIXTURE: &[(&str, &str)] = &[
        (
            "src/Alpha/Config.cs",
            "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
        ),
        (
            "src/Beta/Config.cs",
            "namespace Fixture.Beta { public class Config { public void Load() { } } }",
        ),
        (
            "src/App/Runner.cs",
            "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Config _config;\n\n  public void Run() => Config.Load();\n}\n",
        ),
    ];

    /// Every `ambiguous` edge out of one file as (origin, raw, candidate ids,
    /// `candidate_count`) -- the capped list AND the uncapped total, since
    /// narrowing has to shrink both or neither.
    fn ambiguous_edges_from<'a>(
        g: &'a Graph,
        from: &str,
    ) -> Vec<(&'a str, &'a str, Vec<&'a str>, usize)> {
        g.edges
            .iter()
            .filter_map(|e| match e {
                Edge::Ambiguous {
                    origin,
                    from_file,
                    raw,
                    candidates,
                    candidate_count,
                    ..
                } if from_file == from => Some((
                    origin.as_str(),
                    raw.as_str(),
                    candidates.iter().map(|c| c.id.as_str()).collect(),
                    *candidate_count,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn stage6_narrowing_turns_a_two_project_ambiguity_into_a_precise_edge_when_only_one_is_reachable(
    ) {
        let files = fragments_for(CROSS_PROJECT_AMBIGUITY_FIXTURE);
        let root = no_git_root();

        let bare = resolve_graph(&root, &files);
        assert_eq!(
            ambiguous_edges_from(&bare, "src/App/Runner.cs"),
            vec![(
                "uses-type",
                "Config",
                vec!["Fixture.Alpha.Config", "Fixture.Beta.Config"],
                2
            )],
            "without a model the two Configs are indistinguishable and the type ref stays an ambiguity"
        );
        assert_eq!(
            heuristic_member_edges_from(&bare, "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
            "and the qualifier's ambiguity is what feeds the scored tier's strong pool"
        );

        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/App/App.csproj", &["src/Alpha/Alpha.csproj"], false),
            unit("src/Beta/Beta.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

        assert_eq!(
            type_edge_targets_from(&g, "src/App/Runner.cs"),
            vec!["Fixture.Alpha.Config"],
            "App cannot reference Beta, so `Config` in this file has exactly one meaning and the type ref is a FACT"
        );
        assert_eq!(
            member_edges_from(&g, "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8)],
            "the same narrowing at the uses-member qualifier promotes the call out of the scored tier entirely"
        );
        assert!(
            ambiguous_edges_from(&g, "src/App/Runner.cs").is_empty()
                && g.stats.ambiguous_count == 0,
            "a settled ambiguity is not an ambiguity: the edge and the count both go"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "nothing is guessed when the language's own reference rule already answers"
        );
        assert_eq!(
            g.stats.unresolved_external_count, 0,
            "narrowed to ONE, not to zero -- the external counter must not move"
        );
    }

    #[test]
    fn stage6_narrowing_settles_the_receiver_probe_so_a_field_hop_lands_on_a_precise_edge() {
        // The third consumer: tier (e) resolves the RECEIVER's recorded type
        // through the same ladder, and an ambiguous answer there stops the hop
        // dead -- the tier emits only on exactly one def. Narrowing the probe
        // is what turns `_config.Load()` from two scored guesses into the one
        // edge the compiler would bind.
        let files = fragments_for(&[
            (
                "src/Alpha/Config.cs",
                "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
            ),
            (
                "src/Beta/Config.cs",
                "namespace Fixture.Beta { public class Config { public void Load() { } } }",
            ),
            (
                "src/App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Config _config;\n\n  public void Run() => _config.Load();\n}\n",
            ),
        ]);
        let root = no_git_root();

        assert_eq!(
            heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
            "without a model the receiver type is ambiguous, tier (e) declines and the scored tier names both"
        );

        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/App/App.csproj", &["src/Alpha/Alpha.csproj"], false),
            unit("src/Beta/Beta.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));
        assert_eq!(
            member_edges_from(&g, "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8)],
            "one reachable receiver type is one receiver type, and the field hop is precise again"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "the guesses are replaced, not joined"
        );
    }

    #[test]
    fn stage6_narrowing_keeps_an_ambiguity_between_two_reachable_projects() {
        // The gate is subtractive and nothing more: when the site can
        // reference both projects the model has nothing to say, and the
        // resolver must go on refusing to pick rather than inventing a
        // tie-break.
        let files = fragments_for(CROSS_PROJECT_AMBIGUITY_FIXTURE);
        let root = no_git_root();
        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit(
                "src/App/App.csproj",
                &["src/Alpha/Alpha.csproj", "src/Beta/Beta.csproj"],
                false,
            ),
            unit("src/Beta/Beta.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

        assert_eq!(
            ambiguous_edges_from(&g, "src/App/Runner.cs"),
            ambiguous_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
            "both candidates survive the filter, so the edge is the one the model-less resolve emits"
        );
        assert_eq!(g.stats.ambiguous_count, 1);
        assert_eq!(
            heuristic_member_edges_from(&g, "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
            "and the qualifier still reaches the scored tier with both candidates in its pool"
        );
    }

    #[test]
    fn stage6_narrowing_never_touches_ctor_di_implementor_choice() {
        // The ctor-DI resolver picks an IMPLEMENTATION of an interface the
        // site names -- a different question from "which same-named type did
        // this reference mean", and one the model is not entitled to answer:
        // an unreachable implementor is still evidence that the interface has
        // more than one, and silently promoting the reachable one would turn a
        // reported ambiguity into a confident wrong answer whenever the
        // path-based ownership guess is off.
        let files = fragments_for(&[
            (
                "src/Contracts/IRepo.cs",
                "namespace Fixture.Contracts { public interface IRepo { void Save(); } }",
            ),
            (
                "src/Files/FileRepo.cs",
                "using Fixture.Contracts;\n\nnamespace Fixture.Files { public class FileRepo : IRepo { public void Save() { } } }",
            ),
            (
                "src/Sql/SqlRepo.cs",
                "using Fixture.Contracts;\n\nnamespace Fixture.Sql { public class SqlRepo : IRepo { public void Save() { } } }",
            ),
            (
                "src/App/Service.cs",
                "using Fixture.Contracts;\n\nnamespace Fixture.App;\n\npublic class Service\n{\n  public Service(IRepo repo) { }\n}\n",
            ),
        ]);
        let root = no_git_root();
        // App can reach Sql and not Files -- exactly the shape that settles a
        // ladder ambiguity, applied to a question the ladder never asked.
        let model = model_of(vec![
            unit(
                "src/App/App.csproj",
                &["src/Contracts/Contracts.csproj", "src/Sql/Sql.csproj"],
                false,
            ),
            unit("src/Contracts/Contracts.csproj", &[], false),
            unit(
                "src/Files/Files.csproj",
                &["src/Contracts/Contracts.csproj"],
                false,
            ),
            unit(
                "src/Sql/Sql.csproj",
                &["src/Contracts/Contracts.csproj"],
                false,
            ),
        ]);

        let ctor_di = |g: &Graph| -> (String, Vec<String>) {
            match find_edge(g, |e| matches!(e, Edge::CtorDi { .. })).expect("ctor-di edge present")
            {
                Edge::CtorDi {
                    resolution,
                    candidates,
                    ..
                } => (
                    resolution.clone(),
                    candidates.iter().map(|c| c.id.clone()).collect(),
                ),
                _ => unreachable!(),
            }
        };
        assert_eq!(
            ctor_di(&resolve_graph_with_model(&root, &files, &[], Some(&model))),
            ctor_di(&resolve_graph(&root, &files)),
            "two implementors is two implementors, model or no model"
        );
        assert_eq!(
            ctor_di(&resolve_graph(&root, &files)),
            (
                "ambiguous".to_string(),
                vec![
                    "Fixture.Files.FileRepo".to_string(),
                    "Fixture.Sql.SqlRepo".to_string()
                ]
            ),
            "pinned so the assertion above cannot pass on two identically-broken answers"
        );
    }

    #[test]
    fn stage6_narrowing_to_zero_gives_the_scored_tier_an_empty_pool_not_a_graph_wide_guess() {
        // Narrowing can also empty the candidate list, and the result is an
        // ordinary External -- not a silently-kept ambiguity and not an
        // invented pick. For the type ref that means the unresolved counter
        // rather than the ambiguous one.
        //
        // For the QUALIFIER it means no heuristic edge at all. The ladder did
        // find candidates here; the project model answered that none of them
        // is nameable at this site. That is an answer, so the scored tier gets
        // an empty pool rather than the graph-wide member-name uniqueness pool
        // an unfound name would get. `Ledger` is the proof the tier really
        // declines: it is not a `Config` at all, it is reachable from `App`,
        // and it declares `Load` -- so it is exactly the stranger the
        // uniqueness pool would have handed over.
        let mut files: Vec<(&str, &str)> = CROSS_PROJECT_AMBIGUITY_FIXTURE.to_vec();
        files.push((
            "src/Shared/Ledger.cs",
            "namespace Fixture.Shared { public class Ledger { public void Load() { } } }",
        ));
        let files = fragments_for(&files);
        let root = no_git_root();

        assert_eq!(
            heuristic_member_edges_from(&resolve_graph(&root, &files), "src/App/Runner.cs"),
            vec![("Fixture.Alpha.Config", 8), ("Fixture.Beta.Config", 8)],
            "without a model the ladder's ambiguous pool wins and Ledger is never in the running"
        );

        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/App/App.csproj", &["src/Shared/Shared.csproj"], false),
            unit("src/Beta/Beta.csproj", &[], false),
            unit("src/Shared/Shared.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

        assert!(
            ambiguous_edges_from(&g, "src/App/Runner.cs").is_empty(),
            "neither Config is nameable here, so there is nothing left to be ambiguous between"
        );
        assert_eq!(
            (g.stats.ambiguous_count, g.stats.unresolved_external_count),
            (0, 1),
            "the type ref moves from the ambiguous count to the external one, which is what an emptied pool MEANS"
        );
        assert!(
            member_edges_from(&g, "src/App/Runner.cs").is_empty(),
            "no precise edge is invented out of an empty candidate list"
        );
        assert!(
            heuristic_member_edges_from(&g, "src/App/Runner.cs").is_empty(),
            "every real `Config` candidate was ruled unreachable, which is an ANSWER -- the tier must not answer it again with a reachable stranger that merely declares `Load`"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "and nothing counted either: a declined guess is not a guess"
        );
    }

    #[test]
    fn stage6_a_bare_qualifier_narrowed_to_zero_declines_while_an_unfound_one_still_guesses() {
        // The two `External`s the scored tier must tell apart, in one resolve
        // and one file:
        //   `Foo.Bar()`     -- two real `Foo` candidates, neither reachable
        //                      from `App`. Narrowed to zero, so the tier
        //                      declines even though reachable `Ledger`
        //                      declares `Bar`.
        //   `Missing.Bar()` -- a name the ladder never found at all. Nothing
        //                      was ever narrowed, so the member-name
        //                      uniqueness pool applies as it always has and
        //                      `Ledger` IS the guess.
        // Without the split, both lines would guess `Ledger`.
        let files = fragments_for(&[
            (
                "src/Alpha/Foo.cs",
                "namespace Fixture.Alpha { public class Foo { public void Bar() { } } }",
            ),
            (
                "src/Beta/Foo.cs",
                "namespace Fixture.Beta { public class Foo { public void Bar() { } } }",
            ),
            (
                "src/Shared/Ledger.cs",
                "namespace Fixture.Shared { public class Ledger { public void Bar() { } } }",
            ),
            (
                "src/App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  public void Run()\n  {\n    Foo.Bar();\n    Missing.Bar();\n  }\n}\n",
            ),
        ]);
        let root = no_git_root();

        let model = model_of(vec![
            unit("src/Alpha/Alpha.csproj", &[], false),
            unit("src/App/App.csproj", &["src/Shared/Shared.csproj"], false),
            unit("src/Beta/Beta.csproj", &[], false),
            unit("src/Shared/Shared.csproj", &[], false),
        ]);
        let g = resolve_graph_with_model(&root, &files, &[], Some(&model));

        assert_eq!(
            heuristic_member_edges_from(&g, "src/App/Runner.cs"),
            vec![("Fixture.Shared.Ledger", 9)],
            "line 8's `Foo` was narrowed to zero and declines; line 9's `Missing` was never found and still reaches the uniqueness pool"
        );
        assert!(
            member_edges_from(&g, "src/App/Runner.cs").is_empty(),
            "no precise edge on either line"
        );
    }

    // The three-tier fixture: one file whose three member references are
    // claimed by three different tiers, so a single resolve exercises the whole
    // schema. `_widget.Render()` is a precise field hop, `_widget.Tally()` is
    // an extension call only tier (f) can claim, and `Config.Load()` is
    // ambiguous between two imported namespaces and reaches the scored tier.
    const THREE_TIER_FIXTURE: &[(&str, &str)] = &[
        ("Core/Widget.cs", "namespace App.Core { public class Widget { public void Render() { } } }"),
        (
            "Ext/WidgetExtensions.cs",
            "namespace App.Ext { public static class WidgetExtensions { public static void Tally(this Widget w) { } } }",
        ),
        ("Alpha/Config.cs", "namespace App.Alpha { public class Config { public void Load() { } } }"),
        ("Beta/Config.cs", "namespace App.Beta { public class Config { public void Load() { } } }"),
        (
            "Consumers/Consumer.cs",
            "\nusing App.Core;\nusing App.Ext;\nusing App.Alpha;\nusing App.Beta;\n\nnamespace App.Consumers;\n\npublic class Consumer\n{\n  private Widget _widget;\n\n  public void Run()\n  {\n    _widget.Render();\n    _widget.Tally();\n    Config.Load();\n  }\n}\n",
        ),
    ];

    #[test]
    fn stage5_schema_every_uses_member_edge_carries_its_member_and_only_heuristic_edges_carry_a_tier(
    ) {
        let files = fragments_for(THREE_TIER_FIXTURE);
        let g = resolve_graph(&no_git_root(), &files);

        let member_edges: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| matches!(e, Edge::UsesMember { .. }))
            .collect();
        for e in &member_edges {
            let Edge::UsesMember {
                heuristic,
                tier,
                member,
                ..
            } = e
            else {
                unreachable!()
            };
            assert!(
                member.is_some(),
                "every uses-member edge names its member, precise ones included: {e:?}"
            );
            assert_eq!(
                *heuristic,
                tier.is_some(),
                "the flag and the tier are one fact -- `Edge::uses_member` derives one from the other: {e:?}"
            );
        }

        let rows: Vec<(&str, Option<HeuristicTier>, Option<&str>)> = member_edges
            .iter()
            .map(|e| match e {
                Edge::UsesMember {
                    to, tier, member, ..
                } => (to.as_str(), *tier, member.as_deref()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                ("App.Core.Widget", None, Some("Render")),
                (
                    "App.Ext.WidgetExtensions",
                    Some(HeuristicTier::Ext),
                    Some("Tally")
                ),
                ("App.Alpha.Config", Some(HeuristicTier::Guess), Some("Load")),
                ("App.Beta.Config", Some(HeuristicTier::Guess), Some("Load")),
            ]
        );

        // One serialized sample per tier, pinned: the precise row gains
        // `member` and nothing else, and the two guess rows spell their tier
        // between the flag and the member.
        let bytes: Vec<String> = member_edges
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        assert_eq!(
            bytes[0],
            r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":15,"to":"App.Core.Widget","to_file":"Core/Widget.cs","member":"Render"}"#
        );
        assert_eq!(
            bytes[1],
            r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":16,"to":"App.Ext.WidgetExtensions","to_file":"Ext/WidgetExtensions.cs","heuristic":true,"tier":"ext","member":"Tally"}"#
        );
        assert_eq!(
            bytes[2],
            r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":17,"to":"App.Alpha.Config","to_file":"Alpha/Config.cs","heuristic":true,"tier":"guess","member":"Load"}"#
        );

        // And the counters partition the guesses: every heuristic edge is in
        // exactly one tier, so the two add up to the total.
        assert_eq!(
            g.stats.heuristic_by_tier,
            HeuristicByTier { ext: 1, guess: 2 }
        );
        assert_eq!(
            g.stats.heuristic_by_tier.ext + g.stats.heuristic_by_tier.guess,
            g.stats.heuristic_edge_count
        );
        assert_eq!(
            g.schema_version, GRAPH_SCHEMA_VERSION,
            "a graph carrying tier and member is a schema-2 graph"
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
            r#"{"def_count":9,"file_count":7,"edges_by_kind":{"inherits":1,"uses-type":1,"imports":4,"uses-member":2,"ctor-di":0},"ambiguous_count":0,"ambiguous_pct":0,"unresolved_external_count":0,"heuristic_edge_count":3,"test_def_count":0,"heuristic_by_tier":{"ext":0,"guess":3}}"#,
            "heuristic_by_tier is appended LAST, after test_def_count -- the stats key order graph.json pins"
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
            // The SAME extension call twice on ONE line: same declaring static
            // class, same member, same line -- two guesses that serialize to
            // the same bytes.
            (
                "Ops/Caller.cs",
                "\nusing App.Ext;\nusing App.Widgets;\n\nnamespace App.Ops;\n\npublic class Caller\n{\n  public string Run()\n  {\n    Widget widget = new Widget();\n    return widget.Tag() + widget.Tag();\n  }\n}\n",
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
        assert_eq!(
            g.stats.heuristic_by_tier,
            HeuristicByTier { ext: 1, guess: 0 },
            "and it leaves ITS tier's counter, not the other one"
        );
    }

    // The other side of the same rule, and the reason `member` had to join the
    // dedup key: two guesses that agree on every key the edge used to carry
    // and differ ONLY in the member they name are two facts, not a duplicate.
    // Before `member` existed these collapsed into one, and a reader lost a
    // call.
    #[test]
    fn heuristic_side_dedup_keeps_two_guesses_that_name_different_members_of_one_target() {
        let files = fragments_for(&[
            ("Widgets/Widget.cs", "namespace App.Widgets { public class Widget { } }"),
            (
                "Ext/Helpers.cs",
                "\nnamespace App.Ext;\n\npublic static class Helpers\n{\n  public static string Slug(this Widget widget)\n  {\n    return \"s\";\n  }\n\n  public static string Tag(this Widget widget)\n  {\n    return \"t\";\n  }\n}\n",
            ),
            (
                "Ops/Caller.cs",
                "\nusing App.Ext;\nusing App.Widgets;\n\nnamespace App.Ops;\n\npublic class Caller\n{\n  public string Run()\n  {\n    Widget widget = new Widget();\n    return widget.Tag() + widget.Slug();\n  }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Ops/Caller.cs"),
            vec![("App.Ext.Helpers", 12), ("App.Ext.Helpers", 12)],
            "two calls, two edges -- identical but for the member each names"
        );
        assert_eq!(
            heuristic_member_names_from(&g, "Ops/Caller.cs"),
            vec![Some("Tag"), Some("Slug")],
            "and the member is what tells them apart, in source order"
        );
        assert_eq!(g.stats.heuristic_edge_count, 2);
        assert_eq!(
            g.stats.heuristic_by_tier,
            HeuristicByTier { ext: 2, guess: 0 }
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
                    receiver_base: false,
                    receiver_awaited: false,
                    receiver_local: false,
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
                            receiver_base: false,
                            receiver_awaited: false,
                            receiver_local: false,
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

    // --- Stage 7: this/base receiver typing, awaited Task unwrap ------------
    //
    // All four run real C# through the extractor (`fragments_for`), the same
    // choice the tier-(e) end-to-end block above makes: a `this.`/`base.`
    // qualifier's `receiverBase`/`receiverAwaited` bits and a method's
    // `methodReturnArgs` are extractor facts, so a test that hand-built the
    // fragments would take the extractor's word for them rather than proving
    // them.

    #[test]
    fn stage7_this_member_resolves_to_the_declaring_def_across_partial_files() {
        let files = fragments_for(&[
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public string Name;\n}\n",
            ),
            (
                "Domain/Order.Validation.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public int Describe() => this.Name.Length;\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let name_edges: Vec<(&str, &str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "Domain/Order.Validation.cs"
                    && member.as_deref() == Some("Name") =>
                {
                    Some((to.as_str(), member.as_deref().unwrap(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            name_edges,
            vec![("App.Domain.Order", "Name", false)],
            "this.Name resolves through the ordinary typed-receiver path -- Name is declared in the \
             OTHER partial-class file, which the merged member lists already cover; no self-edge \
             rule was needed"
        );
    }

    #[test]
    fn stage7_base_member_resolves_to_the_first_base_that_declares_it() {
        let files = fragments_for(&[
            (
                "Domain/GrandBase.cs",
                "\nnamespace App.Domain;\n\npublic class GrandBase\n{\n    public void Touch() { }\n}\n",
            ),
            (
                "Domain/Base.cs",
                "\nnamespace App.Domain;\n\npublic class Base : GrandBase\n{\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Touch() { }\n\n    public void Poke()\n    {\n        base.Touch();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let touch_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Touch") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            touch_edges,
            vec![("App.Domain.GrandBase", false)],
            "base.Touch() starts at Order's OWN bases -- Base does not declare Touch, so the walk \
             continues to Base's own base GrandBase, which does; Order's OWN override (also named \
             Touch) is never even considered"
        );
    }

    #[test]
    fn stage7_base_member_declared_nowhere_in_graph_resolves_external() {
        let files = fragments_for(&[
            (
                "Domain/Base.cs",
                "\nnamespace App.Domain;\n\npublic class Base\n{\n    public void Other() { }\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Touch() { }\n\n    public void Poke()\n    {\n        base.Touch();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let touch_edges: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| {
                matches!(e, Edge::UsesMember { from_file, member, .. }
                    if from_file == "Domain/Order.cs" && member.as_deref() == Some("Touch"))
            })
            .collect();
        assert!(
            touch_edges.is_empty(),
            "no in-graph base declares Touch -- base.Touch() is external like any other unresolved \
             receiver, never a scored guess, even though Order itself declares Touch: {touch_edges:?}"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage7_base_member_lookup_on_a_cyclic_hierarchy_never_binds_the_enclosing_type() {
        // `class A : B` / `class B : A` is not valid C#, but it parses, and a
        // graph built from half-written source can hold it. Walking B's own
        // bases leads straight back to A, and A declares Only -- so without a
        // guard the `base.` lookup answers with the very type the call was
        // written in, the self-edge a `base.` qualifier can never mean.
        let files = fragments_for(&[
            (
                "Domain/A.cs",
                "\nnamespace App.Domain;\n\npublic class A : B\n{\n    public void Only() { }\n\n    public void Go() { base.Only(); }\n}\n",
            ),
            (
                "Domain/B.cs",
                "\nnamespace App.Domain;\n\npublic class B : A\n{\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Domain/A.cs").is_empty(),
            "no in-graph BASE of A declares Only -- reaching A again through the cycle is not an \
             answer, so base.Only() is external exactly like a member no base declares at all: \
             {:?}",
            member_edges_from(&g, "Domain/A.cs")
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "and a `base.` ref never falls through to a guess either"
        );
    }

    #[test]
    fn stage7_awaited_static_call_local_unwraps_task_once() {
        let files = fragments_for(&[
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
            ),
            (
                "Infra/Repo.cs",
                "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static Task<Order> LoadAsync() => null;\n    public static Task<Task<Order>> LoadNestedAsync() => null;\n}\n",
            ),
            (
                "App/Worker.cs",
                "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public async Task Run()\n    {\n        var order = await Repo.LoadAsync();\n        order.Validate();\n\n        var nested = await Repo.LoadNestedAsync();\n        nested.Validate();\n\n        var plain = Repo.LoadAsync();\n        plain.Validate();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let validate_edges: Vec<(&str, usize, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    from_line,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                    Some((to.as_str(), *from_line, *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            validate_edges,
            vec![("App.Domain.Order", 9, false)],
            "the SINGLY-wrapped AWAITED call (`order`) unwraps Task<Order> to Order precisely; the \
             DOUBLY-wrapped awaited call (`nested`) unwraps only once, landing on the bare name \
             \"Task\" (never Order), and the UNAWAITED call (`plain`) is never unwrapped at all -- \
             both of the latter two stay typed \"Task\", resolve to nothing in-graph, and earn no \
             edge at all, guessed or otherwise"
        );
    }

    // --- Unit C: chain-tail receivers -----------------------------------

    #[test]
    fn stage7_chain_tail_resolves_through_one_method_return_hop() {
        let files = fragments_for(&[
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
            ),
            (
                "Infra/Repo.cs",
                "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static Order Load() => null;\n}\n",
            ),
            (
                "App/Worker.cs",
                "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public void Run()\n    {\n        Repo.Load().Validate();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let validate_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            validate_edges,
            vec![("App.Domain.Order", false)],
            "the chain tail `.Validate()` resolves through the ONE method_returns hop off \
             `Repo.Load()`, precisely and non-heuristically -- exactly like a `var x = \
             Repo.Load(); x.Validate();` local already would"
        );
    }

    // --- Unit B: cross-file field facts, the bare-identifier fallback -------
    //
    // All three run real C# through the extractor (`fragments_for`), the same
    // choice the four stage-7 tests above make: a field's declared type is an
    // extractor fact (`FragDef.fieldTypes`), so a test that hand-built the
    // fragments would take the extractor's word for it rather than proving
    // it end to end.

    #[test]
    fn stage7_partial_class_field_declared_in_a_sibling_file_types_the_receiver() {
        let files = fragments_for(&[
            (
                "Infra/Widget.cs",
                "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    private Widget _widget;\n}\n",
            ),
            (
                "Domain/Order.Extra.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        _widget.Spin();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let spin_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "Domain/Order.Extra.cs" && member.as_deref() == Some("Spin") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            spin_edges,
            vec![("App.Infra.Widget", false)],
            "_widget.Spin() carries no in-file fact at all in Order.Extra.cs -- _widget is declared \
             as a field only in the OTHER partial-class file -- so it is typed from the merged \
             field_types table the resolver builds across both files instead"
        );
    }

    #[test]
    fn stage7_protected_field_declared_on_a_base_types_the_receiver() {
        let files = fragments_for(&[
            (
                "Infra/Logger.cs",
                "\nnamespace App.Infra;\n\npublic class Logger\n{\n    public void Log() { }\n}\n",
            ),
            (
                "Domain/Base.cs",
                "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected Logger _logger;\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke()\n    {\n        _logger.Log();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let log_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Log") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            log_edges,
            vec![("App.Infra.Logger", false)],
            "_logger.Log() has no in-file fact anywhere in Order.cs -- Order itself declares no \
             _logger field at all -- so the fallback walks Order's OWN bases: Base declares it, \
             typed Logger, which declares Log"
        );
    }

    #[test]
    fn stage7_a_cross_file_field_type_resolves_in_its_declaring_files_context() {
        // Two types named Alpha, in two namespaces. The base declares the
        // field under `using N1`; the derived file that reads it imports N2
        // instead and has never heard of N1.Alpha. The field's declared type
        // is a bare name that only means N1.Alpha, so the reading file's own
        // imports must not be what decides which Alpha it names.
        let files = fragments_for(&[
            (
                "N1/Alpha.cs",
                "\nnamespace N1;\n\npublic class Alpha\n{\n    public void Ship() { }\n}\n",
            ),
            (
                "N2/Alpha.cs",
                "\nnamespace N2;\n\npublic class Alpha\n{\n    public void Ship() { }\n}\n",
            ),
            (
                "App/BaseT.cs",
                "\nusing N1;\n\nnamespace App;\n\npublic class BaseT\n{\n    protected Alpha _thing;\n}\n",
            ),
            (
                "App/Derived.cs",
                "\nusing N2;\n\nnamespace App;\n\npublic class Derived : BaseT\n{\n    public void Go() { _thing.Ship(); }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "App/Derived.cs"),
            vec![("N1.Alpha", 8)],
            "the field fact came from BaseT.cs, so its type name is resolved under BaseT.cs's own \
             usings, namespace and nesting -- Derived.cs's `using N2` is not evidence about a \
             declaration written in another file"
        );
    }

    #[test]
    fn stage7_an_in_file_local_shadows_a_same_named_field_fact() {
        let files = fragments_for(&[
            (
                "Infra/Widget.cs",
                "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
            ),
            (
                "Infra/Gadget.cs",
                "\nnamespace App.Infra;\n\npublic class Gadget\n{\n    public void Zap() { }\n}\n",
            ),
            (
                "Domain/Base.cs",
                "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected Widget _item;\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke()\n    {\n        var _item = new Gadget();\n        _item.Zap();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let zap_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "Domain/Order.cs" && member.as_deref() == Some("Zap") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            zap_edges,
            vec![("App.Infra.Gadget", false)],
            "Poke's own local `_item` (typed Gadget, which declares Zap) is an in-file fact for the \
             name, so the base's same-named field (typed Widget, which does NOT declare Zap) is \
             never even consulted -- a local always shadows a same-named field fact, precisely \
             because the fallback only ever runs when receiver_type is still unset"
        );
    }

    #[test]
    fn stage7_an_untyped_in_file_local_still_shadows_a_same_named_field_fact() {
        // `var order = Unknown();` is a BARE (undotted) call -- a shape
        // `invocation_call` never matches (it requires a dotted qualifier,
        // "Q.M()") -- so `order` settles as an ordinary taken-but-unknown
        // member-table entry: no `Fact` vouches for its type, but the name
        // IS in scope, exactly like a real (typed) local. `Order.Fields.cs`
        // declares a field of the SAME name in a SIBLING partial-class
        // file, which is precisely the shape the field/property fallback
        // exists to answer for a name with no in-file fact -- this proves
        // it does NOT answer here, because `order` is one.
        let with_field = fragments_for(&[
            (
                "Infra/Widget.cs",
                "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
            ),
            (
                "Domain/Order.Fields.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    private Widget order;\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        var order = Unknown();\n        order.Spin();\n    }\n}\n",
            ),
        ]);
        let without_field = fragments_for(&[
            (
                "Infra/Widget.cs",
                "\nnamespace App.Infra;\n\npublic class Widget\n{\n    public void Spin() { }\n}\n",
            ),
            (
                "Domain/Order.Fields.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic partial class Order\n{\n    public void Poke()\n    {\n        var order = Unknown();\n        order.Spin();\n    }\n}\n",
            ),
        ]);
        let g_with = resolve_graph(&no_git_root(), &with_field);
        let g_without = resolve_graph(&no_git_root(), &without_field);
        // Scoped to `uses-member` edges: the field's OWN declared type earns
        // an ordinary `uses-type` ref (see the walk's `field_declaration`
        // arm) whether or not this test's concern holds, so comparing the
        // WHOLE graph would differ by that one incidental edge every time --
        // it is not what this test is about. What this test is about is
        // whether the field ever gets to answer a `uses-member` ref it has
        // no business answering.
        fn uses_member_edges(g: &Graph) -> Vec<Edge> {
            g.edges
                .iter()
                .filter(|e| matches!(e, Edge::UsesMember { .. }))
                .cloned()
                .collect()
        }
        assert_eq!(
            uses_member_edges(&g_with),
            uses_member_edges(&g_without),
            "the untyped local `order` is a member-table entry for the name (taken, unknown) -- \
             `receiver_local` -- so it shadows the sibling file's same-named field exactly like a \
             typed local already does; the uses-member edge set must be identical whether or not \
             that field exists at all"
        );
        // Both variants DO carry one `uses-member` edge for `order.Spin()` --
        // `Spin` is declared by exactly one def anywhere in this fixture
        // (Widget), so the SCORED tier's own uniqueness fallback (a
        // wholly separate mechanism from the field/property fallback this
        // test guards, reached only when a ref carries NO receiver fact at
        // all) claims it as a heuristic guess in BOTH variants alike --
        // proof by itself that the field played no part, since it fires
        // identically whether or not the field exists. What distinguishes
        // "the field answered" from "an unrelated tier guessed" is
        // `heuristic`: the field/property fallback feeds the ordinary
        // typed-receiver path, which only ever emits a PRECISE
        // (non-heuristic) edge.
        let edges = uses_member_edges(&g_with);
        assert_eq!(
            edges,
            vec![Edge::UsesMember {
                from_file: "Domain/Order.cs".to_string(),
                from_line: 9,
                to: "App.Infra.Widget".to_string(),
                to_file: "Infra/Widget.cs".to_string(),
                member: Some("Spin".to_string()),
                heuristic: true,
                tier: Some(HeuristicTier::Guess),
            }],
            "the one edge present is the SCORED tier's own heuristic guess, never a precise edge \
             from the field/property fallback (which the shadowing rule keeps from ever running \
             here): {edges:?}"
        );
    }

    // --- Unit A3: non-public hierarchy-internal members, interface-skipped
    // base lookup, tier (f)'s closure fallback, and the typed-receiver
    // precise tier's own base walk -----------------------------------------

    #[test]
    fn stage7_base_member_that_is_protected_resolves_to_the_base_that_declares_it() {
        let files = fragments_for(&[
            (
                "Domain/Base.cs",
                "\nnamespace App.Domain;\n\npublic class Base\n{\n    protected void Touch() { }\n}\n",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : Base\n{\n    public void Poke() => base.Touch();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Domain/Order.cs"),
            vec![("App.Domain.Base", 6)],
            "Touch is protected -- absent from Base's public `methods` list, present only in \
             `nonPublicMethods` -- but a `base.` site is by construction inside the hierarchy it \
             is walking, so base_member_declared reads any-visibility and resolves precisely \
             anyway"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage7_base_lookup_skips_interface_bases() {
        let files = fragments_for(&[
            (
                "Domain/IGreeter.cs",
                "namespace App.Domain { public interface IGreeter { void Greet(); } }",
            ),
            (
                "Domain/Base.cs",
                "namespace App.Domain { public class Base { public void Greet() { } } }",
            ),
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order : IGreeter, Base\n{\n    public void Greet() { }\n\n    public void Poke() => base.Greet();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Domain/Order.cs"),
            vec![("App.Domain.Base", 8)],
            "IGreeter is listed FIRST in Order's base list and also declares Greet, but base. \
             never names an interface member -- base_member_declared skips it (and never walks \
             its own closure) and continues to Base, the class, which is the right target. \
             Order's own override (also named Greet) is never even considered, matching the \
             existing non-interface base test."
        );
    }

    #[test]
    fn stage7_extension_declared_on_an_interface_binds_through_the_receivers_base_closure() {
        let files = fragments_for(&[
            (
                "Domain/ISpecification.cs",
                "namespace App.Domain { public interface ISpecification { } }",
            ),
            (
                "Domain/BatchOptions.cs",
                "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class BatchOptions : ISpecification\n{\n    public void Validate() => this.Fail();\n}\n",
            ),
            (
                "Ext/SpecExtensions.cs",
                "namespace App.Ext { public static class SpecExtensions { public static void Fail(this ISpecification spec) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(BatchOptions opts) => opts.Fail();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        // The `this` receiver: BatchOptions itself declares nothing named
        // Fail, and typed_receiver_base_member's own base walk skips
        // ISpecification (an interface, per its own rule) and finds nothing
        // either -- so the exact-key lookup at tier (f) misses ("Fail
        // BatchOptions" names no bucket) and the closure fallback (Unit A3
        // item 3) tries BatchOptions's raw base string "ISpecification" next,
        // which the extension actually keys on.
        assert_eq!(
            heuristic_member_edges_from(&g, "Domain/BatchOptions.cs"),
            vec![("App.Ext.SpecExtensions", 8)],
            "this.Fail() binds through BatchOptions's OWN raw base string, tried as a fallback key \
             once the exact receiver-type key misses"
        );
        // The ordinary LOCAL receiver: `opts` is a ref with no this. shape at
        // all (its enclosing type is Runner, not BatchOptions), proving the
        // fallback is not `this.`-specific.
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Ext.SpecExtensions", 9)],
            "opts.Fail() -- an ordinary typed local, not this. -- binds through the exact same \
             closure fallback: item 3 applies to every typed receiver"
        );
        assert_eq!(g.stats.heuristic_by_tier.ext, 2);
    }

    #[test]
    fn stage7_typed_receiver_member_declared_on_an_in_graph_base_resolves_to_the_base() {
        let files = fragments_for(&[
            (
                "Domain/Base.cs",
                "namespace App.Domain { public class Base { public void Touch() { } } }",
            ),
            (
                "Domain/Order.cs",
                "namespace App.Domain { public class Order : Base { } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Order o) => o.Touch();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Domain.Base", 8)],
            "Order itself declares nothing named Touch -- the typed-receiver precise tier \
             (previously exact-def-only) now walks Order's in-graph base closure (Unit A3 item 4) \
             and binds to Base, the first def that declares it. `o` is an ordinary parameter, not \
             `this.`, so only the public list is consulted -- proven sufficient here since Touch \
             is public."
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "a precise hit, not a guess"
        );
    }

    #[test]
    fn stage7_a_scored_guess_never_vouches_through_a_non_public_member() {
        // Two same-named, same-shaped classes in different namespaces (no
        // using imports either), so `_widget`'s declared type "Widget"
        // resolves AMBIGUOUS -- the scored tier's ambiguous pool, filtered by
        // `member_vouched`. Alpha.Widget declares Ping publicly; Beta.Widget
        // declares the SAME name but only privately.
        let files = fragments_for(&[
            (
                "Alpha/Widget.cs",
                "namespace Fixture.Alpha { public class Widget { public void Ping() { } } }",
            ),
            (
                "Beta/Widget.cs",
                "namespace Fixture.Beta { public class Widget { private void Ping() { } } }",
            ),
            (
                "App/Runner.cs",
                "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Widget _widget;\n\n  public void Run() => _widget.Ping();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "App/Runner.cs"),
            vec![("Fixture.Alpha.Widget", 8)],
            "Beta.Widget also declares Ping, but only NON-publicly -- member_vouched (and the \
             Call-shape `declares_member` it reads) is untouched by Unit A3's non-public tables, \
             so Beta.Widget never enters the scored guess at all, even though it sits right in the \
             ambiguous pool this ref's receiver resolved to; only Alpha.Widget, which declares \
             Ping publicly, vouches"
        );
        assert_eq!(g.stats.heuristic_by_tier.guess, 1);
    }

    // --- Unit A4: base-walk declaration order (+ interface skip at any
    // depth) and arity-aware call vouching -----------------------------

    #[test]
    fn stage7_base_walk_visits_class_bases_in_declaration_order_before_any_interface() {
        // Endpoint : BaseEndpoint (a single class base). BaseEndpoint's OWN
        // base list names its class base FIRST, an interface SECOND --
        // BasePipe declares Go directly; IEndpoint reaches Go only through
        // ITS OWN base, IPipe, two levels down. A walk that visits siblings
        // in REVERSE declaration order (a LIFO stack popping the
        // last-pushed base first) would explore IEndpoint's entire closure
        // -- and find IPipe's Go -- before ever touching BasePipe, which is
        // the correct C# answer.
        let files = fragments_for(&[
            (
                "Domain/IPipe.cs",
                "namespace App.Domain { public interface IPipe { void Go(); } }",
            ),
            (
                "Domain/IEndpoint.cs",
                "namespace App.Domain { public interface IEndpoint : IPipe { } }",
            ),
            (
                "Domain/BasePipe.cs",
                "namespace App.Domain { public class BasePipe { public void Go() { } } }",
            ),
            (
                "Domain/BaseEndpoint.cs",
                "namespace App.Domain { public class BaseEndpoint : BasePipe, IEndpoint { } }",
            ),
            (
                "Domain/Endpoint.cs",
                "namespace App.Domain { public class Endpoint : BaseEndpoint { } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Endpoint ep) => ep.Go();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Domain.BasePipe", 8)],
            "BasePipe, BaseEndpoint's FIRST base, wins over IEndpoint's (SECOND base) own \
             interface closure -- declaration order, not stack order"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "a precise hit, not a guess"
        );
    }

    #[test]
    fn stage7_class_typed_receiver_never_binds_to_an_interface_declaration_at_any_depth() {
        // Touch is declared ONLY on IHasTouch, an interface reached
        // TRANSITIVELY through Base's own base list -- not a direct base of
        // the class-typed receiver Derived at all (Derived -> Base ->
        // IHasTouch, two levels down). Base implements IHasTouch but
        // declares no override of its own, and Derived adds nothing either.
        let files = fragments_for(&[
            (
                "Domain/IHasTouch.cs",
                "namespace App.Domain { public interface IHasTouch { void Touch(); } }",
            ),
            (
                "Domain/Base.cs",
                "namespace App.Domain { public class Base : IHasTouch { } }",
            ),
            (
                "Domain/Derived.cs",
                "namespace App.Domain { public class Derived : Base { } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Derived d) => d.Touch();\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
            "the base walk must skip IHasTouch at EVERY depth it is reached, not only when it is \
             Derived's own direct base -- an interface's member declaration is a contract, never a \
             precise bind target, for a class-typed receiver"
        );
        assert!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
            "no extension method named Touch exists anywhere in this fixture either, so this is a \
             silent miss, not a guess"
        );
    }

    #[test]
    fn stage7_a_call_whose_arity_matches_no_instance_overload_falls_through_to_the_extension_tier()
    {
        // Widget.Stop takes exactly one argument; the call passes two. No
        // overload admits it, so the precise tier must decline -- and tier
        // (f)'s own veto, reading the SAME arity-aware `declares_member`,
        // must decline too, letting the two-argument extension bind.
        let files = fragments_for(&[
            (
                "Domain/Widget.cs",
                "namespace App.Domain { public class Widget { public void Stop(int a) { } } }",
            ),
            (
                "Ext/WidgetExt.cs",
                "using App.Domain;\n\nnamespace App.Ext { public static class WidgetExt { public static void Stop(this Widget w, int a, int b) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\nusing App.Ext;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void Run(Widget w) => w.Stop(1, 2);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert!(
            member_edges_from(&g, "Consumers/Runner.cs").is_empty(),
            "Widget declares Stop, but only a ONE-argument overload -- the call passes two \
             arguments, which no overload admits, so the precise tier must not claim the ref"
        );
        assert_eq!(
            heuristic_member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Ext.WidgetExt", 9)],
            "the arity mismatch on the instance side also un-vetoes tier (f): Stop(this Widget w, \
             int a, int b) admits two arguments and is the only candidate"
        );
    }

    #[test]
    fn stage7_a_call_admitted_by_a_params_or_optional_overload_binds_to_the_instance_member() {
        let files = fragments_for(&[
            (
                "Domain/Widget.cs",
                "namespace App.Domain { public class Widget { public void Send(int a, int b = 0) { } public void Spray(params int[] xs) { } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public void RunOptional(Widget w) => w.Send(1);\n    public void RunParams(Widget w) => w.Spray(1, 2, 3, 4, 5);\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Domain.Widget", 8), ("App.Domain.Widget", 9)],
            "Send(1) falls inside the OPTIONAL-parameter overload's (1, 2) range, and Spray(1, 2, \
             3, 4, 5) falls inside the `params` overload's unbounded (0, -1) range -- both admit \
             the call, so both bind precisely to the instance member"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "two precise hits, no guess"
        );
    }

    #[test]
    fn stage7_partial_class_overloads_declared_in_sibling_files_both_admit_their_calls() {
        // One partial class, one overload set, split across two files. The
        // merged arity table has to hold BOTH overloads: either call is a
        // call the type accepts, and the file that cannot see the other
        // part's declaration is exactly the file that needs the merge.
        let files = fragments_for(&[
            (
                "Domain/Svc.cs",
                "\nnamespace App.Domain;\n\npublic partial class Svc\n{\n    public void Run() { }\n\n    public void First() { this.Run(1); }\n}\n",
            ),
            (
                "Domain/Svc.More.cs",
                "\nnamespace App.Domain;\n\npublic partial class Svc\n{\n    public void Run(int n) { }\n\n    public void Second() { this.Run(); }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Domain/Svc.cs"),
            vec![("App.Domain.Svc", 8)],
            "this.Run(1) is admitted by the ONE-argument overload the SIBLING file declares --              merging the arity ranges per name is what lets the first-declaring part's              zero-argument range stop hiding it"
        );
        assert_eq!(
            member_edges_from(&g, "Domain/Svc.More.cs"),
            vec![("App.Domain.Svc", 8)],
            "and the zero-argument call keeps binding from the other direction -- the merge is a              union, so neither part's overload set is lost"
        );
        assert_eq!(
            g.stats.heuristic_edge_count, 0,
            "two precise hits, no guess"
        );
    }

    #[test]
    fn stage7_a_read_of_a_property_is_still_name_only() {
        let files = fragments_for(&[
            (
                "Domain/Sensor.cs",
                "namespace App.Domain { public class Sensor { public string Label { get; } } }",
            ),
            (
                "Consumers/Runner.cs",
                "\nusing App.Domain;\n\nnamespace App.Consumers;\n\npublic class Runner\n{\n    public string Run(Sensor s) => s.Label;\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            member_edges_from(&g, "Consumers/Runner.cs"),
            vec![("App.Domain.Sensor", 8)],
            "s.Label is a READ (no argCount at all) -- declares_member's arg_count == None branch \
             is untouched by Unit A4 item 2's arity gate, so a property still resolves precisely on \
             name alone, exactly as before"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    // --- Unit A5: chain-tail hop failures, and closure-key generic
    // unification against the MATCHED base's own arguments ------------------

    #[test]
    fn stage7_a_chain_tail_whose_hop_fails_emits_no_guess() {
        // `Unknown` names no in-graph def at all, so the chain tail's own
        // method-return hop cannot even resolve an OWNER, let alone a
        // return type: `receiver_type_name` stays `None`. `Order.Validate`
        // is the only in-graph def vouching for the member name "Validate"
        // -- exactly the sole candidate a scored guess drawn from the raw,
        // receiver-blind name-uniqueness pool would land on, since nothing
        // can filter that pool by receiver when there IS no receiver type
        // at all.
        let files = fragments_for(&[
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
            ),
            (
                "App/Worker.cs",
                "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public void Run()\n    {\n        Unknown.Load().Validate();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let validate_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            validate_edges,
            Vec::<(&str, bool)>::new(),
            "Unknown.Load() never resolves an owner in-graph at all -- the hop yields NO receiver \
             type, in-graph or otherwise -- so the chain tail `.Validate()` is finished as external \
             right there: without this guard it would fall into the scored tier's unfiltered \
             name-uniqueness pool and guess App.Domain.Order, the sole in-graph Validate"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage7_a_chain_tail_whose_hop_lands_on_an_external_type_is_silent() {
        // `Repo.Load()` DOES resolve an in-graph owner and DOES have a
        // recorded return type -- "ExternalWidget" -- so the hop is not the
        // empty case the guard above catches: `receiver_type_name` is
        // `Some("ExternalWidget")`, exactly like a `Q.M()` local's own call
        // hop, and this ref keeps walking the ordinary typed-receiver path
        // rather than being force-silenced. "ExternalWidget" is declared
        // NOWHERE in this fixture, so that path itself comes up empty on
        // its own: tier (f) finds no "Validate ExternalWidget" extension
        // bucket, and the scored tier's own receiver rule
        // (`receiver_admits_candidate`) correctly refuses the one same-named
        // candidate (App.Domain.Order, which declares Validate) because
        // Order is nominally assignable to nothing named "ExternalWidget" --
        // no base, no name match. The observable result is the same silence
        // Unit A5 item 1 requires, produced by the EXISTING filters rather
        // than a new one: a chain tail with a real but external target type
        // is still an answerable receiver, just one this corpus proves
        // nothing about here.
        let files = fragments_for(&[
            (
                "Domain/Order.cs",
                "\nnamespace App.Domain;\n\npublic class Order\n{\n    public void Validate() { }\n}\n",
            ),
            (
                "Infra/Repo.cs",
                "\nnamespace App.Infra;\n\npublic static class Repo\n{\n    public static ExternalWidget Load() => null;\n}\n",
            ),
            (
                "App/Worker.cs",
                "\nnamespace App.Workers;\n\npublic class Worker\n{\n    public void Run()\n    {\n        Repo.Load().Validate();\n    }\n}\n",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        let validate_edges: Vec<(&str, bool)> = g
            .edges
            .iter()
            .filter_map(|e| match e {
                Edge::UsesMember {
                    from_file,
                    to,
                    member,
                    heuristic,
                    ..
                } if from_file == "App/Worker.cs" && member.as_deref() == Some("Validate") => {
                    Some((to.as_str(), *heuristic))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            validate_edges,
            Vec::<(&str, bool)>::new(),
            "the hop lands the receiver on \"ExternalWidget\", a real but external type name -- no \
             extension binds it and App.Domain.Order (the only in-graph Validate) is not \
             assignable to it, so the ref is silent"
        );
        assert_eq!(g.stats.heuristic_edge_count, 0);
    }

    #[test]
    fn stage7_extension_on_an_implemented_interface_binds_for_a_generic_enclosing_type() {
        // BatchOptions<T> is GENERIC (unlike Unit A3's own non-generic
        // BatchOptions fixture), so `this.Fail()`'s receiver_args is
        // `Some(["*"])` -- BatchOptions's own type parameter, wildcarded.
        // ISpecification is written into BatchOptions's base list with NO
        // type-argument list at all (it is not generic), so
        // `base_generic_args` records no entry for it at all. Before Unit
        // A5 item 2, filter 3 compared SpecExtensions's `this_args` (`None`
        // -- Fail's `this ISpecification` is non-generic) against the
        // RECEIVER's own `Some(["*"])`, a hard (None, Some) mismatch that
        // dropped the edge; the fix compares against the matched base's own
        // arguments (`None`, since ISpecification carries none), which
        // unify with a non-generic `this` regardless of BatchOptions's own
        // arity.
        let files = fragments_for(&[
            (
                "Domain/ISpecification.cs",
                "namespace App.Domain { public interface ISpecification { } }",
            ),
            (
                "Domain/BatchOptions.cs",
                "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class BatchOptions<T> : ISpecification\n{\n    public void Validate() => this.Fail();\n}\n",
            ),
            (
                "Ext/SpecExtensions.cs",
                "namespace App.Ext { public static class SpecExtensions { public static void Fail(this ISpecification spec) { } } }",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Domain/BatchOptions.cs"),
            vec![("App.Ext.SpecExtensions", 8)],
            "this.Fail() binds through BatchOptions's OWN raw base string \"ISpecification\", with \
             the non-generic base's OWN (absent) arguments unifying against Fail's non-generic \
             this-parameter -- BatchOptions's own generic arity never enters the comparison"
        );
        assert_eq!(g.stats.heuristic_by_tier.ext, 1);
    }

    #[test]
    fn stage7_extension_unification_uses_the_matched_base_arguments() {
        // Repository<TKey, TValue> implements IRepository<TValue> -- ONE of
        // its own two type parameters, not both -- so `base_generic_args`
        // records IRepository's own arity as a SINGLE wildcard
        // (`Some(["*"])`), one element shorter than the receiver's own
        // `receiver_args` (`Some(["*", "*"])`, both of Repository's own type
        // parameters). RepoExtensions.Validate<T>(this IRepository<T> repo)
        // is generic too, so `this_args` is also a single wildcard
        // (`Some(["*"])`). Unifying against the RECEIVER's own two-element
        // arguments (the pre-Unit-A5 behaviour) is a length mismatch that
        // drops the edge; unifying against the matched base's own
        // one-element arguments -- what Unit A5 item 2 wires -- matches.
        let files = fragments_for(&[
            (
                "Domain/IRepository.cs",
                "namespace App.Domain { public interface IRepository<T> { } }",
            ),
            (
                "Domain/Repository.cs",
                "\nusing App.Ext;\n\nnamespace App.Domain;\n\npublic class Repository<TKey, TValue> : IRepository<TValue>\n{\n    public void Poke() => this.Validate();\n}\n",
            ),
            (
                "Ext/RepoExtensions.cs",
                "namespace App.Ext { public static class RepoExtensions { public static void Validate<T>(this IRepository<T> repo) { } } }",
            ),
        ]);
        let g = resolve_graph(&no_git_root(), &files);
        assert_eq!(
            heuristic_member_edges_from(&g, "Domain/Repository.cs"),
            vec![("App.Ext.RepoExtensions", 8)],
            "this.Validate() binds through IRepository, unifying Validate's own single wildcard \
             this-argument against IRepository's own single wildcard argument AS Repository \
             DECLARED IT (\"IRepository<TValue>\") -- not against Repository's own two-argument \
             receiver_args, which would fail the length check"
        );
        assert_eq!(g.stats.heuristic_by_tier.ext, 1);
    }
}
