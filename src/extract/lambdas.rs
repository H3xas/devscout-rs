use std::collections::HashSet;

use tree_sitter::Node;

use super::receivers::{qualifier_type_name, FactTable};
use super::text::{named_children, text};
use super::types::{Fact, LambdaSlot};

// One untyped lambda parameter awaiting the second pass: the parameter's own
// name, plus whichever of the two shapes it matched (either or both --
// `collection` and `slot` are independent structural reads of the SAME
// node). `collection` is Unit C's own shape -- a single-parameter lambda
// sitting as the FIRST argument of an invocation on a bare-identifier
// receiver -- carrying that receiver's bare identifier, resolved into an
// element-type fact by `lambda_receiver_element_fact`. `slot` is the
// broader shape -- ANY untyped lambda parameter sitting as an invocation
// argument -- carrying the callee coordinates `lambda_slot_fact`
// turns into a `LambdaSlot`. When both are set, the collection-element
// fact wins and the slot is never consulted (see the deferred-lambda loop
// in `collect_member_facts`).
pub(super) struct DeferredLambdaParam {
    pub(super) name: String,
    pub(super) collection: Option<String>,
    pub(super) slot: Option<PendingSlot>,
}

// One untyped lambda parameter's callee coordinates, read purely
// structurally by `lambda_argument_slot` (never consulting a fact table --
// the qualifier's OWN type is looked up separately, in the second pass,
// once it has settled). `qualifier` is `None` for `this.M(...)` and a bare
// `M(...)` callee, `Some` for `q.M(...)`.
pub(super) struct PendingSlot {
    qualifier: Option<String>,
    member: String,
    arg_count: usize,
    arg_index: usize,
    arity: usize,
    index: usize,
}

// `n` (a `parameter` or `implicit_parameter` node) is the SOLE, UNTYPED
// parameter of a `lambda_expression` sitting as the FIRST argument of an
// `invocation_expression` whose function is a `member_access_expression` --
// the shape `xs.Where(x => ...)` / `xs.Select((x) => ...)` asks for -- and
// its receiver is a bare identifier: returns that receiver node. Purely
// structural, never consulting a fact table (the receiver's OWN type is
// looked up separately, in the second pass, once it has settled). `None`
// for every other shape: an explicitly typed parameter (already handled by
// the ordinary `type_fact` path below), a multi-parameter list, a lambda
// that is not an invocation argument at all, a lambda argument that is not
// the FIRST one, or a receiver that is not a bare identifier.
pub(super) fn lambda_first_arg_receiver(n: Node) -> Option<Node> {
    let lambda = match n.kind() {
        "implicit_parameter" => n.parent()?,
        "parameter" => {
            if n.child_by_field_name("type").is_some() {
                return None;
            }
            let list = n.parent()?;
            if list.kind() != "parameter_list" || named_children(list).len() != 1 {
                return None;
            }
            list.parent()?
        }
        _ => return None,
    };
    if lambda.kind() != "lambda_expression" {
        return None;
    }
    let argument = lambda.parent()?;
    if argument.kind() != "argument" {
        return None;
    }
    let argument_list = argument.parent()?;
    if argument_list.kind() != "argument_list" {
        return None;
    }
    if named_children(argument_list).first()?.id() != argument.id() {
        return None;
    }
    let invocation = argument_list.parent()?;
    if invocation.kind() != "invocation_expression" {
        return None;
    }
    let function = invocation.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    let receiver = function.child_by_field_name("expression")?;
    if receiver.kind() != "identifier" {
        return None;
    }
    Some(receiver)
}

// `n` (a `parameter` or `implicit_parameter` node) is an UNTYPED lambda
// parameter sitting somewhere inside a lambda that is itself an
// `invocation_expression` argument -- the broader shape
// `lambda_first_arg_receiver` above declines whenever the lambda is not the
// sole first argument on a bare-identifier member-access receiver. Purely
// structural, never consulting a fact table: the qualifier's OWN type is
// looked up separately, once it has settled (`lambda_slot_fact`).
//
// The lambda's own parent must be an `argument` WITHOUT a `name` field -- a
// named argument (`Register(configure: x => ...)`) yields `None`, since the
// slot this would name is not necessarily the parameter position a
// resolver's positional delegate lookup expects. The invocation's
// `function` must be a bare `identifier` (a static-looking callee, no
// qualifier) or a `member_access_expression` whose own `expression` is
// either a bare `identifier` (`q.M(...)`) or the `this` keyword
// (`this.M(...)`, qualifier `None`) -- anything else (a `generic_name`
// member or function, a chained `invocation_expression`/
// `member_access_expression` receiver, `base`, a conditional-access
// binding) declines: none of those name a callee this slot can safely
// resolve a positional delegate parameter against.
pub(super) fn lambda_argument_slot(n: Node, src: &[u8]) -> Option<PendingSlot> {
    let (lambda, arity, index) = match n.kind() {
        "implicit_parameter" => (n.parent()?, 1usize, 0usize),
        "parameter" => {
            if n.child_by_field_name("type").is_some() {
                return None;
            }
            let list = n.parent()?;
            if list.kind() != "parameter_list" {
                return None;
            }
            let params = named_children(list);
            let index = params.iter().position(|p| p.id() == n.id())?;
            (list.parent()?, params.len(), index)
        }
        _ => return None,
    };
    if lambda.kind() != "lambda_expression" {
        return None;
    }
    let argument = lambda.parent()?;
    if argument.kind() != "argument" || argument.child_by_field_name("name").is_some() {
        return None;
    }
    let argument_list = argument.parent()?;
    if argument_list.kind() != "argument_list" {
        return None;
    }
    let args = named_children(argument_list);
    let arg_index = args.iter().position(|a| a.id() == argument.id())?;
    let arg_count = args.len();
    let invocation = argument_list.parent()?;
    if invocation.kind() != "invocation_expression" {
        return None;
    }
    let function = invocation.child_by_field_name("function")?;
    let (qualifier, member) = match function.kind() {
        "member_access_expression" => {
            let expr = function.child_by_field_name("expression")?;
            let qualifier = match expr.kind() {
                "identifier" => Some(text(expr, src)),
                "this" => None,
                _ => return None,
            };
            let name_node = function.child_by_field_name("name")?;
            if name_node.kind() != "identifier" {
                return None;
            }
            (qualifier, text(name_node, src))
        }
        "identifier" => (None, text(function, src)),
        _ => return None,
    };
    if member.is_empty() {
        return None;
    }
    Some(PendingSlot {
        qualifier,
        member,
        arg_count,
        arg_index,
        arity,
        index,
    })
}

