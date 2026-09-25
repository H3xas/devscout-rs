// Message-bus hop edges: a publish site's resolved message reaching the
// consumer/handler def that SAME message resolves to on the other end, both
// sides run through the ordinary ladder in their own file's context. Mirrors
// dispatch.rs's own shape -- a registration's raw type-argument text
// resolved in the registration site's own context -- applied to a publish
// site's message and a consumer base's own generic argument instead of a
// two-type-argument registration call.
//
// A publish site's message already comes out of extraction as a single
// resolved string (see extract/bus.rs's `PublishRecord`): whichever of a
// generic argument, a constructed type or a declared identifier supplied it
// is not recorded alongside it, so this module has no way to tell those
// shapes apart after the fact. The evidence vocabulary below therefore names
// the CONSUMER/handler side's own shape instead, the one side this module
// can still distinguish once a hop is found.

use std::collections::{HashMap, HashSet};

use super::bus_vocab::{self, BusVocabulary, MessageIdentity};
use super::index::DefIndex;
use super::receiver::def_outer_types;
use super::scope::FileContext;
use crate::graph::{Edge, EdgesByKind, FragDef, Fragment};

// The closed evidence vocabulary `Edge::BusHop`'s own `evidence` field
// draws from -- the one place any of its words is spelled, so a rename is a
// one-line change.
//
//   base-arg          A base carrying its message as a plain, single
//                      generic argument (`IConsumer<T>`, `BaseConsumer<T>`,
//                      `IAmInitiatedBy<T>`, `IWorkHandler<T>`).
//   nested-base-arg    The argument at that same position is itself
//                      generic and wraps exactly one further argument
//                      (`IConsumer<Batch<T>>`); the message is the WRAPPED
//                      argument, read structurally off the rendered
//                      descriptor, never by naming the wrapper.
//   mediator-request   The base carries two or more generic arguments
//                      (`IRequestHandler<TRequest, TResponse>`); the message
//                      is still position 0, but a second argument sitting
//                      beside it is a distinct enough shape to earn its own
//                      word.
//   property-arg       The message is not on the base list at all: a
//                      handler already recognized by one of the shapes
//                      above declares a property wrapping exactly one type
//                      argument, which is how a long-running handler binds
//                      the further messages it reacts to.
const EVIDENCE_BASE_ARG: &str = "base-arg";
const EVIDENCE_NESTED_BASE_ARG: &str = "nested-base-arg";
const EVIDENCE_MEDIATOR_REQUEST: &str = "mediator-request";
const EVIDENCE_PROPERTY_ARG: &str = "property-arg";

// Moq's own public verb surface (the dominant open-source .NET mocking
// library, MIT-licensed NuGet package `Moq`, not a house or application
// tool). A call spelled with one of these names, sitting inside a lambda
// argument of an invocation the repository itself does not declare, is a
// setup/verify lambda by Moq's own closed, documented shape -- never a
// house or application name.
const TEST_DOUBLE_DIRECT_VERBS: &[&str] = &[
    "Setup",
    "SetupGet",
    "SetupSet",
    "SetupSequence",
    "SetupAllProperties",
    "Verify",
    "VerifyGet",
    "VerifySet",
    "VerifyAll",
    "VerifyNoOtherCalls",
];

