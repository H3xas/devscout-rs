use crate::graph::{AlsoIn, Def, FragExtensionMethod, FragFact, FragRef, Fragment, OrderedMap};
use std::collections::{HashMap, HashSet};

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
    /// Method name -> the overloads' own parameter-descriptor lists, each
    /// paired with the file whose fragment declared it -- see
    /// `FragDef.method_params`. Merged across a partial class as a UNION per
    /// NAME, the way `method_arities` is: every declaring part contributes
    /// the overloads it declares, an overload whose `params` list already
    /// exists for that name skipped (first file wins for duplicates). In
    /// memory only; nothing here is serialized.
    pub method_params: HashMap<String, Vec<MethodOverloadParams>>,
}

/// One method overload's parameter-descriptor list plus its declaring file.
///
/// The (params, file) pair `MemberLists::method_params` keeps per method
/// name. In-memory resolution input only; nothing here is serialized.
pub struct MethodOverloadParams {
    /// The parameter type descriptors, in order (see extract.rs's
    /// `type_descriptor`/`DefRecord::method_params`).
    pub params: Vec<String>,
    /// The fragment-relative file path that declared this overload.
    pub file: String,
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "one ordered pass building every lookup table the resolver ladder reads, so the tables stay mutually consistent"
)]
pub(super) fn build_def_index(fragments_by_file: &[(String, Fragment)]) -> DefIndex {
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
                        method_params: {
                            let mut m: HashMap<String, Vec<MethodOverloadParams>> = HashMap::new();
                            for (name, overloads) in d.method_params.iter() {
                                m.insert(
                                    name.clone(),
                                    overloads
                                        .iter()
                                        .map(|params| MethodOverloadParams {
                                            params: params.clone(),
                                            file: file.clone(),
                                        })
                                        .collect(),
                                );
                            }
                            m
                        },
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
                    // A UNION per NAME, same as `method_arities` just above:
                    // every declaring part's overloads are admitted, an
                    // overload whose `params` list a merged entry already
                    // holds skipped -- first file wins for that duplicate.
                    for (name, overloads) in d.method_params.iter() {
                        let merged = member_lists[idx]
                            .method_params
                            .entry(name.clone())
                            .or_default();
                        for params in overloads {
                            if !merged.iter().any(|o| &o.params == params) {
                                merged.push(MethodOverloadParams {
                                    params: params.clone(),
                                    file: file.clone(),
                                });
                            }
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
pub(super) fn name_probe(name: String, namespace: &str, outer_types: Vec<String>) -> FragRef {
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
        receiver_lambda: None,
    }
}
