use std::collections::HashSet;

use tree_sitter::Node;

use super::refs::{base_type_identifier, type_descriptor, type_parameter_names};
use super::text::{declared_name, is_explicit_implementation, named_children, text};
use super::types::INTERPOLATED_HANDLER_MARK;

// Additional methodParams entries support argument-type proof without changing
// the existing public method, return, property, or field projections.
pub(super) fn append_argument_member_types(
    node: Node,
    body: Node,
    src: &[u8],
    type_params: &HashSet<String>,
    pairs: &mut Vec<(String, Vec<Vec<String>>)>,
) {
    append_declaration_marks(node, src, pairs);
    if named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
        .is_some_and(|list| {
            named_children(list).into_iter().any(|param| {
                let mut cursor = param.walk();
                let variant = param
                    .children(&mut cursor)
                    .any(|token| matches!(token.kind(), "in" | "out"));
                variant
            })
        })
    {
        pairs.push(("@variant".to_string(), vec![Vec::new()]));
    }
    for c in named_children(body) {
        if is_implicit_conversion(c) {
            pairs.push(("@implicit-conversion".to_string(), vec![Vec::new()]));
        }
        let mut params = type_params.clone();
        params.extend(type_parameter_names(c, src));
        let entry = match c.kind() {
            "method_declaration" if !is_explicit_implementation(c) => {
                let ty = c.child_by_field_name("returns");
                let descriptor = type_descriptor(ty, src, &params);
                (!matches!(descriptor.as_str(), "?" | "void")
                    && (base_type_identifier(ty, src, false).is_none() || descriptor.contains('*')))
                .then(|| (format!("@result:{}", declared_name(c, src)), descriptor))
            }
            "property_declaration" if !is_explicit_implementation(c) => {
                let ty = c.child_by_field_name("type");
                let descriptor = type_descriptor(ty, src, &params);
                (descriptor != "?"
                    && (base_type_identifier(ty, src, false).is_none() || descriptor.contains('*')))
                .then(|| (format!("@property:{}", declared_name(c, src)), descriptor))
            }
            _ => None,
        };
        if let Some((name, descriptor)) = entry {
            match pairs.iter_mut().find(|(key, _)| *key == name) {
                Some((_, values)) => values.push(vec![descriptor]),
                None => pairs.push((name, vec![vec![descriptor]])),
            }
        }
        if c.kind() == "field_declaration" {
            let Some(decl) = named_children(c)
                .into_iter()
                .find(|n| n.kind() == "variable_declaration")
            else {
                continue;
            };
            let ty = decl.child_by_field_name("type");
            let descriptor = type_descriptor(ty, src, &params);
            if descriptor == "?"
                || (base_type_identifier(ty, src, false).is_some() && !descriptor.contains('*'))
            {
                continue;
            }
            for var in named_children(decl)
                .into_iter()
                .filter(|n| n.kind() == "variable_declarator")
            {
                if let Some(name) = var.child_by_field_name("name").map(|n| text(n, src)) {
                    pairs.push((format!("@field:{name}"), vec![vec![descriptor.clone()]]));
                }
            }
        }
    }
}

// The marks a type declaration's own attributes contribute. A declaration
// without a body -- a positional record written `record X(int V);` -- still
// carries its attributes, so these are read by the caller before it looks for
// a body at all.
pub(super) fn append_declaration_marks(
    node: Node,
    src: &[u8],
    pairs: &mut Vec<(String, Vec<Vec<String>>)>,
) {
    if declares_handler(node, src) {
        pairs.push((INTERPOLATED_HANDLER_MARK.to_string(), vec![Vec::new()]));
    }
}

fn is_implicit_conversion(node: Node) -> bool {
    if node.kind() != "conversion_operator_declaration" {
        return false;
    }
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|token| token.kind() == "implicit");
    found
}

const HANDLER_ATTRIBUTE: &str = "InterpolatedStringHandler";

// C# binds an interpolated string only to a parameter whose type implements
// the handler pattern, and the attribute is the one on-graph evidence of it.
// The declaration kinds are the ones that can hold those members; an
// attribute spelled through a using alias is not resolved here, so it stays
// unmarked and keeps the plain string rule.
fn declares_handler(node: Node, src: &[u8]) -> bool {
    matches!(
        node.kind(),
        "class_declaration" | "struct_declaration" | "record_declaration"
    ) && super::members::attribute_names(node, src).contains(HANDLER_ATTRIBUTE)
}
