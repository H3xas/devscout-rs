// ---------------------------------------------------------------------------
// Message-bus publish-site facts, and the nested base type-argument fact a
// generic consumer base (`IConsumer<Batch<T>>`) needs so its inner type
// argument survives extraction. Facts only: no edge, edge kind, traversal or
// query code lives here -- see extract.rs's module doc for the extraction
// pipeline this slots into.
// ---------------------------------------------------------------------------

use std::collections::HashSet;

use tree_sitter::Node;

use super::qualifiers::{member_name_text, resolve_member_qualifier, Scope};
use super::refs::{
    base_type_identifier, invocation_arg_count, type_descriptor, type_parameter_names,
};
use super::text::named_children;
use super::types::{EnclosingCallFact, Fact, PublishRecord};

// Verbs the local bus/mediator/scheduler stand-ins expose for handing a
// message instance to another dispatch, checked on the invoked method's
// bare name only -- the same shape-only rule `registration_fact` (refs.rs)
// already applies to a DI registration call. Neither this list nor the rest
// of this module validates the receiver's own declared type: a call this
// shape-only test wrongly accepts simply earns no edge at the resolve
// layer, which needs a registered consumer before it builds one at all.
const PUBLISH_VERBS: &[&str] = &["Publish", "PublishAsync", "SubmitJob", "Reply", "Send"];

/// A publish-site fact for `function` when it is the callee of an
/// invocation this module can read a message off. `function` must already
/// be confirmed as an invocation's own callee (the same
/// `invocation_arg_count` gate `registration_fact` sits behind); this never
/// re-walks that check itself.
///
/// Which verbs count is NOT settled here. A message named by the call
/// itself -- a generic type argument, or a type constructed as its first
/// argument -- is recorded whatever the method is called, because a
/// repository's own forwarding wrapper carries a name this engine cannot
/// know in advance and the resolver is what decides which names its
/// vocabulary ended up holding. The remaining shape, a plain identifier
/// argument, stays gated on `PUBLISH_VERBS`: it is by far the most common
/// call shape in any codebase, and recording every one of them would cost
/// fragment bytes out of all proportion to the wrappers it would find.
pub(super) fn publish_fact(
    function: Node,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    scope: &Scope,
) -> Option<PublishRecord> {
    let name_node = function.child_by_field_name("name")?;
    let verb = member_name_text(Some(name_node), src)?;
    let known_verb = PUBLISH_VERBS.contains(&verb.as_str());
    let message = match named_message(function, name_node, src, &scope.type_params) {
        Some(message) => message,
        None if known_verb => identifier_message(function, src, type_stack, scope)?,
        None => return None,
    };
    // A message that is the enclosing METHOD's own type parameter is not a
    // message this call could name: the method is handing its caller's
    // message on. `scope.type_params` carries the enclosing TYPE's
    // parameters, so a method-level one renders as its own name here and
    // has to be checked against the method that declared it. Worth
    // recording only when what it hands off to is already a publish, which
    // is what makes the enclosing method a wrapper for one.
    let enclosing = enclosing_method(function, src);
    let pass_through = message == "*"
        || enclosing
            .as_ref()
            .is_some_and(|(_, params)| params.contains(&message));
    let enclosing_method = if pass_through {
        if !known_verb {
            return None;
        }
        Some(enclosing?.0)
    } else {
        None
    };
    // A pass-through names no type, and spelling the type parameter would
    // leave a message text that could collide with a real type of that name.
    let message = if pass_through {
        "*".to_string()
    } else {
        message
    };
    Some(PublishRecord {
        verb,
        message,
        namespace: ns.to_string(),
        line: function.start_position().row + 1,
        outer_types: type_stack.to_vec(),
        enclosing_method,
        arg_count: invocation_arg_count(function).unwrap_or(0),
        enclosing_call: enclosing_call_through_lambda(function, src),
    })
}

