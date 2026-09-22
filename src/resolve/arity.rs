use super::index::DefIndex;
use super::ladder::{resolve_ref, Resolution};
use super::scope::FileContext;
use crate::graph::FragRef;
use std::collections::{HashMap, HashSet};

// Resolve a probe built from a type NAME the resolver derived itself (a
// receiver's declared type, a base-list entry) when the number of type
// ARGUMENTS that name was written with is known. `Foo` and `Foo<T>` share one
// id and one `qualified_name_to_def` slot (first-indexed wins), so the
// arity-blind ladder answers for whichever sibling the index met first; the
// arity-keyed ladder answers for the sibling the language names. `Resolved`
// and `Ambiguous` from the exact-arity pass stand; `External` re-runs the
// blind ladder, so a name whose exact arity has no in-graph def anywhere
// keeps the arity-blind answer (an arity of 0 is inferred from an ABSENT
// argument list, and the extractor records no descriptors for an argument
// shape it cannot read). When the exact arity exists only outside the site's
// imports, the ladder's global-uniqueness step answers exactly as it does for
// the declaration's own type reference.
//
// The exact pass runs only when at least two defs share the name's simple
// form (the alias target's, for an aliased name): with one def or none, the
// two ladders provably agree -- a lone def of the right arity is found at the
// same step by both, and a lone def of another arity leaves the exact pass
// external at every step, including the global pool it filters by arity --
// so an external receiver or base, the common case, costs one ladder as
// before.
pub(super) fn resolve_ref_by_arity(
    mut probe: FragRef,
    arity: Option<usize>,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
) -> Resolution {
    if let Some(n) = arity {
        let full = aliases.get(&probe.name).unwrap_or(&probe.name);
        let simple = full.rsplit('.').next().unwrap_or(full);
        let shared = index.simple_name_to_defs.get(simple).map_or(0, Vec::len) >= 2;
        if shared {
            probe.type_arg_count = Some(n);
            let exact = resolve_ref(&probe, usings, ns, index, aliases, file_contexts);
            if !matches!(exact, Resolution::External) {
                return exact;
            }
            probe.type_arg_count = None;
        }
    }
    resolve_ref(&probe, usings, ns, index, aliases, file_contexts)
}

// The receiver-type counterpart of `resolve_ref_by_arity`, for a probe built
// from an arity the extractor RECORDED off the receiver's own written type
// (`receiver_args`), never one this resolver merely inferred. `Foo` and
// `Foo<T>` sharing one `qualified_name_to_def` slot is exactly why the blind
// fallback exists at all -- to answer when the extractor could not READ an
// argument shape, an arity of 0 inferred from an ABSENT list. It was never
// meant to let a written, multi-argument receiver settle for whichever
// same-named sibling the index happened to index first: once a SECOND def of
// the same simple name proves the graph actually has an opinion on this
// name's arity, a recorded arity of one or more gets the exact-arity pass
// and nothing else, so `External` from it is final rather than a cue to try
// again arity-blind. A lone def carries no such opinion -- the graph simply
// never recorded this name at the written arity, and refusing the reference
// over an arity fact the graph cannot confirm OR refute would punish every
// ordinary reference to a type the extractor only ever saw written bare
// elsewhere, so a lone def keeps `resolve_ref_by_arity`'s arity-blind answer,
// exactly as `Some(0)` and `None` -- the absent-argument-list case the blind
// fallback is FOR -- already do.
pub(super) fn resolve_receiver_type(
    mut probe: FragRef,
    arity: Option<usize>,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
) -> Resolution {
    if let Some(n) = arity {
        if n >= 1 {
            let full = aliases.get(&probe.name).unwrap_or(&probe.name);
            let simple = full.rsplit('.').next().unwrap_or(full);
            let shared = index.simple_name_to_defs.get(simple).map_or(0, Vec::len) >= 2;
            if shared {
                probe.type_arg_count = Some(n);
                return resolve_ref(&probe, usings, ns, index, aliases, file_contexts);
            }
        }
    }
    resolve_ref_by_arity(probe, arity, usings, ns, index, aliases, file_contexts)
}

// The type-argument count a `bases` entry of def `idx` was written with:
// `base_generic_args` keeps the descriptors of a generic base and no entry
// for a base written bare.
pub(super) fn base_arity(index: &DefIndex, idx: usize, base: &str) -> usize {
    index.member_lists[idx]
        .base_generic_args
        .iter()
        .find(|(k, _)| k == base)
        .map_or(0, |(_, v)| v.len())
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
pub(super) fn method_arity_admits(
    index: &DefIndex,
    idx: usize,
    member: &str,
    arg_count: usize,
) -> bool {
    match index.member_lists[idx].method_arities.get(member) {
        Some(ranges) => ranges
            .iter()
            .any(|&(min, max)| min <= arg_count && (max == -1 || (arg_count as i64) <= max)),
        None => true,
    }
}

// The call's argument count against the entry's declared RANGE. `arity_max ==
// -1` is the `params` sentinel: unbounded above.
pub(super) fn arity_accepts(entry: &crate::graph::FragExtensionMethod, arg_count: usize) -> bool {
    entry.arity_min <= arg_count && (entry.arity_max == -1 || (arg_count as i64) <= entry.arity_max)
}

// The this-parameter's top-level type arguments against the receiver's. Both
// sides absent (neither type is generic) is the base-name match. Exactly one
// side absent is a genuine generic/non-generic mismatch and never binds.
// Otherwise the lists unify position by position, where "*" -- a type parameter
// neither side can resolve to a concrete type -- matches anything.
pub(super) fn generic_args_unify(
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
