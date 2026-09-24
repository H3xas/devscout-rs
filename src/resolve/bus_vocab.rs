// The message-handling vocabulary a repository declares about ITSELF, and
// the closure that spreads it to the declarations it implies.
//
// A bus is a library, and the useful ones are not all public. Matching a
// fixed list of framework type names finds the buses this engine happens to
// have heard of and silently finds nothing in a house that wrote its own, so
// the list below is a seed rather than the answer: a repository registering
// its handlers names them, and a named handler's own base list says which
// generic base that repository treats as a handler shape and at which
// position it carries its message.
//
// A registered type's OWN generic base is not proof by itself: a base that
// merely carries a type argument says nothing about whether that argument is
// ever a message the corpus actually sends, or whether the registered type
// ever actually receives it. `IEquatable<T>` carries a concrete argument on
// every type that implements it and is never a handler base -- admitting it
// on argument shape alone routes a message to itself. The admission rule
// below (`harvest_registrations`) requires the registered type to both NOT
// be a message the corpus sends, and to actually receive -- through a
// declared parameter or a bound property -- a message the corpus DOES send,
// before its base earns a place in the vocabulary. Requiring both is what
// rules a self-route out structurally: when the base's own argument names
// the registered type itself, either the corpus never sends that type (the
// receiving half has nothing to point at) or it does (the type fails the
// first half outright). A base admitted only through a bound property,
// never through its own base-list argument, is kept in a SEPARATE set
// (`binding_only_bases`): its carriers are handlers of what they bind on
// their properties, never of their own base argument.
//
// Two closures then spread the admitted vocabulary. A base reached through a
// local abstract intermediate is still a handler base, of the SAME role the
// base it passes through to holds, and a method that hands its caller's
// message to a publish is itself a publish under whatever name it was given.

use std::collections::{HashMap, HashSet};

use super::edges::type_probe;
use super::index::DefIndex;
use super::ladder::{resolve_ref, Resolution};
use super::receiver::def_outer_types;
use super::scope::FileContext;
use crate::graph::{FragDef, Fragment};

// Handler bases every repository gets for free, before it has said anything
// about itself. These are the seed a repository with no registration call at
// all falls back to, never the limit of what can be recognized.
const ROOT_CONSUMER_BASES: &[&str] = &[
    "IConsumer",
    "BaseConsumer",
    "IAmInitiatedBy",
    "IWorkHandler",
    "IRequestHandler",
];

// Publish verbs on the same footing. Extraction keeps its own copy of these
// names for a different job -- deciding whether a plain identifier argument
// is worth recording at all -- so the two lists answer different questions
// and neither can be derived from the other.
const ROOT_PUBLISH_VERBS: &[&str] = &["Publish", "PublishAsync", "SubmitJob", "Reply", "Send"];

/// A message's own identity: the element definition it resolves to, plus
/// how many array layers wrap it (0 for a plain message, 1 for `M[]`, 2 for
/// `M[][]`, ...). A single message and an array of that SAME message are
/// different identities on both sides of a hop -- a base's own argument, a
/// property binding, a publish site's generic argument, an array creation
/// or a declared array-typed variable all resolve through this pair rather
/// than through the element alone, so `IConsumer<M>` and `IConsumer<M[]>`
/// can never satisfy the same publish site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct MessageIdentity {
    pub(super) def_idx: usize,
    pub(super) rank: u8,
}

impl MessageIdentity {
    /// A plain (non-array) identity for `def_idx` -- what a registered
    /// handler TYPE resolves to, and what a base's own argument resolves
    /// to when it carries no array suffix at all.
    pub(super) fn plain(def_idx: usize) -> Self {
        MessageIdentity { def_idx, rank: 0 }
    }

    /// The edge-facing rendering: the element def's own id, followed by
    /// `rank` array-suffix pairs. Handler counts and route-key comparisons
    /// treat this rendered id as its own message, exactly as the element's
    /// bare id is treated today.
    pub(super) fn rendered_id(&self, index: &DefIndex) -> String {
        let mut id = index.defs[self.def_idx].id.clone();
        for _ in 0..self.rank {
            id.push_str("[]");
        }
        id
    }
}

// The trailing array-suffix layers a descriptor text carries, and the bare
// element text underneath them -- `type_descriptor`'s own array rendering
// (`{element}[]`, applied once per `array_type` layer) is what every raw
// message text this module reads was built with, so splitting it back off
// here is the exact inverse of that rendering, never a re-parse of source.
fn split_array_suffix(text: &str) -> (&str, u8) {
    let mut rest = text;
    let mut rank: u8 = 0;
    while let Some(stripped) = rest.strip_suffix("[]") {
        rest = stripped;
        rank = rank.saturating_add(1);
    }
    (rest, rank)
}

