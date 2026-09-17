use super::arity::{arity_accepts, generic_args_unify, resolve_ref_by_arity};
use super::dispatch::append_dispatch_edges;
use super::edges::{build_implementor_index, heuristic_edge_key, resolve_ctor_param, type_edge};
use super::index::{build_def_index, name_probe, ExtCandidate};
use super::ladder::{
    capped_candidates, narrow_by_reachability, narrow_tracked, resolve_ref, type_candidate,
    Admission, Narrowed, Resolution, Via,
};
use super::members::{
    base_member_declared, declares_member, declares_member_any_visibility, extension_closure_key,
    inherited_member_declared, member_shape, member_vouched, typed_receiver_base_member,
};
use super::provenance::{self, Step};
use super::receiver::{
    bare_receiver_field_or_property_type, def_outer_types, extractor_vouches_instance,
    is_this_shaped_receiver, lambda_slot_receiver_type, receiver_admits_candidate,
    AssignabilityCache, ReceiverFieldType,
};
use super::scope::{
    build_file_contexts, collect_global_usings_by_unit, namespace_encloses, score_candidate,
    FileContext,
};
use crate::graph::{
    Edge, EdgesByKind, Fragment, Graph, GraphName, HeuristicByTier, HeuristicTier, Percent1, Stats,
    GRAPH_SCHEMA_VERSION,
};
use crate::manifest;
use std::collections::{HashMap, HashSet};
use std::path::Path;

