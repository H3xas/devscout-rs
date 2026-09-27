use tree_sitter::Node;

use super::text::{declared_name, named_children};
use super::types::RefRecord;

// A declaration's own type-parameter count: the `type_parameter` entries of
// its own list, so an enclosing type's parameters never count.
pub(super) fn own_type_parameter_count(node: Node) -> usize {
    named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
        .map_or(0, |list| {
            named_children(list)
                .into_iter()
                .filter(|p| p.kind() == "type_parameter")
                .count()
        })
}

// Per method name, one type-parameter count per overload, walking exactly the
// declarations `raw_method_arities` walks and skipping exactly the ones it
// skips, so each list lines up position for position with that name's ranges.
pub(super) fn raw_method_type_arities(node: Node, src: &[u8]) -> Vec<(String, Vec<usize>)> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut pairs: Vec<(String, Vec<usize>)> = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() {
            continue;
        }
        let count = own_type_parameter_count(c);
        match pairs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, counts)) => counts.push(count),
            None => pairs.push((name, vec![count])),
        }
    }
    pairs
}

// The length of the type-argument list written on an invoked member's own
// name node. Only commas that are direct children of the list separate
// arguments, so a nested generic argument's own commas and a comment inside
// the list never count. `None` for a name written without a list.
fn written_type_arg_count(name: Option<Node>) -> Option<usize> {
    let name = name?;
    if name.kind() != "generic_name" {
        return None;
    }
    let list = named_children(name)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let mut cursor = list.walk();
    let commas = list
        .children(&mut cursor)
        .filter(|c| c.kind() == ",")
        .count();
    Some(commas + 1)
}

// Stamps the member ref just pushed for an invocation window with the count
// written on that window's own member name, so the fact always comes from the
// same node the ref's member name was read from.
pub(super) fn stamp_written_type_args(refs: &mut [RefRecord], name: Option<Node>) {
    if let Some(last) = refs.last_mut() {
        last.method_type_arg_count = written_type_arg_count(name);
    }
}
