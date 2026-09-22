use std::collections::HashSet;

use tree_sitter::Node;

use super::delegate_args::delegate_argument_param_count;
use super::text::{named_children, text};
use super::types::{Fact, RefRecord, RegistrationRecord};

// Unwraps nullable/array/alias-qualified wrappers,
// pulls the base identifier out of a generic_name (discarding its type
// arguments -- those are recorded separately when the walk reaches the
// type_argument_list node), and takes qualified_name's full dotted text
// verbatim. Anything else (predefined_type, implicit_type/`var`, ...) is
// not a user-defined type reference.
fn outer_type_name(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "nullable_type" | "array_type" => match node.child_by_field_name("type") {
            Some(inner) => outer_type_name(inner, src),
            None => None,
        },
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src)),
        "qualified_name" => {
            let tail = node.child_by_field_name("name")?;
            let full = text(node, src);
            let tail_text = text(tail, src);
            let normalized_tail = outer_type_name(tail, src)?;
            full.strip_suffix(&tail_text)
                .map(|prefix| format!("{prefix}{normalized_tail}"))
        }
        "alias_qualified_name" => match node.child_by_field_name("name") {
            Some(n) => outer_type_name(n, src),
            None => None,
        },
        "identifier" => Some(text(node, src)),
        _ => None,
    }
}

// The base IDENTIFIER of a type node, for the local type facts. Deliberately
// NOT outer_type_name: that one
// returns a qualified name's FULL dotted text because the resolution ladder
// tries an exact-FQN match first. A stage-2 fact is a NAME only -- generic
// arguments stripped, a qualified name reduced to its last segment, and
// predefined types (string/int/void/...) plus `var` (implicit_type) yielding
// None, which every caller reads as "no fact" rather than as a type called
// "string". An empty result is folded to None as well: every caller guards
// with a presence check, for which "" and absent are the same answer.
//
// `keep_predefined` flips ONLY the predefined-type case, for the
// one fact family that wants "string" as an answer rather than as a refusal:
// an extension method's this-parameter type. `this string s` is legal and
// common C#, and its thisType is a name a receiver has to match exactly, not a
// def anyone resolves. Every local-fact caller passes `false` and therefore
// keeps its exact prior output.
pub(super) fn base_type_identifier(
    node: Option<Node>,
    src: &[u8],
    keep_predefined: bool,
) -> Option<String> {
    let node = node?;
    let name = match node.kind() {
        "nullable_type" | "array_type" => {
            base_type_identifier(node.child_by_field_name("type"), src, keep_predefined)
        }
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src)),
        "qualified_name" | "alias_qualified_name" => {
            base_type_identifier(node.child_by_field_name("name"), src, keep_predefined)
        }
        "identifier" => Some(text(node, src)),
        "predefined_type" => {
            if keep_predefined {
                Some(text(node, src))
            } else {
                None
            }
        }
        _ => None,
    };
    name.filter(|n| !n.is_empty())
}

// The generic_name a type node bottoms out at, following the SAME unwrapping
// path
// base_type_identifier takes (nullable and array wrappers, qualified/alias
// tails), or None when the type is not generic at its top level. `Ns.Box<T>`
// parses as a qualified_name whose `name` field IS the generic_name, which is
// why the qualified case recurses rather than stopping.
fn top_level_generic_name(node: Option<Node>) -> Option<Node> {
    let node = node?;
    match node.kind() {
        "nullable_type" | "array_type" => top_level_generic_name(node.child_by_field_name("type")),
        "qualified_name" | "alias_qualified_name" => {
            top_level_generic_name(node.child_by_field_name("name"))
        }
        "generic_name" => Some(node),
        _ => None,
    }
}

fn type_argument_arity(node: Option<Node>, src: &[u8]) -> Option<usize> {
    let generic = top_level_generic_name(node)?;
    let list = named_children(generic)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    Some(text(list, src).bytes().filter(|b| *b == b',').count() + 1)
}