// The two caps the scored heuristic tier lives inside. The uniqueness cap is a
// REFUSAL threshold: a member name carried by more than this many defs
// graph-wide is too common to guess from, so the tier emits nothing at all
// rather than a wide fan of maybes. The emit cap bounds how many of the
// surviving candidates a single ref may name.
const SCORED_UNIQUENESS_CAP: usize = 3;
const SCORED_EMIT_CAP: usize = 3;

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
/// alongside.
///
/// This is `devscout map`'s entry point, and the only one that can produce a
/// graph carrying `units`. The other two wrap this one with `None`.
///
/// `model` is `None` for a repo that declares no `.csproj`, and a `None`
/// model must leave the resolve BYTE-IDENTICAL to what it was: `units` is
/// omitted when empty, so the whole artifact is unchanged for such a tree.
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "one ordered resolve pipeline whose stages share the same def index and file contexts; the None-model byte-identical guarantee only holds if every stage stays in this one place"
)]
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
                //       exact-qualified ladder step, or by step 1.5's
                //       segment-by-segment walk through nested types.
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
                //
                // A CHAIN TAIL carrying the bit (`base.Make().Validate()`,
                // which sets both `receiverBase` and
                // `receiverCallOwner`/`receiverCallMember`) is not this
                // shape at all: its `name` is the inner invocation's own
                // source text, which resolves to nothing, so this rule
                // would only silence it. It belongs to the method-return
                // hop below, which reads the same bit and starts the
                // lookup at the bases for exactly the same reason.
                if r.receiver_base && r.receiver_call_owner.is_none() {
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
                            provenance::note(&edges, Step::BaseMember);
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
                        provenance::note(&edges, Step::QualifierMember);
                        edges_by_kind.uses_member += 1;
                        emitted = true;
                    } else if !extractor_vouches_instance(r)
                        && r.member.as_deref().is_some_and(|m| {
                            type_candidate(&index, &format!("{}+{}", index.defs[idx].id, m), None)
                                .is_some()
                        })
                    {
                        // C# forbids a member and a nested type sharing one
                        // name on the same type, so a member name that
                        // matches a nested type id under `idx` names a chain
                        // SEGMENT, not a member -- the deeper window of the
                        // same chain (step 1.5 above) carries the real edge.
                        // Marking this window emitted keeps tiers (e)/(f)/
                        // scored from guessing at it as a member access. A
                        // qualifier the extractor typed as an instance is
                        // exempt: its name merely coincides with a type's,
                        // and the receiver tiers below own it.
                        emitted = true;
                    } else {
                        // generic counts only for BARE qualifiers: a
                        // flattened chain inherits the flag from its inner
                        // segment while ladder steps 2-4 resolve by the
                        // chain's TAIL name, which can name-match an
                        // unrelated type. Dotted
                        // chains earn their edge via the member lists, the
                        // exact-qualified step, or step 1.5's nested walk
                        // instead.
                        //
                        // `this_shaped` is precision rule (a)'s guard: this
                        // resolution arm is where a `this.M` ref lands (its
                        // `name` IS the enclosing type, resolved through the
                        // ordinary type ladder like any other bare type
                        // name), so a member declared non-publicly on the
                        // enclosing type itself, or on one of its bases,
                        // must still bind precisely. Every other
                        // typed-qualified access reaching this arm
                        // (`SomeType.Member`, an inherited STATIC member
                        // named through a derived type) keeps the
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
                        // A qualifier that resolved as a TYPE binds the def
                        // that DECLARES the member, in this order: the named
                        // type itself; else the first in-graph base in its
                        // closure -- the widening `base_member_declared`
                        // already does for `base.`, applied to a receiver
                        // whose OWN type resolved directly rather than
                        // through a `base.` qualifier; else, on type
                        // certainty alone, the named type. A
                        // type-argument list (`Cache<T>.x`) or an exact
                        // qualified name (`Ns.Utils.Helper()`) is syntax
                        // only a type can carry, so when nothing in the graph
                        // declares the member it is still that type's as far
                        // as this graph can see -- an extension, an external
                        // base, an extractor gap. The certainty hatches come
                        // LAST so that an inherited static member named
                        // through a derived type (`Ns.Derived.Create()`,
                        // `Derived<int>.Create()`) binds the base that
                        // declares it, exactly as the same member named
                        // through the bare derived name already does.
                        let target = if declares_here {
                            Some(idx)
                        } else if let Some(target) = typed_receiver_base_member(
                            &index,
                            &file_contexts,
                            idx,
                            r.member.as_deref(),
                            r.arg_count,
                            this_shaped,
                        ) {
                            Some(target)
                        } else if (r.generic && r.qualified.is_none())
                            || (r.qualified.is_some() && via == Via::Qualified)
                        {
                            Some(idx)
                        } else {
                            None
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
                            provenance::note(&edges, Step::QualifierType);
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
                // in-graph base closure, mirroring the widening
                // `base_member_declared` already does for `base.`
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
                            // `base.Make().Validate()`: the owner the
                            // extractor could name is the ENCLOSING type
                            // (that is what a `base.` qualifier types as),
                            // but the method being called is the first
                            // in-graph base's, so its return type is the
                            // one the hop must read. An enclosing type that
                            // hides `Make` with an override or a `new`
                            // declaration of its own returns something
                            // else, and reading THAT would send the tail
                            // to the wrong type. No in-graph base declares
                            // the member -> no fact, exactly as an owner
                            // with no recorded return already gives.
                            // `this.` and every ordinary chain tail keep
                            // hopping through the owner itself.
                            //
                            // Arity is deliberately not asked here: the
                            // ref's own `arg_count` belongs to the OUTER
                            // call (`Validate`), never to the inner one.
                            let hop_owner = if r.receiver_base {
                                base_member_declared(
                                    &index,
                                    &file_contexts,
                                    oidx,
                                    Some(member.as_str()),
                                    None,
                                )
                            } else {
                                Some(oidx)
                            };
                            let returns = hop_owner.and_then(|idx| {
                                index.member_lists[idx].method_returns.get(member).cloned()
                            });
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
                                    match hop_owner.and_then(|idx| {
                                        index.member_lists[idx].method_return_args.get(member)
                                    }) {
                                        Some(args) if args.len() == 1 && args[0] != "*" => {
                                            Some(args[0].clone())
                                        }
                                        _ => returns,
                                    }
                                }
                                _ => returns,
                            };
                        }
                    } else if let Some(slot) = &r.receiver_lambda {
                        // The qualifier is an untyped lambda parameter: no
                        // fact in THIS file can type it, because the type is
                        // written on the callee's own delegate parameter, in
                        // whatever file declares it. Reading it back yields
                        // an ordinary bare-identifier receiver fact, so every
                        // tier below -- precision, admission, narrowing --
                        // treats the site exactly like any other typed
                        // receiver.
                        receiver_field = lambda_slot_receiver_type(
                            &index,
                            &file_contexts,
                            ns,
                            usings,
                            aliases,
                            r,
                            slot,
                        );
                        receiver_type_name = receiver_field.as_ref().map(|f| f.type_name.clone());
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
                        // A type the extractor read off a declaration carries
                        // its argument list (`receiver_args`, absent for a
                        // non-generic type); one the resolver derived (a call
                        // hop's return type, a field typed on a base) carries
                        // a bare name and stays arity-blind.
                        let receiver_arity = r
                            .receiver_type
                            .as_ref()
                            .map(|_| r.receiver_args.as_ref().map_or(0, Vec::len));
                        let Narrowed {
                            res: rr,
                            narrowed_away,
                        } = narrow_tracked(
                            resolve_ref_by_arity(
                                probe,
                                receiver_arity,
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
                            // The receiver's OWN def may not declare the
                            // member while an in-graph base of it does --
                            // `IS_THIS_SHAPED` decides only
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
                                provenance::note(&edges, Step::TypedReceiver);
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
                // A chain-tail ref (one carrying
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
                                        provenance::note(&edges, Step::PropertyHop);
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
                        // The exact key misses for an extension whose
                        // `this` parameter is a BASE of the receiver
                        // rather than the receiver's own exact type --
                        // widen to the receiver's nominal closure only
                        // once the exact key itself names no bucket, and
                        // only when the receiver resolved in-graph
                        // (`receiver_def`, the same resolution tier (e)
                        // already computed). Applies to every typed
                        // receiver, `this.` included -- `receiver_def` is
                        // set identically for both.
                        //
                        // The widened key names a DIFFERENT type than the
                        // receiver (a base or an ancestor), so
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
                        // Arity-gated exactly like the precise tier's own
                        // `declares_here` check -- a
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
                            provenance::note(&edges, Step::Extension);
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
                                            r.arg_count,
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
                            provenance::note(&edges, Step::Scored);
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
                let (resolution, to, candidates) = classification.edge_parts(&index);
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

    append_dispatch_edges(
        fragments_by_file,
        &index,
        &file_contexts,
        &mut edges,
        &mut edges_by_kind,
    );

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

    provenance::flush(&edges);
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
        let alias = crate::tsgraph::read_ts_alias_scopes(
            root,
            ts_fragments_by_file.iter().map(|(f, _)| f.as_str()),
        );
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
