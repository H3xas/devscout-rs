use super::arity::{arity_accepts, base_arity, generic_args_unify, resolve_ref_by_arity};
use super::index::{name_probe, DefIndex, MethodOverloadParams};
use super::ladder::{resolve_ref, Resolution};
use super::members::{
    base_member_declared, declares_member, extension_closure_key, inheritance_walk_find,
    inheritance_walk_matches, MemberShape,
};
use super::scope::FileContext;
use crate::graph::{Def, FragFact, FragLambdaSlot, FragRef};
use std::collections::{HashMap, HashSet};

// `true` exactly for the `this.` shape (precision rule (a)): a `uses-member`
// ref whose recorded receiver type IS the innermost `outer_types` entry --
// the enclosing type itself, whether the qualifier was literally `this.` or
// an ordinary same-typed local/parameter/field (C#'s own private/protected
// access rule reaches every expression of the declaring type from within
// its own members, not only `this`). `false` whenever `receiver_type` is
// unset (nothing to compare) or names a different type.
// True when the extractor recorded any fact that types the qualifier as an
// INSTANCE -- a scope-typed receiver, a member-scoped local (typed or not),
// a property-owner hop, or a call receiver. Such a qualifier is never a type
// path, so the nested-type walk and the nested-segment suppression both
// stand aside and leave it to the receiver tiers.
pub(super) fn extractor_vouches_instance(r: &FragRef) -> bool {
    r.receiver_type.is_some()
        || r.receiver_local
        || r.receiver_property_owner.is_some()
        || r.receiver_call_owner.is_some()
}