/// Resolves a raw message text to its full identity -- the element def plus
/// its array rank -- through the ordinary ladder, in the given
/// namespace/file context. The one entry point every message-identity
/// comparison in this lane (a kept publish site, the consumer/handler
/// table, the registration-harvest clauses, the route key and the edge's
/// own `message`) reads a raw text through, so a single message and an
/// array of it can never be conflated by resolving through two different
/// paths.
pub(super) fn resolve_message_identity(
    raw: &str,
    ns: &str,
    outer_types: &[String],
    file_ctx: &FileContext,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> Option<MessageIdentity> {
    let (element, rank) = split_array_suffix(raw);
    let def_idx = resolve_type_text(element, ns, outer_types, file_ctx, index, file_contexts)?;
    Some(MessageIdentity { def_idx, rank })
}

/// The consumer-base vocabulary a repository turned out to call its own
/// message handling, split by the role each base earned.
pub(super) struct BusVocabulary {
    /// Base names admitted as full consumer bases: a def carrying one of
    /// these reads its own base-list argument as a message, exactly as a
    /// root base always has.
    pub(super) consumer_bases: HashSet<String>,
    /// Base names admitted ONLY through a registered carrier's bound
    /// property, never through the base's own argument. A def carrying one
    /// of these is a handler for the messages it binds on its own
    /// properties; its base-list argument earns no route.
    pub(super) binding_only_bases: HashSet<String>,
}

/// Resolves a bare-or-dotted type text through the ordinary ladder, in the
/// given namespace/file context -- never a bare-name or short-name
/// fallback: two or more candidates (`Resolution::Ambiguous`) and no
/// in-graph candidate at all (`Resolution::External`) both answer `None`,
/// exactly as unsupported as each other. `outer_types` is the reference
/// site's own enclosing-type chain (step 0b of the ladder): a bus-hop
/// message resolves through the SAME shadow step an ordinary `uses-type`
/// reference at that position would, so a message nested beside its
/// publisher or handler shadows a same-named namespace/using candidate.
pub(super) fn resolve_type_text(
    raw: &str,
    ns: &str,
    outer_types: &[String],
    file_ctx: &FileContext,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> Option<usize> {
    let probe = type_probe(raw, ns, outer_types);
    match resolve_ref(
        &probe,
        &file_ctx.usings,
        ns,
        index,
        &file_ctx.aliases,
        file_contexts,
    ) {
        Resolution::Resolved(idx, _) => Some(idx),
        Resolution::Ambiguous(..) | Resolution::External => None,
    }
}

// The simple name a base list would spell for this qualified def id: the
// tail after the last namespace dot, and after the last nested-type marker.
fn simple_name(id: &str) -> &str {
    let tail = id.rsplit_once('.').map_or(id, |(_, t)| t);
    tail.rsplit_once('+').map_or(tail, |(_, t)| t)
}

// Every top-level comma-separated slice of `inner`, splitting only at
// bracket depth 0 -- a nested generic argument's own commas never split its
// parent's argument list.
fn split_top_level_args(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(inner[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(inner[start..].to_string());
    parts
}

// The sole type argument a generic WRAPPER descriptor carries, read
// structurally off its rendered `Outer<Inner>` text -- never by checking the
// wrapper's own name, which is what keeps this able to unwrap any
// single-argument envelope rather than one hardcoded name. `None` when the
// descriptor carries no top-level argument list, or carries anything other
// than exactly one: a multi-argument or argument-less wrapper is not the
// single-message-envelope shape this unwraps, so it resolves nothing rather
// than guessing which slot is the message.
fn single_nested_argument(descriptor: &str) -> Option<String> {
    let open = descriptor.find('<')?;
    if !descriptor.ends_with('>') {
        return None;
    }
    let inner = &descriptor[open + 1..descriptor.len() - 1];
    match split_top_level_args(inner).as_slice() {
        [single] if !single.is_empty() => Some(single.clone()),
        _ => None,
    }
}

/// The concrete message-position argument a base carries: the first flat
/// generic argument, or -- when that same position is itself generic and
/// wraps exactly one further argument -- the wrapped argument, read
/// structurally off the rendered descriptor. `None` when the base's own
/// first argument is the unbound pass-through marker `"*"`: a wildcard
/// names no concrete message at this declaration, however many further
/// subclasses eventually close it over a real type. `first_arg_is_array` is
/// the array-marked-base fact (`array_message_bases`, extract/bus.rs) --
/// applied only to the FLAT reading: a nested wrapper's own argument
/// (`Batch<M[]>`) already carries any array suffix it has through its own
/// rendered text, so the flag never touches that branch. The `bool` answers
/// whether the nested-wrapper reading was used, which is all a caller needs
/// to pick its own evidence word; this function itself names none.
pub(super) fn concrete_base_argument(
    nested: Option<&[String]>,
    flat: &[String],
    first_arg_is_array: bool,
) -> Option<(String, bool)> {
    let first_flat = flat.first()?;
    if first_flat == "*" {
        return None;
    }
    if let Some(inner) = nested
        .and_then(<[String]>::first)
        .and_then(|d| single_nested_argument(d))
    {
        return Some((inner, true));
    }
    let message = if first_arg_is_array {
        format!("{first_flat}[]")
    } else {
        first_flat.clone()
    };
    Some((message, false))
}

/// Derives this corpus's own publish-verb vocabulary -- roots plus one hop
/// of wrapper closure. Independent of which messages are sent or which
/// bases are admitted, so the bus pass runs this FIRST, filters and resolves
/// every publish site through it, and only then has the "what does this
/// corpus send" fact the consumer-base vocabulary needs.
pub(super) fn derive_publish_verbs(fragments_by_file: &[(String, Fragment)]) -> HashSet<String> {
    let mut publish_verbs: HashSet<String> = ROOT_PUBLISH_VERBS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    harvest_wrappers(fragments_by_file, &mut publish_verbs);
    publish_verbs
}

/// Derives this corpus's own consumer-base vocabulary against `sent` -- the
/// resolved message-def indices of every KEPT publish site (a recognized or
/// wrapper-derived verb, a message that resolves, no structural refusal),
/// the corpus's own definition of "sends". Returns the full-role and
/// binding-only sets and whether any registration admitted a base under
/// either role.
pub(super) fn derive_consumer_bases(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    sent: &HashSet<MessageIdentity>,
) -> (HashSet<String>, HashSet<String>, bool) {
    let mut consumer_bases: HashSet<String> = ROOT_CONSUMER_BASES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut binding_only_bases: HashSet<String> = HashSet::new();
    let contributed = harvest_registrations(
        fragments_by_file,
        index,
        file_contexts,
        sent,
        &mut consumer_bases,
        &mut binding_only_bases,
    );
    close_over_intermediates(
        fragments_by_file,
        index,
        &mut consumer_bases,
        &mut binding_only_bases,
    );
    // A base promoted to full role by a later registration than the one
    // that first admitted it binding-only keeps only the fuller role.
    binding_only_bases.retain(|base| !consumer_bases.contains(base));
    (consumer_bases, binding_only_bases, contributed)
}

// Every def's own bound-property message facts, unioned across a partial
// class's declarations and keyed to the def index -- built once so the
// admission rule and the emission pass never have to walk the corpus twice
// for the same fact.
fn property_messages_by_def(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
) -> HashMap<usize, Vec<String>> {
    let mut out: HashMap<usize, Vec<String>> = HashMap::new();
    for (_, frag) in fragments_by_file {
        for d in &frag.defs {
            if d.property_message_args.is_empty() {
                continue;
            }
            let key = (d.id.clone(), d.type_params.len());
            let Some(&def_idx) = index.qualified_name_and_arity_to_def.get(&key) else {
                continue;
            };
            out.entry(def_idx)
                .or_default()
                .extend(d.property_message_args.iter().cloned());
        }
    }
    out
}

// Whether `def_idx` binds, on one of its own properties, resolved in
// `def_idx`'s own scope exactly as the emission pass resolves the same
// fact, an in-graph type the corpus sends. This is the admission rule's
// binding route: independent of any particular base's own argument, so a
// carrier can earn its base a place in the vocabulary this way even when
// the base's own argument names a message nobody sends.
fn def_binds_sent_message(
    def_idx: usize,
    property_messages: &HashMap<usize, Vec<String>>,
    sent: &HashSet<MessageIdentity>,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> bool {
    let Some(messages) = property_messages.get(&def_idx) else {
        return false;
    };
    let def = &index.defs[def_idx];
    let Some(file_ctx) = file_contexts.get(&def.file) else {
        return false;
    };
    let outer_types = def_outer_types(def);
    messages.iter().any(|message| {
        resolve_message_identity(
            message,
            &def.namespace,
            &outer_types,
            file_ctx,
            index,
            file_contexts,
        )
        .is_some_and(|identity| sent.contains(&identity))
    })
}

// A parameter descriptor's own candidate type texts: the descriptor's bare
// head with any generic-argument suffix stripped, plus -- recursively --
// every argument that suffix carries at any depth. `Task<Order>` yields
// `["Task", "Order"]`; `Task<Result<Order, Error>>` yields
// `["Task", "Result", "Order", "Error"]`. A leading `this ` extension-method
// marker is stripped first, defensively: a receiving method is never itself
// an extension method taking the message as `this`, but stripping costs
// nothing and keeps this reusable if that ever changes.
fn descriptor_type_texts(descriptor: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![descriptor
        .strip_prefix("this ")
        .unwrap_or(descriptor)
        .to_string()];
    while let Some(candidate) = stack.pop() {
        if let Some(open) = candidate.find('<') {
            if candidate.ends_with('>') {
                out.push(candidate[..open].to_string());
                let inner = &candidate[open + 1..candidate.len() - 1];
                stack.extend(split_top_level_args(inner));
                continue;
            }
        }
        out.push(candidate);
    }
    out
}

// Whether `def_idx` declares -- across every method and every overload of
// its own merged member surface, over every partial declaration -- a
// parameter whose type, or a type argument nested inside it at any depth,
// resolves to `message`'s own full identity (element and array rank alike).
// Each overload's own parameter descriptors resolve in `def_idx`'s own
// namespace and enclosing-type chain (a partial class shares both across
// its declarations) but that OVERLOAD's own declaring file's
// usings/aliases, mirroring how a merged field or property fact elsewhere
// in this resolver is read against the file that actually wrote it.
fn def_receives_message(
    def_idx: usize,
    message: MessageIdentity,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> bool {
    let def = &index.defs[def_idx];
    let ns = &def.namespace;
    let outer_types = def_outer_types(def);
    for overloads in index.member_lists[def_idx].method_params.values() {
        for overload in overloads {
            let Some(file_ctx) = file_contexts.get(&overload.file) else {
                continue;
            };
            let receives = overload.params.iter().any(|descriptor| {
                descriptor_type_texts(descriptor).iter().any(|text| {
                    resolve_message_identity(text, ns, &outer_types, file_ctx, index, file_contexts)
                        == Some(message)
                })
            });
            if receives {
                return true;
            }
        }
    }
    false
}

// Every registered type's own generic bases join the vocabulary -- but only
// the ones the admission rule actually admits (see the module doc comment):
// the registered type `R` must not itself be a message the corpus sends,
// and `R` must actually receive, through a declared parameter typed with
// the base's own argument or through a bound property, a message the
// corpus DOES send. A base whose registered type satisfies the rule
// through its own base-list argument is admitted as a full consumer base;
// one admitted only through a bound property is kept binding-only.
//
// Returns whether any registration admitted a base under either role.
fn harvest_registrations(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    sent: &HashSet<MessageIdentity>,
    consumer_bases: &mut HashSet<String>,
    binding_only_bases: &mut HashSet<String>,
) -> bool {
    let mut by_id: HashMap<&str, Vec<&FragDef>> = HashMap::new();
    for (_, frag) in fragments_by_file {
        for d in &frag.defs {
            by_id.entry(d.id.as_str()).or_default().push(d);
        }
    }
    let property_messages = property_messages_by_def(fragments_by_file, index);
    let mut contributed = false;
    for (file, frag) in fragments_by_file {
        if frag.handler_registrations.is_empty() {
            continue;
        }
        let Some(file_ctx) = file_contexts.get(file) else {
            continue;
        };
        for registration in &frag.handler_registrations {
            let Some(def_idx) = resolve_type_text(
                &registration.handler,
                &registration.namespace,
                &[],
                file_ctx,
                index,
                file_contexts,
            ) else {
                continue;
            };
            // The registered type must not itself be a message the corpus
            // sends. This is what makes a self-route through a harvested
            // base impossible: when a base's own argument is this SAME
            // type, either the corpus does not send it (the receiving
            // check below then has nothing to point at) or it does (this
            // check fails first). A registered type is never itself an
            // array, so this is always the plain (rank 0) identity.
            if sent.contains(&MessageIdentity::plain(def_idx)) {
                continue;
            }
            let r = &index.defs[def_idx];
            let Some(r_file_ctx) = file_contexts.get(&r.file) else {
                continue;
            };
            let r_ns = r.namespace.clone();
            let r_outer = def_outer_types(r);
            let receives_bound_message =
                def_binds_sent_message(def_idx, &property_messages, sent, index, file_contexts);
            // A partial class declares its bases across several files, so
            // every declaration of the registered id contributes.
            for d in by_id.get(r.id.as_str()).map_or(&[][..], Vec::as_slice) {
                for base in &d.bases {
                    if consumer_bases.contains(base) {
                        continue; // already full role; nothing left to learn
                    }
                    let Some(flat) = d.base_generic_args.get(base) else {
                        continue;
                    };
                    // A concrete argument is required: not a
                    // type-parameter pass-through.
                    let first_arg_is_array = d.array_message_bases.contains(base);
                    let Some((message_text, _)) = concrete_base_argument(
                        d.base_type_args.get(base).map(Vec::as_slice),
                        flat,
                        first_arg_is_array,
                    ) else {
                        continue;
                    };
                    // Argument route: the base's own argument resolves, in
                    // the registered type's own scope, to a message
                    // identity the corpus sends, AND the registered type
                    // actually declares a parameter that receives it.
                    let argument_admits = resolve_message_identity(
                        &message_text,
                        &r_ns,
                        &r_outer,
                        r_file_ctx,
                        index,
                        file_contexts,
                    )
                    .is_some_and(|message| {
                        sent.contains(&message)
                            && def_receives_message(def_idx, message, index, file_contexts)
                    });
                    if argument_admits {
                        if consumer_bases.insert(base.clone()) {
                            contributed = true;
                        }
                    } else if receives_bound_message && binding_only_bases.insert(base.clone()) {
                        contributed = true;
                    }
                }
            }
        }
    }
    contributed
}

// A handler base reached through a local abstract intermediate is still a
// handler base, of the SAME role the base it passes through to holds.
// `abstract class OrderWorkerBase<T> : QueueWorkerBase<T>` passes its own
// type parameter straight through, which extraction records as the wildcard
// `*`, so the intermediate names no message itself and its own subclasses
// are where the messages are. Adding the intermediate's name to the
// matching set is what lets those subclasses be read.
//
// Run to a fixpoint so a chain of intermediates is followed to its end. The
// sets only grow and are bounded by the number of declared types, so the
// loop terminates.
fn close_over_intermediates(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    consumer_bases: &mut HashSet<String>,
    binding_only_bases: &mut HashSet<String>,
) {
    loop {
        let mut added = false;
        for (_, frag) in fragments_by_file {
            for d in &frag.defs {
                let key = (d.id.clone(), d.type_params.len());
                if !index.qualified_name_and_arity_to_def.contains_key(&key) {
                    continue;
                }
                let name = simple_name(&d.id);
                if consumer_bases.contains(name) {
                    continue;
                }
                let mut becomes_consumer = false;
                let mut becomes_binding_only = false;
                for base in &d.bases {
                    let passes_through = d
                        .base_generic_args
                        .get(base)
                        .and_then(|args| args.first())
                        .is_some_and(|first| first == "*");
                    if !passes_through {
                        continue;
                    }
                    if consumer_bases.contains(base) {
                        becomes_consumer = true;
                    } else if binding_only_bases.contains(base) {
                        becomes_binding_only = true;
                    }
                }
                if becomes_consumer {
                    consumer_bases.insert(name.to_string());
                    added = true;
                } else if becomes_binding_only && binding_only_bases.insert(name.to_string()) {
                    added = true;
                }
            }
        }
        if !added {
            return;
        }
    }
}

// A method whose body hands its own caller's message to a publish is a
// publish under its own name. Extraction records exactly that pairing and
// nothing else, so every wrapper name here was written beside a verb the
// seed already held.
//
// Deliberately ONE hop: a wrapper around a wrapper is not promoted. Each
// hop costs the message identity another layer of indirection this engine
// cannot check, and a chain would let one mis-recognized verb recruit the
// methods around it.
fn harvest_wrappers(fragments_by_file: &[(String, Fragment)], publish_verbs: &mut HashSet<String>) {
    // Read against a frozen copy of the seed: promoting into the same set
    // being tested would let a wrapper recruit the method wrapping IT, and
    // the hop limit would depend on corpus iteration order.
    let seed = publish_verbs.clone();
    for (_, frag) in fragments_by_file {
        for publish in &frag.publishes {
            let Some(method) = &publish.enclosing_method else {
                continue;
            };
            if seed.contains(&publish.verb) {
                publish_verbs.insert(method.clone());
            }
        }
    }
}
