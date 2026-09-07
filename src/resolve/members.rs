use super::arity::{base_arity, method_arity_admits, resolve_ref_by_arity};
use super::index::{name_probe, DefIndex};
use super::ladder::Resolution;
use super::scope::FileContext;
use crate::graph::FragRef;
use std::collections::{HashMap, HashSet};

// The def's member lists, unioned: methods ∪ properties ∪ fields for a READ
// (`arg_count == None`), which is what lets a static PROPERTY access
// (MessageUrn.Prefix) and a const/static FIELD access earn an edge on the
// same evidence a static method call already did.
//
// A CALL (`arg_count == Some(n)`) is narrower on both axes: properties and
// fields never satisfy a call (the rule `member_vouched`'s own
// Call/Read split already enforces for the scored tier; this is where the
// PRECISE tier gains it too), and `methods` alone is not enough either -- the
// name must ALSO have an overload whose own arity range admits `n`
// (`method_arity_admits`), or this answers `false` exactly as it would for a
// name this def never declares at all. That is what lets tier (f)/the scored
// tier run when a same-named instance member exists but at the WRONG
// signature: the precise tier's own callers read `false` here as "nothing
// declared", never mark the ref `emitted`, and every later tier proceeds
// undisturbed.
pub(super) fn declares_member(
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
// the SAME arity gate applies to a non-public method: a
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
pub(super) fn declares_member_any_visibility(
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
pub(super) enum MemberShape {
    Call,
    Read,
}

pub(super) fn member_shape(r: &FragRef) -> MemberShape {
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
pub(super) fn member_vouched(
    index: &DefIndex,
    idx: usize,
    member: Option<&str>,
    shape: MemberShape,
) -> bool {
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
pub(super) fn inheritance_walk_find(
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
            if let Resolution::Resolved(bidx, _) = resolve_ref_by_arity(
                probe,
                Some(base_arity(index, cur, base)),
                &ctx.usings,
                &ns,
                index,
                &ctx.aliases,
                file_contexts,
            ) {
                if seen.insert(bidx) {
                    stack.push(bidx);
                }
            }
        }
    }
    None
}

pub(super) fn inheritance_walk_matches(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    matches: impl FnMut(usize) -> bool,
) -> bool {
    inheritance_walk_find(index, file_contexts, start, matches).is_some()
}

pub(super) fn inherited_member_declared(
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

// The recursive worker `first_base_declaring` drives: `cur`
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
        let Resolution::Resolved(bidx, _) = resolve_ref_by_arity(
            probe,
            Some(base_arity(index, cur, base)),
            &ctx.usings,
            &ns,
            index,
            &ctx.aliases,
            file_contexts,
        ) else {
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
// over a later one when both would otherwise answer (the walk this
// replaces, `inheritance_walk_find`'s LIFO stack, visited bases in
// REVERSE declaration order). Returns the first in-graph def,
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
// skipping the whole base is equivalent to skipping its closure);
// `typed_receiver_base_member` passes `false` only for a receiver that is
// itself an interface.
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
        let Resolution::Resolved(bidx, _) = resolve_ref_by_arity(
            probe,
            Some(base_arity(index, start, base)),
            &ctx.usings,
            &ns,
            index,
            &ctx.aliases,
            file_contexts,
        ) else {
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
// by the ref's own `arg_count` exactly like the
// typed-receiver walk below. `None` is the ordinary external-receiver
// answer to the caller, never a candidate for a scored guess.
pub(super) fn base_member_declared(
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

// The typed-receiver precise tier's own base walk: when a
// resolved receiver def does not itself declare the member, the first def
// in its in-graph base closure that does is the precise target -- exactly
// the widening `base_member_declared` already does for `base.`, applied to
// an ORDINARY typed receiver. Interfaces in the closure are skipped, at
// every depth, for a CLASS or struct receiver, for the same
// reason `base_member_declared` skips them: a class must supply a body for
// every interface member it is called through, so the compiler binds that
// body's declaring class, never the interface (a C# 8+ default interface
// implementation is reachable only through the interface type, so it is not
// a bind target for a class-typed receiver either) -- see
// `stage3_veto_a_member_declared_by_the_receivers_interface_beats_a_matching_visible_extension`,
// which pins exactly this: an interface-only ancestor must NOT earn a
// precise edge from a class receiver, only veto the extension tier (which
// reads the closure itself, not this function). An INTERFACE receiver is the
// other half of the same rule: its closure holds nothing but interfaces, and
// the compiler binds the base interface that declares the member
// (`IExtended : IContract`, `ext.Fulfil()` is `IContract.Fulfil`), so the
// walk keeps them for exactly that receiver kind. `any_visibility` is the caller's own answer
// to "is this receiver the enclosing type itself" (the `this.` shape,
// `receiver_type == outer_types.last()`): `true` walks
// `declares_member_any_visibility`, `false` keeps the public-only
// `declares_member`, so a receiver typed by anything OTHER than the
// enclosing type can only ever bind to a member C# would let it see from
// outside. `arg_count` is the ref's own call-shape fact: a
// base that declares the name at the WRONG arity is skipped exactly like
// one that does not declare it at all.
pub(super) fn typed_receiver_base_member(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    start: usize,
    member: Option<&str>,
    arg_count: Option<usize>,
    any_visibility: bool,
) -> Option<usize> {
    let skip_interfaces = index.defs[start].kind != "interface";
    first_base_declaring(
        index,
        file_contexts,
        start,
        skip_interfaces,
        |index, idx| {
            if any_visibility {
                declares_member_any_visibility(index, idx, member, arg_count)
            } else {
                declares_member(index, idx, member, arg_count)
            }
        },
    )
}

// The extension bucket key tier (f) tries when the exact
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
// Also returns the MATCHED node's own generic-argument
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
pub(super) fn extension_closure_key(
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