// The member name a callee expression's own invoked-name node spells,
// covering every callee shape an invocation's `function` field can take
// that carries a name at all (`member_access_expression`/`qualified_name`'s
// own `name` field, or a bare `identifier`/`generic_name` callee) -- the
// same normalization `member_name_text` already applies to a `generic_name`.
fn callee_bare_name(function: Node, src: &[u8]) -> Option<String> {
    match function.kind() {
        "member_access_expression" | "qualified_name" => {
            member_name_text(function.child_by_field_name("name"), src)
        }
        "generic_name" | "identifier" => member_name_text(Some(function), src),
        _ => None,
    }
}

// Whether `function`'s own invocation sits inside a lambda body that is
// itself an argument of an ENCLOSING invocation -- distinct from
// `find_construction`'s lambda descent, which walks INTO a lambda argument
// of the recognized call itself; this walks UP from `function` to find an
// enclosing `lambda_expression`/`anonymous_method_expression` ancestor, then
// one level further up to that lambda's own parent invocation. Plain
// node-kind matching on kinds the grammar already exposes -- no new
// tree-sitter query.
fn enclosing_call_through_lambda(function: Node, src: &[u8]) -> Option<EnclosingCallFact> {
    let mut node = function.parent();
    let mut lambda = None;
    while let Some(current) = node {
        if current.kind() == "lambda_expression" || current.kind() == "anonymous_method_expression"
        {
            lambda = Some(current);
            break;
        }
        node = current.parent();
    }
    let lambda = lambda?;
    let mut node = lambda.parent();
    let argument = loop {
        let current = node?;
        if current.kind() == "argument" {
            break current;
        }
        node = current.parent();
    };
    let argument_list = argument.parent().filter(|p| p.kind() == "argument_list")?;
    let invocation = argument_list
        .parent()
        .filter(|p| p.kind() == "invocation_expression")?;
    let outer_function = invocation.child_by_field_name("function")?;
    let verb = callee_bare_name(outer_function, src)?;
    let args = named_children(argument_list);
    let arg_position = args.iter().position(|a| a.id() == argument.id())?;
    Some(EnclosingCallFact {
        verb,
        arg_position,
        arg_count: args.len(),
    })
}

