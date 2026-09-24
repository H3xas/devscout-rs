// ---------------------------------------------------------------------------
// Facts that let a repository teach the resolver its OWN message-handling
// vocabulary, rather than the resolver matching a fixed list of framework
// type names it ships. Two shapes live here:
//
//   * a one-type-argument registration call, which names a type the
//     repository itself treats as a handler,
//   * a property whose declared type wraps exactly one type argument, which
//     is how a long-running handler declares the further messages it binds.
//
// Facts only: neither shape decides anything on its own. The resolver keeps
// a registration whose named type actually carries a generic base, and keeps
// a property argument only on a type it already holds to be a handler, so a
// permissive rule here costs fragment bytes rather than edges.
// ---------------------------------------------------------------------------

use tree_sitter::Node;

use super::qualifiers::Scope;
use super::refs::type_descriptor;
use super::text::{named_children, text};
use super::types::HandlerRegistrationRecord;
use std::collections::HashSet;

// A registration call hands a container a type it does not otherwise use, so
// its own name carries the only signal available before resolution. Both
// halves below are role words rather than vendor names: a prefix that says
// something is being installed, or a suffix naming the role the installed
// type plays. A name matching neither cannot be a registration, and a name
// matching either still has to survive the resolver's own check that the
// type it names carries a generic base.
const INSTALL_PREFIXES: &[&str] = &["Add", "Register", "Use", "Map", "Subscribe"];
const ROLE_SUFFIXES: &[&str] = &[
    "Consumer",
    "Handler",
    "Processor",
    "Worker",
    "Saga",
    "Subscriber",
    "Listener",
];

fn is_installation_name(name: &str) -> bool {
    INSTALL_PREFIXES.iter().any(|p| name.starts_with(p))
        || ROLE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// A handler-registration fact for `function` when it is the callee of an
/// invocation carrying EXACTLY ONE type argument whose own name reads as an
/// installation. One argument is the whole discriminator against the
/// two-argument DI registration `registration_fact` (refs.rs) already
/// records: that shape pairs a service with an implementation, this one
/// names a single type and lets the container work out the rest.
///
/// `function` must already be confirmed as an invocation's own callee, the
/// same gate `registration_fact` sits behind.
pub(super) fn handler_registration_fact(
    function: Node,
    ns: &str,
    src: &[u8],
    scope: &Scope,
) -> Option<HandlerRegistrationRecord> {
    let name_node = function.child_by_field_name("name")?;
    if name_node.kind() != "generic_name" {
        return None;
    }
    let ident = named_children(name_node)
        .into_iter()
        .find(|c| c.kind() == "identifier")?;
    if !is_installation_name(&text(ident, src)) {
        return None;
    }
    let list = named_children(name_node)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let args = named_children(list);
    let [only] = args.as_slice() else {
        return None;
    };
    let handler = type_descriptor(Some(*only), src, &scope.type_params);
    // A type parameter names no concrete handler at this call, and a
    // descriptor the renderer could not read names none either.
    if handler == "*" || handler == "?" {
        return None;
    }
    Some(HandlerRegistrationRecord {
        handler,
        namespace: ns.to_string(),
        line: function.start_position().row + 1,
    })
}

/// Every distinct single type argument the properties declared DIRECTLY in
/// `node`'s own body wrap, in declaration order. A property whose type
/// carries no type argument, or more than one, contributes nothing: a
/// single-argument wrapper is the only shape whose message position is
/// unambiguous without naming the wrapper itself.
///
/// This is recorded for every type, not only a handler: which types are
/// handlers is not known until the whole corpus has been indexed, and the
/// resolver discards the rest.
pub(super) fn property_message_args(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<String> {
    let Some(body) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "declaration_list")
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for member in named_children(body) {
        if member.kind() != "property_declaration" {
            continue;
        }
        let Some(arg) = single_type_argument(member.child_by_field_name("type"), src, type_params)
        else {
            continue;
        };
        if seen.insert(arg.clone()) {
            out.push(arg);
        }
    }
    out
}

// The sole type argument a declared type wraps, rendered with the same
// descriptor the rest of extraction uses. `None` unless the type is a
// generic name carrying exactly one argument, and `None` when that argument
// is an unbound type parameter, which names no message at this declaration.
fn single_type_argument(
    type_node: Option<Node>,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<String> {
    let node = type_node?;
    if node.kind() != "generic_name" {
        return None;
    }
    let list = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let args = named_children(list);
    let [only] = args.as_slice() else {
        return None;
    };
    let descriptor = type_descriptor(Some(*only), src, type_params);
    if descriptor == "*" || descriptor == "?" {
        return None;
    }
    Some(descriptor)
}