// The TOP-LEVEL generic argument descriptors of a type node, or None when it
// carries no
// type-argument list (`Box<>`, an unbound generic, counts as none). One
// descriptor per argument, in source order: the argument's base identifier
// (predefined types KEPT, so `IDictionary<string, object>` records
// ["string", "object"]), except a position naming one of `type_params` -- the
// enclosing method's or type's own type parameters -- which records "*".
//
// A single argument that yields NO base identifier at all (a tuple type, a
// pointer) drops the WHOLE list: the match rule compares positions by index,
// so a partial list would silently shift every argument after the hole.
pub(super) fn generic_arg_descriptors(
    node: Option<Node>,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Vec<String>> {
    let generic = top_level_generic_name(node)?;
    let list = named_children(generic)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let args = named_children(list);
    if args.is_empty() {
        return None;
    }
    let mut descriptors = Vec::with_capacity(args.len());
    for arg in args {
        let base = base_type_identifier(Some(arg), src, true)?;
        descriptors.push(if type_params.contains(&base) {
            "*".to_string()
        } else {
            base
        });
    }
    Some(descriptors)
}

// A canonical, NESTED text descriptor for a type node -- unlike
// `base_type_identifier`, which strips generic arguments to a bare name,
// this one renders them recursively: `Action<Options>` records exactly
// that, not `Action`. Used for `DefRecord::method_params`, where an
// overload's shape has to distinguish `Action<Options>` from
// `Func<Options,bool>` at a glance rather than collapsing both to `Action`/
// `Func`. Never returns an empty string -- positions in a parameter list
// must stay aligned, so an unreadable node still gets a one-character
// placeholder rather than dropping out silently.
//
//   - `nullable_type` -- the descriptor of its inner `type` (the `?` is not
//     part of the shape this records).
//   - `array_type` -- the element's descriptor, plus a literal `[]`.
//   - `qualified_name`/`alias_qualified_name` -- the descriptor of the
//     `name` field only, exactly like `base_type_identifier`: the
//     qualifier is dropped.
//   - `generic_name` -- the identifier text, `<`, each named child of its
//     `type_argument_list` recursively descriptor'd and joined by `,` (no
//     spaces), then `>`.
//   - `identifier` -- `*` when the name is one of `type_params` (the
//     enclosing method's or type's own type parameter), else the text.
//   - `predefined_type` -- its own text (`string`, `int`, ...).
//   - anything else (a tuple type, a pointer, a function pointer, a
//     missing node) -- `?`.
pub(super) fn type_descriptor(
    node: Option<Node>,
    src: &[u8],
    type_params: &HashSet<String>,
) -> String {
    let Some(node) = node else {
        return "?".to_string();
    };
    match node.kind() {
        "nullable_type" => type_descriptor(node.child_by_field_name("type"), src, type_params),
        "array_type" => {
            let element = type_descriptor(node.child_by_field_name("type"), src, type_params);
            format!("{element}[]")
        }
        "qualified_name" | "alias_qualified_name" => {
            type_descriptor(node.child_by_field_name("name"), src, type_params)
        }
        "generic_name" => {
            let ident = named_children(node)
                .into_iter()
                .find(|c| c.kind() == "identifier")
                .map(|id| text(id, src))
                .unwrap_or_default();
            let args = named_children(node)
                .into_iter()
                .find(|c| c.kind() == "type_argument_list")
                .map(|list| {
                    named_children(list)
                        .into_iter()
                        .map(|a| type_descriptor(Some(a), src, type_params))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!("{ident}<{args}>")
        }
        "identifier" => {
            let name = text(node, src);
            if type_params.contains(&name) {
                "*".to_string()
            } else {
                name
            }
        }
        "predefined_type" => text(node, src),
        _ => "?".to_string(),
    }
}

// The type-parameter names a declaration introduces (`class Box<T>`,
// `void Then<TSaga, TData>(...)`). Empty for every non-generic declaration.
pub(super) fn type_parameter_names(node: Node, src: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
    else {
        return names;
    };
    for p in named_children(list) {
        if p.kind() != "type_parameter" {
            continue;
        }
        let name = match p.child_by_field_name("name") {
            Some(n) => text(n, src),
            None => text(p, src),
        };
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

// Same traversal as type_parameter_names, but order-preserving
// (first-occurrence order, deduped) rather than a HashSet: this is the one
// caller that serializes the list itself (DefRecord.type_params) rather than
// only testing membership, and a HashSet's iteration order is undefined --
// This must emit the type parameters in source order for the extract-dump
// bytes to be stable.
pub(super) fn type_parameter_names_ordered(node: Node, src: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Some(list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
    else {
        return names;
    };
    for p in named_children(list) {
        if p.kind() != "type_parameter" {
            continue;
        }
        let name = match p.child_by_field_name("name") {
            Some(n) => text(n, src),
            None => text(p, src),
        };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

// A member-access qualifier is a uses-member candidate when it is a plain
// name ('identifier' or 'qualified_name'), a 'generic_name' ("Cache<T>.x" --
// type args dropped for resolution, the same normalization outer_type_name
// applies in type positions), OR a member_access_expression chain that
// itself bottoms out at a plain name -- "Some.Namespace.MyEnum" in
// "Some.Namespace.MyEnum.Member" parses as nested member_access_expression
// (not qualified_name) since it's in expression position, so the chain is
// flattened recursively into its full dotted text. walk() still recurses
// into every level of the chain regardless (each level is its own
// member_access_expression node), so a shorter window within the same chain
// also gets its own (separately resolved) candidate -- the resolution ladder
// (resolve.rs) silently drops whichever one doesn't clear its emission
// tiers. A chain that never bottoms out at a plain name (a call, an
// object-creation expression, an indexer, ...) simply yields no candidate at
// that position, per the "never guess" rule.
//
// Returns (text, generic) rather than bare text: a type-argument list
// anywhere in the qualifier is syntax only a
// TYPE can carry (locals, fields, and properties cannot), so the resolver
// uses `generic` as a type-certainty signal for non-enum member emission.
// `type_stack` resolves the two keyword qualifiers, `this` and `base`
// (bare anonymous tokens in this grammar -- neither wraps in a
// `this_expression`/`base_expression` rule, so `node.kind()` for either is
// literally "this"/"base"; verified against the shipped grammar, not
// assumed) into the innermost enclosing type's own simple name. Both read
// the SAME name: the walk() caller tells them apart by re-inspecting the
// qualifier node's own kind when it needs the `base`-vs-`this` distinction
// (a receiver_base flag, a lookup starting point) -- this function only
// ever answers "what NAME does this qualifier denote", never "which
// keyword". Outside any type (`type_stack` empty) neither keyword denotes
// anything, so both return `None`, same as every other unresolvable
// qualifier.
pub(super) fn member_qualifier_info(
    node: Option<Node>,
    src: &[u8],
    type_stack: &[String],
) -> Option<(String, bool)> {
    let node = node?;
    match node.kind() {
        "identifier" | "qualified_name" => Some((text(node, src), false)),
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| (text(id, src), true)),
        "this" | "base" => type_stack.last().cloned().map(|name| (name, false)),
        "member_access_expression" => {
            let inner =
                member_qualifier_info(node.child_by_field_name("expression"), src, type_stack);
            let name_node = node.child_by_field_name("name")?;
            let (inner_text, inner_generic) = inner?;
            if name_node.kind() == "generic_name" {
                return named_children(name_node)
                    .into_iter()
                    .find(|c| c.kind() == "identifier")
                    .map(|id| (format!("{inner_text}.{}", text(id, src)), true));
            }
            let name = text(name_node, src);
            if name.is_empty() {
                return None;
            }
            Some((format!("{inner_text}.{name}"), inner_generic))
        }
        _ => None,
    }
}

// The argument count of the CALL this member access is the callee of, or
// `None` when it is
// not a callee at all.
//
// The parent test is deliberately narrow on both halves. The parent must be an
// `invocation_expression`, and `node` must be its `function` field -- an access
// sitting in the parent's ARGUMENT list ("Send(x.Payload)") has an
// invocation_expression parent too, and inheriting that call's argument count
// would be exactly the kind of borrowed fact stage 2's chain-tail hazard is
// about. Everything else -- a property read, an element access, a member access
// used as a value -- yields `None`, and an absent argCount is what keeps a
// non-call out of the arity-matched extension tier entirely.
//
// Because the test reads `node`'s OWN parent, every ref site gets its own
// answer for free: a flattened chain window asks the question of its own
// position in the tree and can never inherit a neighbour's count. Node identity is compared here by byte-range: two
// distinct nodes of one tree cannot share both a start and an end byte AND a
// kind at the same tree position.
// `node`'s own `argument_list`, when `node` is the `function` field of the
// `invocation_expression` it is a direct child of -- the one structural
// check `invocation_arg_count` and `invocation_lambda_arg_arity` both need,
// kept in one place so a chain window answers only for its OWN call, never
// the one wrapping it, in exactly one way.
fn invocation_arguments(node: Node) -> Option<Vec<Node>> {
    let parent = node.parent()?;
    if parent.kind() != "invocation_expression" {
        return None;
    }
    let function = parent.child_by_field_name("function")?;
    if function.id() != node.id() {
        return None;
    }
    let args = parent.child_by_field_name("arguments")?;
    if args.kind() != "argument_list" {
        return None;
    }
    Some(named_children(args))
}

pub(super) fn invocation_arg_count(node: Node) -> Option<usize> {
    Some(invocation_arguments(node)?.len())
}

// The parameter count of each delegate-shaped argument of the SAME
// invocation `invocation_arg_count` measures -- a lambda literal or a local
// function passed as a method group (`delegate_argument_param_count`) --
// one entry per argument position, `None` at every other position. `None`
// entirely when `node` is not an invocation's callee, or when every
// position answers `None` -- an absent WHOLE fact, like every other "no
// fact" `Option` in this file, rather than an all-`None` list. Never reads
// the delegate parameter list its OWN eventual overload has: that
// comparison is the resolver's job, this is only the argument's own arity.
pub(super) fn invocation_lambda_arg_arity(node: Node, src: &[u8]) -> Option<Vec<Option<usize>>> {
    let arities: Vec<Option<usize>> = invocation_arguments(node)?
        .into_iter()
        .map(|argument| delegate_argument_param_count(argument, src))
        .collect();
    arities.iter().any(Option::is_some).then_some(arities)
}

pub(super) fn push_ref(
    refs: &mut Vec<RefRecord>,
    kind: &str,
    name: String,
    line: usize,
    ns: Option<String>,
    qualified: Option<String>,
    type_stack: &[String],
) {
    refs.push(RefRecord {
        kind: kind.to_string(),
        name,
        qualified,
        member: None,
        line,
        namespace: ns,
        type_arg_count: None,
        generic: false,
        receiver_type: None,
        arg_count: None,
        receiver_args: None,
        outer_types: type_stack.to_vec(),
        args: None,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
        receiver_base: false,
        receiver_awaited: false,
        receiver_local: false,
        receiver_lambda: None,
        receiver_nullable: false,
        lambda_arg_arity: None,
    });
}

// One 'ctor-param' ref per constructor parameter (see the
// "constructor_declaration" walk arm). Kept separate from push_ref, like
// push_member_ref is, rather than overloading it with a slot every other
// caller would pass as None.
pub(super) fn push_ctor_param_ref(
    refs: &mut Vec<RefRecord>,
    name: String,
    line: usize,
    ns: String,
    args: Option<Vec<String>>,
    type_stack: &[String],
) {
    refs.push(RefRecord {
        kind: "ctor-param".to_string(),
        name,
        qualified: None,
        member: None,
        line,
        namespace: Some(ns),
        type_arg_count: Some(args.as_ref().map_or(0, Vec::len)),
        generic: false,
        receiver_type: None,
        arg_count: None,
        receiver_args: None,
        outer_types: type_stack.to_vec(),
        args,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
        receiver_base: false,
        receiver_awaited: false,
        receiver_local: false,
        receiver_lambda: None,
        receiver_nullable: false,
        lambda_arg_arity: None,
    });
}

// Member access needs the member name alongside the qualifier, which
// push_ref has no slot for -- kept separate rather than overloading it with
// an extra argument every other caller would have to pass as None.
//
// `receiver_type` is appended AFTER `generic`, and only ever
// set for a BARE qualifier -- see the member_access_expression arm for the
// guard. Keeping the guard at the call site (rather than here) is what makes
// the chain-tail hazard structurally impossible: a flattened tail ("x.Foo"
// as the qualifier of ".Bar") is dotted, so it can never be handed a
// receiver fact it did not earn.
//
// `arg_count` is appended AFTER
// `receiver_type` and computed the same way -- at the call site, from the ref's
// OWN node -- for the same reason.
//
// `receiver_args` is appended LAST and travels
// with the receiver FACT -- both halves come out of the same `Fact`, so a
// receiverArgs can never land on a ref that has no receiver.
//
// `outer_types` is appended LAST of all, after `receiver_args` -- see
// push_ref for what it carries.
//
// `receiver_base` is appended LAST of all, after `outer_types` -- `true`
// only for a `base.M` qualifier, `false` for everything else including a
// plain `this.M` qualifier.
//
// `receiver_awaited` travels with the call fact, not with the function's own
// parameters -- it comes out of the SAME `Fact` `receiver_type`/
// `receiver_call_owner` do, read from the fact's own `awaited` bit rather
// than threaded in separately, which is what keeps a call fact and its
// awaited-ness from ever landing on two different refs. Appended LAST of
// all, after `receiver_base`.
//
// `receiver_local`, unlike every field above it, is NOT derivable from
// `receiver: Option<Fact>` -- a `None` receiver means either "no fact
// anywhere for this name" or "a same-named local/parameter is taken but
// nothing vouches for its type", and those two cases must answer this
// field differently. It is threaded in as its own explicit parameter,
// computed by the caller (`resolve_member_qualifier`, which has the
// `Scope` this function does not). Appended LAST of all, after
// `receiver_awaited`.
//
// `receiver_nullable` rides out of the SAME `Fact` `receiver_type`/
// `receiver_args` already came off -- `Fact::nullable`, verbatim -- so it is
// folded into the same three-way match rather than threaded in separately.
//
// `lambda_arg_arity`, unlike every other field here, is not a property of
// the QUALIFIER at all: it belongs to the invocation this ref is the callee
// of, exactly like `arg_count`, and is threaded in as its own parameter for
// the same reason -- computed by the caller from its OWN node, appended
// LAST of all, after `receiver_local`.
pub(super) fn push_member_ref(
    refs: &mut Vec<RefRecord>,
    qualifier_text: &str,
    member: String,
    line: usize,
    ns: String,
    generic: bool,
    receiver: Option<Fact>,
    arg_count: Option<usize>,
    type_stack: &[String],
    property_owner: Option<String>,
    receiver_base: bool,
    receiver_local: bool,
    lambda_arg_arity: Option<Vec<Option<usize>>>,
) {
    // A call fact records the CALLEE it depends on and never a receiver type;
    // a lambda-slot fact records the SLOT this untyped parameter fills on a
    // callee invocation and never either of the other two shapes. The three
    // are mutually exclusive on one ref, which is what lets every reader
    // tell a recorded type, a call lookup, and a lambda-slot lookup apart.
    let (
        receiver_type,
        receiver_args,
        receiver_call_owner,
        receiver_call_member,
        receiver_awaited,
        receiver_lambda,
        receiver_nullable,
    ) = match receiver {
        Some(Fact {
            lambda: Some(slot), ..
        }) => (None, None, None, None, false, Some(slot), false),
        Some(Fact {
            type_name,
            call: Some(member),
            awaited,
            ..
        }) => (
            None,
            None,
            Some(type_name),
            Some(member),
            awaited,
            None,
            false,
        ),
        Some(Fact {
            type_name,
            args,
            call: None,
            nullable,
            ..
        }) => (Some(type_name), args, None, None, false, None, nullable),
        None => (None, None, None, None, false, None, false),
    };
    match qualifier_text.rfind('.') {
        Some(dot) => refs.push(RefRecord {
            kind: "uses-member".to_string(),
            name: qualifier_text[dot + 1..].to_string(),
            qualified: Some(qualifier_text.to_string()),
            member: Some(member),
            line,
            namespace: Some(ns),
            type_arg_count: None,
            generic,
            receiver_type,
            arg_count,
            receiver_args,
            outer_types: type_stack.to_vec(),
            args: None,
            receiver_property_owner: property_owner.clone(),
            receiver_call_owner: receiver_call_owner.clone(),
            receiver_call_member: receiver_call_member.clone(),
            receiver_base,
            receiver_awaited,
            receiver_local,
            receiver_lambda: receiver_lambda.clone(),
            receiver_nullable,
            lambda_arg_arity: lambda_arg_arity.clone(),
        }),
        None => refs.push(RefRecord {
            kind: "uses-member".to_string(),
            name: qualifier_text.to_string(),
            qualified: None,
            member: Some(member),
            line,
            namespace: Some(ns),
            type_arg_count: None,
            generic,
            receiver_type,
            arg_count,
            receiver_args,
            outer_types: type_stack.to_vec(),
            args: None,
            receiver_property_owner: property_owner,
            receiver_call_owner,
            receiver_call_member,
            receiver_base,
            receiver_awaited,
            receiver_local,
            receiver_lambda,
            receiver_nullable,
            lambda_arg_arity,
        }),
    }
}

// Line comes from the type node itself, not the enclosing declaration -- a
// declaration's own span starts at its attribute list when it has one,
// which would otherwise point a reader at the attribute line instead of
// the line the type reference is actually on.
pub(super) fn record_single_type(
    node: Option<Node>,
    kind: &str,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    refs: &mut Vec<RefRecord>,
) {
    let Some(node) = node else {
        return;
    };
    if node.kind() == "tuple_type" {
        for el in named_children(node) {
            if el.kind() != "tuple_element" {
                continue;
            }
            record_single_type(
                el.child_by_field_name("type"),
                kind,
                ns,
                type_stack,
                src,
                refs,
            );
        }
        return;
    }
    let Some(raw) = outer_type_name(node, src) else {
        return;
    };
    let line = node.start_position().row + 1;
    let arity = Some(type_argument_arity(Some(node), src).unwrap_or(0));
    match raw.rfind('.') {
        Some(dot) => push_ref(
            refs,
            kind,
            raw[dot + 1..].to_string(),
            line,
            Some(ns.to_string()),
            Some(raw.clone()),
            type_stack,
        ),
        None => push_ref(
            refs,
            kind,
            raw.clone(),
            line,
            Some(ns.to_string()),
            None,
            type_stack,
        ),
    }
    if let Some(last) = refs.last_mut() {
        last.type_arg_count = arity;
    }
}

// The shape rule for a two-type-argument DI service registration: the
// invoked name begins `Add` or `TryAdd` and ends `Singleton`, `Scoped` or
// `Transient`. A prefix-and-suffix test rather than a fixed name list, so the
// keyed spellings (`AddKeyedSingleton`, `TryAddKeyedScoped`, ...) and the
// generic `TryAdd*` family match with no spelling of their own to maintain.
fn is_registration_method_name(name: &str) -> bool {
    (name.starts_with("Add") || name.starts_with("TryAdd"))
        && (name.ends_with("Singleton") || name.ends_with("Scoped") || name.ends_with("Transient"))
}

// A registration fact for `node` when it is the callee of an invocation
// whose generic method name matches `is_registration_method_name` and whose
// type-argument list carries EXACTLY two arguments -- a
// one-type-argument form (or any other count) yields `None`, recording
// nothing. `node` is a `member_access_expression`'s own node; the caller
// confirms it is actually invoked (`invocation_arg_count(node).is_some()`)
// before calling this, so the shape test here is purely the name-and-arity
// rule.
pub(super) fn registration_fact(node: Node, ns: &str, src: &[u8]) -> Option<RegistrationRecord> {
    let name_node = node.child_by_field_name("name")?;
    if name_node.kind() != "generic_name" {
        return None;
    }
    let ident = named_children(name_node)
        .into_iter()
        .find(|c| c.kind() == "identifier")?;
    if !is_registration_method_name(&text(ident, src)) {
        return None;
    }
    let list = named_children(name_node)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let args = named_children(list);
    if args.len() != 2 {
        return None;
    }
    let service = outer_type_name(args[0], src)?;
    let implementation = outer_type_name(args[1], src)?;
    Some(RegistrationRecord {
        service,
        implementation,
        namespace: ns.to_string(),
        line: node.start_position().row + 1,
    })
}

pub(super) fn record_base_list(
    node: Node,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    refs: &mut Vec<RefRecord>,
) {
    let Some(bl) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return;
    };
    for child in named_children(bl) {
        if child.kind() == "argument_list" {
            continue;
        }
        if child.kind() == "primary_constructor_base_type" {
            record_single_type(
                child.child_by_field_name("type"),
                "inherits",
                ns,
                type_stack,
                src,
                refs,
            );
            continue;
        }
        record_single_type(Some(child), "inherits", ns, type_stack, src, refs);
    }
}