// The message the call NAMES itself: its own type argument, or a type
// constructed as its first argument. Both are written at the call site, so
// neither depends on the method being one this engine already knows.
fn named_message(
    function: Node,
    name_node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<String> {
    generic_argument_message(name_node, src, type_params)
        .or_else(|| construction_message(function, src, type_params))
}

// The nearest enclosing method declaration's own name and type-parameter
// names. A publish sitting in a property accessor, a constructor or a lambda
// that is not inside a method has no wrapper name to offer and answers
// `None`.
fn enclosing_method(function: Node, src: &[u8]) -> Option<(String, HashSet<String>)> {
    let mut node = function.parent();
    while let Some(current) = node {
        if current.kind() == "method_declaration" {
            let name = member_name_text(current.child_by_field_name("name"), src)?;
            return Some((name, type_parameter_names(current, src)));
        }
        node = current.parent();
    }
    None
}

// The call's own type argument (`bus.PublishAsync<T>(...)`) -- the message
// every verb this module recognizes carries at most one of. `name_node` is
// the invoked member's own name node, a `generic_name` only when the call
// spelled an explicit type-argument list.
fn generic_argument_message(
    name_node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<String> {
    if name_node.kind() != "generic_name" {
        return None;
    }
    let list = named_children(name_node)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let first = named_children(list).into_iter().next()?;
    Some(type_descriptor(Some(first), src, type_params))
}

// The type a call's FIRST argument constructs -- directly, or nested
// inside a lambda argument (a saga initialiser's `context =>
// context.Init(new T())` shape). Written at the call site, so this is
// readable without knowing what the method is. Covers an explicit array
// creation (`new M[n]`, `new M[] { ... }`) the same way as a plain
// `new M(...)`: `array_creation_expression`'s own `type` field is already
// an `array_type` node, and `type_descriptor` already renders that as
// `M[]`. An implicitly typed array creation (`new[] { ... }`) has no `type`
// field to read at all, so it is answered separately, off its elements.
fn construction_message(
    function: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<String> {
    let call = function.parent()?;
    let args = call.child_by_field_name("arguments")?;
    let first_arg = named_children(args).into_iter().next()?;
    let value = argument_value(first_arg)?;
    match find_construction(value)? {
        Construction::Typed(ctor) => Some(type_descriptor(
            ctor.child_by_field_name("type"),
            src,
            type_params,
        )),
        Construction::ImplicitArray(node) => implicit_array_message(node, src, type_params),
    }
}

// A plain identifier argument's own declared type, read through the SAME
// scope machinery an ordinary member-access qualifier resolves against --
// never a name merely adjacent in the source, which is what keeps a
// same-line decoy from ever supplying this answer.
fn identifier_message(
    function: Node,
    src: &[u8],
    type_stack: &[String],
    scope: &Scope,
) -> Option<String> {
    let call = function.parent()?;
    let args = call.child_by_field_name("arguments")?;
    let first_arg = named_children(args).into_iter().next()?;
    let value = argument_value(first_arg)?;
    if value.kind() != "identifier" {
        return None;
    }
    let resolution = resolve_member_qualifier(Some(value), src, type_stack, scope)?;
    let fact = resolution.receiver?;
    // A call-shaped or lambda-slot-shaped fact needs another resolve hop
    // this layer never takes; only a genuinely typed fact yields a message.
    if fact.call.is_some() || fact.lambda.is_some() {
        return None;
    }
    let descriptor = fact_descriptor(&fact);
    // `fact.is_array` is the one bit `resolve_member_qualifier`'s own
    // shared `type_fact` still carries about a declared array-typed local
    // or parameter -- `fact.type_name`/`fact.args` already look THROUGH the
    // array wrapper to its element, the same unwrapping every other shared
    // fact in this engine performs, so this is the only place left that can
    // still tell a plain message from an array of it.
    if fact.is_array {
        Some(format!("{descriptor}[]"))
    } else {
        Some(descriptor)
    }
}

// An `argument` node's own value: its single unnamed child, or (for a
// named argument, `f(name: value)`) whichever child is not the `name`
// field.
fn argument_value(arg: Node) -> Option<Node> {
    let name_field = arg.child_by_field_name("name").map(|n| n.id());
    named_children(arg)
        .into_iter()
        .find(|c| Some(c.id()) != name_field)
}

// The one construction `find_construction` found: either a node
// `type_descriptor` can read a `type` field off directly (`Typed`, an
// `object_creation_expression` or an `array_creation_expression` -- the
// latter's own `type` field is already an `array_type`, so no separate
// case is needed there), or an `implicit_array_creation_expression`, which
// carries no `type` field at all and is answered off its own elements
// instead (see `implicit_array_message`).
enum Construction<'a> {
    Typed(Node<'a>),
    ImplicitArray(Node<'a>),
}

// The first construction in `node`'s own subtree, visited pre-order:
// `node` itself first, then its children left to right. A direct argument
// (`new T(...)`, `new T[n]`, `new T[] { ... }`, `new[] { ... }`) is found
// immediately; a saga initialiser's lambda argument is searched INTO,
// since its message is constructed inside the lambda body rather than at
// the argument itself. Neither array shape is itself descended into: a
// construction INSIDE an array creation is one of the array's own
// elements, never the message the array creation itself names.
fn find_construction(node: Node) -> Option<Construction> {
    match node.kind() {
        "object_creation_expression" | "array_creation_expression" => {
            Some(Construction::Typed(node))
        }
        "implicit_array_creation_expression" => Some(Construction::ImplicitArray(node)),
        _ => {
            for child in named_children(node) {
                if let Some(found) = find_construction(child) {
                    return Some(found);
                }
            }
            None
        }
    }
}

// An implicitly typed array creation's own message: every element of its
// initializer must be a construction of the SAME type `M`, read
// structurally off each `object_creation_expression`'s own `type` field --
// never off the first element alone, which would name the wrong element
// type for a mixed initializer. An empty initializer, or one holding
// anything other than a construction, names no message.
fn implicit_array_message(node: Node, src: &[u8], type_params: &HashSet<String>) -> Option<String> {
    let initializer = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "initializer_expression")?;
    let elements = named_children(initializer);
    if elements.is_empty() {
        return None;
    }
    let mut element_type: Option<String> = None;
    for element in elements {
        if element.kind() != "object_creation_expression" {
            return None;
        }
        let descriptor = type_descriptor(element.child_by_field_name("type"), src, type_params);
        match &element_type {
            None => element_type = Some(descriptor),
            Some(existing) if *existing == descriptor => {}
            Some(_) => return None,
        }
    }
    element_type.map(|t| format!("{t}[]"))
}

// A resolved receiver fact's own descriptor, matching `type_descriptor`'s
// generic-name rendering (`Name<Arg1,Arg2>`) when the fact carried
// top-level type arguments.
fn fact_descriptor(fact: &Fact) -> String {
    match &fact.args {
        Some(args) if !args.is_empty() => format!("{}<{}>", fact.type_name, args.join(",")),
        _ => fact.type_name.clone(),
    }
}

// The generic_name a type node bottoms out at -- the same unwrapping path
// `base_type_identifier` (refs.rs) takes (nullable/array wrappers, a
// qualified name's own tail), reimplemented here since that primitive is
// private to refs.rs.
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

/// Per base name in `node`'s own base list that carries at least one
/// top-level type argument which is ITSELF generic, that argument list
/// rendered with `type_descriptor` -- nested structure intact -- rather
/// than the flattened bare identifiers `raw_base_generic_args` (members.rs)
/// already carries. A base whose arguments are all non-generic contributes
/// no entry: the flattened form already carries the same information for
/// that case, so recording it twice would add no fact.
pub(super) fn nested_base_type_args(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<(String, Vec<String>)> {
    let Some(base_list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for child in named_children(base_list) {
        if child.kind() == "argument_list" {
            continue;
        }
        let type_node = if child.kind() == "primary_constructor_base_type" {
            child.child_by_field_name("type")
        } else {
            Some(child)
        };
        let Some(name) = base_type_identifier(type_node, src, false) else {
            continue;
        };
        if out.iter().any(|(k, _)| k == &name) {
            continue;
        }
        let Some(generic) = top_level_generic_name(type_node) else {
            continue;
        };
        let Some(list) = named_children(generic)
            .into_iter()
            .find(|c| c.kind() == "type_argument_list")
        else {
            continue;
        };
        let args = named_children(list);
        if args.is_empty() {
            continue;
        }
        let nested = args
            .iter()
            .any(|a| top_level_generic_name(Some(*a)).is_some());
        if !nested {
            continue;
        }
        let descriptors = args
            .iter()
            .map(|a| type_descriptor(Some(*a), src, type_params))
            .collect();
        out.push((name, descriptors));
    }
    out
}

/// Base names in `node`'s own base list whose message-position argument --
/// `base_generic_args`' own position 0 -- is an array type
/// (`IConsumer<M[]>`). `base_generic_args`/`generic_arg_descriptors`
/// (members.rs/refs.rs) both look THROUGH an `array_type` to its bare
/// element name, the same unwrapping `base_type_identifier` performs
/// everywhere else in this engine, so the flattened fact alone cannot tell
/// `IConsumer<M>` from `IConsumer<M[]>` apart. This is the one place left
/// that still reads the base list's own type-argument NODE rather than its
/// already-unwrapped text, purely to recover that one bit. A base already
/// carrying a NESTED wrapper argument (`IConsumer<Batch<M[]>>`) needs no
/// entry here: `nested_base_type_args` renders that position with
/// `type_descriptor`, which never drops the array suffix in the first
/// place.
pub(super) fn array_message_bases(
    node: Node,
    src: &[u8],
    _type_params: &HashSet<String>,
) -> Vec<String> {
    let Some(base_list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for child in named_children(base_list) {
        if child.kind() == "argument_list" {
            continue;
        }
        let type_node = if child.kind() == "primary_constructor_base_type" {
            child.child_by_field_name("type")
        } else {
            Some(child)
        };
        let Some(name) = base_type_identifier(type_node, src, false) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(generic) = top_level_generic_name(type_node) else {
            continue;
        };
        let Some(list) = named_children(generic)
            .into_iter()
            .find(|c| c.kind() == "type_argument_list")
        else {
            continue;
        };
        let Some(first) = named_children(list).into_iter().next() else {
            continue;
        };
        if first.kind() == "array_type" {
            out.push(name);
        }
    }
    out
}