pub(super) fn is_this_shaped_receiver(r: &FragRef) -> bool {
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
pub(super) struct ReceiverFieldType {
    pub(super) type_name: String,
    pub(super) declaring_def: usize,
    pub(super) declaring_file: String,
}

// The enclosing-type chain a ref written INSIDE `def`'s own body carries
// (`FragRef::outer_types`): every nesting level from the outermost in,
// ending with the def itself. A def id spells nesting exactly that way --
// the namespace, a dot, then the chain joined with "+", which is how
// `resolve_ref`'s step 0b rebuilds an id from a ref's chain -- so the chain
// is the id with its namespace prefix taken off. A namespace-level def
// yields a one-entry chain holding its own name.
pub(super) fn def_outer_types(def: &Def) -> Vec<String> {
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
pub(super) fn bare_receiver_field_or_property_type(
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
        if let Resolution::Resolved(bidx, _) = resolve_ref_by_arity(
            probe,
            Some(base_arity(index, start, base)),
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

// A parameter descriptor's own bare NAME: the head identifier with its
// type-argument list and array brackets taken off, which is the only half of
// a descriptor that names something this resolver can look up.
// `Func<Options,bool>` is `Func`, `Options[]` is `Options`; the descriptor
// for an unknown shape is `?`, whose head is empty.
fn descriptor_head(text: &str) -> &str {
    let end = text.find(['<', '[', '?']).unwrap_or(text.len());
    &text[..end]
}

// A descriptor's TOP-LEVEL type arguments, split on the commas that sit at
// nesting depth zero so `Func<Options,Func<int,bool>>` yields two arguments
// rather than three. Empty when the descriptor carries no argument list at
// all; the descriptors this reads are written without spaces (see
// `FragDef::method_params`), so no trimming is needed.
fn descriptor_args(text: &str) -> Vec<String> {
    let Some(open) = text.find('<') else {
        return Vec::new();
    };
    if !text.ends_with('>') {
        return Vec::new();
    }
    let inner = &text[open + 1..text.len() - 1];
    let mut args: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(inner[start..i].to_string());
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    args.push(inner[start..].to_string());
    args
}

// The PARAMETER LIST of the delegate a parameter descriptor names, which is
// where an untyped lambda's own parameter types are written down. The three
// BCL delegate shapes are read structurally -- `Action<A,B>` takes its
// arguments as they stand, `Func<A,B,R>` drops the return type, `Predicate<A>`
// takes its single argument -- because no in-graph def declares them. Any
// other head is a name: resolved under the DECLARING file's context (the
// descriptor is a bare identifier, and only that file's usings, aliases and
// nesting say what it meant) and answered only when it lands on a `delegate`
// def, whose own list this reader keeps under `Invoke`.
//
// An ARRAY of delegates is not a delegate (`Action<Options>[]` takes a
// collection, never a lambda), so a descriptor that ends in brackets answers
// nothing at all.
fn delegate_parameters(
    text: &str,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
) -> Option<Vec<String>> {
    if text.ends_with(']') {
        return None;
    }
    // `Expression<Func<Options,bool>>` is the expression-tree wrapper the LINQ
    // shapes are written with; the delegate inside it is what the lambda binds
    // to. Unwrapped exactly ONCE -- a doubly-wrapped expression is not a shape
    // C# accepts a lambda for, and unwrapping again would invent a binding.
    let args = descriptor_args(text);
    if descriptor_head(text) == "Expression" && args.len() == 1 {
        return delegate_invoke_parameters(
            &args[0],
            index,
            file_contexts,
            declaring_def,
            declaring_file,
        );
    }
    delegate_invoke_parameters(text, index, file_contexts, declaring_def, declaring_file)
}

// `delegate_parameters` minus the expression-tree unwrap -- split out so the
// unwrap can never run twice.
fn delegate_invoke_parameters(
    text: &str,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
) -> Option<Vec<String>> {
    if text.ends_with(']') {
        return None;
    }
    let head = descriptor_head(text);
    let args = descriptor_args(text);
    match head {
        "Action" if !args.is_empty() => Some(args),
        "Func" if args.len() >= 2 => {
            let mut params = args;
            params.pop();
            Some(params)
        }
        "Predicate" if args.len() == 1 => Some(args),
        "Action" | "Func" | "Predicate" | "" => None,
        _ => {
            let ctx = file_contexts.get(declaring_file)?;
            let def = &index.defs[declaring_def];
            let probe = name_probe(
                head.to_string(),
                def.namespace.as_str(),
                def_outer_types(def),
            );
            let Resolution::Resolved(didx, _) = resolve_ref(
                &probe,
                &ctx.usings,
                def.namespace.as_str(),
                index,
                &ctx.aliases,
                file_contexts,
            ) else {
                return None;
            };
            if index.defs[didx].kind != "delegate" {
                return None;
            }
            index.member_lists[didx]
                .method_params
                .get("Invoke")
                .and_then(|overloads| overloads.first())
                .map(|o| o.params.clone())
        }
    }
}

// One untyped lambda parameter's type, read off the CALLEE's own declared
// parameter list. `_registrar.Register(x => x.Configure())` records nothing
// about `x` at the site -- the extractor cannot see across files -- but the
// overload the call lands on declares `Action<Options> configure`, and that
// delegate's parameter list is where `x`'s type is written down. The answer
// is shaped like any other bare-identifier receiver fact so the tier below
// resolves it under the DECLARING file's usings, aliases, namespace and
// nesting: the descriptor is a bare identifier that only means what the file
// that wrote it meant, exactly as a field's declared type is.
//
// Nothing binds unless every overload that could take the lambda AGREES on
// the parameter type. `Attach(Action<Options>)` beside `Attach(Action<Endpoint>)`
// leaves the site as untyped as the extractor found it, because choosing
// either would be a guess. An overload whose delegate takes a different
// number of parameters than the lambda declares is not a binding candidate at
// all and is dropped rather than counted as disagreement, which is what lets
// `Same(string tag)` sit beside `Same(Action<Options>)` without silencing it.
// An overload that DOES take the lambda but types the parameter with a type
// parameter (`Action<T>`) still counts, and blocks: the site knows nothing
// about what `T` is bound to, the same refusal every other wildcard
// generic-arg fact in this file makes.
pub(super) fn lambda_slot_receiver_type(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    ns: &str,
    usings: &HashSet<String>,
    aliases: &HashMap<String, String>,
    r: &FragRef,
    slot: &FragLambdaSlot,
) -> Option<ReceiverFieldType> {
    let probe = name_probe(slot.owner.clone(), ns, r.outer_types.clone());
    let owner = match resolve_ref(&probe, usings, ns, index, aliases, file_contexts) {
        Resolution::Resolved(oidx, _) => Some(oidx),
        _ => None,
    };
    // (overload, the def that declares it, the position the lambda fills in
    // its parameter list). An extension method invoked through its receiver
    // carries the receiver in position 0, so every argument the site wrote is
    // one place further right.
    let mut candidates: Vec<(&MethodOverloadParams, usize, usize)> = Vec::new();
    if let Some(oidx) = owner {
        let declaring = if index.member_lists[oidx]
            .method_params
            .contains_key(&slot.member)
        {
            Some(oidx)
        } else {
            base_member_declared(index, file_contexts, oidx, Some(&slot.member), None)
        };
        if let Some(didx) = declaring {
            if let Some(overloads) = index.member_lists[didx].method_params.get(&slot.member) {
                for overload in overloads {
                    candidates.push((overload, didx, slot.arg_index));
                }
            }
        }
    }
    if candidates.is_empty() {
        // The owner is external, unresolved, or declares no such member --
        // the shapes an extension method answers. The bucket key is the
        // receiver's own closure key when the owner resolved (an extension
        // written against a base or interface is reached the same way tier
        // (f) reaches it) and the raw receiver-type text otherwise, which is
        // all an external receiver ever offers.
        let key = match owner {
            Some(oidx) => {
                extension_closure_key(index, file_contexts, oidx, &slot.member).map(|(k, _)| k)
            }
            None => Some(format!("{} {}", slot.member, slot.owner)),
        };
        if let Some(candidate_list) = key.and_then(|k| index.extension_index.get(&k)) {
            for cand in candidate_list {
                let Some(overloads) = index.member_lists[cand.def_idx]
                    .method_params
                    .get(&slot.member)
                else {
                    continue;
                };
                for overload in overloads {
                    let matches_this = overload
                        .params
                        .first()
                        .and_then(|p| p.strip_prefix("this "))
                        .is_some_and(|t| descriptor_head(t) == cand.entry.this_type);
                    if matches_this {
                        candidates.push((overload, cand.def_idx, slot.arg_index + 1));
                    }
                }
            }
        }
    }

    let mut agreed: Option<(String, String, usize, String)> = None;
    for (overload, declaring_def, position) in candidates {
        // An extension overload's list starts with its `this` parameter, so
        // the call's argument count is compared one slot further along.
        let shift = position - slot.arg_index;
        if slot.arg_count + shift > overload.params.len() || position >= overload.params.len() {
            continue;
        }
        // A static class's extension method may also be invoked statically,
        // in which case the `this` marker rides along on a descriptor read at
        // its ordinary position.
        let text = overload.params[position]
            .strip_prefix("this ")
            .unwrap_or(&overload.params[position]);
        let Some(params) =
            delegate_parameters(text, index, file_contexts, declaring_def, &overload.file)
        else {
            continue;
        };
        // The delegate must take exactly as many parameters as the lambda
        // declares, or it is not the overload the lambda binds to at all --
        // dropped rather than counted as disagreement.
        if params.len() != slot.arity || slot.index >= params.len() {
            continue;
        }
        let head = descriptor_head(&params[slot.index]);
        if head.is_empty() || head == "*" {
            return None;
        }
        // Two overloads that both write `Action<Options>` agree only when the
        // name means the same type from where each was written: a partial
        // class or an extension bucket may span files with different usings,
        // and the site binds to what the FIRST taker's file meant, so every
        // other taker must resolve to that same def (or to the same name, when
        // none resolves in-graph) before it counts as agreement.
        let meaning = descriptor_meaning(index, file_contexts, declaring_def, &overload.file, head);
        match &agreed {
            Some((known, _, _, _)) if *known != meaning => return None,
            Some(_) => {}
            None => {
                agreed = Some((
                    meaning,
                    head.to_string(),
                    declaring_def,
                    overload.file.clone(),
                ))
            }
        }
    }
    agreed.map(
        |(_, type_name, declaring_def, declaring_file)| ReceiverFieldType {
            type_name,
            declaring_def,
            declaring_file,
        },
    )
}

// What a descriptor head names from the file that wrote it: the def it
// resolves to under that file's usings, aliases, namespace and nesting, or
// the bare name itself when nothing in the graph answers. Only used to
// compare takers with each other; the tier below re-resolves the winner
// under the same context.
fn descriptor_meaning(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
    head: &str,
) -> String {
    let def = &index.defs[declaring_def];
    let resolved = file_contexts.get(declaring_file).and_then(|ctx| {
        let probe = name_probe(
            head.to_string(),
            def.namespace.as_str(),
            def_outer_types(def),
        );
        match resolve_ref(
            &probe,
            &ctx.usings,
            def.namespace.as_str(),
            index,
            &ctx.aliases,
            file_contexts,
        ) {
            Resolution::Resolved(didx, _) => Some(index.defs[didx].id.clone()),
            _ => None,
        }
    });
    resolved.unwrap_or_else(|| format!("?{head}"))
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
pub(super) type AssignabilityCache = HashMap<(usize, String, Option<Vec<String>>, bool), bool>;

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
//     arguments unified and the call's argument count inside the declared
//     arity -- the same (member, thisType) key and the same unification and
//     arity filters tier (f) uses. Tier (f) declined this ref for one of its
//     own reasons (most often the namespace test, which is narrower than the
//     language), and re-admitting the candidate HERE, as a guess, is the
//     honest answer: the this-parameter is direct evidence about this exact
//     receiver type, which is more than the uniqueness pool alone ever had.
//     Arity is NOT one of those reasons: a call the extension cannot accept
//     has no binding under any import, so it stays refused.
//
// The two are OR-ed rather than tried in order because an extension method is
// also an ordinary public static method, so the static class holding it
// vouches through `methods` too -- requiring assignability of a candidate that
// merely LOOKS instance-vouched would refuse every extension there is.
//
// Ten parameters, deliberately: every one is a distinct fact about the ONE
// question asked here, and bundling them into a struct built per candidate
// would add an allocation and a second name for each field without making any
// caller shorter -- there is exactly one caller.
#[allow(clippy::too_many_arguments)]
pub(super) fn receiver_admits_candidate(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    cache: &mut AssignabilityCache,
    candidate: usize,
    member: &str,
    shape: MemberShape,
    arg_count: Option<usize>,
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
            c.def_idx == candidate
                && arg_count.is_none_or(|n| arity_accepts(&c.entry, n))
                && generic_args_unify(c.entry.this_args.as_ref(), receiver_args)
        })
}

pub(super) fn nested_candidate_visible_from_site(
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
