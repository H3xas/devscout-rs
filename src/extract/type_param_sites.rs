use std::collections::HashSet;

use tree_sitter::Node;

use super::refs::{type_descriptor, type_parameter_names_ordered};
use super::text::{declared_name, named_children, text};
use super::types::{DelegateSignature, OverloadTypeParamSites};

// Per method name, per overload, where the method's own type parameters occur
// in each parameter's type. Walks exactly the declarations and parameters
// `raw_method_params` walks, so every list lines up with that name's
// descriptors overload for overload and parameter for parameter.
pub(super) fn raw_method_type_param_sites(
    node: Node,
    src: &[u8],
) -> Vec<(String, Vec<OverloadTypeParamSites>)> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut pairs: Vec<(String, Vec<OverloadTypeParamSites>)> = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() {
            continue;
        }
        let own = type_parameter_names_ordered(c, src);
        let sites: OverloadTypeParamSites = match c.child_by_field_name("parameters") {
            Some(parameters) => parameter_types(parameters)
                .into_iter()
                .map(|ty| type_param_sites(ty, &own, src))
                .collect(),
            None => Vec::new(),
        };
        match pairs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, overloads)) => overloads.push(sites),
            None => pairs.push((name, vec![sites])),
        }
    }
    pairs
}

// A delegate's return descriptor and, for each of its own type parameters,
// whether it occurs in the parameters, the return, both or neither. Type
// inference reads a lambda's parameter types off the first kind of position
// and fixes type arguments from the second, so the resolver needs to tell
// them apart for a delegate declared in the graph.
pub(super) fn delegate_signature(node: Node, src: &[u8]) -> Option<DelegateSignature> {
    if node.kind() != "delegate_declaration" {
        return None;
    }
    let own = type_parameter_names_ordered(node, src);
    let own_set: HashSet<String> = own.iter().cloned().collect();
    let return_type = node.child_by_field_name("type");
    let params: Vec<Node> = node
        .child_by_field_name("parameters")
        .map(|p| parameter_types(p).into_iter().flatten().collect())
        .unwrap_or_default();
    let type_params = own
        .iter()
        .map(|name| {
            let in_params = params.iter().any(|t| mentions(*t, name, src));
            let in_return = return_type.is_some_and(|t| mentions(t, name, src));
            match (in_params, in_return) {
                (true, true) => "both",
                (true, false) => "params",
                (false, true) => "return",
                (false, false) => "none",
            }
            .to_string()
        })
        .collect();
    Some(DelegateSignature {
        returns: type_descriptor(return_type, src, &own_set),
        type_params,
    })
}

// The type node of each parameter, one entry per position, covering the
// ordinary `parameter` node and the flattened `params` array the grammar
// writes as field-tagged children of the list itself.
fn parameter_types(parameters: Node) -> Vec<Option<Node>> {
    let mut out = Vec::new();
    for i in 0..parameters.child_count() {
        let Some(c) = parameters.child(i) else {
            continue;
        };
        if c.kind() == "parameter" {
            out.push(c.child_by_field_name("type"));
        } else if parameters.field_name_for_child(i) == Some("type") {
            out.push(Some(c));
        }
    }
    out
}

fn type_param_sites(ty: Option<Node>, own: &[String], src: &[u8]) -> Vec<(usize, i64)> {
    let Some(ty) = ty else {
        return Vec::new();
    };
    if own.is_empty() {
        return Vec::new();
    }
    let args = top_level_args(ty, src);
    let mut ids = Vec::new();
    identifiers(ty, &mut ids);
    let mut sites = Vec::new();
    for id in ids {
        let name = text(id, src);
        let Some(k) = own.iter().position(|o| *o == name) else {
            continue;
        };
        let position = args
            .iter()
            .position(|a| a.start_byte() <= id.start_byte() && id.end_byte() <= a.end_byte())
            .map_or(-1, |p| p as i64);
        if !sites.contains(&(k, position)) {
            sites.push((k, position));
        }
    }
    sites
}

// The top-level type arguments of a parameter type, reading through one
// `Expression<...>` wrapper exactly as the resolver's delegate reader does.
fn top_level_args<'a>(ty: Node<'a>, src: &[u8]) -> Vec<Node<'a>> {
    let head = unqualified(ty);
    let head = match generic_parts(head, src) {
        Some((name, args)) if name == "Expression" && args.len() == 1 => unqualified(args[0]),
        _ => head,
    };
    generic_parts(head, src).map_or_else(Vec::new, |(_, args)| args)
}

fn unqualified(mut node: Node) -> Node {
    loop {
        let next = match node.kind() {
            "nullable_type" => node.child_by_field_name("type"),
            "qualified_name" | "alias_qualified_name" => node.child_by_field_name("name"),
            _ => None,
        };
        match next {
            Some(n) => node = n,
            None => return node,
        }
    }
}

fn generic_parts<'a>(node: Node<'a>, src: &[u8]) -> Option<(String, Vec<Node<'a>>)> {
    if node.kind() != "generic_name" {
        return None;
    }
    let children = named_children(node);
    let name = children
        .iter()
        .find(|c| c.kind() == "identifier")
        .map(|id| text(*id, src))?;
    let args = children
        .iter()
        .find(|c| c.kind() == "type_argument_list")
        .map(|list| named_children(*list))
        .unwrap_or_default();
    Some((name, args))
}

fn identifiers<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.kind() == "identifier" {
        out.push(node);
        return;
    }
    for c in named_children(node) {
        identifiers(c, out);
    }
}

fn mentions(ty: Node, name: &str, src: &[u8]) -> bool {
    let mut ids = Vec::new();
    identifiers(ty, &mut ids);
    ids.iter().any(|id| text(*id, src) == name)
}
