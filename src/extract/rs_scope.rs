use std::collections::HashSet;

use tree_sitter::Node;

use super::rs_macro_bind::{is_statement, tree_names};
use crate::parse;

// ---------------------------------------------------------------------------
// Extraction-time scope: the names a Rust function body, a block or a
// generic list binds, which a reference site's first segment must not be
// read through. Locals are flow-insensitive: every name a pattern binds
// anywhere in a function body counts for the whole body, which can drop a
// call but never binds a local to an item. Every walk is on an explicit
// stack, because nesting in a body is unbounded in hostile input.
// ---------------------------------------------------------------------------

/// The node kinds that declare an item, which a block's own items shadow.
const BLOCK_ITEM_KINDS: &[&str] = &[
    "function_item",
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "type_item",
    "const_item",
    "static_item",
    "mod_item",
];

// The field of each node kind that holds a pattern binding names.
fn pattern_field(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "let_declaration" | "parameter" | "for_expression" | "let_condition" => "pattern",
        "match_arm" => "pattern",
        _ => return None,
    })
}

/// Every name the patterns of `node`'s subtree bind: parameters, `let`,
/// closure parameters, `match` arms, `if let`, `while let` and `for`, and
/// any name a statement macro's token trees may bind. A nested function
/// item is its own body and is not entered.
pub(super) fn body_locals(node: Node, src: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "macro_invocation" && is_statement(current) {
            names.extend(tree_names(current, src).outward.values);
        }
        if let Some(field) = pattern_field(current.kind()) {
            if let Some(pattern) = current.child_by_field_name(field) {
                bindings(pattern, src, &mut names);
            }
        }
        if current.kind() == "closure_parameters" {
            let mut cursor = current.walk();
            for child in current.named_children(&mut cursor) {
                if child.kind() != "parameter" {
                    bindings(child, src, &mut names);
                }
            }
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            if child.kind() != "function_item" {
                stack.push(child);
            }
        }
    }
    names
}

/// Adds the names one pattern binds. A lowercase identifier binds; an
/// uppercase one is a unit struct, variant or constant by convention, and a
/// path, a literal or a struct pattern's type binds nothing.
pub(super) fn bindings(pattern: Node, src: &[u8], names: &mut HashSet<String>) {
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" | "shorthand_field_identifier" => {
                let text = parse::node_text(node, src);
                let text = text.strip_prefix("r#").unwrap_or(&text);
                if text.starts_with(|c: char| c.is_lowercase() || c == '_') {
                    names.insert(text.to_string());
                }
            }
            "scoped_identifier" | "range_pattern" | "field_identifier" => {}
            "tuple_struct_pattern" | "struct_pattern" => {
                let ty = node.child_by_field_name("type").map(|t| t.id());
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if Some(child.id()) != ty {
                        stack.push(child);
                    }
                }
            }
            "match_pattern" => {
                let condition = node.child_by_field_name("condition").map(|c| c.id());
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if Some(child.id()) != condition {
                        stack.push(child);
                    }
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    stack.push(child);
                }
            }
        }
    }
}

/// The names a block's own item declarations bind, including any item or
/// import a statement macro of the block may expand to.
pub(super) fn block_items(block: Node, src: &[u8]) -> Vec<String> {
    let mut out = macro_items(block, src);
    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        if BLOCK_ITEM_KINDS.contains(&child.kind()) {
            if let Some(name) = child.child_by_field_name("name") {
                let text = parse::node_text(name, src);
                out.push(text.strip_prefix("r#").unwrap_or(&text).to_string());
            }
        }
    }
    out
}

/// The items and imports the macro invocations standing directly in a block
/// or a module body may expand to. A module's own recorded items are not
/// among them: only what a token tree may declare.
pub(super) fn macro_items(container: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        let invocation = match child.kind() {
            "macro_invocation" => Some(child),
            "expression_statement" => child
                .named_child(0)
                .filter(|c| c.kind() == "macro_invocation"),
            _ => None,
        };
        if let Some(invocation) = invocation {
            let names = tree_names(invocation, src).outward;
            out.extend(names.items);
            out.extend(names.values);
        }
    }
    out
}

/// Inline module scopes whose contents a macro invocation may extend.
/// The empty scope names this file's root module. An `include!` invocation
/// has no identifier argument but can still declare any item.
pub(super) fn macro_scopes(root: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root, String::new())];
    while let Some((container, scope)) = stack.pop() {
        let mut opaque = false;
        let mut cursor = container.walk();
        for child in container.named_children(&mut cursor) {
            let invocation = match child.kind() {
                "macro_invocation" => Some(child),
                "expression_statement" => child
                    .named_child(0)
                    .filter(|c| c.kind() == "macro_invocation"),
                _ => None,
            };
            opaque |= invocation.is_some();
            if child.kind() == "mod_item" {
                if let (Some(name), Some(body)) = (
                    child.child_by_field_name("name"),
                    child.child_by_field_name("body"),
                ) {
                    let text = parse::node_text(name, src);
                    let text = text.strip_prefix("r#").unwrap_or(&text);
                    let nested = if scope.is_empty() {
                        text.to_string()
                    } else {
                        format!("{scope}::{text}")
                    };
                    stack.push((body, nested));
                }
            }
        }
        if opaque {
            out.push(scope);
        }
    }
    out.sort();
    out
}

/// The type and const parameter names an item declares.
pub(super) fn generic_names(item: Node, src: &[u8]) -> Vec<String> {
    let Some(params) = item.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = params.walk();
    for child in params.named_children(&mut cursor) {
        if matches!(child.kind(), "type_parameter" | "const_parameter") {
            if let Some(name) = child.child_by_field_name("name") {
                out.push(parse::node_text(name, src).to_string());
            }
        }
    }
    out
}
