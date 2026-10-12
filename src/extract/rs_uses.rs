use std::collections::HashMap;

use tree_sitter::Node;

use super::rs_facts_types::{RsBlock, RsUse};
use super::rs_paths::{path_segments, Segment};

// ---------------------------------------------------------------------------
// `use` declarations, flattened into one leaf per bound name, and the blocks
// that hold them.
//
// The walk visits module-level code and every function body, keeping the
// inline-module scope and the innermost recorded block. A block is recorded
// when a `use` declaration is its direct child; `block_ids` maps each
// recorded block node to its index so the reference-site walk can place its
// sites in the same blocks. Token trees, macro definitions and attributes
// are never entered, and neither is an inline module inside a function body:
// no recorded item lives there.
// ---------------------------------------------------------------------------

/// The `use` facts of one file.
pub(super) struct UseFacts {
    pub uses: Vec<RsUse>,
    pub blocks: Vec<RsBlock>,
    pub block_ids: HashMap<usize, usize>,
}

struct Frame<'t> {
    node: Node<'t>,
    scope: String,
    block: Option<usize>,
    in_body: bool,
}

/// Collects every `use` leaf and every block that directly holds one.
pub(super) fn collect_uses(root: Node, src: &[u8]) -> UseFacts {
    let mut facts = UseFacts {
        uses: Vec::new(),
        blocks: Vec::new(),
        block_ids: HashMap::new(),
    };
    let mut stack = vec![Frame {
        node: root,
        scope: String::new(),
        block: None,
        in_body: false,
    }];
    while let Some(frame) = stack.pop() {
        let node = frame.node;
        match node.kind() {
            "use_declaration" => {
                if let Some(arg) = node.child_by_field_name("argument") {
                    push_leaves(arg, src, &frame.scope, frame.block, &mut facts.uses);
                }
                continue;
            }
            "token_tree" | "macro_definition" | "attribute_item" | "inner_attribute_item" => {
                continue
            }
            _ => {}
        }
        let (scope, block, in_body) = child_context(&frame, src, &mut facts);
        if node.kind() == "mod_item" && frame.in_body {
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(Frame {
                node: child,
                scope: scope.clone(),
                block,
                in_body,
            });
        }
    }
    facts
}

// The scope, block and body flag a node's children inherit.
fn child_context(frame: &Frame, src: &[u8], facts: &mut UseFacts) -> (String, Option<usize>, bool) {
    let node = frame.node;
    match node.kind() {
        "mod_item" if !frame.in_body => {
            let name = node
                .child_by_field_name("name")
                .map(|n| crate::parse::node_text(n, src))
                .unwrap_or_default();
            let name = name.strip_prefix("r#").unwrap_or(&name).to_string();
            let scope = if frame.scope.is_empty() {
                name
            } else {
                format!("{}::{name}", frame.scope)
            };
            (scope, frame.block, false)
        }
        "block" if holds_use(node) => {
            let index = facts.blocks.len();
            facts.blocks.push(RsBlock {
                parent: frame.block,
            });
            facts.block_ids.insert(node.id(), index);
            (frame.scope.clone(), Some(index), true)
        }
        "function_item" | "closure_expression" | "const_item" | "static_item" | "block" => {
            (frame.scope.clone(), frame.block, true)
        }
        _ => (frame.scope.clone(), frame.block, frame.in_body),
    }
}

fn holds_use(block: Node) -> bool {
    let mut cursor = block.walk();
    let found = block
        .named_children(&mut cursor)
        .any(|c| c.kind() == "use_declaration");
    found
}

// Flattens one `use` tree, on an explicit stack: groups nest without bound.
fn push_leaves(arg: Node, src: &[u8], scope: &str, block: Option<usize>, out: &mut Vec<RsUse>) {
    let mut stack: Vec<(Node, Vec<Segment>)> = vec![(arg, Vec::new())];
    while let Some((node, prefix)) = stack.pop() {
        match node.kind() {
            "use_list" => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.named_children(&mut cursor).collect();
                for child in children.into_iter().rev() {
                    stack.push((child, prefix.clone()));
                }
            }
            "scoped_use_list" => {
                let mut inner = prefix.clone();
                if let Some(path) = node.child_by_field_name("path") {
                    match path_segments(path, src) {
                        Some(segs) => inner.extend(segs),
                        None => continue,
                    }
                }
                if let Some(list) = node.child_by_field_name("list") {
                    stack.push((list, inner));
                }
            }
            "use_wildcard" => {
                let mut path = prefix.clone();
                if let Some(target) = node.named_child(0) {
                    match path_segments(target, src) {
                        Some(segs) => path.extend(segs),
                        None => continue,
                    }
                }
                let line = node.end_position().row + 1;
                push_leaf(out, scope, block, &path, "*", line);
            }
            "use_as_clause" => as_clause_leaf(node, src, &prefix, scope, block, out),
            _ => plain_leaf(node, src, &prefix, scope, block, out),
        }
    }
}

fn as_clause_leaf(
    node: Node,
    src: &[u8],
    prefix: &[Segment],
    scope: &str,
    block: Option<usize>,
    out: &mut Vec<RsUse>,
) {
    let (Some(path), Some(alias)) = (
        node.child_by_field_name("path"),
        node.child_by_field_name("alias"),
    ) else {
        return;
    };
    let alias_text = crate::parse::node_text(alias, src);
    let alias_text = alias_text.strip_prefix("r#").unwrap_or(&alias_text);
    if alias_text == "_" {
        return;
    }
    if let Some(target) = leaf_path(path, src, prefix) {
        let line = alias.start_position().row + 1;
        push_leaf(out, scope, block, &target, alias_text, line);
    }
}

fn plain_leaf(
    node: Node,
    src: &[u8],
    prefix: &[Segment],
    scope: &str,
    block: Option<usize>,
    out: &mut Vec<RsUse>,
) {
    let Some(target) = leaf_path(node, src, prefix) else {
        return;
    };
    let Some(last) = target.last() else { return };
    if last.text.is_empty() {
        return;
    }
    let (name, line) = (last.text.clone(), last.line);
    push_leaf(out, scope, block, &target, &name, line);
}

// The full path a leaf imports: `self` inside a group names the group's
// prefix itself.
fn leaf_path(node: Node, src: &[u8], prefix: &[Segment]) -> Option<Vec<Segment>> {
    if node.kind() == "self" && !prefix.is_empty() {
        return Some(prefix.to_vec());
    }
    let mut path = prefix.to_vec();
    path.extend(path_segments(node, src)?);
    Some(path)
}

fn push_leaf(
    out: &mut Vec<RsUse>,
    scope: &str,
    block: Option<usize>,
    path: &[Segment],
    name: &str,
    line: usize,
) {
    if path.is_empty() {
        return;
    }
    out.push(RsUse {
        scope: scope.to_string(),
        block,
        path: super::rs_paths::join(path),
        name: name.to_string(),
        line,
    });
}