// The settled `LambdaSlot` fact for one `PendingSlot`, once every
// declaration in the member has settled: `owner` is the callee's type, read
// the same three-way way `qualifier_type_name` reads `Q` in `var x =
// Q.M()` (an in-file fact's type, the bare name itself when nothing claims
// it -- the static-class shape -- or refused when something claims the
// name but vouches for no type), except when the qualifier is itself one of
// `pending`'s own deferred lambda parameter names, refused rather than read
// from a half-settled table (one hop, never a chain, same as the
// deferred-call loop's own qualifier refusal). A `None` qualifier (a bare
// `M(...)` or `this.M(...)` callee) reads `owner` off `enclosing_type`
// directly instead.
pub(super) fn lambda_slot_fact(
    table: &FactTable,
    type_facts: &FactTable,
    enclosing_type: Option<&str>,
    pending: &HashSet<&str>,
    slot: &PendingSlot,
) -> Option<Fact> {
    let owner = match &slot.qualifier {
        None => enclosing_type?.to_string(),
        Some(q) => {
            if pending.contains(q.as_str()) {
                return None;
            }
            qualifier_type_name(table, type_facts, q)?
        }
    };
    Some(Fact {
        type_name: String::new(),
        args: None,
        call: None,
        awaited: false,
        is_array: false,
        lambda: Some(LambdaSlot {
            owner,
            member: slot.member.clone(),
            arg_count: slot.arg_count,
            arg_index: slot.arg_index,
            arity: slot.arity,
            index: slot.index,
        }),
    })
}

// The element fact of a `xs.Where(x => ...)`-shaped lambda parameter's
// COLLECTION receiver -- read the same two tables `collection_element_fact`
// reads, over the SAME two shapes Unit C asks for, kept as its own function
// rather than folded into `collection_element_fact` because the two rules
// disagree on array handling (`foreach` never unwraps one, this rule
// always does) and must never be conflated:
//   - `T[]` (an array): `base_type_identifier`/`generic_arg_descriptors`
//     already collapse the wrapper away at fact-recording time, so the
//     ONLY signal left that a `Widget` fact denotes `Widget[]` rather than
//     one `Widget` is `Fact::is_array` (see its own doc comment) --
//     `args` is empty either way, since an array's OWN top-level generic
//     name (if its element is itself generic, `List<Order>[]`) is a
//     different, THIRD shape this rule declines rather than guesses at.
//   - a generic type with EXACTLY one top-level type argument (`List<T>`,
//     `IEnumerable<T>`, ...): the non-array branch below, identical to
//     `collection_element_fact`'s own single-argument case. A two-argument
//     generic (`Dictionary<K,V>`) and a wildcard argument (the enclosing
//     declaration's own type parameter, nothing at this call site knows
//     what it is bound to) both decline, same as `collection_element_fact`.
// A call-shaped fact (`var xs = Q.GetItems();`, still unresolved at
// extraction time -- the callee's return type lives in another file) is
// never read as either shape: `fact.args` is always `None` for a call fact
// by construction (`invocation_call`'s own Fact literal never sets it), so
// it already falls through the generic-argument arm to `None`, and
// `is_array` is always `false` for one too (never set by anything but
// `type_fact`) -- both refusals happen for free, no explicit check needed.
// A lambda-slot fact (an untyped lambda parameter whose type the resolver
// reads off its callee) lands in the same table with the same `args: None`
// and `is_array: false`, so it falls through the same way.
pub(super) fn lambda_receiver_element_fact(
    locals: &FactTable,
    type_facts: &FactTable,
    name: &str,
) -> Option<Fact> {
    let fact = match locals.get(name).or_else(|| type_facts.get(name)) {
        Some(Some(fact)) => fact,
        _ => return None,
    };
    if fact.is_array {
        return if fact.args.is_none() {
            Some(Fact {
                type_name: fact.type_name.clone(),
                args: None,
                call: None,
                awaited: false,
                is_array: false,
                lambda: None,
            })
        } else {
            None
        };
    }
    match fact.args.as_deref() {
        Some([arg]) if arg != "*" => Some(Fact {
            type_name: arg.clone(),
            args: None,
            call: None,
            awaited: false,
            is_array: false,
            lambda: None,
        }),
        _ => None,
    }
}