// A recognized-verb call is refused as non-dispatch only when the
// repository itself declares EVERY arity-matching overload of that name to
// take the message's own resolved IDENTITY (element and array rank alike)
// as an input -- never merely because the verb name coincides with a
// library call. `message` and the overload descriptors are resolved
// through the SAME ladder, in the CALL SITE's own `(ns, outer_types)`
// context (the context the call's own message already resolved in), so a
// repo method whose parameter is a same-named-but-different message, or an
// array of the message where the call sends one alone, never wrongly
// refuses the site.
#[allow(clippy::too_many_arguments)]
fn structural_non_dispatch(
    verb: &str,
    arg_count: usize,
    message: MessageIdentity,
    ns: &str,
    outer_types: &[String],
    file_ctx: &FileContext,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> bool {
    let Some(candidate_defs) = index.member_name_to_defs.get(verb) else {
        return false;
    };
    let mut matched_any = false;
    for &def_idx in candidate_defs {
        let Some(overloads) = index.member_lists[def_idx].method_params.get(verb) else {
            continue;
        };
        for overload in overloads {
            if overload.params.len() != arg_count {
                continue;
            }
            matched_any = true;
            let takes_message = overload.params.iter().any(|descriptor| {
                bus_vocab::resolve_message_identity(
                    descriptor,
                    ns,
                    outer_types,
                    file_ctx,
                    index,
                    file_contexts,
                ) == Some(message)
            });
            if !takes_message {
                return false;
            }
        }
    }
    matched_any
}

// One method overload's required/total argument-count RANGE, paired with
// its own parameter descriptors -- both read from the SAME fragment def
// record's `methodArities`/`methodParams` entry for one verb name (the k-th
// range belongs to the k-th descriptor list, within that one record).
// `DefIndex`'s merged `MemberLists::method_params` cannot supply the pairing:
// it merges a partial class's declarations by removing duplicates from each
// list SEPARATELY, so the merged lists are no longer paired by position,
// which is why `verb_overloads` reads the unmerged fragment def records.
struct VerbOverload {
    /// The parameter count with no default value and no `params` tail,
    /// the `this` receiver included when the overload declares one.
    required: usize,
    /// The parameter count, or `usize::MAX` for an unbounded `params` tail
    /// (`FragDef::method_arities`'s own `-1` sentinel, widened here).
    total: usize,
    /// The overload's own parameter descriptors, in order.
    params: Vec<String>,
}

// The overloads of each verb name, read only from the def records of the
// defs `DefIndex::member_name_to_defs` lists for that name: a def declaring
// it as a public method, a property, a field or an extension method, the
// same pool the resolver's member-name lookup draws from. A def that
// declares the name only as a non-public, non-extension method is not a
// candidate: counting it would let an unrelated class's private
// `Setup(Action<…>)` align a mocking library's setup lambda as a plain
// delegate and keep a hop the direct seed refuses.
//
// Built once per resolve pass and shared by every `is_test_double_lambda`
// call, so the O(defs) scan runs once rather than once per publish site.
// An overload whose def record has no paired range (`methodArities` carries
// no entry for the verb at all, or a shorter list than `methodParams`'s) is
// read with `required == total == params.len()`, the exact-arity reading.
fn verb_overloads(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
) -> HashMap<String, Vec<VerbOverload>> {
    // A partial class's merged def owns one record per declaring file; the
    // (id, type-parameter count) key is the one `build_def_index` merges by.
    let mut records: Vec<Vec<&FragDef>> = vec![Vec::new(); index.defs.len()];
    for (_, frag) in fragments_by_file {
        for d in &frag.defs {
            if let Some(&idx) = index
                .qualified_name_and_arity_to_def
                .get(&(d.id.clone(), d.type_params.len()))
            {
                records[idx].push(d);
            }
        }
    }
    let mut out: HashMap<String, Vec<VerbOverload>> = HashMap::new();
    for (verb, candidate_defs) in &index.member_name_to_defs {
        for &def_idx in candidate_defs {
            for d in &records[def_idx] {
                let Some(overload_params) = d.method_params.get(verb) else {
                    continue;
                };
                let ranges = d.method_arities.get(verb);
                let entry = out.entry(verb.clone()).or_default();
                for (k, params) in overload_params.iter().enumerate() {
                    let (required, total) = ranges
                        .and_then(|r| r.get(k))
                        .map(|&(r, t)| (r, if t < 0 { usize::MAX } else { t as usize }))
                        .unwrap_or((params.len(), params.len()));
                    entry.push(VerbOverload {
                        required,
                        total,
                        params: params.clone(),
                    });
                }
            }
        }
    }
    out
}

// A call is refused as a test double only when its own lambda demonstrably
// sits, on EVERY admissible reading of EVERY aligned overload, inside an
// expression-tree-typed parameter of a repository-declared helper, or
// inside a directly Moq-shaped setup/verify call the repository does not
// itself declare; every saga-initialiser and other plain-delegate lambda
// call keeps its site.
//
// A repository-declared extension method's `this`-marked first parameter is
// never one of the call's own arguments when the call is written in
// EXTENSION form (`receiver.Helper(…)`) -- the receiver fills it implicitly
// -- but static form (`Helper(receiver, …)`) passes it explicitly, and
// nothing in the call site's own AST distinguishes the two: both parse as
// a member-access callee, `receiver.Helper` and `TypeName.Helper` alike.
// An optional trailing parameter lets one overload accept several argument
// counts, so each reading checks the overload's own required/total RANGE,
// not a single exact arity: the *direct* reading (a static-form call, or a
// helper without the `this` marker) is admissible when the call's own
// argument count `n` falls in `[required, total]`, aligning the lambda with
// parameter `arg_position`; the *extension* reading (only when the
// overload's first parameter carries the `this` marker) is admissible when
// `n + 1` falls in `[required, total]`, aligning the lambda with parameter
// `arg_position + 1`, the receiver filled in implicitly. A direct reading
// that would align the lambda with the `this`-marked receiver itself (only
// possible when `arg_position == 0`) is never admissible -- a lambda never
// binds an extension call's own receiver. An overload ALIGNS when at least
// one of its (up to two) readings is admissible; refusal holds only when
// some overload aligns and every admissible reading of every aligned
// overload lands on an expression-tree-typed parameter. Any admissible
// reading landing on another parameter, or past the last recorded one,
// keeps the site -- the fail-safe default. With no optional parameter
// (`required == total`) at most one of an overload's two readings can fit
// the call's argument count, so the choice between the two call forms is
// never a guess.
fn is_test_double_lambda(
    outer_verb: &str,
    arg_position: usize,
    outer_arg_count: usize,
    verb_overloads: &HashMap<String, Vec<VerbOverload>>,
) -> bool {
    let mut aligned_any = false;
    if let Some(overloads) = verb_overloads.get(outer_verb) {
        for overload in overloads {
            let is_this_extension = overload
                .params
                .first()
                .is_some_and(|p| p.starts_with("this "));
            let in_range = |n: usize| overload.required <= n && n <= overload.total;
            // Direct reading: excluded outright when it would bind the
            // lambda to the `this`-marked receiver's own slot.
            if in_range(outer_arg_count) && !(is_this_extension && arg_position == 0) {
                aligned_any = true;
                let is_expression_tree = overload
                    .params
                    .get(arg_position)
                    .is_some_and(|d| d.starts_with("Expression<"));
                if !is_expression_tree {
                    return false;
                }
            }
            // Extension reading: only when the overload declares a `this`
            // receiver.
            if is_this_extension && in_range(outer_arg_count + 1) {
                aligned_any = true;
                let is_expression_tree = overload
                    .params
                    .get(arg_position + 1)
                    .is_some_and(|d| d.starts_with("Expression<"));
                if !is_expression_tree {
                    return false;
                }
            }
        }
    }
    if aligned_any {
        return true;
    }
    TEST_DOUBLE_DIRECT_VERBS.contains(&outer_verb)
}

// One consumer/handler def's own recorded message, before resolution: which
// def (by its already-assigned `DefIndex` slot), the message text as
// written on its base list, and the evidence word that text's shape earns.
struct RawConsumer {
    def_idx: usize,
    message: String,
    evidence: &'static str,
}

// One consumer/handler def's message once resolved to an in-graph
// identity, the input the publish-site loop matches against.
struct ResolvedConsumer {
    def_idx: usize,
    message: MessageIdentity,
    evidence: &'static str,
}

// One recognized base's own message text and evidence word, from that
// base's flattened generic arguments (`flat`, always present when the base
// itself is recorded) and its nested-argument fact (`nested`, present only
// when the base's own argument list carries a further generic argument).
// `None` when the base's own first argument is an unbound type-parameter
// pass-through -- `base_generic_args` records that as `"*"` -- a wildcard
// names no concrete message at THIS declaration, however many further
// subclasses eventually close it over a real type. The concrete-argument
// reading itself lives in `bus_vocab`, shared with the vocabulary admission
// rule that reads the SAME argument before this evidence word is ever
// picked.
fn base_message(
    nested: Option<&[String]>,
    flat: Option<&[String]>,
    first_arg_is_array: bool,
) -> Option<(String, &'static str)> {
    let flat = flat?;
    let (message, from_nested) =
        bus_vocab::concrete_base_argument(nested, flat, first_arg_is_array)?;
    let evidence = if from_nested {
        EVIDENCE_NESTED_BASE_ARG
    } else if flat.len() >= 2 {
        EVIDENCE_MEDIATOR_REQUEST
    } else {
        EVIDENCE_BASE_ARG
    };
    Some((message, evidence))
}

// Every consumer/handler def in the corpus, keyed to the def index
// `DefIndex` already assigned it. `bases`/`base_generic_args`/
// `base_type_args` are read straight off the raw per-file `FragDef` records
// rather than through `DefIndex`'s own `MemberLists`, which carries the
// first two but not `base_type_args`. Iterated in the same fragment/def
// order `build_def_index` used, so a base name already recorded from an
// earlier file keeps that file's own facts -- first-declaration-wins,
// mirroring `MemberLists.base_generic_args`'s own rule for a partial class.
fn raw_consumers(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    vocabulary: &BusVocabulary,
) -> Vec<RawConsumer> {
    let mut out = Vec::new();
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut recognized: HashSet<usize> = HashSet::new();
    for (_, frag) in fragments_by_file {
        for d in &frag.defs {
            let key = (d.id.clone(), d.type_params.len());
            let Some(&def_idx) = index.qualified_name_and_arity_to_def.get(&key) else {
                continue;
            };
            for base_name in &d.bases {
                let is_consumer = vocabulary.consumer_bases.contains(base_name);
                let is_binding_only =
                    !is_consumer && vocabulary.binding_only_bases.contains(base_name);
                if !is_consumer && !is_binding_only {
                    continue;
                }
                if is_binding_only {
                    // A binding-only base recognizes this def as a handler
                    // for the property scan below alone: its own type
                    // argument earns no route.
                    recognized.insert(def_idx);
                    continue;
                }
                if !seen.insert((def_idx, base_name.clone())) {
                    continue;
                }
                let flat = d.base_generic_args.get(base_name).map(Vec::as_slice);
                let nested = d.base_type_args.get(base_name).map(Vec::as_slice);
                let first_arg_is_array = d.array_message_bases.contains(base_name);
                if let Some((message, evidence)) = base_message(nested, flat, first_arg_is_array) {
                    recognized.insert(def_idx);
                    out.push(RawConsumer {
                        def_idx,
                        message,
                        evidence,
                    });
                }
            }
        }
    }
    append_property_messages(fragments_by_file, index, &recognized, &mut seen, &mut out);
    out
}

// The messages a recognized handler binds on its own properties rather than
// on its base list. Gated on `recognized` -- a type this pass already holds
// to be a handler -- because a generic property is an ordinary thing for any
// type to declare and says nothing about messages on its own.
fn append_property_messages(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    recognized: &HashSet<usize>,
    seen: &mut HashSet<(usize, String)>,
    out: &mut Vec<RawConsumer>,
) {
    for (_, frag) in fragments_by_file {
        for d in &frag.defs {
            if d.property_message_args.is_empty() {
                continue;
            }
            let key = (d.id.clone(), d.type_params.len());
            let Some(&def_idx) = index.qualified_name_and_arity_to_def.get(&key) else {
                continue;
            };
            if !recognized.contains(&def_idx) {
                continue;
            }
            for message in &d.property_message_args {
                if seen.insert((def_idx, message.clone())) {
                    out.push(RawConsumer {
                        def_idx,
                        message: message.clone(),
                        evidence: EVIDENCE_PROPERTY_ARG,
                    });
                }
            }
        }
    }
}

// Every consumer/handler site whose OWN message resolves to an in-graph
// def, resolved in that def's OWN declaring file's namespace/using context
// -- never the publish site's -- so a later match can only ever bind
// through a def index both sides independently agreed on.
fn resolved_consumers(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    vocabulary: &BusVocabulary,
) -> Vec<ResolvedConsumer> {
    // First-wins on the RESOLVED (def_idx, message identity) pair: this is
    // the pair that actually determines whether two `RawConsumer` entries
    // become the same route once both resolve, and `raw_consumers` already
    // yields every base-list entry for a def (in declaration order) before
    // any of that def's property entries, so keeping only the first
    // resolution for a pair structurally prefers base-list evidence over
    // `property-arg`, and an earlier-declared base over a later one -- with
    // no separate precedence table.
    let mut seen_resolved: HashSet<(usize, MessageIdentity)> = HashSet::new();
    raw_consumers(fragments_by_file, index, vocabulary)
        .into_iter()
        .filter_map(|c| {
            let def = &index.defs[c.def_idx];
            let file_ctx = file_contexts.get(&def.file)?;
            let message = bus_vocab::resolve_message_identity(
                &c.message,
                &def.namespace,
                &def_outer_types(def),
                file_ctx,
                index,
                file_contexts,
            )?;
            if !seen_resolved.insert((c.def_idx, message)) {
                return None;
            }
            Some(ResolvedConsumer {
                def_idx: c.def_idx,
                message,
                evidence: c.evidence,
            })
        })
        .collect()
}

// One publish site the bus pass keeps: a recognized or wrapper-derived
// verb, a message that resolved to an in-graph def, and neither structural
// refusal. Resolved and filtered exactly ONCE, before the consumer
// vocabulary is derived -- the vocabulary admission rule needs the very
// same "what does this corpus send" fact emission does, so both read it off
// this one list rather than two independent scans that could disagree.
struct KeptPublish {
    file: String,
    line: usize,
    message: MessageIdentity,
}

fn kept_publish_sites(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    publish_verbs: &HashSet<String>,
    verb_overloads: &HashMap<String, Vec<VerbOverload>>,
) -> Vec<KeptPublish> {
    let mut out = Vec::new();
    for (file, frag) in fragments_by_file {
        if frag.publishes.is_empty() {
            continue;
        }
        let Some(file_ctx) = file_contexts.get(file) else {
            continue;
        };
        for publish in &frag.publishes {
            // A publish whose message is its own caller's type parameter is
            // a forwarding wrapper's own call, not a site: it named the
            // wrapper for the vocabulary and has no message of its own.
            if publish.enclosing_method.is_some() {
                continue;
            }
            if !publish_verbs.contains(&publish.verb) {
                continue;
            }
            let Some(message) = bus_vocab::resolve_message_identity(
                &publish.message,
                &publish.namespace,
                &publish.outer_types,
                file_ctx,
                index,
                file_contexts,
            ) else {
                continue;
            };
            // Two structural refusals, checked once per publish site rather
            // than per candidate consumer: neither depends on which handler
            // the message might reach, only on the call's own shape.
            if structural_non_dispatch(
                &publish.verb,
                publish.arg_count,
                message,
                &publish.namespace,
                &publish.outer_types,
                file_ctx,
                index,
                file_contexts,
            ) {
                continue;
            }
            if publish.enclosing_call.as_ref().is_some_and(|c| {
                is_test_double_lambda(&c.verb, c.arg_position, c.arg_count, verb_overloads)
            }) {
                continue;
            }
            out.push(KeptPublish {
                file: file.clone(),
                line: publish.line,
                message,
            });
        }
    }
    out
}

/// Appends one `bus-hop` edge per (publish site, consumer) pair whose
/// message identities resolve to the SAME def -- fan-out included, one edge
/// per handler when several share a message.
///
/// A repository with no publish site, or no consumer/handler site, runs the
/// corpus scan and adds nothing, which is what keeps its `graph.json`
/// byte-identical: `edges_by_kind.bus_hop` stays `None`, never `Some(0)`, so
/// a bus-free graph serializes exactly as it did before this kind existed.
///
/// Answers whether the vocabulary this ran with came from the repository's
/// own registrations, and `None` whenever no hop was emitted -- the same
/// omit-when-absent rule the counter follows, for the same byte-identity
/// reason.
pub(super) fn append_bus_edges(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    edges: &mut Vec<Edge>,
    edges_by_kind: &mut EdgesByKind,
) -> Option<bool> {
    if fragments_by_file
        .iter()
        .all(|(_, f)| f.publishes.is_empty())
    {
        return None;
    }
    let publish_verbs = bus_vocab::derive_publish_verbs(fragments_by_file);
    let verb_overloads = verb_overloads(fragments_by_file, index);
    let kept = kept_publish_sites(
        fragments_by_file,
        index,
        file_contexts,
        &publish_verbs,
        &verb_overloads,
    );
    let sent: HashSet<MessageIdentity> = kept.iter().map(|k| k.message).collect();
    let (consumer_bases, binding_only_bases, derived_from_registrations) =
        bus_vocab::derive_consumer_bases(fragments_by_file, index, file_contexts, &sent);
    let vocabulary = BusVocabulary {
        consumer_bases,
        binding_only_bases,
    };
    let consumers = resolved_consumers(fragments_by_file, index, file_contexts, &vocabulary);
    if consumers.is_empty() {
        return None;
    }
    let mut hop_count = 0usize;
    for publish in &kept {
        for consumer in &consumers {
            if consumer.message != publish.message {
                continue;
            }
            let target = &index.defs[consumer.def_idx];
            edges.push(Edge::BusHop {
                from_file: publish.file.clone(),
                from_line: publish.line,
                message: publish.message.rendered_id(index),
                to: target.id.clone(),
                to_file: target.file.clone(),
                evidence: consumer.evidence.to_string(),
            });
            hop_count += 1;
        }
    }
    if hop_count == 0 {
        return None;
    }
    edges_by_kind.bus_hop = Some(hop_count);
    Some(derived_from_registrations)
}
